# Context Discipline (v1.1)

**Status:** Draft, pending user review
**Date:** 2026-05-14
**Owner:** Ezzy
**Target release:** v1.1

## 1. Motivation

Argyph v1.0.0-rc.1 ships a strong backend (three-tier index, symbol graph,
hybrid search, `locate_smart`, memory) but agents still fall back to grep /
Read because:

1. The MCP surface exposes 17 tools, and the agent must pick well every
   time.
2. Tool descriptions are written for humans, not for LLM tool-selection.
3. Several tools can return arbitrarily large payloads — a single bad
   choice (e.g. `pack_repo` on the wrong subset) leaks thousands of tokens
   into the main agent's context.
4. There is no MCP-level habituation: nothing the agent loads on
   connection tells it to prefer Argyph over grep/Read.

This spec closes those gaps. **No new indexing, no ML, no new
dependencies.** All changes are inside existing crate boundaries.

## 2. Goals and non-goals

**Goals**
- Make agents reach for Argyph reliably without operator coaching.
- Make context bloat structurally impossible from Argyph's side: no
  retrieval tool may leak more than a bounded number of lines.
- Improve hybrid-search result quality with cheap, deterministic signals.
- One-command project bootstrap that habituates the agent.

**Non-goals**
- Trained retriever models. Stay local-first.
- Binary-quantized vector search. Premature at v1.0 scale.
- Git-history retrieval ("Context Lineage"). Future spec.
- Branch-aware index views. Future spec.
- Any write/exec capability. Argyph remains read-only.

## 3. The six changes

### 3.1 `ask` — the meta-tool

A single agent-facing entry point that routes internally:

```
ask(query: string, focus?: { file?, symbol?, line? }, mode?: auto|structural|semantic|smart, limit?: int)
  -> AskResponse { spans: [Span], strategy_used: string, truncated: bool }
```

**Routing heuristics (in order):**
1. If `query` is a bare identifier matching `[A-Za-z_][A-Za-z0-9_]*` and
   Tier 1 is ready → `find_definition`. If 0 hits, fall through.
2. If `query` parses as a structured locator (`file:symbol`, `file:Lnn`,
   `path/glob`) → `locate`.
3. If `mode == smart` and `locate_smart` is enabled → `locate_smart`.
4. Else → hybrid `search_semantic` with the new reranker (§3.5).

All results normalize to `Span`. `strategy_used` is reported so the agent
learns which path served the query.

**Implementation location:** `crates/argyph-mcp/src/tools/ask.rs`. Thin
handler, ≤200 LOC, dispatches to existing tool handlers and adapts their
responses to `Span`.

### 3.2 The `Span` contract

```rust
pub struct Span {
    pub file: Utf8PathBuf,
    pub start_line: u32,       // 1-indexed, inclusive
    pub end_line: u32,         // 1-indexed, inclusive
    pub byte_range: (u64, u64),
    pub text: String,          // already truncated to MAX_SPAN_LINES
    pub kind: SpanKind,        // Definition | Reference | Match | Outline
    pub symbol: Option<String>,
    pub language: Option<String>,
}
```

**Cap:** `ARGYPH_MAX_SPAN_LINES` (default **80**). If a natural match is
larger:
- The first 40 and last 20 lines are returned, with a `[...N lines
  elided...]` marker in between.
- The full byte range is still reported, and the response sets
  `truncated: true`.
- An `outline_handle` field gives the caller a token to fetch the full
  span explicitly via a new sub-call `expand_span(handle)`.

**Per-call cap:** `ARGYPH_MAX_TOTAL_LINES` (default **400**) across all
spans in a single response. Beyond this, lower-ranked spans are dropped
and `truncated: true` is set.

**Tools affected:** `search_text`, `search_semantic`, `find_definition`,
`find_references`, `get_callers`, `get_callees`, `locate`,
`locate_smart`. `pack_repo`, `get_repo_overview`, `get_symbol_outline`,
and `get_index_status` are exempt (their job is breadth, not depth).

`expand_span` is a new tool that takes a handle issued by Argyph in this
session and returns the full elided range. Handles are session-scoped,
TTL 10 minutes, stored in-memory.

### 3.3 MCP Prompts

Implement `explore_codebase`, `trace_symbol`, `prepare_review` per the
v1.1 roadmap. Each prompt's body begins with the same standing
instruction:

> "For any lookup of code, symbols, files, or content in this repo,
> prefer the `ask` tool over grep, find, or reading files directly.
> Argyph returns minimal validated spans, not full files."

**Prompt bodies (concise):**
- `explore_codebase`: orientation pass — calls `get_repo_overview`, then
  `ask` for likely entry points.
- `trace_symbol(symbol)`: `ask` → `find_definition` → `get_callers` →
  `get_callees`. Returns a span-only call graph fragment.
- `prepare_review(base_ref?)`: outline of changed files + `ask` for
  related code. (Cannot run git; relies on caller-supplied diff or
  changed-file list.)

**Implementation location:** `crates/argyph-mcp/src/prompts.rs`. Register
via `rmcp` prompts capability.

### 3.4 Tool description rewrite

Every tool description follows this template:

> "**Use this when** {concrete trigger}. **Returns** {shape, bounded}.
> **Do not** {anti-pattern, e.g. 'use grep for a known symbol name —
> use `ask` instead'}. Tier requirement: {0/1/1.5/2}."

The `ask` description is the longest and most directive; it explicitly
names the bad alternatives (grep, Read on a 2k-line file, etc.). All
other retrieval tools' descriptions are de-emphasized: they end with
"**Most callers should use `ask` instead.**"

This change is text-only and lives entirely in `argyph-mcp/src/lib.rs`.

### 3.5 Heuristic reranker for hybrid search

A deterministic re-ranker layered on top of the existing BM25+vector
fusion in `argyph-store`. Final score:

```
score = w_base * fusion_score
      + w_recency * recency_signal      // 1.0 if mtime within 7d, decays to 0
      + w_focus_call * call_distance    // 1.0 if same-symbol or 1-hop, 0 otherwise
      + w_focus_module * module_match   // 1.0 if same module/dir as focus, else 0
      + w_size_penalty * size_penalty   // penalize huge files
```

Default weights:
`w_base=1.0, w_recency=0.15, w_focus_call=0.30, w_focus_module=0.15,
w_size_penalty=-0.10`.

`focus` is optional — passed by `ask` when the caller provides
`focus.symbol` or `focus.file`. Without focus, only recency and size
penalty apply.

All signals are computed from already-indexed data (file mtime, symbol
graph edges, file size in `FileEntry`). No new tables.

**Implementation location:** new `argyph-store::rerank` module. Reranker
is a pure function over `(Vec<Hit>, FocusContext) -> Vec<Hit>`.

### 3.6 `argyph init`

CLI subcommand that:
1. Detects which of `CLAUDE.md`, `AGENTS.md`, `GEMINI.md` exist in the
   repo root (or asks which to create).
2. Appends (or creates) a marked block:

```
<!-- argyph:begin -->
## Code & context lookup

This repo is indexed by Argyph (MCP). For any lookup of code, symbols,
files, or content, prefer the `ask` tool over grep, find, or reading
files directly. Argyph returns minimal validated spans, not full files.

- `ask` — primary entry point. Pass a query and optional focus.
- `pack_repo` — only when you genuinely need a flat dump.
- Other Argyph tools — advanced, prefer `ask` first.
<!-- argyph:end -->
```

3. Idempotent: re-running replaces the block in-place.

**Implementation location:** `crates/argyph-cli/src/commands/init.rs`.
≤150 LOC.

## 4. Architecture impact

| Crate          | Change                                              |
|----------------|-----------------------------------------------------|
| `argyph-mcp`   | Add `tools/ask.rs`, `tools/expand_span.rs`, `prompts.rs`. Rewrite descriptions in `lib.rs`. Add `Span` type to `types.rs`. |
| `argyph-store` | Add `rerank` module. Extend hybrid-search to take `FocusContext`. |
| `argyph-core`  | Expose mtime + dir info needed by reranker via the `Index` facade if not already there. |
| `argyph-cli`   | Add `init` subcommand.                              |
| `argyph-graph` | No changes — call-graph queries already available.  |

No new dependencies. No schema migrations. All changes additive — the
existing tools keep working with unchanged contracts (except for the
span cap, which is enforced at the response boundary).

## 5. Compatibility

- Existing tool callers keep working; responses gain optional
  `truncated`, `outline_handle` fields they can ignore.
- The 17 existing tools remain registered. Adoption is driven by
  description rewrite + prompts, not by removal.
- Span cap is configurable via env var; setting it to `u32::MAX`
  restores v1.0 behavior for users who want it.

## 6. Testing

- **Unit:** `ask` routing decision table (one test per heuristic branch);
  span truncation under cap; reranker scoring with synthetic hits.
- **Integration:** end-to-end MCP call to `ask` against the existing
  Tier-0/1 fixture repo, assert `strategy_used` and span line counts.
- **Snapshot:** prompt bodies (insta).
- **Smoke:** `argyph init` against a temp repo with each of
  `CLAUDE.md` / no file / pre-existing argyph block.

No new fixtures required.

## 7. Out of scope (deferred)

- Trained reranker (future, if user demand)
- Quantized vector prefilter
- Git history retrieval
- Branch-aware index
- New language packs (separate v1.1 work)

## 8. Sequencing

A → B → C → D, each landable independently:

- **A.** Span contract + cap + `expand_span` (§3.2). Foundational; all
  later items depend on `Span`.
- **B.** `ask` meta-tool (§3.1). Depends on A.
- **C.** Description rewrite + MCP Prompts (§3.3, §3.4). Depends on B.
- **D.** Reranker + `argyph init` (§3.5, §3.6). Independent; can land in
  parallel with C.
