# Argyph Build Guide

This is the operational guide for taking the docs in this repo from "spec" to "shipped MCP server." It covers, in order:

1. [Creating the GitHub repo](#1-creating-the-github-repo)
2. [Initial local setup](#2-initial-local-setup)
3. [The full MVP prompt sequence (Phases 0–5)](#3-mvp-prompt-sequence)
4. [Post-MVP upgrade prompts](#4-post-mvp-upgrade-prompts)
5. [Working rhythm and review checklist](#5-working-rhythm-and-review-checklist)

Each prompt is meant to be copy-pasted (with minor edits) into Claude Code (or another agent) inside a fresh chat scoped to a single crate. The order matters; do not skip ahead.

---

## 1. Creating the GitHub Repo

### Settings

| Setting                                | Value                                                              |
|----------------------------------------|--------------------------------------------------------------------|
| **Owner**                              | `Ezzy1630`                                                         |
| **Repository name**                    | `argyph`                                                           |
| **Description**                        | `Local-first MCP server giving AI coding agents fast, structured, and semantic context over any codebase. Zero config, zero cloud, full context.` |
| **Visibility**                         | Public                                                             |
| **Initialize with README?**            | **No.** You already have a richer one in this folder.              |
| **Add .gitignore?**                    | **No.** Already in this folder.                                    |
| **Choose a license?**                  | **No.** Dual-licensed manually with `LICENSE-MIT` + `LICENSE-APACHE` already in this folder. |
| **Default branch**                     | `main`                                                             |

### Topics (after creating)

Set the following repository topics so it shows up in searches:

```
mcp, mcp-server, model-context-protocol, rust, claude, claude-code,
ai-coding, code-search, semantic-search, tree-sitter, lancedb,
embeddings, code-intelligence, local-first
```

### Repository settings to enable after creation

In **Settings → General**:
- Uncheck "Wikis"
- Uncheck "Projects" (you'll use GitHub Projects v2 separately if needed)
- Check "Discussions"
- Check "Issues"
- Pull requests: keep only **Allow squash merging**. Disable merge commits and rebase merging.
- Check "Automatically delete head branches"

In **Settings → Branches → Branch protection rules**, add a rule for `main`:
- Require a pull request before merging (1 approval; or 0 since you're solo — but keep "Require status checks to pass")
- Require status checks: `fmt`, `clippy`, `test (ubuntu-latest)`, `test (macos-latest)`, `test (windows-latest)`
- Require conversation resolution before merging
- Do not allow force pushes; do not allow deletions

In **Settings → Code security**:
- Enable Dependabot alerts
- Enable Dependabot version updates (you'll add `dependabot.yml` in Phase 0)
- Enable secret scanning

In **Settings → Actions → General**:
- Allow GitHub Actions
- Workflow permissions: Read and write (needed for releases later)

---

## 2. Initial Local Setup

```bash
# 1. Make a working directory for the project on your machine
mkdir -p ~/code/argyph
cd ~/code/argyph

# 2. Copy every file from this Cowork folder into it
# (Files to copy are listed in §6 below — everything except audit.jsonl,
# outputs/, uploads/, and .claude/, which are Cowork session artifacts.)

# 3. Initialize git
git init -b main
git add .
git status     # sanity check — should NOT include audit.jsonl, outputs/, uploads/

# 4. First commit (note: no Co-authored-by trailer — see docs/COMMIT_CONVENTIONS.md)
git commit -m "docs: initial project specification, architecture, and module ownership

Full source-of-truth documentation set: README, ARCHITECTURE, SPEC,
BUILD_PLAN, AGENT_WORKFLOW, COMMIT_CONVENTIONS, tools-reference, and
per-crate MODULE.md files. No implementation yet — that begins with
Phase 0."

# 5. Add the GitHub remote
git remote add origin https://github.com/Ezzy1630/argyph.git
git branch -M main
git push -u origin main
```

You now have a documented, empty-of-code repo on GitHub. Every subsequent change happens on a feature branch and lands via PR.

---

## 3. MVP Prompt Sequence

### How to use each prompt

1. Create a feature branch: `git switch -c <branch-name>` (suggested branch names listed per milestone)
2. Open a fresh Claude Code chat in the repo root.
3. Paste the prompt under that milestone verbatim, edited only where marked `[FILL IN]`.
4. Let the agent propose signatures first, review them, then implement.
5. Run `cargo fmt && cargo clippy -- -D warnings && cargo test` locally.
6. Open a PR. Use the squash-merge subject from the milestone description.
7. Verify CI green, merge, delete branch, tag if the milestone is a release gate.

**Important: the prompt template referenced by each milestone is at `docs/agent-prompts/template.md`. Each milestone prompt below builds on that template.**

---

### Phase 0 — Skeleton

#### M0.1 — Workspace + CI

**Branch:** `m0.1-workspace-ci`
**Goal:** Cargo workspace with 10 empty crates, green CI on 3 OSes.

```
ROLE: You are setting up the initial Cargo workspace for the Argyph MCP server. No business logic yet — pure scaffolding.

CONTEXT: Read these files first, in this order:
- README.md
- ARCHITECTURE.md
- docs/SPEC.md (the abridged source-of-truth spec)
- docs/MODULES.md (the per-crate ownership map)
- CONTRIBUTING.md §3 (lint rules) and §4 (AI Agent Rules)

TASK (M0.1): Create:
1. A workspace Cargo.toml that includes 10 member crates under crates/:
   argyph, argyph-core, argyph-fs, argyph-parse, argyph-graph,
   argyph-embed, argyph-store, argyph-pack, argyph-mcp, argyph-cli.
   Each member crate is an empty lib (except argyph which is bin).
   Each lib.rs declares one no-op trait that names the crate's primary
   responsibility (e.g., argyph-fs::Walker, argyph-store::Store) per
   that crate's MODULE.md.
2. A workspace-level [workspace.lints] section enforcing:
   unsafe_code = "forbid" (with allowlisted exception path documented for argyph-embed only)
   clippy::unwrap_used = "deny"
   clippy::expect_used = "warn"
3. rust-toolchain.toml pinned to a current stable (e.g., 1.83.0).
4. deny.toml configured for cargo-deny with sensible defaults.
5. .github/workflows/ci.yml with a job matrix:
   - os: [ubuntu-latest, macos-latest, windows-latest]
   - jobs: fmt (cargo fmt --check), clippy (cargo clippy -- -D warnings), test (cargo test --workspace)
   - cache cargo registry + target
6. A custom CI check that fails if any *.rs file exceeds 600 lines.
7. .github/dependabot.yml for daily updates of Cargo and GitHub Actions deps.

CONSTRAINTS:
- No real logic. Every crate has a single trait + a single TODO comment per its MODULE.md.
- No new top-level dependencies beyond `thiserror` per crate, and dev-deps as needed.
- All crates re-export their public types via `pub use` in lib.rs.

PROCESS:
1. Propose the file tree you intend to create — STOP and wait.
2. Propose the workspace Cargo.toml — STOP and wait.
3. Implement the rest.
4. Run `cargo build --workspace` and `cargo test --workspace` locally; report any failures.

DELIVERABLE: A PR titled `feat(build): cargo workspace + CI matrix + lint baseline`. Empty trait stubs and green CI on all 3 OSes.
```

**Commit subject (squash):** `feat(build): cargo workspace + CI matrix + lint baseline`

---

#### M0.2 — Binary + CLI dispatch

**Branch:** `m0.2-cli-dispatch`

```
ROLE: You are implementing argyph-cli command dispatch and the main binary in the `argyph` crate.

CONTEXT: Read:
- crates/argyph/MODULE.md
- crates/argyph-cli/MODULE.md
- ARCHITECTURE.md §"Crate breakdown"

TASK (M0.2): Implement:
1. `argyph-cli` with `clap` v4 (derive) defining these subcommands as stubs:
   serve, index, status, search, symbols, graph, pack, doctor, init.
2. `argyph` (binary) main.rs that initializes tracing-subscriber from `ARGYPH_LOG` env, then dispatches to argyph-cli.
3. `argyph doctor` actually does something: prints platform info (OS, arch, Rust version), Argyph version, and "OK".
4. `argyph --version` prints the version from Cargo.toml.

CONSTRAINTS: No other subcommand may do real work yet. Each one prints "not implemented in this milestone" and exits 0.

PROCESS:
1. Propose the clap command structure — STOP and wait.
2. Implement.
3. Manually run `cargo run -- doctor` and `cargo run -- --version`; report output.

DELIVERABLE: PR `feat(cli): top-level command dispatch + argyph doctor`.
```

**🎯 Gate:** End of Phase 0. Tag `v0.0.1-alpha` (just to prove the release ceremony works locally).

---

### Phase 1 — Tier 0: Filesystem

#### M1.1 — argyph-fs walker

**Branch:** `m1.1-fs-walker`

```
ROLE: Implement the filesystem walker in argyph-fs.

CONTEXT: Read crates/argyph-fs/MODULE.md and the data-flow section of ARCHITECTURE.md.

TASK (M1.1): Implement:
1. `Walker` trait per MODULE.md.
2. A concrete `IgnoreWalker` using the `ignore` crate (from ripgrep). Respects .gitignore, .ignore, global gitignore, and a configurable allowlist.
3. `FileEntry { path: Utf8PathBuf, hash: Blake3Hash, language: Option<Language>, size: u64 }`.
4. Hash computation with `blake3` (parallelized via `rayon`).
5. Language detection from extensions (start with a small static table: rs, ts, tsx, py, js, jsx, md).
6. Unit tests against a fixture in `examples/tiny-rust-app/` (you create this fixture).

CONSTRAINTS:
- No dependencies beyond ignore, blake3, camino, rayon, thiserror.
- All paths handled as Utf8PathBuf (camino), never PathBuf.
- No public function undocumented.
- Hard size cap: skip files > 5MB by default (configurable in a later milestone).

DELIVERABLE: PR `feat(fs): ignore-aware filesystem walker with blake3 hashing`.
```

---

#### M1.2 — argyph-store: SQLite for file metadata

**Branch:** `m1.2-store-files`

```
ROLE: Implement the initial argyph-store layer covering file metadata only.

CONTEXT: Read crates/argyph-store/MODULE.md, especially the on-disk layout section.

TASK (M1.2): Implement:
1. The `Store` trait subset for files: `upsert_files`, `get_file`, `list_files`, `delete_file`.
2. SQLite backend via `rusqlite` with WAL mode.
3. Schema in `crates/argyph-store/src/schema.rs`: a `files` table with columns id, path, hash, language, size, last_seen.
4. Migration system using `refinery` (or hand-rolled if you prefer). One migration file: `001_initial_files.sql`.
5. On-disk layout under `.argyph/`: `meta.sqlite`.
6. Integration tests round-tripping a few hundred file entries.

CONSTRAINTS:
- NEVER edit migrations after they're merged. New schema changes = new migration files.
- All errors typed with thiserror at the crate boundary.

DELIVERABLE: PR `feat(store): SQLite file metadata table with migrations`.
```

---

#### M1.3 — argyph-core Supervisor + Tier 0

**Branch:** `m1.3-supervisor-tier0`

```
ROLE: Implement the Supervisor lifecycle and wire up Tier 0 indexing.

CONTEXT: Read crates/argyph-core/MODULE.md and ARCHITECTURE.md §"Lifecycle (the Supervisor)".

TASK (M1.3): Implement:
1. `Supervisor` struct per the architecture: boot, run, shutdown, with a CancellationToken.
2. `Index` facade in argyph-core that wraps the Store.
3. The boot flow: walk repo (argyph-fs) → upsert files (argyph-store) → mark Tier 0 ready.
4. A `TierState` enum and `get_tier_state` accessor.
5. `tracing` instrumentation on the boot flow.
6. Integration test: boot against examples/tiny-rust-app/, assert Tier 0 ready in <1s.

CONSTRAINTS:
- No background spawn outside Supervisor::spawn.
- No public access to underlying Store; everything goes through Index.

DELIVERABLE: PR `feat(core): Supervisor lifecycle + Tier 0 boot flow`.
```

---

#### M1.4 — argyph-mcp skeleton + first three tools

**Branch:** `m1.4-mcp-skeleton`

```
ROLE: Implement the MCP server skeleton and three Tier 0 tools.

CONTEXT: Read crates/argyph-mcp/MODULE.md, docs/tools-reference.md (the schemas for the three tools you'll implement), and ARCHITECTURE.md §"Crate breakdown".

TASK (M1.4): Implement:
1. argyph-mcp using the `rmcp` crate (official Rust MCP SDK).
2. JSON Schema validation at the boundary for every tool's request/response.
3. Three tool handlers, each in its own file under src/tools/, each <100 lines:
   - get_index_status
   - get_repo_overview
   - search_text  (use the `grep` crate or shell out to `rg` — your call, justify in PR)
4. Wire `argyph serve` (in argyph-cli) to actually start the MCP server bound to stdio.
5. End-to-end smoke test: spawn `argyph serve` as a subprocess and exchange three MCP messages.

CONSTRAINTS:
- Handler logic stays under 100 lines per tool. Heavy work goes into core/store/etc.
- Tools must handle the "index not ready" case by returning INDEX_NOT_READY (retryable: true).
- NEVER write logs to stdout — stdio is the MCP channel. Use stderr only.

DELIVERABLE: PR `feat(mcp): rmcp integration + three Tier 0 tools`.
```

**🎯 Gate:** End of Phase 1. Argyph is now usable as a basic file-aware MCP server. Tag `v0.1.0-alpha`.

Verify the gate works:
```bash
claude mcp add argyph -- cargo run --release -- serve
claude
# In chat: "Run get_repo_overview." Should return tree + languages.
```

---

### Phase 2 — Tier 1: Symbol Graph

#### M2.1 — argyph-parse tree-sitter integration

**Branch:** `m2.1-parse-treesitter`

```
ROLE: Implement tree-sitter parsing for Rust, TypeScript, and Python.

CONTEXT: Read crates/argyph-parse/MODULE.md.

TASK (M2.1): Implement:
1. `Parser` trait per MODULE.md.
2. One implementor per language: RustParser, TypeScriptParser, PythonParser, each in src/languages/.
3. Symbol extraction via tree-sitter `.scm` query files committed at crates/argyph-parse/queries/{rust,typescript,python}.scm.
4. `Symbol`, `Chunk`, `Import` types per the MODULE.md.
5. AST-aware chunking that respects function/class/struct boundaries, with a character-based fallback for nodes too large to use whole.
6. Unit tests against `examples/tiny-rust-app/`, `examples/tiny-ts-app/`, `examples/tiny-py-app/` — create these fixtures.
7. Aim for >95% symbol extraction on the fixtures.

CONSTRAINTS:
- One language per file under src/languages/.
- Tree-sitter grammar versions pinned in Cargo.toml.
- No parser file > 400 lines.

DELIVERABLE: PR `feat(parse): tree-sitter parsers for Rust, TypeScript, Python`.
```

---

#### M2.2 — argyph-graph symbol graph + edges

**Branch:** `m2.2-graph-edges`

```
ROLE: Implement the symbol graph and edge resolution.

CONTEXT: Read crates/argyph-graph/MODULE.md. NOTE: cross-file resolution is best-effort, not LSP-precise. Document accuracy bounds in MODULE.md.

TASK (M2.2): Implement:
1. Symbol ID assignment (content-addressed, stable across runs).
2. Edge types: Defines, References, Calls, Imports.
3. Within-file resolution first; cross-file via fully-qualified-name + module-path heuristics per language.
4. A `Graph` facade with: find_definition, find_references, get_callers, get_callees, get_imports, get_symbol_outline.
5. Unit tests on each fixture covering each edge type.

CONSTRAINTS:
- Be honest in MODULE.md about cross-file resolution accuracy ("v1 is best-effort, not LSP-precise").
- Pure data structures + queries; no I/O in this crate.

DELIVERABLE: PR `feat(graph): symbol graph with intra-file edges and best-effort cross-file resolution`.
```

---

#### M2.3 — Storage extension for symbols, chunks, edges

**Branch:** `m2.3-store-symbols`

```
ROLE: Extend argyph-store to persist symbols, chunks, and edges.

CONTEXT: Read crates/argyph-store/MODULE.md.

TASK (M2.3): Implement:
1. New migration `002_symbols_chunks_edges.sql` adding tables: symbols, chunks, edges. Indexes for fast lookups by name, file, symbol_id.
2. SQLite FTS5 virtual table over chunks.text for the BM25 side of hybrid search (Phase 3 will use it).
3. New Store methods: upsert_symbols, upsert_chunks, upsert_edges, find_symbol, find_references, get_callers, get_callees, get_imports, get_symbol_outline.
4. Integration tests round-tripping the full symbol/chunk/edge set on a fixture.

CONSTRAINTS: NEVER edit migration 001. This is migration 002.

DELIVERABLE: PR `feat(store): symbols + chunks + edges schema with FTS5`.
```

---

#### M2.4 — MCP graph tools

**Branch:** `m2.4-mcp-graph-tools`

```
ROLE: Wire up the symbol-graph MCP tools.

CONTEXT: Read docs/tools-reference.md sections for find_definition, find_references, get_callers, get_callees, get_imports, get_symbol_outline.

TASK (M2.4): Implement each as a separate handler under crates/argyph-mcp/src/tools/, <100 lines each. Update get_index_status to also report Tier 1 readiness.

CONSTRAINTS: Every tool handles the INDEX_NOT_READY case gracefully.

DELIVERABLE: PR `feat(mcp): symbol-graph query tools`.
```

---

#### M2.5 — Incremental updates + filesystem watcher

**Branch:** `m2.5-watcher`

```
ROLE: Add filesystem watching and incremental reindexing.

CONTEXT: Read ARCHITECTURE.md §"Lifecycle" and SECURITY.md §"Filesystem watcher abuse".

TASK (M2.5): Implement:
1. `notify` watcher with debouncing (500ms).
2. ARGYPH_WATCHER=poll fallback mode.
3. Incremental reindex: on file change, re-parse only that file; update graph deltas; reuse stable symbol_ids when possible.
4. Hard cap on reindex events per minute (default 60); above threshold, fall back to periodic polling.
5. Integration test: edit a fixture file, assert index reflects change in <500ms.

CONSTRAINTS:
- Watcher runs as a Supervisor task — no ad-hoc spawn.
- inotify watch counts on Linux: handle ENOSPC gracefully by falling back to poll.

DELIVERABLE: PR `feat(core): filesystem watcher with debouncing + poll fallback`.
```

**🎯 Gate:** End of Phase 2. Tag `v0.2.0-alpha`. Full structural intelligence — no embeddings yet.

---

### Phase 3 — Tier 2: Semantic Search

#### M3.1 — Embed trait + OpenAI provider

**Branch:** `m3.1-embed-openai`

```
ROLE: Implement the Embedder trait and the OpenAI provider.

CONTEXT: Read crates/argyph-embed/MODULE.md.

TASK (M3.1): Implement:
1. `Embedder` async trait per MODULE.md.
2. OpenAI implementation using `reqwest`. Reads OPENAI_API_KEY from env.
3. Batching (default 100 inputs per call), exponential backoff retries, rate limiting.
4. An `ApiKey` newtype with a redacting Display impl so keys never leak to logs.
5. Mock-based unit tests + a `--features=live-providers` flag for opt-in real-API integration tests.

CONSTRAINTS:
- Keys read from env ONLY, never config file.
- Never log API responses at INFO level.

DELIVERABLE: PR `feat(embed): Embedder trait + OpenAI provider with redacting key newtype`.
```

---

#### M3.2 — Bundled local embedder (ONNX)

**Branch:** `m3.2-embed-local`

```
ROLE: Add bundled local embeddings via ONNX Runtime.

CONTEXT: Read crates/argyph-embed/MODULE.md and SECURITY.md §"ONNX model supply-chain".

TASK (M3.2): Implement:
1. Local ONNX-based Embedder using the `ort` crate. Default model: bge-small-en-v1.5 (int8-quantized).
2. Tokenizer via the `tokenizers` crate.
3. Lazy model download on first use into ~/.cache/argyph/models/. SHA-256 verify against a hardcoded checksum.
4. ONNX Runtime is the ONE allowed `unsafe` exception in the workspace. Isolate behind a safe API. Document at the module top with SAFETY: comments.
5. Threaded pool, not per-task model instances — measure and document memory.
6. Cross-platform: test on macOS arm64, macOS x64, Linux x64, Windows x64. Windows often needs extra setup — document in CONTRIBUTING.

CONSTRAINTS: This is the riskiest milestone for cross-platform builds. Budget extra time. If Windows is blocking, land the milestone with Windows behind a feature flag and follow up.

DELIVERABLE: PR `feat(embed): bundled local ONNX embeddings (bge-small-en-v1.5)`.
```

---

#### M3.3 — Voyage provider

**Branch:** `m3.3-embed-voyage`

```
ROLE: Add Voyage as a remote embedding provider. <200-line PR.

CONTEXT: Read crates/argyph-embed/src/openai.rs (the pattern to mirror).

TASK (M3.3): Mirror the OpenAI implementation for Voyage's embeddings API. Same retries, batching, key handling.

DELIVERABLE: PR `feat(embed): Voyage provider`.
```

---

#### M3.4 — LanceDB integration + hybrid search

**Branch:** `m3.4-store-lance`

```
ROLE: Add LanceDB for vector storage and implement hybrid search.

CONTEXT: Read crates/argyph-store/MODULE.md.

TASK (M3.4): Implement:
1. LanceDB initialization under .argyph/vectors/.
2. New Store methods: upsert_vectors(chunk_id, vector), search_vectors(query_vec, k, filter).
3. Hybrid search via reciprocal-rank fusion: BM25 (SQLite FTS5) + vector (LanceDB) → fused ranking.
4. Filter support: language, paths_glob, exclude_glob.
5. Integration tests covering: pure BM25, pure vector, hybrid with various alpha values.

CONSTRAINTS:
- LanceDB version pinned.
- Keep the Store trait swappable — no LanceDB types in the trait, only in the impl.

DELIVERABLE: PR `feat(store): LanceDB vector backend + RRF hybrid search`.
```

---

#### M3.5 — Background Tier 2 indexing

**Branch:** `m3.5-tier2-background`

```
ROLE: Wire Tier 2 (embedding generation) into the Supervisor as a background task.

CONTEXT: Read ARCHITECTURE.md §"Data flow: indexing".

TASK (M3.5): Implement:
1. A tokio-based work queue for chunks needing embeddings.
2. Concurrency cap per provider (default: local=cpus, openai=8, voyage=4).
3. Persistent state: on restart, resume embedding from where it stopped.
4. Update get_index_status to report Tier 2 progress (embedded / total / fraction).
5. Backpressure: pause embedding if memory pressure crosses threshold.

CONSTRAINTS: Spawned via Supervisor::spawn, tied to its CancellationToken.

DELIVERABLE: PR `feat(core): background Tier 2 indexer with resumable progress`.
```

---

#### M3.6 — search_semantic MCP tool

**Branch:** `m3.6-mcp-semantic`

```
ROLE: Expose hybrid search as an MCP tool.

CONTEXT: docs/tools-reference.md §search_semantic.

TASK (M3.6): Implement the search_semantic handler in argyph-mcp/src/tools/. Return index_coverage on every response (fraction of chunks currently embedded). Handler must be <100 lines.

DELIVERABLE: PR `feat(mcp): search_semantic tool with index_coverage`.
```

**🎯 Gate:** End of Phase 3. Tag `v0.3.0-beta`. All three pillars except packing.

---

### Phase 4 — Packing

#### M4.1 — argyph-pack core

**Branch:** `m4.1-pack-core`

```
ROLE: Implement the repo packing logic.

CONTEXT: Read crates/argyph-pack/MODULE.md and docs/tools-reference.md §pack_repo.

TASK (M4.1): Implement:
1. Token counting using `tiktoken-rs` (default tokenizer; configurable).
2. Priority heuristic: entry points → READMEs → recently modified → rest.
3. Format renderers: XML and Markdown (drop JSON per the spec's §16 cut).
4. Scope support: all, paths, symbol (via graph traversal: definition file + immediate references + immediate callees).
5. Snapshot tests with `insta` for output formats.

CONSTRAINTS: argyph-pack depends only on argyph-fs/parse/graph types, NEVER on argyph-store directly.

DELIVERABLE: PR `feat(pack): token-budgeted XML/markdown packing with priority heuristic`.
```

---

#### M4.2 — pack_repo MCP tool

**Branch:** `m4.2-mcp-pack`

```
TASK (M4.2): Add pack_repo handler under argyph-mcp/src/tools/. Tier 0-only for `all`/`paths` scopes; needs Tier 1 for `symbol` scope.

DELIVERABLE: PR `feat(mcp): pack_repo tool`.
```

**🎯 Gate:** End of Phase 4. Tag `v1.0.0-rc.1`.

---

### Phase 5 — Distribution Polish

#### M5.1 — Prebuilt binaries via cargo-dist

**Branch:** `m5.1-cargo-dist`

```
TASK (M5.1):
1. Add dist-workspace.toml configured for cargo-dist.
2. Targets: aarch64-apple-darwin, x86_64-apple-darwin, x86_64-unknown-linux-gnu, aarch64-unknown-linux-gnu, x86_64-pc-windows-msvc.
3. .github/workflows/release.yml that triggers on tag push.
4. Test by pushing v1.0.0-rc.1 (or a throwaway tag) and verifying release artifacts.

DELIVERABLE: PR `build: cargo-dist release pipeline for 5 platform targets`.
```

#### M5.2 — npm wrapper

**Branch:** `m5.2-npm-wrapper`

```
TASK (M5.2):
1. Create /npm/ with package.json (name `@argyph/server`), postinstall.js, bin/argyph.js.
2. postinstall.js: detect platform, download matching prebuilt binary from GitHub Release, verify SHA256, extract to npm/bin/.
3. bin/argyph.js: thin dispatcher that exec's the native binary with args.
4. Test on a clean machine: `npx @argyph/server@1.0.0-rc.1` should just work.

DELIVERABLE: PR `feat(npm): @argyph/server wrapper with prebuilt binary postinstall`.
```

#### M5.3 — Homebrew, cargo install, install.sh

**Branch:** `m5.3-distribution-extras`

```
TASK (M5.3):
1. Push Argyph to crates.io: `cargo publish` for each crate in dependency order.
2. Create a homebrew tap repo (`Ezzy1630/homebrew-argyph`); add Formula/argyph.rb.
3. Add scripts/install.sh (universal installer).
4. Update README install section if anything changed.

DELIVERABLE: PR `build: cargo install + Homebrew tap + install.sh`.
```

#### M5.4 — DXT bundle for Claude Desktop

**Branch:** `m5.4-dxt`

```
TASK (M5.4):
1. /dxt/manifest.json + icon.png per Anthropic's DXT spec.
2. .github/workflows/release.yml: build argyph.dxt and attach to releases.
3. Test by installing the .dxt into a fresh Claude Desktop.

DELIVERABLE: PR `feat(dxt): Claude Desktop extension bundle`.
```

**🎯 Final gate:** Tag `v1.0.0`. Update CHANGELOG. Write release notes. Post.

---

## 4. Post-MVP Upgrade Prompts

These are the planned post-1.0 capabilities. Each is additive — none breaks v1.0 contracts. Numbering continues from the MVP plan.

### Phase 6 — Memory Layer (high-leverage, low-risk)

#### M6.1 — argyph-memory crate

**Branch:** `m6.1-memory-crate`

```
ROLE: Implement persistent agent memory as a new crate.

TASK: Create crates/argyph-memory:
1. `Memory` trait: save(scope, content, metadata), search(query, k), list(scope), forget(id).
2. Storage: extend argyph-store with a `memories` table (new migration). Optionally FTS-indexed.
3. Scope: per-repo (default) or global (~/.argyph/memories.sqlite).
4. Add MCP tools: memory_save, memory_search, memory_list, memory_forget.
5. Update docs/tools-reference.md.

CONSTRAINTS: New migration, never edit existing ones.

DELIVERABLE: PR `feat(memory): persistent agent memory with scope-aware storage`.
```

---

### Phase 7 — More Languages

#### M7.x — Add a language pack

Repeat this prompt per language. Order suggestion: Go → Java → Kotlin → Swift → Ruby → PHP → C#.

```
ROLE: Add tree-sitter support for [LANGUAGE].

TASK:
1. Add tree-sitter-[language] dependency to argyph-parse (pinned version).
2. crates/argyph-parse/queries/[language].scm covering: functions, methods, classes/structs, interfaces, constants, imports.
3. New language module crates/argyph-parse/src/languages/[language].rs.
4. Module-path import resolution heuristic for the language.
5. examples/tiny-[language]-app/ fixture.
6. Unit tests achieving >95% symbol extraction.

DELIVERABLE: PR `feat(parse): tree-sitter [LANGUAGE] support`.
```

---

### Phase 8 — Library Docs (Context7-lite)

#### M8.1 — lookup_library tool

```
ROLE: Add a library-docs lookup tool.

TASK:
1. New crate crates/argyph-libdocs.
2. Source 1: read Cargo.lock and parse rustdoc JSON from `target/doc/` if present.
3. Source 2: read package.json + node_modules/[name]/README.md.
4. Source 3 (optional): docs.rs lookup with caching.
5. New MCP tool: lookup_library({ name, version_constraint }).
6. Update docs/tools-reference.md.

DELIVERABLE: PR `feat(libdocs): lookup_library tool sourcing local vendored docs`.
```

---

### Phase 9 — Additional Embedding Providers

#### M9.1 — Gemini, M9.2 — Ollama

```
TASK (M9.x):
Mirror the OpenAI provider pattern for [Gemini | Ollama]. Same retries, batching, key handling. <200 lines per provider.

DELIVERABLE: PR `feat(embed): [Gemini | Ollama] provider`.
```

---

### Phase 10 — Diff-Aware Tools

#### M10.1 — pack_diff

```
ROLE: Add a diff-aware packing tool for code review workflows.

TASK:
1. argyph-pack: new entry point `pack_diff(base, head)` using `git2` to walk the diff and pack only changed files + their immediate graph neighbors.
2. New MCP tool: pack_diff({ base: string, head: string, token_budget }).
3. Update docs/tools-reference.md.

DELIVERABLE: PR `feat(pack): pack_diff for code-review workflows`.
```

---

### Phase 11 — Better Cross-File Resolution

#### M11.1 — Per-language module graph

```
ROLE: Replace heuristic cross-file resolution with a per-language module-graph builder.

CONTEXT: ARCHITECTURE.md §"Real weaknesses" #1.

TASK: Per supported language, build a module graph that resolves imports to file paths, then re-resolve references using the module graph instead of name matching.

DELIVERABLE: PR `perf(graph): per-language module graph resolution`.
```

---

### Phase 12 — Multi-Repo Workspaces (architectural)

```
ROLE: Allow Argyph to index a set of related repos and search across them.

TASK: Introduce a Workspace concept; multiple Index instances; tools gain an optional `workspace` parameter; storage layout becomes .argyph/workspaces/<id>/.

DELIVERABLE: PR `feat(core): multi-repo workspaces`.
```

---

### Phase 13 — LSP Bridge (advanced)

```
ROLE: Opportunistically use a running LSP for symbol resolution where present; fall back to tree-sitter heuristics.

TASK: Detect running LSPs (via standard ports/sockets); query them for go-to-definition / references; reconcile with tree-sitter graph. Behind a feature flag.

DELIVERABLE: PR `feat(graph): LSP-bridge for precise cross-file resolution`.
```

---

### Phase 14 — Research-Grade (v3 territory)

Reserve a dedicated tracking issue for these and DO NOT prompt-by-prompt them until v1.0 has real users. Then revisit.

- Fine-tuned code-specific embedding model.
- Learned re-ranker trained on (query, helpful-result) pairs.
- Incremental graph reasoning (propagate edge-level deltas instead of rebuilding).

---

## 5. Working Rhythm and Review Checklist

### Per-milestone checklist

Before opening any PR:

- [ ] On a feature branch (never directly on main)
- [ ] `cargo fmt` clean
- [ ] `cargo clippy -- -D warnings` clean
- [ ] `cargo test --workspace` green
- [ ] Module files all under 600 lines
- [ ] No `unwrap()` outside tests; no `unsafe` outside argyph-embed
- [ ] All new public functions have rustdoc with at least one example
- [ ] Tests cover the new behavior
- [ ] Updated `docs/tools-reference.md` if you changed tool schemas
- [ ] Updated `CHANGELOG.md` under the `[Unreleased]` section
- [ ] Commit subject is Conventional-Commits formatted
- [ ] **No `Co-authored-by: Claude` trailer unless the milestone really merited it per `docs/COMMIT_CONVENTIONS.md` §2.2**

### When to start a fresh chat

- ✅ Moving to a new crate
- ✅ Starting a new milestone
- ✅ When the agent starts repeating itself or contradicting earlier decisions
- ✅ Before any "refactor" task

### Refactor cadence

- Refactor between milestones, never mid-milestone.
- Refactor PRs touch zero behavior; CI integration suite proves it.

### Common AI agent failure modes by phase

| Phase | Watch for                                                                  |
|-------|----------------------------------------------------------------------------|
| 0     | Over-scaffolding, premature abstractions                                   |
| 1     | Hand-rolling instead of using `ignore`; Windows path mistakes              |
| 2     | Stale graph after edits; overclaiming cross-file resolution precision      |
| 3     | ONNX bundling on Windows; hybrid-search ranking bugs; partial-index races  |
| 4     | Tokenizer mismatches; format renderer XML/Markdown bugs                    |
| 5     | Multi-platform CI flakes                                                   |

### Tagging and release rhythm

Tag after each `🎯 Gate`:

| Tag                | Marks                                |
|--------------------|--------------------------------------|
| `v0.0.1-alpha`     | Workspace + CI green                 |
| `v0.1.0-alpha`     | Tier 0 + first three MCP tools       |
| `v0.2.0-alpha`     | Full symbol graph + watcher          |
| `v0.3.0-beta`      | Semantic search                      |
| `v1.0.0-rc.1`      | Packing complete                     |
| `v1.0.0`           | Distribution polish complete         |

Post-tag: write release notes (auto-generated from Conventional Commits + a curated headline section), update CHANGELOG, post the milestone.

---

## 6. File Inventory — What Goes in the Repo (and What Doesn't)

### Files to include (already in this folder)

Top-level documentation and config:

- `README.md`
- `ARCHITECTURE.md`
- `CONTRIBUTING.md`
- `SECURITY.md`
- `ROADMAP.md`
- `CHANGELOG.md`
- `CODE_OF_CONDUCT.md`
- `AUTHORS.md`
- `MAINTAINERS.md`
- `LICENSE-MIT`
- `LICENSE-APACHE`
- `.gitignore`

Detailed docs under `docs/`:

- `docs/SPEC.md`
- `docs/BUILD_PLAN.md`
- `docs/BUILD_GUIDE.md` (this file)
- `docs/AGENT_WORKFLOW.md`
- `docs/COMMIT_CONVENTIONS.md`
- `docs/MODULES.md`
- `docs/tools-reference.md`
- `docs/agent-prompts/template.md`

Per-crate ownership (already created, 10 files) under `crates/`:

- `crates/argyph/MODULE.md`
- `crates/argyph-core/MODULE.md`
- `crates/argyph-fs/MODULE.md`
- `crates/argyph-parse/MODULE.md`
- `crates/argyph-graph/MODULE.md`
- `crates/argyph-embed/MODULE.md`
- `crates/argyph-store/MODULE.md`
- `crates/argyph-pack/MODULE.md`
- `crates/argyph-mcp/MODULE.md`
- `crates/argyph-cli/MODULE.md`

GitHub config under `.github/`:

- `.github/CODEOWNERS`
- `.github/pull_request_template.md`
- `.github/ISSUE_TEMPLATE/config.yml`
- `.github/ISSUE_TEMPLATE/bug_report.yml`
- `.github/ISSUE_TEMPLATE/feature_request.yml`
- `.github/ISSUE_TEMPLATE/tool_request.yml`
- `.github/ISSUE_TEMPLATE/language_request.yml`

### Files NOT to include — Cowork session artifacts

These exist in the folder because of how this Cowork session works. **Do NOT add them to the repo** (they're listed in `.gitignore` as a safety net):

- `audit.jsonl` — Cowork's session audit log
- `.audit-key` — Cowork's session key
- `outputs/` — Cowork's output staging directory
- `uploads/` — Cowork's upload directory
- `.claude/` — Cowork's session state

### Files that will be created during Phase 0 (don't pre-create)

These get generated by the M0.1 and M0.2 prompts — don't write them by hand:

- `Cargo.toml` (workspace manifest)
- `Cargo.lock`
- `rust-toolchain.toml`
- `deny.toml`
- `dist-workspace.toml` (Phase 5)
- `.github/workflows/ci.yml`
- `.github/workflows/release.yml` (Phase 5)
- `.github/dependabot.yml`
- `crates/*/src/` (implementation files)
- `crates/*/Cargo.toml`
- `examples/` (fixture repos, added per milestone)
- `benches/` (criterion benchmarks)
- `npm/` (Phase 5)
- `dxt/` (Phase 5)
- `scripts/install.sh` (Phase 5)

---

## 7. Summary Sequence (TL;DR)

1. Create GitHub repo per §1 (no GitHub-generated README, .gitignore, or LICENSE).
2. Locally: copy docs into `~/code/argyph/`, `git init`, first commit, push.
3. Run the M0.1 prompt in a fresh Claude Code chat → review → merge → tag `v0.0.1-alpha`.
4. Continue M0.2 → M1.1 → ... → M5.4, one PR per milestone, one fresh chat per milestone.
5. At `🎯 Gate` lines, tag and write release notes.
6. After v1.0: pick from Phase 6+ post-MVP prompts as you have time and as user feedback dictates priority.

That's it. The architecture and module boundaries do the heavy lifting; the prompts are short because the project's contracts are tight.
