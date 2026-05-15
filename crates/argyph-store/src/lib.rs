#![forbid(unsafe_code)]

mod error;
mod migration;
pub mod rerank;
pub mod schema;
pub mod search;
mod sqlite;

use std::collections::HashMap;

use argyph_fs::FileEntry;
use argyph_graph::edge::{Edge, EdgeKind};
use argyph_graph::graph::SymbolOutline;
use argyph_graph::selector::SymbolSelector;
use argyph_parse::types::{Chunk, Symbol};
use camino::Utf8Path;

pub use error::{Result, StoreError};
pub use search::{HitSource, HybridSearchResult, SearchFilter, SearchHit, VectorEntry};
pub use sqlite::SqliteStore;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryEntry {
    pub id: String,
    pub scope: String,
    pub content: String,
    pub metadata: HashMap<String, String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuralNodeRecord {
    pub id: i64,
    pub file_id: i64,
    pub kind: String,
    pub label: String,
    pub path_joined: String,
    pub path: Vec<String>,
    pub byte_range: (u32, u32),
    pub line_range: (u32, u32),
    pub parent_id: Option<i64>,
    pub depth: u16,
}

/// Persists file metadata, symbols, chunks, edges, and embedding vectors.
/// Provides hybrid BM25 + vector search and schema migration management.
#[async_trait::async_trait]
pub trait Store: Send + Sync {
    /// Insert or update file entries. Path is the primary key — existing rows
    /// are updated with the new hash, language, and size.
    async fn upsert_files(&self, files: &[FileEntry]) -> Result<()>;

    /// Look up a single file by its repository-relative path.
    async fn get_file(&self, path: &Utf8Path) -> Result<Option<FileEntry>>;

    /// Return every file entry, ordered by path.
    async fn list_files(&self) -> Result<Vec<FileEntry>>;

    /// Remove a file from the index.
    async fn delete_file(&self, path: &Utf8Path) -> Result<()>;

    /// Get the SQLite rowid for a file by path.
    async fn get_file_id(&self, path: &Utf8Path) -> Result<Option<i64>>;

    /// Fetch a FileEntry by its rowid.
    async fn get_file_by_id(&self, id: i64) -> Result<Option<FileEntry>>;

    /// Insert or replace symbol rows. Idempotent — re-running with the same
    /// symbol IDs updates the rows.
    async fn upsert_symbols(&self, symbols: &[Symbol]) -> Result<()>;

    /// Insert or replace chunk rows. FTS5 index is kept in sync via triggers.
    async fn upsert_chunks(&self, chunks: &[Chunk]) -> Result<()>;

    /// Replace all edges for the files represented by the given edge set, then
    /// insert the new edges. Atomic within a transaction.
    async fn upsert_edges(&self, edges: &[Edge]) -> Result<()>;

    /// Replace all edges involving the given file (both `from_id` and `to_id`
    /// prefixes match the file path). Deletes old edges then inserts new ones.
    async fn replace_edges_for_file(&self, file: &Utf8Path, edges: &[Edge]) -> Result<()>;

    /// Find symbols by name, optionally scoped to a specific file.
    async fn find_symbol(&self, name: &str, file: Option<&Utf8Path>) -> Result<Vec<Symbol>>;

    /// Find reference edges pointing to the given symbol selector.
    async fn find_references(&self, sel: &SymbolSelector) -> Result<Vec<Edge>>;

    /// Find edges where `from_id` matches the selector and kind matches.
    /// Outgoing edges from the matched symbols.
    async fn neighbors(&self, sel: &SymbolSelector, kind: EdgeKind) -> Result<Vec<Edge>>;

    /// Find callers of the selected symbol(s) — edges where `to_id` matches
    /// and kind is Calls.
    async fn get_callers(&self, sel: &SymbolSelector) -> Result<Vec<Edge>>;

    /// Find callees of the selected symbol(s) — edges where `from_id` matches
    /// and kind is Calls.
    async fn get_callees(&self, sel: &SymbolSelector) -> Result<Vec<Edge>>;

    /// Find import edges originating from symbols in the given file.
    async fn get_imports(&self, file: &Utf8Path) -> Result<Vec<Edge>>;

    /// Return a hierarchical outline of all symbols defined in a file.
    async fn get_symbol_outline(&self, file: &Utf8Path) -> Result<Vec<SymbolOutline>>;

    /// Store embedding vectors for chunks. Existing vectors for the same
    /// `chunk_id` (and model) are replaced.
    async fn upsert_vectors(&self, vectors: &[VectorEntry]) -> Result<()>;

    /// Vector similarity search using cosine similarity. Returns the top `k`
    /// nearest neighbors optionally filtered by `filter`.
    async fn search_vectors(
        &self,
        query_vec: &[f32],
        k: usize,
        filter: &SearchFilter,
    ) -> Result<Vec<(String, f32)>>;

    /// Full-text search via FTS5 BM25. Returns top `limit` results with scores.
    async fn search_text_bm25(
        &self,
        query: &str,
        limit: usize,
        filter: &SearchFilter,
    ) -> Result<Vec<(String, f32)>>;

    /// Hybrid search combining BM25 scores and vector similarity via
    /// reciprocal rank fusion (RRF).
    async fn search_hybrid(
        &self,
        query: &str,
        query_vec: &[f32],
        k: usize,
        filter: &SearchFilter,
    ) -> Result<HybridSearchResult>;

    /// Return chunk IDs that don't have vectors yet for the given model.
    async fn missing_vectors(&self, model: &str) -> Result<Vec<String>>;

    /// Fetch chunk texts for the given chunk IDs.
    async fn get_chunk_texts(&self, chunk_ids: &[String]) -> Result<Vec<(String, String)>>;

    /// Flush and close the store. The store may not be used after calling this.
    async fn upsert_structural_nodes(
        &self,
        file_id: i64,
        nodes: &[StructuralNodeRecord],
    ) -> Result<()>;

    async fn get_structural_node_by_path(
        &self,
        file_id: Option<i64>,
        path_joined: &str,
    ) -> Result<Option<StructuralNodeRecord>>;

    async fn fts_search_structural(
        &self,
        query: &str,
        file_ids: Option<&[i64]>,
        limit: usize,
    ) -> Result<Vec<StructuralNodeRecord>>;

    async fn enclosing_structural_node(
        &self,
        file_id: i64,
        byte_offset: u32,
    ) -> Result<Option<StructuralNodeRecord>>;

    async fn structural_node_by_id(&self, id: i64) -> Result<Option<StructuralNodeRecord>>;

    async fn save_memory(
        &self,
        scope: &str,
        content: &str,
        metadata: &HashMap<String, String>,
    ) -> Result<String>;

    async fn search_memories(
        &self,
        query: &str,
        scope: Option<&str>,
        k: usize,
    ) -> Result<Vec<MemoryEntry>>;

    async fn list_memories(&self, scope: &str) -> Result<Vec<MemoryEntry>>;

    async fn forget_memory(&self, id: &str) -> Result<()>;

    /// Flush and close the store. The store may not be used after calling this.
    async fn close(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use argyph_fs::{Blake3Hash, Language};
    use argyph_graph::edge::{Confidence, EdgeKind};
    use argyph_graph::selector::SymbolSelector;
    use argyph_parse::types::{ByteRange, Chunk, ChunkId, ChunkKind, Symbol, SymbolId, SymbolKind};
    use camino::Utf8PathBuf;
    use rusqlite::params;
    use std::time::SystemTime;

    fn open_temp() -> SqliteStore {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        SqliteStore::open_at(&root).unwrap()
    }

    fn open_mem() -> SqliteStore {
        SqliteStore::open_in_memory().unwrap()
    }

    fn make_entry(path: &str, content: &[u8]) -> FileEntry {
        let hash = Blake3Hash::from(*blake3::hash(content).as_bytes());
        let ext = path.rsplit('.').next().unwrap_or("");
        let lang = argyph_fs::Language::from_extension(ext);
        FileEntry {
            path: Utf8PathBuf::from(path),
            hash,
            language: lang,
            size: content.len() as u64,
            modified: SystemTime::UNIX_EPOCH,
        }
    }

    fn make_symbol(file: &str, name: &str, kind: SymbolKind, start: usize, end: usize) -> Symbol {
        let path = Utf8PathBuf::from(file);
        Symbol {
            id: SymbolId::new(&path, name, start),
            name: name.to_string(),
            kind,
            file: path,
            range: ByteRange::new(start, end),
            signature: None,
            parent: None,
        }
    }

    fn make_chunk(file: &str, text: &str, kind: ChunkKind, start: usize, end: usize) -> Chunk {
        Chunk {
            id: ChunkId::from_text(text),
            file: Utf8PathBuf::from(file),
            range: ByteRange::new(start, end),
            text: text.to_string(),
            kind,
            language: Language::Rust,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn make_edge(
        from_file: &str,
        from_name: &str,
        from_pos: usize,
        to_file: &str,
        to_name: &str,
        to_pos: usize,
        kind: EdgeKind,
        confidence: Confidence,
    ) -> Edge {
        Edge {
            from: SymbolId::new(&Utf8PathBuf::from(from_file), from_name, from_pos),
            to: SymbolId::new(&Utf8PathBuf::from(to_file), to_name, to_pos),
            kind,
            confidence,
        }
    }

    // ---- Existing file-entry tests ----

    #[tokio::test]
    async fn upsert_and_list() {
        let store = open_temp();
        let entries = vec![
            make_entry("src/main.rs", b"fn main() {}"),
            make_entry("src/lib.rs", b"pub fn add(a: i32, b: i32) -> i32 { a + b }"),
        ];
        store.upsert_files(&entries).await.unwrap();

        let mut list = store.list_files().await.unwrap();
        list.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].path.as_str(), "src/lib.rs");
        assert_eq!(list[1].path.as_str(), "src/main.rs");
        for entry in &list {
            let expected = entries.iter().find(|e| e.path == entry.path).unwrap();
            assert_eq!(entry.hash, expected.hash);
            assert_eq!(entry.language, expected.language);
            assert_eq!(entry.size, expected.size);
        }
    }

    #[tokio::test]
    async fn get_file_found_and_not_found() {
        let store = open_temp();
        let entry = make_entry("README.md", b"# Hello");
        store.upsert_files(&[entry.clone()]).await.unwrap();

        let found = store
            .get_file(&Utf8PathBuf::from("README.md"))
            .await
            .unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().hash, entry.hash);

        let missing = store
            .get_file(&Utf8PathBuf::from("nope.txt"))
            .await
            .unwrap();
        assert!(missing.is_none());
    }

    #[tokio::test]
    async fn delete_removes_file() {
        let store = open_temp();
        let a = make_entry("a.rs", b"a");
        let b = make_entry("b.rs", b"b");
        store.upsert_files(&[a, b]).await.unwrap();

        store.delete_file(&Utf8PathBuf::from("a.rs")).await.unwrap();
        let list = store.list_files().await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].path.as_str(), "b.rs");
    }

    #[tokio::test]
    async fn upsert_is_idempotent() {
        let store = open_temp();
        let e1 = make_entry("x.rs", b"v1");
        let e2 = FileEntry {
            hash: Blake3Hash::from(*blake3::hash(b"v2").as_bytes()),
            size: 2,
            ..e1.clone()
        };

        store.upsert_files(&[e1.clone()]).await.unwrap();
        store.upsert_files(&[e2.clone()]).await.unwrap();

        let found = store
            .get_file(&Utf8PathBuf::from("x.rs"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.hash, e2.hash);
        assert_eq!(found.size, 2);
    }

    #[tokio::test]
    async fn round_trip_many_entries() {
        let store = open_temp();
        let count = 300;
        let entries: Vec<_> = (0..count)
            .map(|i| make_entry(&format!("src/mod{i}.rs"), format!("// file {i}").as_bytes()))
            .collect();

        store.upsert_files(&entries).await.unwrap();
        let list = store.list_files().await.unwrap();

        assert_eq!(list.len(), count);
        let paths: std::collections::HashSet<_> =
            list.iter().map(|e| e.path.as_str().to_string()).collect();
        for entry in &entries {
            assert!(paths.contains(entry.path.as_str()));
        }
    }

    #[tokio::test]
    async fn empty_upsert_does_not_crash() {
        let store = open_temp();
        store.upsert_files(&[]).await.unwrap();
        assert!(store.list_files().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn language_is_round_tripped() {
        let store = open_temp();
        let entries = vec![
            make_entry("lib.rs", b"rust"),
            make_entry("app.ts", b"ts"),
            make_entry("util.py", b"py"),
            make_entry("readme.md", b"md"),
        ];
        store.upsert_files(&entries).await.unwrap();
        let list = store.list_files().await.unwrap();
        for entry in &list {
            let expected = entries.iter().find(|e| e.path == entry.path).unwrap();
            assert_eq!(
                entry.language, expected.language,
                "language mismatch for {}",
                entry.path
            );
        }
    }

    #[tokio::test]
    async fn db_persists_across_opens() {
        let dir = tempfile::tempdir().unwrap();
        let root = Utf8PathBuf::from_path_buf(dir.path().to_path_buf()).unwrap();
        let entry = make_entry("persist.rs", b"data");

        {
            let store = SqliteStore::open_at(&root).unwrap();
            store.upsert_files(&[entry.clone()]).await.unwrap();
        }
        {
            let store = SqliteStore::open_at(&root).unwrap();
            let found = store
                .get_file(&Utf8PathBuf::from("persist.rs"))
                .await
                .unwrap();
            assert!(found.is_some());
            assert_eq!(found.unwrap().hash, entry.hash);
        }
    }

    // ---- New symbol / chunk / edge tests ----

    #[tokio::test]
    async fn upsert_symbols_and_find_by_name() {
        let store = open_mem();
        let sym = make_symbol("src/lib.rs", "add", SymbolKind::Function, 10, 50);
        store.upsert_symbols(&[sym.clone()]).await.unwrap();

        let found = store.find_symbol("add", None).await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "add");
        assert_eq!(found[0].file.as_str(), "src/lib.rs");
    }

    #[tokio::test]
    async fn find_symbol_scoped_to_file() {
        let store = open_mem();
        let a = make_symbol("src/a.rs", "helper", SymbolKind::Function, 0, 20);
        let b = make_symbol("src/b.rs", "helper", SymbolKind::Function, 0, 20);
        store.upsert_symbols(&[a, b]).await.unwrap();

        let found = store
            .find_symbol("helper", Some(&Utf8PathBuf::from("src/b.rs")))
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].file.as_str(), "src/b.rs");
    }

    #[tokio::test]
    async fn find_symbol_missing() {
        let store = open_mem();
        let found = store.find_symbol("nope", None).await.unwrap();
        assert!(found.is_empty());
    }

    #[tokio::test]
    async fn upsert_symbols_is_idempotent() {
        let store = open_mem();
        let sym = make_symbol("x.rs", "f", SymbolKind::Function, 5, 25);
        store.upsert_symbols(&[sym]).await.unwrap();
        store
            .upsert_symbols(&[make_symbol("x.rs", "f", SymbolKind::Function, 5, 25)])
            .await
            .unwrap();
        let found = store.find_symbol("f", None).await.unwrap();
        assert_eq!(found.len(), 1);
    }

    #[tokio::test]
    async fn upsert_chunks_round_trips_via_fts5() {
        let store = open_mem();
        let chunk = make_chunk(
            "src/lib.rs",
            "fn greet() { hello(); }",
            ChunkKind::FunctionBody,
            0,
            25,
        );
        store.upsert_chunks(&[chunk]).await.unwrap();

        // Verify via FTS5: BM25 search should find it.
        let conn = store.conn.lock().expect("poisoned");
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks_fts WHERE chunks_fts MATCH ?1",
                params!["greet"],
                |row| row.get(0),
            )
            .unwrap();
        assert!(count > 0, "FTS5 should match 'greet'");
    }

    #[tokio::test]
    async fn upsert_chunks_fts5_does_not_match_absent_term() {
        let store = open_mem();
        let chunk = make_chunk("src/lib.rs", "fn foo() {}", ChunkKind::FunctionBody, 0, 12);
        store.upsert_chunks(&[chunk]).await.unwrap();

        let conn = store.conn.lock().expect("poisoned");
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM chunks_fts WHERE chunks_fts MATCH ?1",
                params!["zzzz_not_present"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn empty_upsert_edges_does_nothing() {
        let store = open_mem();
        store.upsert_edges(&[]).await.unwrap();
    }

    #[tokio::test]
    async fn upsert_edges_and_find_references() {
        let store = open_mem();
        let edge = make_edge(
            "src/main.rs",
            "main",
            0,
            "src/lib.rs",
            "add",
            10,
            EdgeKind::References,
            Confidence::Heuristic,
        );
        store.upsert_edges(&[edge.clone()]).await.unwrap();

        let refs = store
            .find_references(&SymbolSelector::ByName {
                file: Utf8PathBuf::from("src/lib.rs"),
                name: "add".into(),
            })
            .await
            .unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].kind, EdgeKind::References);
        assert_eq!(refs[0].from.as_str(), "src/main.rs::main::0");
        assert_eq!(refs[0].to.as_str(), "src/lib.rs::add::10");
    }

    #[tokio::test]
    async fn get_callers_and_callees() {
        let store = open_mem();
        let caller = make_symbol("src/a.rs", "caller_fn", SymbolKind::Function, 0, 40);
        let callee = make_symbol("src/a.rs", "callee_fn", SymbolKind::Function, 50, 90);
        store
            .upsert_symbols(&[caller.clone(), callee.clone()])
            .await
            .unwrap();

        let edge = make_edge(
            "src/a.rs",
            "caller_fn",
            0,
            "src/a.rs",
            "callee_fn",
            50,
            EdgeKind::Calls,
            Confidence::Heuristic,
        );
        store.upsert_edges(&[edge]).await.unwrap();

        let callers = store
            .get_callers(&SymbolSelector::ById(callee.id.clone()))
            .await
            .unwrap();
        assert_eq!(callers.len(), 1);
        assert_eq!(callers[0].from.as_str(), "src/a.rs::caller_fn::0");

        let callees = store
            .get_callees(&SymbolSelector::ById(caller.id.clone()))
            .await
            .unwrap();
        assert_eq!(callees.len(), 1);
        assert_eq!(callees[0].to.as_str(), "src/a.rs::callee_fn::50");
    }

    #[tokio::test]
    async fn get_imports_finds_file_imports() {
        let store = open_mem();
        let edge = make_edge(
            "src/main.rs",
            "main",
            0,
            "src/math.rs",
            "add",
            10,
            EdgeKind::Imports,
            Confidence::Heuristic,
        );
        store.upsert_edges(&[edge.clone()]).await.unwrap();

        let imports = store
            .get_imports(&Utf8PathBuf::from("src/main.rs"))
            .await
            .unwrap();
        assert!(!imports.is_empty());
        assert_eq!(imports[0].kind, EdgeKind::Imports);
        assert!(imports[0].from.as_str().starts_with("src/main.rs::"));
    }

    #[tokio::test]
    async fn get_imports_empty_for_unrelated_file() {
        let store = open_mem();
        let edge = make_edge(
            "src/main.rs",
            "main",
            0,
            "src/math.rs",
            "add",
            10,
            EdgeKind::Imports,
            Confidence::Heuristic,
        );
        store.upsert_edges(&[edge]).await.unwrap();

        let imports = store
            .get_imports(&Utf8PathBuf::from("src/other.rs"))
            .await
            .unwrap();
        assert!(imports.is_empty());
    }

    #[tokio::test]
    async fn get_symbol_outline_returns_symbols_in_file() {
        let store = open_mem();
        let a = make_symbol("src/lib.rs", "new", SymbolKind::Function, 0, 30);
        let b = make_symbol("src/lib.rs", "add", SymbolKind::Function, 40, 70);
        store.upsert_symbols(&[a, b]).await.unwrap();

        let outline = store
            .get_symbol_outline(&Utf8PathBuf::from("src/lib.rs"))
            .await
            .unwrap();
        assert_eq!(outline.len(), 2);
        let names: Vec<&str> = outline.iter().map(|o| o.name.as_str()).collect();
        assert!(names.contains(&"add"));
        assert!(names.contains(&"new"));
    }

    #[tokio::test]
    async fn get_symbol_outline_handles_parent_child() {
        let store = open_mem();
        let parent = Symbol {
            id: SymbolId::new(&Utf8PathBuf::from("src/struct.rs"), "MyStruct", 10),
            name: "MyStruct".into(),
            kind: SymbolKind::Struct,
            file: Utf8PathBuf::from("src/struct.rs"),
            range: ByteRange::new(10, 200),
            signature: None,
            parent: None,
        };
        let child_id = SymbolId::new(&Utf8PathBuf::from("src/struct.rs"), "method_a", 50);
        let child = Symbol {
            id: child_id.clone(),
            name: "method_a".into(),
            kind: SymbolKind::Method,
            file: Utf8PathBuf::from("src/struct.rs"),
            range: ByteRange::new(50, 100),
            signature: None,
            parent: Some(SymbolId::new(
                &Utf8PathBuf::from("src/struct.rs"),
                "MyStruct",
                10,
            )),
        };
        store.upsert_symbols(&[parent, child]).await.unwrap();

        let outline = store
            .get_symbol_outline(&Utf8PathBuf::from("src/struct.rs"))
            .await
            .unwrap();
        assert_eq!(outline.len(), 1);
        assert_eq!(outline[0].name, "MyStruct");
        assert_eq!(outline[0].children.len(), 1);
        assert_eq!(outline[0].children[0].name, "method_a");
    }

    #[tokio::test]
    async fn neighbors_returns_outgoing_edges_of_kind() {
        let store = open_mem();
        let edge = make_edge(
            "src/a.rs",
            "a_fn",
            0,
            "src/b.rs",
            "b_fn",
            100,
            EdgeKind::References,
            Confidence::Heuristic,
        );
        store.upsert_edges(&[edge]).await.unwrap();

        let outgoing = store
            .neighbors(
                &SymbolSelector::ById(SymbolId::new(&Utf8PathBuf::from("src/a.rs"), "a_fn", 0)),
                EdgeKind::References,
            )
            .await
            .unwrap();
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].to.as_str(), "src/b.rs::b_fn::100");
    }

    #[tokio::test]
    async fn find_references_by_qualified_name() {
        let store = open_mem();
        let edge = make_edge(
            "src/main.rs",
            "main",
            0,
            "src/math.rs",
            "multiply",
            42,
            EdgeKind::References,
            Confidence::Heuristic,
        );
        store.upsert_edges(&[edge]).await.unwrap();

        let refs = store
            .find_references(&SymbolSelector::Qualified("multiply".into()))
            .await
            .unwrap();
        assert_eq!(refs.len(), 1);
    }

    #[tokio::test]
    async fn edge_replace_deletes_old_file_edges() {
        let store = open_mem();
        let e1 = make_edge(
            "src/a.rs",
            "a_fn",
            0,
            "src/b.rs",
            "b_fn",
            100,
            EdgeKind::References,
            Confidence::Heuristic,
        );
        store.upsert_edges(&[e1]).await.unwrap();
        assert_eq!(
            store
                .find_references(&SymbolSelector::ByName {
                    file: Utf8PathBuf::from("src/b.rs"),
                    name: "b_fn".into(),
                })
                .await
                .unwrap()
                .len(),
            1
        );

        // Upsert new edges with same file prefix — old edges deleted.
        let e2 = make_edge(
            "src/a.rs",
            "a_fn",
            0,
            "src/c.rs",
            "c_fn",
            200,
            EdgeKind::References,
            Confidence::Heuristic,
        );
        store.upsert_edges(&[e2]).await.unwrap();

        // Old reference to b_fn should be gone since src/a.rs was replaced.
        let refs_b = store
            .find_references(&SymbolSelector::ByName {
                file: Utf8PathBuf::from("src/b.rs"),
                name: "b_fn".into(),
            })
            .await
            .unwrap();
        assert!(
            refs_b.is_empty(),
            "old edge from src/a.rs should be deleted"
        );

        // New reference to c_fn should exist.
        let refs_c = store
            .find_references(&SymbolSelector::ByName {
                file: Utf8PathBuf::from("src/c.rs"),
                name: "c_fn".into(),
            })
            .await
            .unwrap();
        assert_eq!(refs_c.len(), 1);
    }

    #[tokio::test]
    async fn empty_upserts_noop() {
        let store = open_mem();
        store.upsert_symbols(&[]).await.unwrap();
        store.upsert_chunks(&[]).await.unwrap();
        store.upsert_edges(&[]).await.unwrap();
    }

    // ---- Vector / search tests ----

    fn make_test_vector(dim: usize, value: f32) -> Vec<f32> {
        vec![value; dim]
    }

    fn make_vec_entry(chunk_id: &str, vector: Vec<f32>, model: &str) -> VectorEntry {
        VectorEntry {
            chunk_id: chunk_id.to_string(),
            dimension: vector.len(),
            vector,
            model: model.to_string(),
        }
    }

    #[tokio::test]
    async fn upsert_vectors_stores_and_retrieves() {
        let store = open_mem();
        let c = make_chunk("src/a.rs", "fn add() {}", ChunkKind::FunctionBody, 0, 12);
        let cid = c.id.to_string();
        store.upsert_chunks(&[c]).await.unwrap();

        let vec1 = make_test_vector(4, 0.5);
        store
            .upsert_vectors(&[make_vec_entry(&cid, vec1.clone(), "test-model")])
            .await
            .unwrap();

        // Search with a similar vector should return this chunk
        let query = vec![0.5; 4];
        let hits = store
            .search_vectors(&query, 1, &SearchFilter::default())
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, cid);
        // Cosine similarity of two identical normalized vectors = 1.0
        assert!((hits[0].1 - 1.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn upsert_vectors_replaces_existing() {
        let store = open_mem();
        let c = make_chunk("src/a.rs", "fn a() {}", ChunkKind::FunctionBody, 0, 10);
        let cid = c.id.to_string();
        store.upsert_chunks(&[c]).await.unwrap();

        store
            .upsert_vectors(&[make_vec_entry(&cid, make_test_vector(3, 1.0), "m")])
            .await
            .unwrap();
        store
            .upsert_vectors(&[make_vec_entry(&cid, make_test_vector(3, -1.0), "m")])
            .await
            .unwrap();

        // Query similar to -1.0 should have high cosine similarity
        let hits = store
            .search_vectors(&[-1.0; 3], 1, &SearchFilter::default())
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert!((hits[0].1 - 1.0).abs() < 0.001);
    }

    #[tokio::test]
    async fn search_vectors_returns_top_k() {
        let store = open_mem();
        // Use orthogonal vectors to ensure distinct cosine similarities
        let vecs = [
            (vec![1.0_f32, 0.0, 0.0], "mod0.rs"),
            (vec![0.0_f32, 1.0, 0.0], "mod1.rs"),
            (vec![0.0_f32, 0.0, 1.0], "mod2.rs"),
        ];
        for (v, file) in &vecs {
            let c = make_chunk(
                file,
                &format!("fn f_{file}() {{}}"),
                ChunkKind::FunctionBody,
                0,
                12,
            );
            let cid = c.id.to_string();
            store.upsert_chunks(&[c]).await.unwrap();
            store
                .upsert_vectors(&[make_vec_entry(&cid, v.clone(), "m")])
                .await
                .unwrap();
        }

        let query = vec![0.95_f32, 0.3, 0.0];
        let hits = store
            .search_vectors(&query, 2, &SearchFilter::default())
            .await
            .unwrap();
        assert_eq!(hits.len(), 2);
        // The first vector [1,0,0] is closest to [0.95, 0.3, 0]
        assert!(hits[0].1 > hits[1].1);
    }

    #[tokio::test]
    async fn search_text_bm25_finds_matching_chunks() {
        let store = open_mem();
        let c1 = make_chunk(
            "src/main.rs",
            "let greeting = compute_hello_world();",
            ChunkKind::TopLevel,
            0,
            35,
        );
        let c2 = make_chunk(
            "src/lib.rs",
            "fn unrelated() { return 42; }",
            ChunkKind::FunctionBody,
            0,
            25,
        );
        let cid1 = c1.id.to_string();
        store.upsert_chunks(&[c1, c2]).await.unwrap();

        let hits = store
            .search_text_bm25("greeting", 10, &SearchFilter::default())
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].0, cid1);
    }

    #[tokio::test]
    async fn search_text_bm25_no_match_returns_empty() {
        let store = open_mem();
        let c = make_chunk("src/a.rs", "let x = 1;", ChunkKind::TopLevel, 0, 10);
        store.upsert_chunks(&[c]).await.unwrap();

        let hits = store
            .search_text_bm25("zzzznonexistent", 10, &SearchFilter::default())
            .await
            .unwrap();
        assert!(hits.is_empty());
    }

    #[tokio::test]
    async fn search_text_bm25_respects_limit() {
        let store = open_mem();
        for i in 0..5 {
            let c = make_chunk(
                &format!("src/f{i}.rs"),
                &format!("fn f{i}() {{ let x = test_{i}; }}"),
                ChunkKind::FunctionBody,
                0,
                30,
            );
            store.upsert_chunks(&[c]).await.unwrap();
        }

        let hits = store
            .search_text_bm25("test", 3, &SearchFilter::default())
            .await
            .unwrap();
        assert_eq!(hits.len(), 3);
    }

    #[tokio::test]
    async fn search_hybrid_returns_combined_results() {
        let store = open_mem();
        let _dim = 3;

        // Create multiple chunks: some with vectors, some without
        let c1 = make_chunk(
            "src/a.rs",
            "greeting hello world",
            ChunkKind::TopLevel,
            0,
            20,
        );
        let c2 = make_chunk(
            "src/b.rs",
            "compute sum of numbers",
            ChunkKind::FunctionBody,
            0,
            22,
        );
        let c3 = make_chunk(
            "src/c.rs",
            "greeting response handler",
            ChunkKind::TopLevel,
            0,
            25,
        );

        let cid1 = c1.id.to_string();
        let cid2 = c2.id.to_string();
        let cid3 = c3.id.to_string();

        store.upsert_chunks(&[c1, c2, c3]).await.unwrap();

        store
            .upsert_vectors(&[
                make_vec_entry(&cid1, vec![1.0, 0.0, 0.0], "m"),
                make_vec_entry(&cid2, vec![0.0, 1.0, 0.0], "m"),
                make_vec_entry(&cid3, vec![0.0, 0.0, 1.0], "m"),
            ])
            .await
            .unwrap();

        // Query: "greeting" text + vector close to [1, 0, 0]
        let result = store
            .search_hybrid("greeting", &[1.0, 0.0, 0.0], 5, &SearchFilter::default())
            .await
            .unwrap();

        assert!(!result.hits.is_empty(), "hybrid search should return hits");
        assert_eq!(result.total_embedded, 3);
        assert_eq!(result.total_chunks, 3);

        // c1 (greeting + matching vector) should rank highest
        assert!(
            result.hits.iter().any(|h| h.chunk_id == cid1),
            "c1 should be in results"
        );
    }

    #[tokio::test]
    async fn search_hybrid_handles_empty_vector_store() {
        let store = open_mem();
        let c = make_chunk(
            "src/a.rs",
            "fn hello() { world(); }",
            ChunkKind::FunctionBody,
            0,
            22,
        );
        store.upsert_chunks(&[c]).await.unwrap();

        let result = store
            .search_hybrid("hello", &[1.0_f32; 4], 5, &SearchFilter::default())
            .await
            .unwrap();

        // Should still work using BM25 only
        assert!(!result.hits.is_empty());
        assert_eq!(result.total_embedded, 0);
        assert_eq!(result.total_chunks, 1);
    }

    #[tokio::test]
    async fn missing_vectors_returns_unembedded_chunks() {
        let store = open_mem();
        let c1 = make_chunk("src/a.rs", "fn a() {}", ChunkKind::FunctionBody, 0, 10);
        let c2 = make_chunk("src/b.rs", "fn b() {}", ChunkKind::FunctionBody, 0, 10);
        let cid1 = c1.id.to_string();
        let cid2 = c2.id.to_string();
        store.upsert_chunks(&[c1, c2]).await.unwrap();

        // No vectors yet — both should be missing
        let missing = store.missing_vectors("m").await.unwrap();
        assert_eq!(missing.len(), 2);
        assert!(missing.contains(&cid1));
        assert!(missing.contains(&cid2));

        // Now embed one
        store
            .upsert_vectors(&[make_vec_entry(&cid1, make_test_vector(3, 0.0), "m")])
            .await
            .unwrap();

        let missing = store.missing_vectors("m").await.unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0], cid2);
    }

    #[tokio::test]
    async fn missing_vectors_respects_model() {
        let store = open_mem();
        let c = make_chunk("src/a.rs", "fn a() {}", ChunkKind::FunctionBody, 0, 10);
        let cid = c.id.to_string();
        store.upsert_chunks(&[c]).await.unwrap();

        // Embed with model A
        store
            .upsert_vectors(&[make_vec_entry(&cid, make_test_vector(3, 0.0), "model-a")])
            .await
            .unwrap();

        // model-a has it covered
        assert!(store.missing_vectors("model-a").await.unwrap().is_empty());

        // model-b does not
        assert_eq!(store.missing_vectors("model-b").await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn filter_by_language_in_vector_search() {
        let store = open_mem();
        let c_rust = make_chunk("src/a.rs", "fn a() {}", ChunkKind::FunctionBody, 0, 10);
        let mut c_py = make_chunk("src/b.rs", "fn b() {}", ChunkKind::FunctionBody, 0, 10);
        c_py.language = Language::Python;
        let cid_rust = c_rust.id.to_string();
        store.upsert_chunks(&[c_rust, c_py]).await.unwrap();

        store
            .upsert_vectors(&[make_vec_entry(&cid_rust, make_test_vector(3, 1.0), "m")])
            .await
            .unwrap();

        let rust_filter = SearchFilter {
            language: Some("Rust".to_string()),
            ..Default::default()
        };
        let hits = store
            .search_vectors(&[1.0; 3], 10, &rust_filter)
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[tokio::test]
    async fn filter_by_language_in_bm25_search() {
        let store = open_mem();
        let c_rust = make_chunk(
            "src/a.rs",
            "greeting fn a() {}",
            ChunkKind::FunctionBody,
            0,
            15,
        );
        let mut c_py = make_chunk(
            "src/b.py",
            "greeting def b(): pass",
            ChunkKind::FunctionBody,
            0,
            20,
        );
        c_py.language = Language::Python;
        store.upsert_chunks(&[c_rust, c_py]).await.unwrap();

        let py_filter = SearchFilter {
            language: Some("Python".to_string()),
            ..Default::default()
        };
        let hits = store
            .search_text_bm25("greeting", 10, &py_filter)
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[tokio::test]
    async fn empty_vector_search_on_empty_store() {
        let store = open_mem();
        let hits = store
            .search_vectors(&[0.5; 4], 10, &SearchFilter::default())
            .await
            .unwrap();
        assert!(hits.is_empty());
    }

    #[tokio::test]
    async fn empty_upsert_vectors_noop() {
        let store = open_mem();
        store.upsert_vectors(&[]).await.unwrap();
    }

    // ---- Structural nodes tests ----

    #[tokio::test]
    async fn migration_004_creates_structural_tables() {
        let store = open_mem();
        let conn = store.conn.lock().expect("poisoned");
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'structural_nodes'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        let fts_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'structural_fts'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fts_count, 1);
    }

    #[tokio::test]
    async fn structural_upsert_and_path_lookup() {
        let store = open_mem();
        let entry = make_entry("a.md", b"# Hello");
        store.upsert_files(&[entry]).await.unwrap();
        let file_id: i64 = {
            let conn = store.conn.lock().expect("poisoned");
            conn.query_row("SELECT rowid FROM files WHERE path = 'a.md'", [], |r| {
                r.get(0)
            })
            .unwrap()
        };

        let rec = StructuralNodeRecord {
            id: 100,
            file_id,
            kind: "MdSection".into(),
            label: "Pricing".into(),
            path_joined: "Pricing".into(),
            path: vec!["Pricing".into()],
            byte_range: (0, 50),
            line_range: (1, 5),
            parent_id: None,
            depth: 0,
        };
        store
            .upsert_structural_nodes(file_id, &[rec.clone()])
            .await
            .unwrap();
        let got = store
            .get_structural_node_by_path(Some(file_id), "Pricing")
            .await
            .unwrap();
        assert_eq!(got, Some(rec));
    }

    #[tokio::test]
    async fn structural_enclosing_node() {
        let store = open_mem();
        let entry = make_entry("b.md", b"data");
        store.upsert_files(&[entry]).await.unwrap();
        let file_id: i64 = {
            let conn = store.conn.lock().expect("poisoned");
            conn.query_row("SELECT rowid FROM files WHERE path = 'b.md'", [], |r| {
                r.get(0)
            })
            .unwrap()
        };

        let rec = StructuralNodeRecord {
            id: 200,
            file_id,
            kind: "MdSection".into(),
            label: "Top".into(),
            path_joined: "Top".into(),
            path: vec!["Top".into()],
            byte_range: (0, 100),
            line_range: (1, 10),
            parent_id: None,
            depth: 0,
        };
        store
            .upsert_structural_nodes(file_id, &[rec])
            .await
            .unwrap();
        let enclosing = store.enclosing_structural_node(file_id, 50).await.unwrap();
        assert!(enclosing.is_some());
        assert_eq!(enclosing.unwrap().label, "Top");
    }

    #[tokio::test]
    async fn structural_fts_search() {
        let store = open_mem();
        let entry = make_entry("c.md", b"stuff");
        store.upsert_files(&[entry]).await.unwrap();
        let file_id: i64 = {
            let conn = store.conn.lock().expect("poisoned");
            conn.query_row("SELECT rowid FROM files WHERE path = 'c.md'", [], |r| {
                r.get(0)
            })
            .unwrap()
        };

        let r1 = StructuralNodeRecord {
            id: 300,
            file_id,
            kind: "MdSection".into(),
            label: "Introduction".into(),
            path_joined: "Introduction".into(),
            path: vec!["Introduction".into()],
            byte_range: (0, 50),
            line_range: (1, 5),
            parent_id: None,
            depth: 0,
        };
        let r2 = StructuralNodeRecord {
            id: 301,
            file_id,
            kind: "MdSection".into(),
            label: "API Reference".into(),
            path_joined: "API Reference".into(),
            path: vec!["API Reference".into()],
            byte_range: (50, 100),
            line_range: (6, 10),
            parent_id: None,
            depth: 0,
        };
        store
            .upsert_structural_nodes(file_id, &[r1, r2])
            .await
            .unwrap();
        let hits = store
            .fts_search_structural("Introduction", None, 10)
            .await
            .unwrap();
        assert!(!hits.is_empty());
        assert!(hits.iter().any(|h| h.label == "Introduction"));
    }
}
