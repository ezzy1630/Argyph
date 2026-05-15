# Argyph — Project Specification

This document is the durable record of *what* Argyph is and *why* it exists. It does not describe how the system is built — that's [`ARCHITECTURE.md`](../ARCHITECTURE.md). It does not describe when each piece is built — that's [`BUILD_PLAN.md`](BUILD_PLAN.md).

If a contribution is not consistent with this spec, the spec is updated first, then the contribution.

---

## 1. Problem

AI coding agents working on real codebases either get starved of context (grep + read-file is slow and lossy) or get drowned in it (dumping whole directories burns tokens). The current solution is to install five different MCP servers — one for grep, one for embeddings, one for symbol search, one for repo packing, one for memory — each with its own setup ritual, API keys, and cold-start tax. Most of those servers depend on a cloud vector DB and a remote embedding API, which is a non-starter for proprietary code at most companies.

Argyph is the single MCP server that gives any agent fast, structured, semantic, and persistent context over a codebase, running entirely on the developer's machine, ready in under a second on previously-indexed repos.

---

## 2. Target users

- **Primary:** individual developers using Claude Code, Codex, Cursor, Continue, Cline, or Aider on real codebases (typically 100K–10M LOC).
- **Secondary:** small teams that want a uniform context layer without setting up infrastructure.
- **Tertiary:** enterprise developers in regulated industries who can't send code to third-party APIs.

---

## 3. Differentiators (in priority order)

1. **Unified surface.** One server, one install, every context capability behind one MCP endpoint.
2. **Local-first by default.** Bundled embedding model, embedded vector DB, no API key required to get full functionality.
3. **Three-tier progressive indexing.** Useful in <1s on warm starts, never blocks the agent.
4. **Single static binary.** Distributed via npm, cargo, brew, and DXT, all wrapping one underlying release.
5. **Incremental and persistent.** Pay the indexing cost once per repo, ever.

---

## 4. Scope — v1.0 (the cut MVP)

The full feature set described in design discussions is deliberately cut to a smaller v1.0 to avoid the most common failure mode of solo projects: scope creep that prevents shipping.

### 4.1 In scope for v1.0

**Four pillars (memory pulled forward from v1.x):**

1. **File and symbol intelligence**
   - File tree with `.gitignore`-aware filtering
   - Tree-sitter symbol extraction for **Rust, TypeScript, Python**
   - Symbol graph: definitions, references, call edges, import edges
   - Structural query tools: `find_definition`, `find_references`, `get_callers`, `get_callees`, `get_imports`, `get_symbol_outline`
2. **Semantic search**
   - Bundled local embedding model (default; lazy-downloaded on first index)
   - Optional API providers: **OpenAI, Voyage** via env vars
   - Hybrid (BM25 + vector) search via LanceDB and SQLite FTS5, fused via reciprocal rank fusion
   - AST-aware chunking with character-based fallback
3. **Repo packing**
   - Token-budgeted, repomix-style flat output
   - Priority heuristic (entry points, READMEs, recently changed first)
   - Output formats: **XML, markdown** (drop JSON for v1.0)
4. **Persistent agent memory**
   - `memory_save`, `memory_search`, `memory_list`, `memory_forget` MCP tools
   - Per-repo (default) and global scopes
   - Stored in the same SQLite substrate (`memories` table, FTS5-backed search)

**Distribution:**

- npm (`npx @argyph/server`)
- cargo (`cargo install argyph`)
- DXT (one-click Claude Desktop)

**Tooling:**

- Same binary doubles as a CLI for debugging and direct use (`argyph index`, `argyph search`, `argyph status`, `argyph doctor`, etc.)

### 4.2 Deliberate cuts from v1.0 (deferred)

These are good ideas, but not v1.0. They're on the roadmap.

| Cut item                                    | Why deferred                                          | Target version |
|---------------------------------------------|-------------------------------------------------------|----------------|
| Gemini and Ollama embedding providers       | Each is a small atomic PR; not gating launch          | v1.1           |
| JSON pack format                            | XML and markdown cover the actual use cases           | v1.1+ if asked |
| Languages beyond Rust/TS/Python             | Community contribs post-1.0                           | v1.1+          |
| MCP Resources and Prompts                   | Few agents use them today                             | v1.1           |
| `argyph doctor` extended perf metrics      | Solve via logs first                                  | v1.x           |
| `--inspect` HTTP server                     | Solve via logs first                                  | v1.x           |
| ~~Memory layer (`memory_save`, etc.)~~      | **Shipped in v1.0** (pulled forward; same storage substrate) | v1.0 ✅  |
| Library docs (Context7-style)               | Vendored docs first, registry fetches later           | v1.x           |
| User-global config file                     | Env vars + repo config cover the use cases            | v1.x if asked  |
| Homebrew tap, install.sh                    | Polish; npm + cargo cover most users at launch        | v1.1           |

### 4.3 Hard non-goals (will not be built, ever, for v1.x)

These are not deferred — they are out of scope by design.

- ❌ No code editing or writing. Read-only intelligence.
- ❌ No agent orchestration or task running.
- ❌ No team sync, multi-user state, or cloud version.
- ❌ No git mutations (commits, branches, pushes). Read-only git observation only.
- ❌ No language server replacement — no completion, refactoring, or type-checking.
- ❌ No web dashboard in v1.
- ❌ No shell execution as an MCP tool. Massive prompt-injection vector.

---

## 5. Competitive landscape

| Tool                           | Strength                       | Weakness                                          |
|--------------------------------|--------------------------------|---------------------------------------------------|
| claude-context (Zilliz)        | Mature semantic search         | Cloud Milvus required; OpenAI key; no graph        |
| GitNexus / CodeGraphContext    | Real code graph                | Slow startup; Neo4j dependency; no semantic search |
| repomix                        | Best-in-class packing          | One-shot; no MCP; no incremental                   |
| mem0                           | Best-known agent memory        | Separate concern; requires its own setup           |
| Context7                       | Up-to-date library docs        | Different problem; cloud-hosted                    |
| Serena                         | Local symbol search            | Narrow scope; no semantic; no memory               |
| **Argyph**                     | All four in one local binary   | Younger; smaller language coverage at launch       |

---

## 6. Success criteria

A claim is only valid if it can be reproduced on a defined hardware spec, methodology in `benches/README.md`. The launch claims are:

| Metric                                                | Target               |
|-------------------------------------------------------|----------------------|
| Cold start on a 1M-LOC TypeScript monorepo, Tier 0    | < 1 s                |
| Warm start (already indexed, ~100K files)             | < 1 s                |
| Tier 1 (symbol graph) on 1M-LOC repo                  | < 60 s               |
| Symbol query (`find_definition`, etc.) p99            | < 50 ms              |
| Semantic search p50 latency                           | < 100 ms             |
| Total install size (binary + bundled model on first index) | < 120 MB        |
| MCP tool schema validation pass rate                  | 100%                 |
| CI pass rate on PRs to main                           | ≥ 95% (excluding flakes) |

The numbers above are aspirational at spec time and become real claims once Phase 5 of the build plan publishes benchmarks against named competitors.

---

## 7. What "done" looks like

v1.0.0 ships when:

1. All three pillars are implemented and exercised by end-to-end tests on the `examples/medium-ts-monorepo/` fixture.
2. The cold-start, warm-start, and symbol-query latency targets in §6 are met on the reference hardware.
3. Distribution paths npm, cargo, and DXT all install successfully on macOS arm64, macOS x64, Linux x64, Linux arm64, and Windows x64 from a clean machine.
4. README, ARCHITECTURE, CONTRIBUTING, COMMIT_CONVENTIONS, SECURITY, ROADMAP, and a per-crate MODULE.md exist and are accurate.
5. CI is green on all three OSes with no skipped or flaky required jobs.
6. At least three named competitors are benchmarked against, with results published in `docs/benchmarks.md`.

---

## 8. Why this is technically interesting (and portfolio-worthy)

This section is for the project author's own reference and for anyone evaluating Argyph as engineering work.

- It is real systems engineering: tree-sitter, FFI to ONNX runtime, async background workers, embedded vector DB, filesystem watching, MCP protocol compliance.
- It forces hard engineering choices: progressive computation, partial-index semantics, cache invalidation across three tiers, cross-platform binary distribution.
- It has measurable, benchmarkable claims that an evaluator can verify.
- It targets a contemporary AI-tooling pain point precisely as the MCP ecosystem is consolidating.
- A single-author Rust systems project with clean architecture, real benchmarks, and visible polish is a top-decile portfolio signal in 2026.

---

## 9. Out of scope for the spec

- Specific algorithms (chunking strategy, ranking fusion, etc.) — those live in module-level rustdoc and in `crates/*/MODULE.md`.
- Implementation order — that's [`BUILD_PLAN.md`](BUILD_PLAN.md).
- How agents work on the repo — that's [`AGENT_WORKFLOW.md`](AGENT_WORKFLOW.md).
