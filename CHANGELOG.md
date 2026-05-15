# Changelog

All notable changes to Argyph will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and Argyph adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Entries marked **breaking** require a major version bump.

---

## [Unreleased]

### Added

- Workspace-level crate metadata (`description`, `homepage`,
  `documentation`, `readme`, `keywords`, `categories`) inherited by
  every crate so each entry on crates.io is self-describing.
- Versioned path dependencies between workspace crates so every member
  is independently publishable to crates.io.
- `crates-io`, `npm`, and `homebrew` jobs in the release workflow:
  tag-driven publish to crates.io (dependency-ordered), tag-driven
  `npm publish` (RCs go to the `next` dist-tag), and automated SHA256
  refresh of `Formula/argyph.rb`.
- `benches/README.md` — operator guide for the criterion harness and
  pointer to `docs/benchmarks.md` as the canonical results record.
- `ask` MCP meta-tool for code, symbol, file, and content lookup with
  bounded `Span` responses.
- `expand_span` MCP tool for resolving session-scoped handles when a span is
  truncated.
- MCP Prompts: `explore_codebase`, `trace_symbol`, and `prepare_review`.
- `argyph init` agent instruction installer for `CLAUDE.md`, `AGENTS.md`, and
  `GEMINI.md`.
- Deterministic hybrid-search reranker with size penalty and module-focus
  signal hooks.
- Live (`-D warnings`) clippy gate on the snapshot tests for `argyph-pack`.
- DXT manifest now ships a real 128x128 icon.

### Changed

- CI: bumped `actions/checkout` to `v5` and pinned
  `dtolnay/rust-toolchain` to `@stable` (the `@master` action now
  requires an explicit `toolchain` input, which was breaking every
  workflow run).
- Documentation: `docs/` is no longer top-level-gitignored. Public docs
  (`SPEC.md`, `BUILD_PLAN.md`, `MODULES.md`, `BUILD_GUIDE.md`,
  `AGENT_WORKFLOW.md`, `COMMIT_CONVENTIONS.md`) are now tracked.
  Agent-prompt subdirectories (`docs/agent-prompts/`,
  `docs/superpowers/`) and the internal `plans/` / `specs/` directories
  remain local-only.
- Untracked the stray `.codegraph/` runtime directory and added it to
  `.gitignore`.
- README benchmark table now references real criterion numbers and
  cross-links the methodology in `docs/benchmarks.md`.
- Retrieval tools now expose additive universal `spans` fields and enforce
  per-span and per-response line caps at the MCP boundary.
- Tool descriptions now bias agents toward `ask` for ordinary code lookup and
  discourage broad file reads.
- Aligned all distribution channels (Cargo workspace, npm package, DXT
  manifest, Homebrew formula) on `1.0.0-rc.1`.
- `npm/postinstall.js` now reads the version from `package.json`, downloads
  the cargo-dist archive (`.tar.xz` on Unix, `.zip` on Windows), verifies the
  `.sha256` sidecar, and extracts the binary safely via `execFileSync`.
- Homebrew formula switched from in-tree `cargo install` to per-target
  prebuilt-binary install; `scripts/update-homebrew.sh` now refreshes the
  four SHA256 slots from the release artifacts.
- DXT manifest collapsed to a single `mcpServers` entry (the bundled binary).

### Fixed

- `locate` now recovers from a stale on-disk index by re-parsing the live
  file inline before serving a span; warning still surfaced for observability.
- Watcher-driven incremental reindex now refreshes Tier 1.5 structural nodes
  for changed Markdown / JSON / YAML / TOML / CSV files.
- `locate_smart` honors both `max_steps` and `max_output_tokens`; on budget
  exhaustion the MCP response surfaces the partial spans collected so far.
- `locate_smart` providers (OpenAI, Anthropic, Ollama) now use a redacting
  `ApiKey` newtype, scrub keys from upstream error bodies, and retry on 5xx
  with exponential backoff.
- `locate_smart` sub-tool `get_repo_overview` is implemented (file counts,
  total bytes, language histogram, largest files).

---

## [1.0.0-rc.1] — 2026-05-14

### Added

- `argyph-pack` crate: token-budgeted XML/Markdown repo packing.
- Priority heuristic: explicit paths → entry points → READMEs → recently
  modified → high in-edge → rest.
- `pack_repo` MCP tool.
- `locate` MCP tool: returns the smallest natural span containing a
  structured locator or natural-language query. Operates over code,
  Markdown, JSON, YAML, TOML, and CSV.
- Tier 1.5 structural index over non-code files, persisted in
  `structural_nodes` SQLite table with FTS5 over labels and joined paths.
- Optional `locate_smart` MCP tool: in-process bounded ReAct loop with an
  allowlisted four-tool surface (`locate`, `read_file_range`,
  `get_symbol_outline`, `get_repo_overview`). Off by default; requires
  `[locate_smart].enabled = true` plus a provider (OpenAI, Anthropic, or
  Ollama-compatible local endpoint).
- Memory layer: `argyph-memory` crate with `memory_save`, `memory_search`,
  `memory_list`, `memory_forget` MCP tools and a `memories` SQLite table.
- Distribution: prebuilt binaries via `cargo-dist` for
  `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`,
  `aarch64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`.
- npm wrapper `@argyph/server` with postinstall binary download and SHA-256
  verification.
- DXT bundle for one-click Claude Desktop install.
- Homebrew formula (`Formula/argyph.rb`) and `scripts/install.sh` universal
  installer.

---

## [0.3.0-beta] — 2026-04-30

### Added

- Tier 2 semantic index backed by LanceDB.
- Embedding provider abstraction with three implementations: bundled local
  ONNX (`bge-small-en-v1.5`), OpenAI, Voyage.
- Hybrid search via reciprocal-rank fusion of BM25 (SQLite FTS5) and vector
  results.
- Background Tier 2 indexing with progress reporting via `get_index_status`.
- `search_semantic` MCP tool with language and path-glob filters.
- Lazy model download to `~/.cache/argyph/models/` with SHA-256 verification.

---

## [0.2.0-alpha] — 2026-03-22

### Added

- Tier 1 symbol index with tree-sitter integration for Rust, TypeScript,
  Python.
- Symbol graph construction with calls, references, imports edges.
- AST-aware chunking with character-based fallback.
- Filesystem watcher with `notify` and debouncing; polling fallback via
  `ARGYPH_WATCHER=poll`.
- MCP tools: `find_definition`, `find_references`, `get_callers`,
  `get_callees`, `get_imports`, `get_symbol_outline`.
- Incremental updates: edited files trigger reparse and graph delta in
  <500 ms.

### Known limitations

- Cross-file symbol resolution is best-effort, per-language heuristic.
  Documented in `crates/argyph-graph/MODULE.md`.

---

## [0.1.0-alpha] — 2026-02-14

### Added

- Tier 0 filesystem index with `.gitignore`-aware walking via the `ignore`
  crate.
- BLAKE3 hashing per file.
- SQLite metadata store with WAL mode and migration runner.
- Supervisor lifecycle in `argyph-core` with cancellation token and
  `JoinSet`.
- MCP server skeleton via `rmcp`, with three tools: `get_index_status`,
  `get_repo_overview`, `search_text`.
- CLI entry point with `serve`, `doctor`, `--version`, `init` subcommands.
- CI matrix for macOS, Linux, Windows on x64 and arm64 where supported.

---

[Unreleased]: https://github.com/Ezzy1630/argyph/compare/v1.0.0-rc.1...HEAD
[1.0.0-rc.1]: https://github.com/Ezzy1630/argyph/releases/tag/v1.0.0-rc.1
[0.3.0-beta]: https://github.com/Ezzy1630/argyph/releases/tag/v0.3.0-beta
[0.2.0-alpha]: https://github.com/Ezzy1630/argyph/releases/tag/v0.2.0-alpha
[0.1.0-alpha]: https://github.com/Ezzy1630/argyph/releases/tag/v0.1.0-alpha
