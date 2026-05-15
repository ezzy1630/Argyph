use std::collections::HashMap;
use std::io::BufRead;
use std::process::Command;
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;

use argyph_embed::Embedder;
use argyph_fs::ChangedPath;
use argyph_fs::FileEntry;
use argyph_graph::edge::Edge;
use argyph_graph::graph::SymbolOutline;
use argyph_graph::selector::SymbolSelector;
use argyph_pack::{self, DefaultPacker, PackContext, PackRequest, PackResult, PackScope, Packer};
use argyph_parse::types::Symbol;
use argyph_parse::SymbolId;
use argyph_store::Store;
use camino::{Utf8Path, Utf8PathBuf};
use regex::Regex;

use crate::error::{CoreError, Result};

pub struct SearchFilter {
    pub paths_glob: Option<Vec<String>>,
    pub exclude_glob: Option<Vec<String>>,
}

pub struct SearchHit {
    pub file: Utf8PathBuf,
    pub line: u64,
    pub column: u64,
    pub match_text: String,
}

pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    pub truncated: bool,
}

pub struct LanguageSummary {
    pub name: String,
    pub files: u64,
}

pub struct GitInfo {
    pub branch: String,
    pub head_short: String,
    pub dirty: bool,
}

pub struct RepoOverview {
    pub languages: Vec<LanguageSummary>,
    pub entry_points: Vec<String>,
    pub readme_excerpt: String,
    pub tree: String,
    pub git: Option<GitInfo>,
}

pub struct SemanticHit {
    pub chunk_id: String,
    pub chunk_text: String,
    pub file: String,
    pub byte_range: (u32, u32),
    pub line_range: (u32, u32),
    pub score: f32,
    pub source: String,
}

pub struct SemanticResult {
    pub hits: Vec<SemanticHit>,
    pub total_embedded: usize,
    pub total_chunks: usize,
}

/// The single domain facade that UI layers consume.
///
/// All queries go through `Index`; no caller outside `argyph-core` touches the
/// underlying [`Store`] directly.
pub struct Index {
    store: Arc<dyn Store>,
    embedder: Arc<OnceLock<Arc<dyn Embedder>>>,
}

impl Index {
    pub(crate) fn new(store: Arc<dyn Store>, embedder: Arc<OnceLock<Arc<dyn Embedder>>>) -> Self {
        Self { store, embedder }
    }

    pub fn protocol_version() -> &'static str {
        "0.1.0"
    }

    pub async fn get_file(&self, path: &Utf8Path) -> Result<Option<FileEntry>> {
        Ok(self.store.get_file(path).await?)
    }

    pub async fn list_files(&self) -> Result<Vec<FileEntry>> {
        Ok(self.store.list_files().await?)
    }

    pub async fn status(&self) -> Result<IndexStatus> {
        let files = self.store.list_files().await?;
        Ok(IndexStatus {
            protocol_version: Self::protocol_version().to_string(),
            file_count: files.len() as u64,
            snapshot_at: SystemTime::now(),
        })
    }

    pub async fn search_text(
        &self,
        root: &Utf8Path,
        pattern: &str,
        regex: bool,
        case_sensitive: bool,
        max_results: u64,
        filter: Option<SearchFilter>,
    ) -> Result<SearchResult> {
        let max = max_results.clamp(1, 1000);

        let re = build_regex(pattern, regex, case_sensitive)?;

        let files = self.store.list_files().await?;
        let files: Vec<_> = files
            .into_iter()
            .filter(|f| match &filter {
                Some(filt) => path_matches_filter(f.path.as_str(), filt),
                None => true,
            })
            .collect();

        let mut hits = Vec::new();
        'outer: for entry in &files {
            let file_path = root.join(entry.path.as_str());
            let f = match std::fs::File::open(file_path.as_str()) {
                Ok(f) => f,
                Err(_) => continue,
            };
            let reader = std::io::BufReader::new(f);
            for (line_no, line_result) in reader.lines().enumerate() {
                let line = match line_result {
                    Ok(l) => l,
                    Err(_) => continue,
                };
                for mat in re.find_iter(&line) {
                    hits.push(SearchHit {
                        file: entry.path.clone(),
                        line: (line_no + 1) as u64,
                        column: (mat.start() + 1) as u64,
                        match_text: mat.as_str().to_string(),
                    });
                    if hits.len() >= max as usize {
                        break 'outer;
                    }
                }
            }
        }

        let total: usize = files
            .iter()
            .filter_map(|f| {
                let fp = root.join(f.path.as_str());
                std::fs::read_to_string(fp.as_str())
                    .ok()
                    .map(|c| re.find_iter(&c).count())
            })
            .sum();
        let truncated = total > max as usize;

        Ok(SearchResult { hits, truncated })
    }

    pub async fn search_semantic(
        &self,
        query: &str,
        k: usize,
        filter: Option<&argyph_store::search::SearchFilter>,
    ) -> Result<SemanticResult> {
        let embedder = self.embedder.get().ok_or_else(|| {
            CoreError::Embed("no embedder configured — cannot perform semantic search".into())
        })?;

        let query_vec = embedder
            .embed_query(query)
            .await
            .map_err(|e| CoreError::Embed(format!("{e}")))?;

        let result = self
            .store
            .search_hybrid(query, &query_vec, k, filter.unwrap_or(&Default::default()))
            .await?;

        Ok(SemanticResult {
            hits: result
                .hits
                .into_iter()
                .map(|h| SemanticHit {
                    chunk_id: h.chunk_id,
                    chunk_text: h.chunk_text,
                    file: h.file,
                    byte_range: h.byte_range,
                    line_range: h.line_range,
                    score: h.score,
                    source: format!("{:?}", h.source).to_lowercase(),
                })
                .collect(),
            total_embedded: result.total_embedded,
            total_chunks: result.total_chunks,
        })
    }

    pub async fn overview(&self, root: &Utf8Path, max_tree_depth: u64) -> Result<RepoOverview> {
        let depth = max_tree_depth.clamp(1, 6) as usize;
        let files = self.store.list_files().await?;

        let mut lang_counts: HashMap<String, u64> = HashMap::new();
        for f in &files {
            if let Some(lang) = &f.language {
                *lang_counts.entry(lang.to_string()).or_default() += 1;
            }
        }
        let mut languages: Vec<LanguageSummary> = lang_counts
            .into_iter()
            .map(|(name, count)| LanguageSummary { name, files: count })
            .collect();
        languages.sort_by(|a, b| b.files.cmp(&a.files));

        let entry_points: Vec<String> = [
            "src/main.rs",
            "src/lib.rs",
            "main.rs",
            "lib.rs",
            "src/index.ts",
            "src/index.js",
            "src/index.py",
        ]
        .iter()
        .filter(|p| files.iter().any(|f| f.path.as_str() == **p))
        .map(|s| s.to_string())
        .collect();

        let readme_excerpt = Self::read_readme(root);
        let tree = Self::build_tree(&files, depth);
        let git = Self::get_git_info(root);

        Ok(RepoOverview {
            languages,
            entry_points,
            readme_excerpt,
            tree,
            git,
        })
    }

    pub async fn find_symbol(&self, name: &str, file: Option<&Utf8Path>) -> Result<Vec<Symbol>> {
        Ok(self.store.find_symbol(name, file).await?)
    }

    pub async fn find_references(&self, sel: &SymbolSelector) -> Result<Vec<Edge>> {
        Ok(self.store.find_references(sel).await?)
    }

    pub async fn get_callers(&self, sel: &SymbolSelector) -> Result<Vec<Edge>> {
        Ok(self.store.get_callers(sel).await?)
    }

    pub async fn get_callees(&self, sel: &SymbolSelector) -> Result<Vec<Edge>> {
        Ok(self.store.get_callees(sel).await?)
    }

    pub async fn get_imports(&self, file: &Utf8Path) -> Result<Vec<Edge>> {
        Ok(self.store.get_imports(file).await?)
    }

    pub async fn get_symbol_outline(&self, file: &Utf8Path) -> Result<Vec<SymbolOutline>> {
        Ok(self.store.get_symbol_outline(file).await?)
    }

    pub async fn reindex(&self, root: &Utf8Path, changes: &[ChangedPath]) -> Result<()> {
        crate::tiers::incremental_reindex(root, &*self.store, changes).await
    }

    pub async fn pack(&self, root: &Utf8Path, req: &PackRequest) -> Result<PackResult> {
        let packer = DefaultPacker::new().map_err(|e| CoreError::Io(std::io::Error::other(e)))?;
        let ctx = IndexPackContext {
            index: self,
            root: root.to_owned(),
        };
        packer
            .pack(req, &ctx)
            .map_err(|e| CoreError::Io(std::io::Error::other(e)))
    }

    // ── helpers ────────────────────────────────────────────────

    fn build_tree(files: &[FileEntry], depth: usize) -> String {
        let mut paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        paths.sort();
        paths.truncate(500);
        let mut out = String::new();
        let mut prev: Vec<&str> = vec![];
        for path in &paths {
            let parts: Vec<&str> = path.split('/').collect();
            let common = prev.iter().zip(&parts).filter(|(a, b)| a == b).count();
            if common < depth {
                for (i, part) in parts.iter().enumerate().skip(common).take(depth - common) {
                    let indent = "  ".repeat(i);
                    out.push_str(&format!("{indent}{part}/\n"));
                }
            }
            prev = parts;
        }
        out
    }

    fn read_readme(root: &camino::Utf8Path) -> String {
        for name in &["README.md", "README", "readme.md"] {
            let path = root.join(name);
            if let Ok(content) = std::fs::read_to_string(path.as_str()) {
                return content.lines().take(10).collect::<Vec<_>>().join("\n");
            }
        }
        String::new()
    }

    fn get_git_info(root: &camino::Utf8Path) -> Option<GitInfo> {
        let git_dir = root.join(".git");
        if !git_dir.exists() {
            return None;
        }
        let run = |args: &[&str]| -> Option<String> {
            Command::new("git")
                .args(args)
                .current_dir(root.as_str())
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        };
        let branch = run(&["rev-parse", "--abbrev-ref", "HEAD"])?;
        let head_short = run(&["rev-parse", "--short", "HEAD"])?;
        let dirty = Command::new("git")
            .args(["diff", "--quiet"])
            .current_dir(root.as_str())
            .status()
            .ok()
            .map(|s| !s.success())?;
        Some(GitInfo {
            branch,
            head_short,
            dirty,
        })
    }
}

struct IndexPackContext<'a> {
    index: &'a Index,
    root: Utf8PathBuf,
}

impl PackContext for IndexPackContext<'_> {
    fn list_files(&self, scope: &PackScope) -> Vec<Utf8PathBuf> {
        let files = tokio::runtime::Handle::current()
            .block_on(self.index.list_files())
            .unwrap_or_default();
        let paths: Vec<Utf8PathBuf> = files.into_iter().map(|f| f.path).collect();
        match scope {
            PackScope::All => paths,
            PackScope::Paths(requested) => {
                let requested_set: std::collections::HashSet<_> = requested.iter().collect();
                paths
                    .into_iter()
                    .filter(|p| requested_set.contains(p))
                    .collect()
            }
            PackScope::Symbol(name) => {
                let indexed_set: std::collections::HashSet<_> = paths.iter().collect();
                let syms = tokio::runtime::Handle::current()
                    .block_on(self.index.find_symbol(name, None))
                    .unwrap_or_default();
                let mut file_set: std::collections::HashSet<Utf8PathBuf> =
                    std::collections::HashSet::new();
                for sym in &syms {
                    file_set.insert(sym.file.clone());
                    let selector = SymbolSelector::ById(sym.id.clone());
                    if let Ok(callees) = tokio::runtime::Handle::current()
                        .block_on(self.index.get_callees(&selector))
                    {
                        for edge in &callees {
                            if let Some(f) = file_from_symbol_id(&edge.to) {
                                file_set.insert(f);
                            }
                        }
                    }
                    if let Ok(refs) = tokio::runtime::Handle::current()
                        .block_on(self.index.find_references(&selector))
                    {
                        for edge in &refs {
                            if let Some(f) = file_from_symbol_id(&edge.from) {
                                file_set.insert(f);
                            }
                        }
                    }
                }
                file_set
                    .into_iter()
                    .filter(|p| indexed_set.contains(p))
                    .collect()
            }
        }
    }

    fn read(&self, file: &Utf8Path) -> argyph_pack::Result<String> {
        let full_path = self.root.join(file.as_str());
        std::fs::read_to_string(full_path.as_str())
            .map_err(|e| argyph_pack::PackError::Io(e.to_string()))
    }

    fn modified(&self, file: &Utf8Path) -> Option<SystemTime> {
        tokio::runtime::Handle::current()
            .block_on(self.index.get_file(file))
            .ok()
            .flatten()
            .map(|entry| entry.modified)
    }

    fn in_edges(&self, file: &Utf8Path) -> argyph_pack::Result<usize> {
        tokio::runtime::Handle::current()
            .block_on(self.index.get_imports(file))
            .map(|edges| edges.len())
            .map_err(|e| argyph_pack::PackError::Io(e.to_string()))
    }
}

fn build_regex(pattern: &str, regex: bool, case_sensitive: bool) -> Result<Regex> {
    let pat = if regex {
        pattern.to_string()
    } else {
        regex::escape(pattern)
    };
    regex::RegexBuilder::new(&pat)
        .case_insensitive(!case_sensitive)
        .build()
        .map_err(|e| crate::CoreError::Io(std::io::Error::new(std::io::ErrorKind::InvalidInput, e)))
}

fn path_matches_filter(path: &str, filter: &SearchFilter) -> bool {
    let globs_ok = filter
        .paths_glob
        .as_ref()
        .is_none_or(|globs| globs.iter().any(|g| glob_match(g, path)));
    let excludes_ok = filter
        .exclude_glob
        .as_ref()
        .is_none_or(|globs| !globs.iter().any(|g| glob_match(g, path)));
    globs_ok && excludes_ok
}

fn glob_match(glob: &str, path: &str) -> bool {
    let cleaned = glob.trim_start_matches('!');
    if let Ok(re) = glob_to_regex(cleaned) {
        re.is_match(path)
    } else {
        path.contains(cleaned)
    }
}

fn glob_to_regex(glob: &str) -> std::result::Result<Regex, regex::Error> {
    let mut pattern = String::from("^");
    let chars: Vec<char> = glob.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '*' if i + 1 < chars.len() && chars[i + 1] == '*' => {
                pattern.push_str(".*");
                i += 1;
            }
            '*' => pattern.push_str("[^/]*"),
            '?' => pattern.push_str("[^/]"),
            '.' | '+' | '(' | ')' | '|' | '^' | '$' | '{' | '}' | '[' | ']' | '\\' => {
                pattern.push('\\');
                pattern.push(chars[i]);
            }
            c => pattern.push(c),
        }
        i += 1;
    }
    pattern.push('$');
    Regex::new(&pattern)
}

/// Read-only snapshot returned by [`Index::status`].
#[derive(Debug, Clone)]
pub struct IndexStatus {
    pub protocol_version: String,
    pub file_count: u64,
    pub snapshot_at: SystemTime,
}

fn file_from_symbol_id(id: &SymbolId) -> Option<Utf8PathBuf> {
    let s = id.as_str();
    let (prefix, _) = s.rsplit_once("::")?;
    let (file, _) = prefix.rsplit_once("::")?;
    Some(Utf8PathBuf::from(file))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn glob_star_star_matches_subdirs() {
        let re = glob_to_regex("src/**").unwrap();
        assert!(re.is_match("src/main.rs"));
        assert!(re.is_match("src/auth/mod.rs"));
    }

    #[test]
    fn glob_single_star_no_slash() {
        let re = glob_to_regex("*.rs").unwrap();
        assert!(re.is_match("main.rs"));
        assert!(!re.is_match("src/main.rs"));
    }

    #[test]
    fn build_regex_literal() {
        let re = build_regex("fn main", false, true).unwrap();
        assert!(re.is_match("fn main() {}"));
        assert!(!re.is_match("FN MAIN"));
    }

    #[test]
    fn build_regex_case_insensitive() {
        let re = build_regex("fn", false, false).unwrap();
        assert!(re.is_match("fn main"));
        assert!(re.is_match("FN MAIN"));
    }
}
