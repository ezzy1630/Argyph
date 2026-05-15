# Roadmap

The roadmap is updated after every release. Items reflect intent, not
commitment. Anything in "Later" may move forward, backward, or out of scope
based on what users actually need.

Detailed milestone definitions are in [`docs/BUILD_PLAN.md`](docs/BUILD_PLAN.md).

---

## Shipped — `v1.1`

- [x] **Context Discipline** — `ask` meta-tool, universal bounded `Span`
      responses, `expand_span`, MCP Prompts, agent-oriented tool
      descriptions, heuristic hybrid reranking, and `argyph init` agent
      instruction installer.

---

## Shipped — `v1.0.0-rc.1`

- [x] **Phase 0** — Skeleton: workspace, CI, binary entry point.
- [x] **Phase 1** — Tier 0: filesystem index, `get_index_status`,
      `get_repo_overview`, `search_text`.
- [x] **Phase 2** — Tier 1: tree-sitter symbol graph; `find_definition`,
      `find_references`, callers/callees, imports, outline.
- [x] **Phase 3** — Tier 2: hybrid search; bundled local model + OpenAI +
      Voyage providers.
- [x] **Phase 4** — Repo packing: XML and Markdown formats.
- [x] **Phase 5** — Distribution: prebuilt binaries, npm, cargo, DXT,
      Homebrew tap, universal `install.sh`.
- [x] **Phase 6** — Precise locate: Tier 1.5 structural index over
      Markdown/JSON/YAML/TOML/CSV, `locate` and (opt-in) `locate_smart`
      MCP tools.
- [x] **Phase 7** — Persistent agent memory: `memory_save`,
      `memory_search`, `memory_list`, `memory_forget` MCP tools.

Target: `v1.0.0` cuts after the success criteria in
[`docs/SPEC.md`](docs/SPEC.md) § 6 are verified against the reference
hardware and benchmarks are published in
[`docs/benchmarks.md`](docs/benchmarks.md).

---

## Now (v1.0 GA hardening)

- [ ] Publish reproducible benchmarks against `claude-context`, `repomix`,
      and `Serena` in `docs/benchmarks.md`.
- [ ] Performance pass: verify Tier 0 cold start <1 s, semantic p50
      <100 ms, structural-path p99 <5 ms on the reference 1M-LOC fixture.
- [ ] Cross-platform install QA: macOS arm64/x64, Linux x64/arm64,
      Windows x64 — npm, cargo install, Homebrew, DXT, install.sh.

---

## Next (v1.1)

- Gemini and Ollama embedding providers (separate atomic PRs).
- Additional language packs: Go, Java, Kotlin.
- MCP Resources: `argyph://overview`, `argyph://status`, `argyph://config`.
- JSON pack format (only if requested).
- HTML/PDF extractors as additional Tier 1.5 parsers.
- `pack_diff` tool for code-review workflows.

---

## Later (v1.x and v2)

### Better cross-file resolution

LSP-bridge prototype. When a language server is running, opportunistically
use it for symbol resolution; fall back to tree-sitter heuristics
otherwise. Closes the cross-file accuracy gap documented in
`crates/argyph-graph/MODULE.md`.

### Multi-repo workspaces

Index a set of related repos; query across them. Probably the most-
requested feature for monorepo-adjacent users.

### Library docs

Context7-style up-to-date library documentation. Start with vendored docs
(`target/doc`, `node_modules/.../README.md`); registry fetches later.

### Plugin system (v2)

Sandboxed WASM or out-of-process tool plugins. Done well, this is a real
differentiator. Done badly, it kills the project. We will not build this
until the v1 surface is stable and the design is clear.

### Optional remote backend (v2)

For teams that want a shared index. Same product, with a managed sync
backend. This is where the monetization story would start — but the
local-first product remains the canonical version.

### Code-specific embeddings

Bundle (or fine-tune) a code-specific embedding model. The `Embedder`
trait makes this a drop-in.

---

## Hard non-goals

These are out of scope by design and will not be built. They are listed so
contributors know not to propose them.

- ❌ Code editing or writing tools.
- ❌ Agent orchestration or task running.
- ❌ Git mutations (commits, branches, pushes).
- ❌ Language server replacement.
- ❌ Web dashboard.
- ❌ Shell execution as an MCP tool.
- ❌ User-provided runtime language packs.

---

## How items move on the roadmap

- "Now" items are tracked as milestones in
  [`docs/BUILD_PLAN.md`](docs/BUILD_PLAN.md) and as issues with the
  `milestone:vX.X` label.
- "Next" items become "Now" after the prior minor ships and the
  maintainer commits to scope.
- "Later" items move forward based on user demand. Three independent
  requests for the same feature is the rough threshold for promotion to
  "Next."
- Hard non-goals do not move. If you believe one should, open a
  discussion, not a PR.
