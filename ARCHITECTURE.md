# Argyph Architecture

This document is the canonical source of truth for how Argyph is built. Every agent prompt and every PR review references it. If you find yourself implementing something that is not described here, stop and update this file first.

---

## 1. Goals and constraints

Argyph is a local-first MCP server that gives an AI coding agent fast, structured, and semantic context over a codebase. The architecture is shaped by five constraints:

1. **The server must be useful before it is fully indexed.** Cold-starting a new repo cannot block the agent for minutes. We solve this with a three-tier progressive indexing model where each tier comes online independently.
2. **The server must run entirely on the developer's machine.** No cloud dependencies for full functionality, no daemon, no account, no API key required to get started.
3. **The server must be easy for AI agents to extend.** The codebase is a polyglot of small, sharply-defined modules with trait interfaces. Adding a new MCP tool, language pack, or embedding provider is a sub-300-line change in one file.
4. **The server must be a single static binary.** Distribution via npm, cargo, brew, and DXT collapses to "download the right prebuilt binary."
5. **The server must be read-only.** Argyph never writes files, runs shells, mutates git, or otherwise touches the user's environment beyond the on-disk index in `.argyph/`.

---

## 2. Three-tier progressive indexing

The single most important design decision in Argyph. Each tier is independently useful; the server marks each tier ready as it completes; tools that need a higher tier than is available either degrade gracefully or return an `INDEX_NOT_READY` error with a `retry_after_ms` hint.

### Tier 0 — Filesystem index

- **Wall-clock target:** < 1 second on cold start for a 1M-LOC repo, sub-second on warm restart.
- **What it produces:** A list of `FileEntry { path, hash, language, size }` records, persisted to SQLite. Honors `.gitignore` via the `ignore` crate.
- **Tools enabled:** `get_repo_overview`, `search_text`, `read_file_range`, basic `pack_repo`.

### Tier 1 — Symbol index

- **Wall-clock target:** Seconds to ~30 seconds on a 1M-LOC repo. Embarrassingly parallel via rayon.
- **What it produces:** Tree-sitter ASTs, `Symbol` records (functions, classes, methods, imports, exports), `Chunk` records (AST-aware with character fallback), `Edge` records (defs, refs, calls, imports). Persisted to SQLite.
- **Tools enabled:** `find_definition`, `find_references`, `get_callers`, `get_callees`, `get_imports`, `get_symbol_outline`, full `pack_repo`.
- **Honest limitation:** Cross-file resolution is best-effort, not LSP-precise. Intra-file accuracy is high; cross-file uses per-language module-resolution heuristics. This is documented prominently in `crates/argyph-graph/MODULE.md`.

### Tier 1.5 — Structural index (non-code)

- **Wall-clock target:** Seconds. Runs after Tier 1, in parallel with Tier 2.
- **What it produces:** `StructuralNode` trees for markdown, JSON, YAML, TOML, CSV. Stored in `structural_nodes` SQLite table with an FTS5 index over labels and paths.
- **Tools enabled:** `locate`.
- **Size threshold:** Files above `ARGYPH_LOCATE_MAX_FILE_BYTES` (default 10 MB) are not pre-indexed; they get scan-on-demand treatment with LRU caching.

### Tier 2 — Semantic index

- **Wall-clock target:** Minutes (or longer for huge repos), runs in background without blocking queries.
- **What it produces:** Embeddings for each chunk, stored in LanceDB. Hybrid BM25 + vector search via reciprocal rank fusion.
- **Tools enabled:** `search_semantic`. Returns partial-index results with `index_coverage` field while building.

### Optional layer — `locate_smart`

Sits above the three tiers. An in-process bounded ReAct loop that dispatches to the four read-only sub-tools (`locate`, `read_file_range`, `get_symbol_outline`, `get_repo_overview`). Off by default. When enabled, validates that every span returned to the caller came from a `locate` call made earlier in the same loop — so the model cannot fabricate byte ranges. Provider abstraction (`LocateModel` trait) supports OpenAI, Anthropic, and Ollama-compatible local endpoints.

### Meta-tool layer — `ask` and `Span`

`argyph-mcp` exposes `ask` as the default lookup entry point for agents. The router selects symbol definition lookup for bare identifiers, structural `locate` for path/glob/line-style locators, `locate_smart` when explicitly requested, and semantic search for natural-language questions.

Retrieval tools now add a universal `Span` array beside legacy fields. Each span carries file, line range, byte range, text, kind, optional symbol/language/score metadata, and truncation state. The MCP boundary enforces `ARGYPH_MAX_SPAN_LINES` (default 80) and `ARGYPH_MAX_TOTAL_LINES` (default 400) so a single tool call cannot flood an agent's context; truncated spans receive a session-scoped `expand_handle` that `expand_span` can resolve within 10 minutes.

### Why this matters

Most agent queries are structural — "where is `parseConfig` defined?", "what calls `validateUser`?", "show me imports of this file". Those are Tier 1 queries served in milliseconds. Embeddings are only needed for fuzzy semantic queries ("find code that handles auth"). Even on a large repo where Tier 2 takes 10 minutes, the server feels instant for ~70% of real queries.

---

## 3. High-level diagram

```
                ┌─────────────────────────────────────────┐
                │  AI Agent (Claude Code, Codex, Cursor)  │
                └────────────────┬────────────────────────┘
                                 │ MCP / stdio (JSON-RPC)
                ┌────────────────▼────────────────────────┐
                │       argyph-mcp (rmcp)                 │
                │   ─────────────────────────────         │
                │   tool handlers (thin)                  │
                └────────────────┬────────────────────────┘
                                 │
        ┌────────────┬───────────┼───────────┬─────────────┬────────────┐
        │            │           │           │             │            │
   ┌────▼────┐  ┌────▼────┐ ┌────▼────┐ ┌────▼────┐  ┌────▼─────┐  ┌────▼─────┐
   │ argyph- │  │ argyph- │ │ argyph- │ │ argyph- │  │ argyph-  │  │ argyph-  │
   │  fs     │  │ parse   │ │ graph   │ │ embed   │  │ store    │  │ locate   │
   │         │  │         │ │         │ │         │  │          │  │          │
   │ walking │  │ tree-   │ │ symbol  │ │ ONNX +  │  │ LanceDB  │  │ structural│
   │ ignore  │  │ sitter  │ │ resolve │ │ HTTP    │  │ + SQLite │  │  search  │
   │ watcher │  │ chunks  │ │  edges  │ │ providers│ │  meta    │  │  + path  │
   └─────────┘  └─────────┘ └─────────┘ └─────────┘  └──────────┘  └──────────┘
                                 ▲
                ┌────────────────┴────────────────────────┐
                │       argyph-core (Supervisor)          │
                │   orchestrates 3-tier indexing,         │
                │   owns lifecycle, scheduling, tasks     │
                └─────────────────────────────────────────┘
                ┌─────────────────────────────────────────┐
                │       argyph-pack                       │
                │   repo packing (uses fs+parse+graph)    │
                └─────────────────────────────────────────┘
```

---

## 4. Crate map

Argyph is a Cargo workspace. Each crate has a single, sharply-defined responsibility. Per-crate ownership is documented in `crates/<name>/MODULE.md`.

| Crate            | Owns                                                                                              | Must NEVER own                                |
|------------------|---------------------------------------------------------------------------------------------------|-----------------------------------------------|
| `argyph-fs`      | Filesystem walking, ignore rules, file-watching (notify), content hashing, file metadata          | Parsing, embedding, query logic               |
| `argyph-parse`   | Tree-sitter parsing, language-pack registration, chunking strategy, symbol extraction             | Persistence, queries beyond the AST           |
| `argyph-graph`   | Symbol graph construction, edge resolution (defs, refs, calls, imports), graph queries            | Parsing, embeddings, packing                  |
| `argyph-embed`   | Embedding provider abstraction, ONNX bundled model, HTTP providers (OpenAI, Voyage, local)        | Storage, retrieval, parsing                   |
| `argyph-store`   | LanceDB integration, SQLite metadata, schema, migrations, hybrid search query                     | Embedding generation, parsing, MCP            |
| `argyph-pack`    | Repo packing, format rendering (XML, markdown), token-budgeted prioritization                     | Indexing or storage                           |
| `argyph-core`    | Supervisor lifecycle, three-tier orchestration, configuration, task scheduling, the `Index` facade| MCP protocol, individual storage details      |
| `argyph-mcp`     | MCP server, tool handlers, request/response types, schema validation                              | Any business logic — handlers must be <100 LOC |
| `argyph-cli`     | CLI commands, output formatting, progress bars                                                    | Anything reusable (it lives in core)          |
| `argyph`         | Single binary entry point. Just dispatches to `serve` (MCP) or CLI subcommands                    | Logic                                         |

This is a deliberately strict separation. PRs that mix concerns across crate boundaries are rejected. See `docs/MODULES.md` for the rationale and contribution recipes.

---

## 5. Indexing data flow

```
┌──────────────────────────────────────────────────────────┐
│ Tier 0 (sync, <1s)                                       │
│ fs::walk(root) → [FileEntry { path, hash, language }]   │
│ → store::upsert_file_meta(...)                          │
└──────────────┬───────────────────────────────────────────┘
               │ done; supervisor marks Tier 0 ready
               ▼
┌──────────────────────────────────────────────────────────┐
│ Tier 1 (parallel, seconds)                               │
│ for each FileEntry (parallel via rayon):                 │
│   parse::parse_file(path) → AST                          │
│   parse::extract_symbols(AST) → [Symbol]                 │
│   parse::chunk(AST) → [Chunk]                            │
│ graph::build_edges([Symbol], [Chunk]) → [Edge]           │
│ store::upsert_symbols_and_chunks_and_edges(...)          │
└──────────────┬───────────────────────────────────────────┘
               │ done; supervisor marks Tier 1 ready
               ▼
┌──────────────────────────────────────────────────────────┐
│ Tier 2 (background, minutes)                             │
│ for each Chunk where embedding is missing:               │
│   embed::embed(chunk.text) → vector                      │
│   store::upsert_vector(chunk_id, vector)                 │
│ Reports progress via core::index_status                  │
└──────────────────────────────────────────────────────────┘
```

Incremental updates: a `notify`-based filesystem watcher with debouncing queues changed paths. The Supervisor walks the queue and re-runs the relevant tier subset for those paths only. Content-addressed chunk IDs (BLAKE3 of content) keep writes idempotent.

---

## 6. The Supervisor lifecycle

`argyph-core::Supervisor` is the single owner of runtime state. It:

- Boots the index from `.argyph/` if present.
- Runs Tier 0 synchronously, marks ready.
- Spawns Tier 1 onto a rayon thread pool, marks ready when complete.
- Spawns Tier 2 onto a tokio task pool with provider-specific concurrency caps.
- Owns the `notify` watcher and queues incremental work.
- Owns a `CancellationToken` and a `JoinSet`; graceful shutdown drains both.

```rust
pub struct Supervisor {
    config:     Arc<Config>,
    index:      Arc<Index>,            // facade over store + caches
    tier_state: Arc<RwLock<TierState>>,
    tasks:      JoinSet<()>,
    watcher:    Option<FsWatcher>,
    shutdown:   CancellationToken,
}

impl Supervisor {
    pub async fn boot(root: PathBuf, config: Config) -> Result<Self> { ... }
    pub async fn run(self) -> Result<()> { ... }
    pub fn index(&self) -> Arc<Index> { ... }
    pub async fn shutdown(self) -> Result<()> { ... }
}
```

**Architectural rule (enforced by code review):** No other module spawns long-lived tasks. All background work goes through `Supervisor::spawn(...)`, which registers the task in the `JoinSet` and ties it to the cancellation token. This is the single most important rule for keeping the codebase maintainable.

---

## 7. Interface contracts

Trait-based interfaces let us mock everything in tests, swap implementations later, and constrain agent contributions to small surface areas.

```rust
// argyph-fs
pub trait Walker {
    fn walk(&self, root: &Path) -> impl Iterator<Item = FileEntry>;
}
pub struct FileEntry {
    pub path: Utf8PathBuf,
    pub hash: Blake3Hash,
    pub language: Option<Language>,
    pub size: u64,
}

// argyph-parse
pub trait Parser {
    fn parse(&self, file: &FileEntry, source: &str) -> Result<ParsedFile>;
}
pub struct ParsedFile {
    pub symbols: Vec<Symbol>,
    pub chunks: Vec<Chunk>,
    pub imports: Vec<Import>,
}

// argyph-embed
#[async_trait]
pub trait Embedder: Send + Sync {
    fn dimension(&self) -> usize;
    fn model_id(&self) -> &str;
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}

// argyph-store
#[async_trait]
pub trait Store: Send + Sync {
    async fn upsert_files(&self, files: &[FileEntry]) -> Result<()>;
    async fn upsert_chunks(&self, chunks: &[ChunkWithEmbedding]) -> Result<()>;
    async fn upsert_symbols(&self, symbols: &[Symbol]) -> Result<()>;
    async fn upsert_edges(&self, edges: &[Edge]) -> Result<()>;
    async fn search_hybrid(
        &self,
        query: &str,
        query_vec: &[f32],
        k: usize,
        filter: Filter,
    ) -> Result<Vec<SearchHit>>;
    async fn find_symbol(&self, name: &str, scope: Option<&str>) -> Result<Vec<Symbol>>;
    async fn find_references(&self, symbol_id: SymbolId) -> Result<Vec<Reference>>;
}
```

---

## 8. Configuration

Layered, in priority order (highest wins):

1. Env vars (`ARGYPH_*`)
2. `.argyph/config.toml` in the repo root
3. Built-in defaults

API keys for embedding providers use provider-standard names (`OPENAI_API_KEY`, `VOYAGE_API_KEY`) — never reinvented in our namespace.

```toml
# .argyph/config.toml
[index]
exclude = ["docs/generated/**", "**/*.min.js"]
languages = ["rust", "typescript", "python"]

[embed]
provider = "local"        # "local" | "openai" | "voyage"
model = "bge-small-en-v1.5"

[search]
hybrid_alpha = 0.5

[pack]
default_token_budget = 50000
```

---

## 9. CLI

The same binary serves as CLI and MCP server:

```
argyph index .                     # Force full reindex
argyph status                      # Show index state, tiers, last update
argyph search "auth middleware"    # Semantic search from terminal
argyph symbols path/to/file.rs     # List symbols in a file
argyph graph callers parseConfig   # Find callers of a symbol
argyph pack --budget 30000 src/    # Pack a subset to stdout
argyph serve                       # Run as MCP server (stdio)
argyph doctor                      # Diagnose env, model files, perms
argyph init                        # Install agent lookup instructions
```

The CLI gives users a way to debug what the agent sees and gives us a free integration testing surface.

---

## 10. Distribution

Single binary built per platform, distributed through:

- **npm** — postinstall script downloads the right binary from GitHub Releases. Same model as `esbuild` and `swc`.
- **cargo install argyph** — works on any Rust-installed system.
- **DXT** — `argyph.dxt` for one-click Claude Desktop install.
- **Homebrew** — tap added in v1.1.

Build matrix: `darwin-arm64`, `darwin-x64`, `linux-x64`, `linux-arm64`, `win32-x64`. CI uses `cargo-dist` to generate binaries, installer scripts, and the npm wrapper from a single config.

---

## 11. Alternatives considered

### Vector store: LanceDB vs sqlite-vec vs Qdrant embedded

**Chosen: LanceDB.** Embedded columnar store with native hybrid search and incremental updates. Rust-native, no daemon, single-file-style storage. Younger than alternatives, but the ergonomics win is large.

- *sqlite-vec:* simpler and more ubiquitous, but weaker for hybrid search and harder to scale to millions of chunks.
- *Qdrant embedded:* capable but heavier; designed for server use even in embedded mode.
- *tantivy + custom vector index:* maximum control, but it's plumbing instead of features.

If LanceDB ever bites us seriously, the `Store` trait makes a swap mechanical. The substitution risk is bounded.

### Pure Rust vs hybrid Rust + TypeScript MCP shell

**Chosen: pure Rust with `rmcp`.** Single binary, faster startup, simpler ops, distributed via npm with a thin postinstall script that downloads the right prebuilt binary. Stronger portfolio signal than a TS wrapper.

The cost is asking AI agents to write more Rust. We mitigate this with small modules, tight trait contracts, and the agent workflow rules in `docs/AGENT_WORKFLOW.md`.

### Embedding: bundled local vs remote-only

**Chosen: bundled local as default, remote providers as upgrade.** The `bge-small-en-v1.5` int8-quantized model is ~80MB downloaded lazily on first index. Search quality is "good enough" for the structural-query majority case; users who care about top-tier semantic quality on prose-like queries configure `OPENAI_API_KEY` or `VOYAGE_API_KEY`.

We considered a code-specific model (`nomic-embed-code-v1`) and may swap later — the `Embedder` trait makes this a drop-in.

### Languages in v1.0

**Chosen: Rust + TypeScript + Python.** Covers the modal Claude Code user. Adding a language is a separate, atomic PR (one tree-sitter dep + one .scm query file + tests). Java, Go, Kotlin, Swift, Ruby targeted for v1.1.

---

## 12. Honest limitations

These are documented prominently in the relevant `MODULE.md` files; agents must not paper over them.

1. **Cross-file symbol resolution is best-effort.** Per-language heuristics, not type-resolved IRs. Documented in `crates/argyph-graph/MODULE.md`. Long-term fix is the LSP-bridge, scoped for v2.
2. **Bundled local embedding is not OpenAI-quality on prose queries.** Hybrid search lifts this somewhat. Documented in `crates/argyph-embed/MODULE.md`.
3. **First-time indexing of a huge repo takes minutes.** "Cold start <1s" applies to warm restarts. Documented in the README.
4. **Watcher reliability varies by OS.** macOS FSEvents, Linux inotify, and Windows ReadDirectoryChangesW all have known quirks. Polling fallback (`ARGYPH_WATCHER=poll`) is always available.
5. **MCP protocol surface is tools-only in v1.0.** Resources and Prompts are deferred to v1.1 based on demand.

---

## 13. Where agents may modify freely

- `crates/argyph-mcp/src/tools/` — adding a new tool is a normal contribution.
- `crates/argyph-parse/src/languages/` — adding a language pack.
- `crates/argyph-embed/src/` — adding a new embedding provider.
- Test files anywhere.
- `docs/`, `examples/`.

## 14. Where agents must NOT modify without human review

These are architecture-protected, enforced by `CODEOWNERS`:

- `crates/argyph-core/src/supervisor.rs` — lifecycle changes.
- `crates/argyph-store/src/schema.rs` and `crates/argyph-store/src/migrations/` — schema changes are migrations, never edits.
- Workspace `Cargo.toml` dependency versions.
- `dist-workspace.toml`, `.github/workflows/release.yml`.
- All `MODULE.md` files.
- This file.

---

## 15. Versioning

- 0.x: API may change between minors.
- 1.0: MCP tool schemas locked. Adding a tool is minor; removing or breaking a required field is major.
- The MCP tool schema gets its own version reported via `get_index_status` so agents can adapt.
