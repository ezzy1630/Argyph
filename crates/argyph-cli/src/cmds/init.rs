use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const BEGIN: &str = "<!-- argyph:begin -->";
const END: &str = "<!-- argyph:end -->";
const TARGETS: [&str; 3] = ["CLAUDE.md", "AGENTS.md", "GEMINI.md"];

const BLOCK: &str = r#"<!-- argyph:begin -->
## Code & context lookup

This repo is indexed by Argyph (MCP). For any lookup of code, symbols, files,
or content, prefer the `ask` tool over grep, find, or reading files directly.
Argyph returns minimal validated spans, not full files.

- `ask` — primary entry point. Pass a query and optional focus.
- `pack_repo` — only when you genuinely need a flat dump.
- Other Argyph tools — advanced, prefer `ask` first.
<!-- argyph:end -->
"#;

pub fn run(path: Option<&str>) -> ExitCode {
    let root = path
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if !root.is_dir() {
        eprintln!("init: not a directory: {}", root.display());
        return ExitCode::FAILURE;
    }

    let existing: Vec<&str> = TARGETS
        .iter()
        .copied()
        .filter(|name| root.join(name).is_file())
        .collect();
    let targets = if existing.is_empty() {
        vec!["CLAUDE.md"]
    } else {
        existing
    };

    for target in targets {
        let path = root.join(target);
        if let Err(error) = install_block(&path) {
            eprintln!("init: failed to update {}: {}", path.display(), error);
            return ExitCode::FAILURE;
        }
        println!("init: updated {}", path.display());
    }

    ExitCode::SUCCESS
}

fn install_block(path: &Path) -> std::io::Result<()> {
    let content = if path.exists() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };

    let updated = if let Some((begin, end)) = existing_block_range(&content) {
        let mut updated = String::with_capacity(content.len() + BLOCK.len());
        updated.push_str(&content[..begin]);
        updated.push_str(BLOCK.trim_end_matches('\n'));
        updated.push_str(&content[end..]);
        updated
    } else {
        let mut updated = content;
        if !updated.is_empty() && !updated.ends_with('\n') {
            updated.push('\n');
        }
        if !updated.is_empty() {
            updated.push('\n');
        }
        updated.push_str(BLOCK);
        updated
    };

    fs::write(path, updated)
}

fn existing_block_range(content: &str) -> Option<(usize, usize)> {
    let begin = content.find(BEGIN)?;
    let end = content[begin..].find(END)? + begin + END.len();
    Some((begin, end))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

    fn tempdir() -> PathBuf {
        let id = NEXT_DIR.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("argyph-init-test-{}-{id}", std::process::id()));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn creates_claude_md_when_none_exists() {
        let dir = tempdir();
        let code = run(Some(dir.to_str().unwrap()));
        assert_eq!(code, ExitCode::SUCCESS);
        let written = fs::read_to_string(dir.join("CLAUDE.md")).unwrap();
        assert!(written.contains(BEGIN));
        assert!(written.contains("## Code & context lookup"));
        assert!(written.contains("`ask`"));
        assert!(written.contains(END));
    }

    #[test]
    fn idempotent_on_rerun() {
        let dir = tempdir();
        assert_eq!(run(Some(dir.to_str().unwrap())), ExitCode::SUCCESS);
        assert_eq!(run(Some(dir.to_str().unwrap())), ExitCode::SUCCESS);
        let written = fs::read_to_string(dir.join("CLAUDE.md")).unwrap();
        assert_eq!(written.matches(BEGIN).count(), 1);
        assert_eq!(written.matches(END).count(), 1);
    }

    #[test]
    fn preserves_surrounding_content() {
        let dir = tempdir();
        let path = dir.join("CLAUDE.md");
        fs::write(&path, "# Project\n\nExisting notes.\n").unwrap();
        assert_eq!(run(Some(dir.to_str().unwrap())), ExitCode::SUCCESS);
        let written = fs::read_to_string(path).unwrap();
        assert!(written.starts_with("# Project\n\nExisting notes.\n"));
        assert!(written.contains(BEGIN));
    }

    #[test]
    fn updates_all_existing_instruction_files() {
        let dir = tempdir();
        fs::write(dir.join("CLAUDE.md"), "# Claude\n").unwrap();
        fs::write(dir.join("AGENTS.md"), "# Agents\n").unwrap();
        fs::write(dir.join("GEMINI.md"), "# Gemini\n").unwrap();
        assert_eq!(run(Some(dir.to_str().unwrap())), ExitCode::SUCCESS);
        for name in ["CLAUDE.md", "AGENTS.md", "GEMINI.md"] {
            let written = fs::read_to_string(dir.join(name)).unwrap();
            assert!(
                written.contains(BEGIN),
                "{name} should contain begin marker"
            );
            assert!(written.contains(END), "{name} should contain end marker");
        }
    }
}
