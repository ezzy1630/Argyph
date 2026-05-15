# Build Plan

The build is structured so that **every milestone produces a working, testable artifact**. There is never a milestone that ends with "and then it'll work once milestone N+2 is done."

Total estimated wall-clock for v1.0: **~10–12 weeks** of focused solo work with AI assistance, assuming the cuts in [`SPEC.md`](SPEC.md) §4.2 hold.

Each milestone is sized to fit in a single AI-agent chat session (one crate, 100–300 lines of new code).

---

## Phase 0 — Skeleton (1 week)

### M0.1 — Workspace + CI

**Deliverables:**

- Cargo workspace with all 10 crates as empty stubs (`lib.rs` + one no-op trait per crate).
- GitHub Actions: `cargo fmt`, `cargo clippy -D warnings`, `cargo test` on the macOS / Linux / Windows matrix.
- `cargo-deny` config (license + advisory).
- `rust-toolchain.toml` pinning the toolchain.
- README, LICENSE-MIT, LICENSE-APACHE, CONTRIBUTING, ARCHITECTURE.

**Success:** `cargo build` and `cargo test` pass on all three platforms in CI.

**Difficulty:** Easy.
**Common AI failure:** Trying to scaffold all crates with implementation at once. Reject — this milestone is empty shells only.

### M0.2 — Binary + CLI dispatch

**Deliverables:**

- `argyph serve`, `argyph doctor` (echo platform info), `argyph --version`.
- `clap` v4 with subcommand dispatch.

**Success:** `cargo run -- doctor` prints platform info on all three OSes.

🎯 **End of Phase 0:** the project is alive and CI is green.

---

## Phase 1 — Tier 0: Filesystem (1–2 weeks)

### M1.1 — `argyph-fs` walker

**Deliverables:**

- `Walker` trait + impl using the `ignore` crate.
- BLAKE3 hashing per file.
- Language detection from extensions.
- Unit tests on the `examples/tiny-rust-app/` fixture.

**Success:** Walking a 10K-file repo completes in <500 ms. `.gitignore` is respected. Symlinks handled per spec.

**Common AI failure:** Hand-rolling a walker instead of using the `ignore` crate; mishandling Windows path separators. Use `camino` for typed UTF-8 paths.

### M1.2 — `argyph-store` SQLite for file metadata

**Deliverables:**

- SQLite schema (files table only).
- Migration system (`refinery` or equivalent).
- `Store::upsert_files`.
- Snapshot tests on schema.

**Success:** Round-trip insert and query of file entries. WAL mode enabled.

**Common AI failure:** Editing existing migrations rather than adding a new one. AI Agent Rule #2 — never violate it.

### M1.3 — `argyph-core` Supervisor + Tier 0

**Deliverables:**

- Boot flow: walk → upsert → mark Tier 0 ready.
- Lifecycle (boot, run, shutdown) with cancellation token.
- `tracing` setup with `ARGYPH_LOG`-style env filter.

**Success:** `argyph serve` (no-op MCP) boots and indexes Tier 0 in <1 s on the medium fixture.

**Common AI failure:** Spawning tasks outside `Supervisor::spawn`. AI Agent Rule #5 — never violate it.

### M1.4 — `argyph-mcp` skeleton + first three tools

**Deliverables:**

- `rmcp` integration.
- Tools: `get_index_status`, `get_repo_overview`, `search_text`.
- JSON Schema validation at the boundary.
- One end-to-end test that boots the server and calls each tool over stdio.

**Success:** Hooks up to Claude Code via `claude mcp add argyph -- cargo run -- serve`. The three tools are callable end-to-end.

🎯 **End of Phase 1:** usable as a basic file-aware MCP. **Tag `v0.1.0-alpha`.**

---

## Phase 2 — Tier 1: Symbol Graph (2–3 weeks)

### M2.1 — `argyph-parse` tree-sitter integration

**Deliverables:**

- Language pack registry; **Rust + TypeScript + Python** to start.
- Symbol-extraction `.scm` queries for those three languages.
- AST-aware chunking with character fallback.

**Success:** Parsing the medium fixture extracts >95% of expected symbols (asserted via snapshot tests).

**Common AI failure:** Misusing tree-sitter queries; over-eager chunking that splits mid-function. Document and assert chunk invariants in property tests.

### M2.2 — `argyph-graph` symbol graph

**Deliverables:**

- Edge resolution: calls, references, imports.
- Within-file resolution first; cross-file with fully-qualified names + per-language import resolution as a v1 heuristic.
- **Honesty:** the `MODULE.md` for `argyph-graph` explicitly documents that cross-file resolution is best-effort, not LSP-grade.

**Success:** Find-references and find-definition work on the fixtures with documented accuracy bounds (>90% intra-file, >70% cross-file in the v1 heuristic).

**Common AI failure:** Overconfident cross-file resolution. Reject any claim of LSP-grade precision.

### M2.3 — Storage extension

**Deliverables:**

- Add `symbols`, `chunks`, `edges` tables to SQLite (new migration).
- Indexes for fast lookups.

**Success:** Symbol queries return in <10 ms on the medium fixture.

### M2.4 — MCP graph tools

**Deliverables:**

- `find_definition`, `find_references`, `get_callers`, `get_callees`, `get_imports`, `get_symbol_outline`.

**Success:** All callable, schemas validated, integration tests pass.

### M2.5 — Incremental updates + watcher

**Deliverables:**

- `notify`-based watcher with debouncing (250–500 ms).
- Re-parse only changed files; update graph deltas.
- Polling fallback (`ARGYPH_WATCHER=poll`) for sandboxed environments.

**Success:** Editing a file in the fixture and saving causes the index to update in <500 ms; subsequent queries reflect the change.

**Common AI failure:** Watcher OS quirks (FSEvents on macOS, inotify limits on Linux). Test in CI on all three OSes.

🎯 **End of Phase 2:** full structural intelligence without ever touching embeddings. **Tag `v0.2.0-alpha`.**

---

## Phase 3 — Tier 2: Semantic Search (2–3 weeks)

### M3.1 — `argyph-embed` provider abstraction + first remote provider

**Deliverables:**

- `Embedder` trait.
- OpenAI implementation with batching, retries, and rate limiting.
- Tests against a mocked HTTP server; opt-in real-API test behind a feature flag.

**Success:** Embedding 100 chunks via OpenAI succeeds in mocked + real tests.

### M3.2 — Bundled local embedder via ONNX

**Deliverables:**

- `ort` (ONNX Runtime) integration.
- Bundle/download `bge-small-en-v1.5` FP32 (no int8-quantized ONNX export exists).
- Tokenizer via `tokenizers` crate.
- Lazy download on first use; cache in `~/.cache/argyph/models/`.
- SHA-256 checksum verification on download.

**Success:** Local embedding works on a fresh machine with no API key set, on all three OSes.

**Common AI failure:** Cross-platform ONNX builds; loading the model per task instead of pooling. Pool the model.

### M3.3 — Voyage provider

**Deliverables:**

- `argyph-embed/src/voyage.rs` — small atomic PR (<200 lines).

**Success:** Schema-compatible with Voyage code-embedding endpoints.

### M3.4 — `argyph-store` LanceDB integration

**Deliverables:**

- Add LanceDB table for chunk vectors (new migration).
- Hybrid search: BM25 (SQLite FTS5) + vector (LanceDB), fused via reciprocal rank fusion.

**Success:** `search_hybrid` returns reasonable results on the medium fixture.

### M3.5 — Background Tier 2 indexing

**Deliverables:**

- Tokio task queue for embedding work.
- Backpressure: provider-specific concurrency caps.
- Persistent: resume embeddings across restarts (no double work).

**Success:** Tier 2 runs in background; `get_index_status` reports progress; `search_semantic` returns partial-index results with `index_coverage` field.

### M3.6 — `search_semantic` MCP tool + filters

**Deliverables:**

- Tool with language and path-glob filters.

**Success:** End-to-end semantic queries via Claude Code work against all three providers (local, OpenAI, Voyage).

🎯 **End of Phase 3:** all three pillars except packing are complete. **Tag `v0.3.0-beta`.**

---

## Phase 4 — Packing (1 week)

### M4.1 — `argyph-pack` core

**Deliverables:**

- Token counting (per-provider tokenizer or default `tiktoken-rs`).
- Priority heuristic (entry points → READMEs → recently modified → rest).
- Format renderers: XML primary, markdown secondary.

**Success:** Packing the medium fixture under various budgets produces valid, well-formed output.

### M4.2 — `pack_repo` MCP tool

**Deliverables:**

- Tool with scope and budget parameters.

**Success:** End-to-end via Claude Code.

🎯 **End of Phase 4:** v1.0 candidate. **Tag `v1.0.0-rc.1`.**

---

## Phase 5 — Distribution polish (1 week)

### M5.1 — Prebuilt binaries via cargo-dist

**Deliverables:**

- `dist-workspace.toml` configured for the full target matrix.
- Release workflow producing artifacts on tag push.

**Success:** Pushing a tag produces signed release artifacts on GitHub for all five targets.

### M5.2 — npm wrapper

**Deliverables:**

- `npm/package.json`, `npm/postinstall.js`, `npm/bin/argyph.js`.
- Postinstall downloads the right binary from GitHub Releases by platform.

**Success:** `npx @argyph/server@latest` works from a clean machine on all three OSes.

**Common AI failure:** Postinstall scripts that fail silently behind corporate proxies. Surface errors with clear remediation.

### M5.3 — DXT bundle

**Deliverables:**

- `dxt/manifest.json` and `dxt/icon.png`.
- `dxt pack` produces `argyph.dxt`.

**Success:** Double-clicking installs into Claude Desktop on macOS.

🎯 **End of Phase 5:** **v1.0.0 ships.**

---

## Phase 6 — Polish and benchmarks (1 week)

### M6.1 — Benchmarks against named competitors

**Deliverables:**

- `benches/` with `criterion` benchmarks.
- `scripts/bench-against.sh` comparing to claude-context, repomix, GitNexus on a defined fixture.
- `docs/benchmarks.md` with methodology, hardware spec, and reported numbers.

**Success:** Reproducible numbers, methodology published.

### M6.2 — README hero GIF and docs site

**Deliverables:**

- 30-second GIF of indexing + a real Claude Code query.
- mdBook docs deployed to `argyph.dev` (or GitHub Pages fallback).

**Success:** Documentation is searchable and discoverable.

🎯 **End of Phase 6:** the project is portfolio-ready. **Tag `v1.0.1`.**

---

## Post-1.0 (additive, not gating)

Items pulled in roughly this order, demand-driven:

- M7.x — Gemini, Ollama embedding providers (separate atomic PRs).
- M8.x — More languages: Go, Java, Kotlin, Swift, Ruby, PHP, C#.
- M9.x — Memory layer (`memory_save`, `memory_search`, `memory_list`, `memory_forget`).
- M10.x — Library docs (vendored first; registry fetches later).
- M11.x — MCP Resources and Prompts.
- M12.x — Better cross-file resolution (LSP-bridge prototype).
- M13.x — Diff-aware tools (`pack_diff`).

---

## Difficulty by phase

| Phase | Difficulty   | Most common AI failure                                       |
|-------|--------------|--------------------------------------------------------------|
| 0     | Easy         | Over-scaffolding; premature abstractions                      |
| 1     | Easy–Medium  | Hand-rolling instead of using `ignore`; Windows paths         |
| 2     | Hard         | Tree-sitter misuse; stale graph after edits; over-confident cross-file resolution |
| 3     | Hard         | ONNX bundling on Windows; hybrid ranking bugs; partial-index race conditions |
| 4     | Medium       | Tokenizer mismatch; format renderer bugs                      |
| 5     | Medium       | Multi-platform CI flakes; npm postinstall failures            |
| 6     | Easy–Medium  | Benchmarks that don't reproduce on others' hardware            |

---

## Refactor windows

Refactor PRs are *non-functional by definition* — they touch zero behavior, and CI runs the full integration suite to prove it. Refactor windows are scheduled at the end of each phase, before tagging. Never mid-feature.
