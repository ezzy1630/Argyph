# Context Discipline (v1.1) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Argyph the GOAT for agent context: add a single `ask` meta-tool, enforce a span-line cap across all retrieval tools, ship MCP Prompts and a CLAUDE.md auto-installer, rewrite tool descriptions for LLM affordance, and layer a deterministic reranker on hybrid search — all inside existing crates with no new dependencies.

**Architecture:** Four sequential phases (A→D), each independently shippable. Phase A introduces a universal `Span` type and a response-level truncation layer that every retrieval tool feeds through. Phase B adds the `ask` router. Phase C rewrites descriptions and adds MCP Prompts. Phase D adds the reranker (in `argyph-store`) and turns the existing stub `init` subcommand into a CLAUDE.md/AGENTS.md installer.

**Tech Stack:** Rust workspace, `rmcp`, `serde`/`schemars`, existing `argyph-core` / `argyph-store` / `argyph-mcp` / `argyph-cli` crates. No new dependencies.

**Spec:** `specs/2026-05-14-context-discipline-design.md`

---

## File Structure

**New files:**

```
crates/argyph-mcp/src/span.rs          # universal Span type + truncation
crates/argyph-mcp/src/handles.rs       # session-scoped expand handles (in-memory LRU)
crates/argyph-mcp/src/tools/ask.rs     # the meta-tool router
crates/argyph-mcp/src/tools/expand_span.rs
crates/argyph-mcp/src/prompts.rs       # MCP Prompts registration
crates/argyph-store/src/rerank.rs      # heuristic reranker
```

**Modified files:**

```
crates/argyph-mcp/src/lib.rs            # register ask + expand_span; rewrite descriptions; wire prompts
crates/argyph-mcp/src/tools/mod.rs      # pub mod ask, expand_span
crates/argyph-mcp/src/tools/search_text.rs       # adapt response through Span truncation
crates/argyph-mcp/src/tools/search_semantic.rs   # adapt + accept FocusContext from ask
crates/argyph-mcp/src/tools/find_definition.rs   # adapt
crates/argyph-mcp/src/tools/find_references.rs   # adapt
crates/argyph-mcp/src/tools/get_callers.rs       # adapt
crates/argyph-mcp/src/tools/get_callees.rs       # adapt
crates/argyph-mcp/src/tools/locate.rs            # adapt (already has Span-shaped output)
crates/argyph-mcp/src/tools/locate_smart.rs      # adapt
crates/argyph-store/src/lib.rs          # pub mod rerank
crates/argyph-cli/src/cmds/init.rs      # replace stub with real installer
crates/argyph-cli/src/lib.rs            # extend Init command args if needed
README.md                               # add `ask` row + Context Discipline section
ARCHITECTURE.md                         # add §2.x note on the meta-tool layer
docs/tools-reference.md                 # create if absent; add `ask` and `expand_span` schemas
ROADMAP.md                              # mark Context Discipline shipped under v1.1
```

**Env vars introduced:**

```
ARGYPH_MAX_SPAN_LINES   (default 80)
ARGYPH_MAX_TOTAL_LINES  (default 400)
ARGYPH_ASK_DEFAULT_K    (default 8)
```

---

## Phase A — Span Contract

### Task A1: Universal `Span` type and truncation function

**Files:**
- Create: `crates/argyph-mcp/src/span.rs`
- Test: `crates/argyph-mcp/src/span.rs` (inline `#[cfg(test)]` module)
- Modify: `crates/argyph-mcp/src/lib.rs` (add `pub mod span;`)

- [ ] **Step 1: Write the failing test**

Append to a new `crates/argyph-mcp/src/span.rs`:

```rust
#![allow(dead_code)]

use schemars::JsonSchema;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    Definition,
    Reference,
    Match,
    Outline,
    Locate,
    Call,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Span {
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub byte_range: (u64, u64),
    pub text: String,
    pub kind: SpanKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expand_handle: Option<String>,
}

pub const DEFAULT_MAX_SPAN_LINES: u32 = 80;
pub const DEFAULT_MAX_TOTAL_LINES: u32 = 400;
pub const HEAD_LINES: usize = 40;
pub const TAIL_LINES: usize = 20;

pub fn max_span_lines() -> u32 {
    std::env::var("ARGYPH_MAX_SPAN_LINES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_MAX_SPAN_LINES)
}

pub fn max_total_lines() -> u32 {
    std::env::var("ARGYPH_MAX_TOTAL_LINES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_MAX_TOTAL_LINES)
}

/// Truncate `text` to at most `cap` lines using head+tail elision.
/// If truncated, returns the elided text and `true`.
pub fn truncate_lines(text: &str, cap: u32) -> (String, bool) {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    if lines.len() as u32 <= cap {
        return (text.to_string(), false);
    }
    let head: String = lines.iter().take(HEAD_LINES).copied().collect();
    let tail_start = lines.len().saturating_sub(TAIL_LINES);
    let tail: String = lines.iter().skip(tail_start).copied().collect();
    let elided = lines.len().saturating_sub(HEAD_LINES + TAIL_LINES);
    let out = format!("{head}[...{elided} lines elided...]\n{tail}");
    (out, true)
}

/// Cap a vector of spans by total line count. Drops trailing spans
/// and sets `truncated = true` on the response wrapper (caller's job).
/// Returns `(kept_spans, was_response_truncated)`.
pub fn cap_total_lines(mut spans: Vec<Span>, cap: u32) -> (Vec<Span>, bool) {
    let mut total: u32 = 0;
    let mut keep = 0usize;
    for s in &spans {
        let lines = s.end_line.saturating_sub(s.start_line).saturating_add(1);
        if total.saturating_add(lines) > cap {
            break;
        }
        total = total.saturating_add(lines);
        keep += 1;
    }
    let truncated = keep < spans.len();
    spans.truncate(keep);
    (spans, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_lines_under_cap_is_noop() {
        let text = "a\nb\nc\n";
        let (out, t) = truncate_lines(text, 80);
        assert_eq!(out, text);
        assert!(!t);
    }

    #[test]
    fn truncate_lines_over_cap_elides_middle() {
        let big: String = (0..200).map(|i| format!("L{i}\n")).collect();
        let (out, t) = truncate_lines(&big, 80);
        assert!(t);
        assert!(out.contains("[...140 lines elided...]"));
        assert!(out.starts_with("L0\n"));
        assert!(out.contains("L199\n"));
    }

    #[test]
    fn cap_total_lines_drops_trailing_when_budget_exceeded() {
        let mk = |s: u32, e: u32| Span {
            file: "f".into(),
            start_line: s,
            end_line: e,
            byte_range: (0, 0),
            text: String::new(),
            kind: SpanKind::Match,
            symbol: None,
            language: None,
            score: None,
            truncated: false,
            expand_handle: None,
        };
        let spans = vec![mk(1, 100), mk(1, 200), mk(1, 300)];
        let (kept, t) = cap_total_lines(spans, 250);
        assert!(t);
        assert_eq!(kept.len(), 1);
    }
}
```

Then add `pub mod span;` near the top of `crates/argyph-mcp/src/lib.rs` (after `pub mod types;`).

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p argyph-mcp span::tests`
Expected: FAIL — module does not yet exist; after Step 1's edits it should compile and **all three tests pass**. If any test fails, fix the truncation logic before continuing.

- [ ] **Step 3: Verify all three tests pass**

Run: `cargo test -p argyph-mcp span::tests -- --nocapture`
Expected: 3 passed.

- [ ] **Step 4: Commit**

```bash
git add crates/argyph-mcp/src/span.rs crates/argyph-mcp/src/lib.rs
git commit -m "feat(mcp): add universal Span type with line-cap truncation"
```

---

### Task A2: Expand-handle store

**Files:**
- Create: `crates/argyph-mcp/src/handles.rs`
- Modify: `crates/argyph-mcp/src/lib.rs` (add `pub mod handles;`)

- [ ] **Step 1: Write the failing test**

Create `crates/argyph-mcp/src/handles.rs`:

```rust
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const HANDLE_TTL: Duration = Duration::from_secs(600); // 10 min
const MAX_HANDLES: usize = 512;

#[derive(Debug, Clone)]
pub struct ExpandTarget {
    pub file: String,
    pub byte_range: (u64, u64),
    pub start_line: u32,
    pub end_line: u32,
}

pub struct HandleStore {
    inner: Mutex<HashMap<String, (ExpandTarget, Instant)>>,
}

impl HandleStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    pub fn issue(&self, target: ExpandTarget) -> String {
        let id = format!("eh_{:016x}", rand_u64());
        let mut g = self.inner.lock().unwrap();
        // Evict expired and oldest if at capacity.
        g.retain(|_, (_, t)| t.elapsed() < HANDLE_TTL);
        if g.len() >= MAX_HANDLES {
            if let Some(oldest_k) = g
                .iter()
                .min_by_key(|(_, (_, t))| *t)
                .map(|(k, _)| k.clone())
            {
                g.remove(&oldest_k);
            }
        }
        g.insert(id.clone(), (target, Instant::now()));
        id
    }

    pub fn lookup(&self, id: &str) -> Option<ExpandTarget> {
        let mut g = self.inner.lock().unwrap();
        if let Some((target, t)) = g.get(id) {
            if t.elapsed() < HANDLE_TTL {
                return Some(target.clone());
            }
        }
        g.remove(id);
        None
    }
}

fn rand_u64() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    // xorshift to avoid trivially-sequential ids
    let mut x = nanos.wrapping_mul(2862933555777941757).wrapping_add(3037000493);
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51afd7ed558ccd);
    x ^= x >> 33;
    x
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_then_lookup_returns_target() {
        let s = HandleStore::new();
        let id = s.issue(ExpandTarget {
            file: "a.rs".into(),
            byte_range: (0, 10),
            start_line: 1,
            end_line: 2,
        });
        let got = s.lookup(&id).expect("handle should resolve");
        assert_eq!(got.file, "a.rs");
        assert_eq!(got.byte_range, (0, 10));
    }

    #[test]
    fn unknown_handle_returns_none() {
        let s = HandleStore::new();
        assert!(s.lookup("eh_deadbeef").is_none());
    }
}
```

Add `pub mod handles;` to `crates/argyph-mcp/src/lib.rs`.

- [ ] **Step 2: Run tests**

Run: `cargo test -p argyph-mcp handles::tests`
Expected: 2 passed.

- [ ] **Step 3: Commit**

```bash
git add crates/argyph-mcp/src/handles.rs crates/argyph-mcp/src/lib.rs
git commit -m "feat(mcp): add session-scoped expand-handle store"
```

---

### Task A3: Wire HandleStore into ArgyphMcp service

**Files:**
- Modify: `crates/argyph-mcp/src/lib.rs`

- [ ] **Step 1: Edit the service struct**

In `crates/argyph-mcp/src/lib.rs`, change the `ArgyphMcp` struct and `serve` function:

```rust
use crate::handles::HandleStore;

#[derive(Clone)]
struct ArgyphMcp {
    supervisor: Arc<Supervisor>,
    root: Arc<Utf8PathBuf>,
    handles: Arc<HandleStore>,
}
```

In `serve`:

```rust
let service = ArgyphMcp {
    supervisor,
    root: Arc::new(root),
    handles: Arc::new(HandleStore::new()),
};
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check -p argyph-mcp`
Expected: clean compile (warnings about unused `handles` are OK; consumed in A4/B1).

- [ ] **Step 3: Commit**

```bash
git add crates/argyph-mcp/src/lib.rs
git commit -m "chore(mcp): thread HandleStore through ArgyphMcp service"
```

---

### Task A4: `expand_span` tool

**Files:**
- Create: `crates/argyph-mcp/src/tools/expand_span.rs`
- Modify: `crates/argyph-mcp/src/tools/mod.rs`
- Modify: `crates/argyph-mcp/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/argyph-mcp/src/tools/expand_span.rs`:

```rust
use std::sync::Arc;

use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{self, McpErrorBody};
use crate::handles::HandleStore;
use crate::span::{Span, SpanKind};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Request {
    pub handle: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Response {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpErrorBody>,
}

pub async fn handle(
    handles: &Arc<HandleStore>,
    root: &Utf8PathBuf,
    req: Request,
) -> Response {
    let Some(target) = handles.lookup(&req.handle) else {
        return Response {
            span: None,
            error: Some(error::invalid_path("unknown or expired handle")),
        };
    };
    let full = root.join(&target.file);
    let content = match std::fs::read(full.as_str()) {
        Ok(b) => b,
        Err(e) => {
            return Response {
                span: None,
                error: Some(error::internal(format!("read: {e}"))),
            };
        }
    };
    let (a, b) = target.byte_range;
    let slice = content
        .get(a as usize..b as usize)
        .unwrap_or(&[])
        .to_vec();
    let text = String::from_utf8_lossy(&slice).into_owned();
    Response {
        span: Some(Span {
            file: target.file,
            start_line: target.start_line,
            end_line: target.end_line,
            byte_range: target.byte_range,
            text,
            kind: SpanKind::Match,
            symbol: None,
            language: None,
            score: None,
            truncated: false,
            expand_handle: None,
        }),
        error: None,
    }
}
```

If `error::invalid_path` does not exist, use the existing closest equivalent — inspect `crates/argyph-mcp/src/error.rs` and either reuse `error::internal(...)` with code `ErrorCode::InvalidPath` directly via `McpErrorBody`, or add a new `pub fn invalid_path(msg: impl Into<String>) -> McpErrorBody` helper there mirroring the existing helpers.

- [ ] **Step 2: Register the tool**

In `crates/argyph-mcp/src/tools/mod.rs` add `pub mod expand_span;`.

In `crates/argyph-mcp/src/lib.rs` add inside `#[tool_router] impl ArgyphMcp`:

```rust
#[tool(
    name = "expand_span",
    description = "Fetch the full text behind an Argyph expand_handle issued earlier in this session. Use only when a previous `ask` result was truncated and you genuinely need the elided middle. Returns one Span. Handles expire after 10 minutes."
)]
async fn expand_span(
    &self,
    Parameters(req): Parameters<tools::expand_span::Request>,
) -> Json<tools::expand_span::Response> {
    let response = tools::expand_span::handle(&self.handles, &self.root, req).await;
    Json(response)
}
```

- [ ] **Step 3: Verify it compiles and registers**

Run: `cargo check -p argyph-mcp`
Expected: clean compile.

Run: `cargo test -p argyph-mcp` (existing smoke / handler tests; they should still pass).
Expected: green.

- [ ] **Step 4: Commit**

```bash
git add crates/argyph-mcp/src/tools/expand_span.rs crates/argyph-mcp/src/tools/mod.rs crates/argyph-mcp/src/lib.rs crates/argyph-mcp/src/error.rs
git commit -m "feat(mcp): add expand_span tool with handle resolution"
```

---

### Task A5: Adapt retrieval tools to enforce span cap

This task fans out across the eight retrieval handlers. Do them as a single commit because they share a helper. **Pack and overview are intentionally exempt** (their job is breadth).

**Files:**
- Create helper inside: `crates/argyph-mcp/src/span.rs` (add `to_span_from_*` adapters)
- Modify: `tools/search_text.rs`, `tools/search_semantic.rs`, `tools/find_definition.rs`, `tools/find_references.rs`, `tools/get_callers.rs`, `tools/get_callees.rs`, `tools/locate.rs`, `tools/locate_smart.rs`

- [ ] **Step 1: Add adapter helpers in `span.rs`**

Append to `crates/argyph-mcp/src/span.rs`:

```rust
use std::path::Path;

use crate::handles::{ExpandTarget, HandleStore};

/// Truncate a span's text against the per-span cap, registering a handle
/// if truncation occurred. Does not mutate line/byte ranges.
pub fn apply_span_cap(span: &mut Span, handles: &HandleStore) {
    let cap = max_span_lines();
    let (new_text, trunc) = truncate_lines(&span.text, cap);
    if trunc {
        let h = handles.issue(ExpandTarget {
            file: span.file.clone(),
            byte_range: span.byte_range,
            start_line: span.start_line,
            end_line: span.end_line,
        });
        span.expand_handle = Some(h);
    }
    span.text = new_text;
    span.truncated = span.truncated || trunc;
}

/// Read the file slice corresponding to a line range. Used by adapters
/// that have line numbers but not pre-loaded text.
pub fn read_line_range(root: &Path, file: &str, start_line: u32, end_line: u32) -> (String, (u64, u64)) {
    let full = root.join(file);
    let Ok(content) = std::fs::read_to_string(&full) else {
        return (String::new(), (0, 0));
    };
    let mut byte_start = 0u64;
    let mut byte_end = content.len() as u64;
    let mut current_line = 1u32;
    let mut cursor = 0usize;
    let bytes = content.as_bytes();
    while cursor < bytes.len() && current_line < start_line {
        if bytes[cursor] == b'\n' {
            current_line += 1;
        }
        cursor += 1;
    }
    byte_start = cursor as u64;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\n' {
            current_line += 1;
            if current_line > end_line {
                byte_end = (cursor + 1) as u64;
                break;
            }
        }
        cursor += 1;
    }
    let text = String::from_utf8_lossy(&bytes[byte_start as usize..byte_end as usize]).into_owned();
    (text, (byte_start, byte_end))
}
```

- [ ] **Step 2: Adapt each handler**

In each of the eight tool files, after constructing the existing response, add a step that converts each hit/definition/reference to a `Span` and calls `apply_span_cap`. Then apply `cap_total_lines` on the whole vector at the response boundary and set a `truncated` flag on the response.

For tools whose response shape already has `text` / `match_text` / `chunk_text`, set `span.text` from that. For tools whose response only has line/byte ranges (e.g., `find_definition` which returns `(u64, u64)` byte ranges), use `read_line_range` from Step 1 to materialize the text.

**Concrete pattern for `search_text.rs`** — replace the `Response::ok` body with:

```rust
fn ok(result: argyph_core::SearchResult, root: &Utf8PathBuf, handles: &HandleStore) -> Self {
    use crate::span::{apply_span_cap, cap_total_lines, Span, SpanKind, max_total_lines};
    let mut spans: Vec<Span> = result
        .hits
        .into_iter()
        .map(|h| {
            let (text, byte_range) = crate::span::read_line_range(
                root.as_std_path(),
                h.file.as_str(),
                h.line as u32,
                h.line as u32,
            );
            let mut s = Span {
                file: h.file.to_string(),
                start_line: h.line as u32,
                end_line: h.line as u32,
                byte_range,
                text,
                kind: SpanKind::Match,
                symbol: None,
                language: None,
                score: None,
                truncated: false,
                expand_handle: None,
            };
            apply_span_cap(&mut s, handles);
            s
        })
        .collect();
    let (kept, was_capped) = cap_total_lines(spans, max_total_lines());
    spans = kept;
    Self {
        spans: Some(spans),
        truncated: Some(result.truncated || was_capped),
        error: None,
    }
}
```

Update the `Response` struct of `search_text` additively. Keep the legacy
`hits` field so v1.0 callers do not break, and add the universal `spans`
field for v1.1 callers:

```rust
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Response {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hits: Option<Vec<SearchHit>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spans: Option<Vec<crate::span::Span>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpErrorBody>,
}
```

Do not delete the old `SearchHit` struct or old response fields in this release.
This is an additive compatibility change. Existing tool callers keep working;
new callers should prefer `spans`.

Update the `handle` signature to accept and pass `handles`:

```rust
pub async fn handle(
    supervisor: &Arc<Supervisor>,
    handles: &Arc<HandleStore>,
    root: &Utf8PathBuf,
    request: Request,
) -> Response { ... Response::ok(result, root, handles) ... }
```

And update the call site in `lib.rs`:

```rust
let response = tools::search_text::handle(&self.supervisor, &self.handles, &self.root, req).await;
```

**Apply the same additive pattern to the seven other handlers**, with these per-tool mappings:

| Tool                 | SpanKind     | Text source                              |
|----------------------|--------------|------------------------------------------|
| `search_semantic`    | `Match`      | `h.chunk_text` (already present)         |
| `find_definition`    | `Definition` | `read_line_range` from byte range → infer lines via file read |
| `find_references`    | `Reference`  | `read_line_range` from `(line, line)`    |
| `get_callers`        | `Call`       | `read_line_range` from each call site    |
| `get_callees`        | `Call`       | `read_line_range` from each call site    |
| `locate`             | `Locate`     | `s.content` (already present)            |
| `locate_smart`       | `Locate`     | `s.content` (already present)            |

For `find_definition`, line numbers come from byte ranges. Add a small helper in `span.rs`:

```rust
pub fn byte_range_to_lines(root: &Path, file: &str, byte_range: (u64, u64)) -> (u32, u32) {
    let full = root.join(file);
    let Ok(content) = std::fs::read(&full) else { return (1, 1); };
    let count_to = |limit: usize| -> u32 {
        let upto = limit.min(content.len());
        content[..upto].iter().filter(|&&b| b == b'\n').count() as u32 + 1
    };
    (count_to(byte_range.0 as usize), count_to(byte_range.1 as usize))
}
```

**For `locate` and `locate_smart`**, which already return their own span-shaped type: keep the existing wire shape but in addition produce the universal `Span` array via a helper. Add a `spans_v2: Vec<crate::span::Span>` field alongside the existing `spans` so callers of the old API keep working. For `locate_smart`, if the provider is disabled before any spans exist, return the existing error and an empty `spans_v2`; `ask(mode=smart)` falls back to semantic on that disabled error.

- [ ] **Step 3: Verify it all compiles**

Run: `cargo check -p argyph-mcp`
Expected: clean. Fix any borrow/type mismatch errors.

- [ ] **Step 4: Run the full test suite**

Run: `cargo test --workspace`
Expected: green. Some snapshot/integration tests may need their snapshots updated:
- If `insta` flags drift, inspect the new output and `cargo insta accept` only if the new output is correct per this plan.
- If a test asserts on `hits` directly, change it to assert on `spans`.

- [ ] **Step 5: Write a new integration test for the cap**

Create or extend the existing MCP smoke test in `crates/argyph-mcp/tests/` (find the right file via `ls crates/argyph-mcp/tests/`). Add a test that:
1. Boots a Supervisor on a fixture repo with one file >200 lines.
2. Calls `search_text` for a pattern that matches inside that file.
3. Asserts the returned span text has ≤80 lines and contains `[...N lines elided...]`.
4. Asserts the response has `truncated: Some(true)` if any span hit the cap.
5. Asserts the span has an `expand_handle` set.

If no fixture-based integration harness exists, add a minimal in-memory one using `Supervisor::boot` against a `tempfile::tempdir()` with one synthesized large file.

Run: `cargo test -p argyph-mcp --test '*'`
Expected: new test passes.

- [ ] **Step 6: Commit**

```bash
git add -u
git commit -m "feat(mcp): enforce span line cap across all retrieval tools"
```

---

## Phase B — The `ask` meta-tool

### Task B1: `ask` request/response types and routing skeleton

**Files:**
- Create: `crates/argyph-mcp/src/tools/ask.rs`
- Modify: `crates/argyph-mcp/src/tools/mod.rs`

- [ ] **Step 1: Write the failing test**

Create `crates/argyph-mcp/src/tools/ask.rs`:

```rust
use std::sync::Arc;

use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use argyph_core::Supervisor;

use crate::error::McpErrorBody;
use crate::handles::HandleStore;
use crate::span::Span;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Focus {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Auto,
    Structural,
    Semantic,
    Smart,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Request {
    pub query: String,
    #[serde(default)]
    pub focus: Option<Focus>,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    std::env::var("ARGYPH_ASK_DEFAULT_K")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8)
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Response {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spans: Option<Vec<Span>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy_used: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpErrorBody>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    Definition,
    Locate,
    Semantic,
    Smart,
}

/// Pure routing decision — exposed for unit testing.
pub fn decide_strategy(req: &Request) -> Strategy {
    match req.mode {
        Mode::Smart => Strategy::Smart,
        Mode::Structural => {
            if is_bare_identifier(&req.query) {
                Strategy::Definition
            } else {
                Strategy::Locate
            }
        }
        Mode::Semantic => Strategy::Semantic,
        Mode::Auto => {
            if is_bare_identifier(&req.query) {
                Strategy::Definition
            } else if looks_like_locator(&req.query) {
                Strategy::Locate
            } else {
                Strategy::Semantic
            }
        }
    }
}

fn is_bare_identifier(q: &str) -> bool {
    let q = q.trim();
    !q.is_empty()
        && q.chars().next().map_or(false, |c| c.is_ascii_alphabetic() || c == '_')
        && q.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn looks_like_locator(q: &str) -> bool {
    // file:Lnn, file:symbol, or glob with path separators or wildcards
    q.contains(":L") || q.contains('/') || q.contains('*') || q.contains('.')
}

pub async fn handle(
    _supervisor: &Arc<Supervisor>,
    _handles: &Arc<HandleStore>,
    _root: &Utf8PathBuf,
    _req: Request,
) -> Response {
    // Filled in by B2.
    Response {
        spans: Some(vec![]),
        strategy_used: Some("unimplemented".into()),
        truncated: Some(false),
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(query: &str, mode: Mode) -> Request {
        Request { query: query.into(), focus: None, mode, limit: 8 }
    }

    #[test]
    fn auto_bare_identifier_picks_definition() {
        assert_eq!(decide_strategy(&req("parseConfig", Mode::Auto)), Strategy::Definition);
    }

    #[test]
    fn auto_path_glob_picks_locate() {
        assert_eq!(decide_strategy(&req("src/**/foo.rs", Mode::Auto)), Strategy::Locate);
    }

    #[test]
    fn auto_natural_language_picks_semantic() {
        assert_eq!(
            decide_strategy(&req("where do we handle auth failures", Mode::Auto)),
            Strategy::Semantic
        );
    }

    #[test]
    fn explicit_smart_overrides_auto() {
        assert_eq!(decide_strategy(&req("parseConfig", Mode::Smart)), Strategy::Smart);
    }

    #[test]
    fn explicit_semantic_overrides_identifier_heuristic() {
        assert_eq!(decide_strategy(&req("parseConfig", Mode::Semantic)), Strategy::Semantic);
    }
}
```

Add `pub mod ask;` to `crates/argyph-mcp/src/tools/mod.rs`.

- [ ] **Step 2: Run the routing tests**

Run: `cargo test -p argyph-mcp ask::tests`
Expected: 5 passed.

- [ ] **Step 3: Commit**

```bash
git add crates/argyph-mcp/src/tools/ask.rs crates/argyph-mcp/src/tools/mod.rs
git commit -m "feat(mcp): add ask request/response types and routing decision"
```

---

### Task B2: `ask` dispatch to sub-handlers

**Files:**
- Modify: `crates/argyph-mcp/src/tools/ask.rs`

- [ ] **Step 1: Implement the body of `handle`**

Replace the stub `handle` in `crates/argyph-mcp/src/tools/ask.rs` with:

```rust
pub async fn handle(
    supervisor: &Arc<Supervisor>,
    handles: &Arc<HandleStore>,
    root: &Utf8PathBuf,
    req: Request,
) -> Response {
    let strategy = decide_strategy(&req);
    let label = match strategy {
        Strategy::Definition => "definition",
        Strategy::Locate => "locate",
        Strategy::Semantic => "semantic",
        Strategy::Smart => "smart",
    };
    let result: Result<Vec<Span>, McpErrorBody> = match strategy {
        Strategy::Definition => dispatch_definition(supervisor, handles, root, &req).await,
        Strategy::Locate => dispatch_locate(supervisor, handles, root, &req).await,
        Strategy::Semantic => dispatch_semantic(supervisor, handles, root, &req).await,
        Strategy::Smart => dispatch_smart(supervisor, handles, root, &req).await,
    };
    match result {
        Ok(spans) => {
            let (kept, truncated) = crate::span::cap_total_lines(spans, crate::span::max_total_lines());
            Response {
                spans: Some(kept),
                strategy_used: Some(label.into()),
                truncated: Some(truncated),
                error: None,
            }
        }
        Err(e) => Response { spans: None, strategy_used: Some(label.into()), truncated: None, error: Some(e) },
    }
}

async fn dispatch_definition(
    supervisor: &Arc<Supervisor>,
    handles: &Arc<HandleStore>,
    root: &Utf8PathBuf,
    req: &Request,
) -> Result<Vec<Span>, McpErrorBody> {
    let inner = super::find_definition::Request {
        name: req.query.clone(),
        language_hint: None,
        file_hint: req.focus.as_ref().and_then(|f| f.file.clone()),
    };
    let resp = super::find_definition::handle(supervisor, root, inner).await;
    if let Some(err) = resp.error {
        // Fall back to semantic for "not found" rather than surfacing an error.
        return dispatch_semantic(supervisor, handles, root, req).await.map_err(|_| err);
    }
    let defs = resp.definitions.unwrap_or_default();
    if defs.is_empty() {
        return dispatch_semantic(supervisor, handles, root, req).await;
    }
    Ok(defs
        .into_iter()
        .take(req.limit as usize)
        .map(|d| {
            let (s, e) = crate::span::byte_range_to_lines(
                root.as_std_path(),
                &d.location.file,
                d.location.range,
            );
            let (text, byte_range) =
                crate::span::read_line_range(root.as_std_path(), &d.location.file, s, e);
            let mut span = Span {
                file: d.location.file,
                start_line: s,
                end_line: e,
                byte_range,
                text,
                kind: crate::span::SpanKind::Definition,
                symbol: Some(d.name),
                language: d.language,
                score: None,
                truncated: false,
                expand_handle: None,
            };
            crate::span::apply_span_cap(&mut span, handles);
            span
        })
        .collect())
}

async fn dispatch_locate(
    supervisor: &Arc<Supervisor>,
    handles: &Arc<HandleStore>,
    root: &Utf8PathBuf,
    req: &Request,
) -> Result<Vec<Span>, McpErrorBody> {
    // Re-use the locate handler. It already returns span-shaped data.
    // We adapt to the universal Span.
    let inner = argyph_locate::Request {
        // Fill from req.query; locate accepts a natural-language or structured query.
        // The exact field names live in argyph_locate::Request — match them precisely.
        // If the field is `query`, use req.query.clone(). If it's a structured locator
        // struct, parse req.query into it.
        ..parse_locate_request(&req.query)
    };
    let resp = super::locate::handle(supervisor, root, inner).await;
    if let Some(err) = resp.error {
        return Err(err);
    }
    let spans = resp.spans.unwrap_or_default();
    Ok(spans
        .into_iter()
        .take(req.limit as usize)
        .map(|s| {
            let mut span = Span {
                file: s.file,
                start_line: s.line_range.0,
                end_line: s.line_range.1,
                byte_range: (s.byte_range.0 as u64, s.byte_range.1 as u64),
                text: s.content,
                kind: crate::span::SpanKind::Locate,
                symbol: s.path.last().cloned(),
                language: None,
                score: Some(s.score),
                truncated: s.truncated,
                expand_handle: None,
            };
            crate::span::apply_span_cap(&mut span, handles);
            span
        })
        .collect())
}

fn parse_locate_request(query: &str) -> argyph_locate::Request {
    // Inspect argyph_locate::Request in crates/argyph-locate/src/lib.rs and
    // construct it correctly. If it has a `query: String` field, set it.
    // If it requires a structured locator, parse `file:symbol` / `file:Lnn`
    // forms here; otherwise pass through as a natural-language query.
    argyph_locate::Request {
        query: query.to_string(),
        ..Default::default()
    }
}

async fn dispatch_semantic(
    supervisor: &Arc<Supervisor>,
    handles: &Arc<HandleStore>,
    root: &Utf8PathBuf,
    req: &Request,
) -> Result<Vec<Span>, McpErrorBody> {
    let inner = super::search_semantic::Request {
        query: req.query.clone(),
        k: req.limit as usize,
        alpha: 0.5,
        filter: None,
    };
    let resp = super::search_semantic::handle(supervisor, root, inner).await;
    if let Some(err) = resp.error {
        return Err(err);
    }
    let hits = resp.hits.unwrap_or_default();
    Ok(hits
        .into_iter()
        .map(|h| {
            // chunk_id encodes byte range in store; if not directly available,
            // compute lines from chunk_text length is unsafe. Use 1..= and let
            // truncate handle the rest. Adapters should be improved in a follow-up
            // once core::SemanticHit carries line/byte ranges.
            let mut span = Span {
                file: h.file,
                start_line: 1,
                end_line: 1 + h.chunk_text.lines().count() as u32,
                byte_range: (0, 0),
                text: h.chunk_text,
                kind: crate::span::SpanKind::Match,
                symbol: None,
                language: None,
                score: Some(h.score),
                truncated: false,
                expand_handle: None,
            };
            crate::span::apply_span_cap(&mut span, handles);
            span
        })
        .collect())
}

async fn dispatch_smart(
    supervisor: &Arc<Supervisor>,
    handles: &Arc<HandleStore>,
    root: &Utf8PathBuf,
    req: &Request,
) -> Result<Vec<Span>, McpErrorBody> {
    let inner = super::locate_smart::LocateSmartRequest {
        // Fill query field name from the actual struct.
        ..parse_smart_request(&req.query)
    };
    let resp = super::locate_smart::handle(supervisor, root, inner).await;
    // Adapt resp.spans → Vec<Span>. If locate_smart is disabled, fall back to semantic.
    if let Some(err) = resp.error.clone() {
        if matches!(err.code, crate::error::ErrorCode::Internal)
            && err.message.contains("disabled")
        {
            return dispatch_semantic(supervisor, handles, root, req).await;
        }
        return Err(err);
    }
    let spans = resp.spans.unwrap_or_default();
    Ok(spans
        .into_iter()
        .map(|s| {
            let mut span = Span {
                file: s.file,
                start_line: s.line_range.0,
                end_line: s.line_range.1,
                byte_range: (s.byte_range.0 as u64, s.byte_range.1 as u64),
                text: s.content,
                kind: crate::span::SpanKind::Locate,
                symbol: s.path.last().cloned(),
                language: None,
                score: Some(s.score),
                truncated: s.truncated,
                expand_handle: None,
            };
            crate::span::apply_span_cap(&mut span, handles);
            span
        })
        .collect())
}

fn parse_smart_request(query: &str) -> super::locate_smart::LocateSmartRequest {
    // Mirror parse_locate_request: inspect LocateSmartRequest fields and fill
    // the natural-language query field.
    super::locate_smart::LocateSmartRequest {
        query: query.to_string(),
        ..Default::default()
    }
}
```

**Note for the implementer:** the `..Default::default()` patterns in `parse_locate_request` and `parse_smart_request` assume those request types implement `Default`. They probably do not. Before writing the code above, open `crates/argyph-locate/src/lib.rs` and `crates/argyph-mcp/src/tools/locate_smart.rs` and look at the actual `Request` struct fields. Construct them with explicit field values rather than `..Default::default()`. The same applies to `super::find_definition::Request` and `super::search_semantic::Request` (those *are* visible above and you can see they take simple fields).

If you need to add `#[derive(Default)]` to a request struct (and all its fields are `Option<_>`-ish), do it as part of this task and note it in the commit message. Don't carry the technical debt.

- [ ] **Step 2: Register `ask` in `lib.rs`**

Add inside `#[tool_router] impl ArgyphMcp`:

```rust
#[tool(
    name = "ask",
    description = "PRIMARY ENTRY POINT for any code, symbol, file, or content lookup in this repo. Pass a natural-language question (\"where do we handle auth?\"), a bare identifier (\"parseConfig\"), or a locator (\"src/auth.rs:login\"). Returns minimal validated spans, never full files. Routes internally to symbol-graph, structural locate, or hybrid semantic search. **Do NOT use grep, find, or read entire files when `ask` will answer the question.**"
)]
async fn ask(
    &self,
    Parameters(req): Parameters<tools::ask::Request>,
) -> Json<tools::ask::Response> {
    let response = tools::ask::handle(&self.supervisor, &self.handles, &self.root, req).await;
    Json(response)
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p argyph-mcp`
Expected: clean. Fix any field-name mismatches against the actual `argyph_locate::Request` / `LocateSmartRequest` structs.

- [ ] **Step 4: Integration test**

Add a test to `crates/argyph-mcp/tests/` (same file as the A5 cap test, or a new `ask_test.rs`):

```rust
#[tokio::test]
async fn ask_routes_bare_identifier_to_definition() {
    let (sv, handles, root) = boot_fixture().await; // helper from A5 test
    let resp = argyph_mcp::tools::ask::handle(&sv, &handles, &root, argyph_mcp::tools::ask::Request {
        query: "main".into(),
        focus: None,
        mode: argyph_mcp::tools::ask::Mode::Auto,
        limit: 4,
    }).await;
    assert_eq!(resp.strategy_used.as_deref(), Some("definition"));
}
```

Note: this requires `pub` access on `argyph_mcp::tools` and `argyph_mcp::tools::ask`. Add `pub` modifiers as needed; this is acceptable because they were already exposed via `pub mod` from `tools::mod`. If they aren't yet, change `mod tools;` to `pub mod tools;` in `lib.rs`.

Run: `cargo test -p argyph-mcp --test '*'`
Expected: green.

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "feat(mcp): implement ask meta-tool dispatch (definition/locate/semantic/smart)"
```

---

## Phase C — Descriptions & Prompts

### Task C1: Rewrite all tool descriptions in "Use this when / Do not" voice

**Files:**
- Modify: `crates/argyph-mcp/src/lib.rs`

- [ ] **Step 1: Replace every `description = "..."` on every `#[tool(...)]` attribute** in `crates/argyph-mcp/src/lib.rs` with the values below.

```
get_index_status:
  "Use this when starting work in a fresh repo or after a long pause to confirm which tiers are ready. Returns tier readiness flags. Do not call on every query — it is cheap but redundant."

get_repo_overview:
  "Use this when you need a high-level shape of the codebase (languages, entry points, README excerpt). Returns a structured overview. Do not use this as a substitute for `ask` when looking up specific code — it is broad, not deep."

search_text:
  "Use this for literal/regex text search when you specifically need pattern-matching semantics (e.g., finding all TODO comments, all occurrences of a magic string). Returns line-anchored spans, capped. Do NOT use this to find a symbol by name — use `ask` instead, which is structural and returns more useful spans."

search_semantic:
  "Use this only when you need fuzzy semantic matching and you have already tried `ask`. Returns chunk-level spans ranked by hybrid BM25+vector. Most callers should use `ask` instead — `ask` calls this internally with better defaults."

find_definition:
  "Use this when you have a symbol id and need its definition only. For a name, use `ask` — it routes here automatically and falls back to semantic if the symbol is unknown. Returns one or more definition spans."

find_references:
  "Use this when you need every reference site for a known symbol id. Returns reference spans. For a name, prefer `ask` first."

get_callers:
  "Use this when tracing a call graph upward from a known function (who calls X?). Returns caller spans grouped by caller. Most exploratory callers should start with `ask`."

get_callees:
  "Use this when tracing a call graph downward from a known function (what does X call?). Returns callee spans. Most exploratory callers should start with `ask`."

get_imports:
  "Use this to enumerate imports for a file in both directions. Returns import edges. Niche; rarely the right first call."

get_symbol_outline:
  "Use this to get a hierarchical outline of a single file's symbols. Returns a tree of symbols with line ranges, no bodies. Cheap and bounded. Good follow-up after `ask` narrows you to a file."

pack_repo:
  "Use this ONLY when you genuinely need a flat dump of many files at once (e.g., feeding a fresh agent or generating a review packet). Returns a token-budgeted XML or Markdown bundle. For any single-question lookup, `ask` is dramatically cheaper."

locate:
  "Use this when you have a structured locator (path/glob, file:symbol, file:Lnn) or need structural search over markdown/JSON/YAML/TOML/CSV. Returns smallest natural spans. `ask` calls this internally when the query looks like a locator."

locate_smart:
  "Use this only when you have configured an LLM provider in argyph.toml and need multi-step retrieval. Bounded ReAct loop with span validation. `ask` calls this when mode=smart."

expand_span:
  "Fetch the full text behind an Argyph expand_handle issued earlier in this session. Use only when a previous result was truncated and you genuinely need the elided middle. Returns one Span. Handles expire after 10 minutes."

memory_save / memory_search / memory_list / memory_forget:
  Prepend "Use this for persistent agent memory across sessions (not code retrieval — that's `ask`)." to each existing description.
```

- [ ] **Step 2: Verify**

Run: `cargo check -p argyph-mcp && cargo test --workspace`
Expected: green. (Description text only — no behavior change.)

- [ ] **Step 3: Commit**

```bash
git add crates/argyph-mcp/src/lib.rs
git commit -m "docs(mcp): rewrite all tool descriptions in agent-affordance voice"
```

---

### Task C2: MCP Prompts

**Files:**
- Create: `crates/argyph-mcp/src/prompts.rs`
- Modify: `crates/argyph-mcp/src/lib.rs`

- [ ] **Step 1: Inspect the rmcp prompts API**

Run: `grep -rn "PromptCapability\|enable_prompts\|prompt\b" target/doc 2>/dev/null | head -10` — or simpler, `cargo doc -p rmcp --open` is unnecessary; instead `grep -rn "fn prompts\|trait.*Prompt" $(cargo metadata --format-version 1 | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')/../registry/src 2>/dev/null | head -20`.

If the rmcp version you depend on doesn't expose ergonomic prompt registration via macros, fall back to overriding `ServerHandler::list_prompts` and `ServerHandler::get_prompt` directly. (The current `impl rmcp::handler::server::ServerHandler for ArgyphMcp` block already exists in `lib.rs`.)

- [ ] **Step 2: Create `prompts.rs`**

```rust
pub const STANDING_INSTRUCTION: &str = "For any lookup of code, symbols, files, or content in this repo, prefer the `ask` tool over grep, find, or reading files directly. Argyph returns minimal validated spans, not full files.";

pub fn explore_codebase_body() -> String {
    format!(
        "{STANDING_INSTRUCTION}\n\nOrient on this codebase:\n1. Call `get_repo_overview` once.\n2. Use `ask` with focused questions about likely entry points (build commands, main modules, public API).\n3. Use `get_symbol_outline` on any file the overview points to."
    )
}

pub fn trace_symbol_body(symbol: &str) -> String {
    format!(
        "{STANDING_INSTRUCTION}\n\nTrace the symbol `{symbol}`:\n1. `ask` for `{symbol}` (mode=structural).\n2. `get_callers` on the definition.\n3. `get_callees` on the definition.\nReturn a span-only call-graph fragment."
    )
}

pub fn prepare_review_body() -> String {
    format!(
        "{STANDING_INSTRUCTION}\n\nPrepare a code review:\n1. Caller supplies a list of changed files.\n2. For each: `get_symbol_outline`.\n3. For each changed symbol: `ask` with its name to surface related code elsewhere in the repo.\nProduce a compact span-only review packet."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standing_instruction_appears_in_all_prompts() {
        assert!(explore_codebase_body().contains(STANDING_INSTRUCTION));
        assert!(trace_symbol_body("X").contains(STANDING_INSTRUCTION));
        assert!(prepare_review_body().contains(STANDING_INSTRUCTION));
    }

    #[test]
    fn trace_symbol_interpolates_the_symbol() {
        assert!(trace_symbol_body("foo_bar").contains("foo_bar"));
    }
}
```

- [ ] **Step 3: Register the prompts in `lib.rs`**

Add `mod prompts;` to `lib.rs`. Then implement the prompt-listing handler methods on `impl ServerHandler for ArgyphMcp`. The exact signatures are determined by the rmcp version in `Cargo.lock`. Likely shape:

```rust
fn list_prompts(...) -> ... { /* return ["explore_codebase", "trace_symbol", "prepare_review"] */ }
fn get_prompt(name, args) -> ... {
    match name {
        "explore_codebase" => prompts::explore_codebase_body(),
        "trace_symbol" => prompts::trace_symbol_body(args.get("symbol").unwrap_or("")),
        "prepare_review" => prompts::prepare_review_body(),
        _ => error
    }
}
```

Also flip the capabilities builder in `get_info`:

```rust
ServerCapabilities::builder().enable_tools().enable_prompts().build()
```

If `enable_prompts` does not exist in your rmcp version, use the most precise equivalent (look at `rmcp::model::ServerCapabilities` source).

- [ ] **Step 4: Verify**

Run: `cargo test -p argyph-mcp prompts::tests`
Expected: 2 passed.

Run: `cargo test --workspace`
Expected: green.

- [ ] **Step 5: Commit**

```bash
git add crates/argyph-mcp/src/prompts.rs crates/argyph-mcp/src/lib.rs
git commit -m "feat(mcp): register explore_codebase / trace_symbol / prepare_review prompts"
```

---

## Phase D — Reranker & `init`

### Task D1: Heuristic reranker module

**Files:**
- Create: `crates/argyph-store/src/rerank.rs`
- Modify: `crates/argyph-store/src/lib.rs` (add `pub mod rerank;`)

- [ ] **Step 1: Write the failing test**

Create `crates/argyph-store/src/rerank.rs`:

```rust
use crate::search::SearchHit;

#[derive(Debug, Clone, Default)]
pub struct FocusContext {
    /// File the caller is currently focused on (full repo-relative path).
    pub file: Option<String>,
    /// Symbol the caller is currently focused on.
    pub symbol: Option<String>,
    /// Module/directory of the focus (derived from `file` if not provided).
    pub module: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct Weights {
    pub base: f32,
    pub recency: f32,
    pub focus_call: f32,
    pub focus_module: f32,
    pub size_penalty: f32,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            base: 1.0,
            recency: 0.15,
            focus_call: 0.30,
            focus_module: 0.15,
            size_penalty: -0.10,
        }
    }
}

/// Signals provided by the caller (computed in `argyph-core`).
#[derive(Debug, Clone, Default)]
pub struct HitSignals {
    /// 1.0 if file was modified in the last 7 days, decaying to 0 over 90 days.
    pub recency: f32,
    /// 1.0 if hit's symbol is 0 or 1 hop from focus.symbol on the call graph, else 0.
    pub call_distance: f32,
    /// 1.0 if hit shares a module prefix with focus.module, else 0.
    pub module_match: f32,
    /// Size penalty in [0, 1]; 1.0 = huge file.
    pub size: f32,
}

pub fn rerank(mut hits: Vec<SearchHit>, signals: Vec<HitSignals>, w: Weights) -> Vec<SearchHit> {
    assert_eq!(hits.len(), signals.len(), "signals must align with hits");
    let mut scored: Vec<(SearchHit, f32)> = hits
        .drain(..)
        .zip(signals.into_iter())
        .map(|(h, s)| {
            let score = w.base * h.score
                + w.recency * s.recency
                + w.focus_call * s.call_distance
                + w.focus_module * s.module_match
                + w.size_penalty * s.size;
            (h, score)
        })
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().map(|(mut h, s)| { h.score = s; h }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::HitSource;

    fn h(id: &str, base: f32) -> SearchHit {
        SearchHit {
            chunk_id: id.into(),
            chunk_text: String::new(),
            file: format!("{id}.rs"),
            byte_range: (0, 0),
            line_range: (1, 1),
            score: base,
            source: HitSource::Hybrid,
        }
    }
    fn s(r: f32, c: f32, m: f32, sz: f32) -> HitSignals {
        HitSignals { recency: r, call_distance: c, module_match: m, size: sz }
    }

    #[test]
    fn focus_call_proximity_overcomes_base_score() {
        // hit B has lower base but 1-hop call distance to focus.
        let hits = vec![h("a", 1.0), h("b", 0.8)];
        let signals = vec![s(0.0, 0.0, 0.0, 0.0), s(0.0, 1.0, 0.0, 0.0)];
        let out = rerank(hits, signals, Weights::default());
        // a: 1.0 ; b: 0.8 + 0.3 = 1.1
        assert_eq!(out[0].chunk_id, "b");
    }

    #[test]
    fn no_signals_preserves_base_order() {
        let hits = vec![h("a", 1.0), h("b", 0.5)];
        let signals = vec![HitSignals::default(), HitSignals::default()];
        let out = rerank(hits, signals, Weights::default());
        assert_eq!(out[0].chunk_id, "a");
        assert_eq!(out[1].chunk_id, "b");
    }
}
```

Add `pub mod rerank;` to `crates/argyph-store/src/lib.rs`.

- [ ] **Step 2: Verify**

Run: `cargo test -p argyph-store rerank::tests`
Expected: 2 passed.

- [ ] **Step 3: Commit**

```bash
git add crates/argyph-store/src/rerank.rs crates/argyph-store/src/lib.rs
git commit -m "feat(store): add deterministic heuristic reranker"
```

---

### Task D2: Wire reranker into hybrid search via `argyph-core`

**Files:**
- Modify: `crates/argyph-core/src/index.rs`
- Modify: `crates/argyph-mcp/src/tools/search_semantic.rs` (accept optional focus)
- Modify: `crates/argyph-mcp/src/tools/ask.rs` (pass focus through to semantic dispatch)

- [ ] **Step 1: Extend the `Index::search_semantic` signature**

In `crates/argyph-core/src/index.rs`, locate the existing `search_semantic` method. Add an optional `focus: Option<&argyph_store::rerank::FocusContext>` parameter. After the underlying BM25+vector fusion produces `SearchHit`s, compute signals from data already in `Index` / `Store`:

- `recency`: read file `mtime` from the SQLite metadata (already stored). Fresh = 1.0; decay linearly to 0 over 90 days from now.
- `call_distance`: if `focus.symbol` is `Some`, query the symbol graph for 1-hop callers/callees of that symbol and check whether each hit's symbol id is in the set. If yes, 1.0 else 0.0.
- `module_match`: if `focus.module` or `focus.file` is `Some`, take the parent directory and check whether the hit's file path starts with it. 1.0 if match.
- `size`: clamp `file_size / 200_000` into `[0, 1]`.

Pass the assembled signals to `argyph_store::rerank::rerank`. Return the reranked hits in `SemanticResult`.

- [ ] **Step 2: Plumb focus through MCP**

In `crates/argyph-mcp/src/tools/search_semantic.rs` add an optional `focus: Option<Focus>` field on `Request` and pass through to core. In `crates/argyph-mcp/src/tools/ask.rs::dispatch_semantic`, populate that field from `req.focus`.

- [ ] **Step 3: Write an end-to-end test**

In `crates/argyph-store/tests/` (or wherever integration tests for hybrid live — confirm via `ls crates/argyph-store/tests/`), add a test that:
1. Indexes a tiny fixture with three files: `a/foo.rs`, `a/bar.rs`, `b/baz.rs`, each with a small symbol.
2. Touches `a/foo.rs`'s mtime to "very recent".
3. Runs hybrid search with `focus.file = "a/bar.rs"` (module = `a`).
4. Asserts `a/foo.rs` and `a/bar.rs` rank above `b/baz.rs` (module signal) — independent of base score.

If a clean test harness doesn't exist, build a thin one — keep it ≤80 LOC.

Run: `cargo test --workspace`
Expected: green.

- [ ] **Step 4: Commit**

```bash
git add -u
git commit -m "feat(core): apply heuristic reranker to hybrid search results"
```

---

### Task D3: `argyph init` writes CLAUDE.md / AGENTS.md block

**Files:**
- Modify: `crates/argyph-cli/src/cmds/init.rs`
- Modify: `crates/argyph-cli/src/lib.rs` (extend `Init` command args)

- [ ] **Step 1: Replace `init.rs` with the installer**

```rust
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const BEGIN: &str = "<!-- argyph:begin -->";
const END: &str = "<!-- argyph:end -->";

const BLOCK_BODY: &str = r#"## Code & context lookup

This repo is indexed by Argyph (MCP). For any lookup of code, symbols, files,
or content, prefer the `ask` tool over grep, find, or reading files directly.
Argyph returns minimal validated spans, not full files.

- `ask` — primary entry point. Pass a query and optional focus.
- `pack_repo` — only when you genuinely need a flat dump.
- Other Argyph tools — advanced, prefer `ask` first.
"#;

pub fn run(path: Option<&str>) -> ExitCode {
    let root: PathBuf = path.map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    if !root.is_dir() {
        eprintln!("init: not a directory: {}", root.display());
        return ExitCode::FAILURE;
    }

    let candidates = ["CLAUDE.md", "AGENTS.md", "GEMINI.md"];
    let existing: Vec<&str> = candidates
        .iter()
        .copied()
        .filter(|f| root.join(f).is_file())
        .collect();

    let targets: Vec<&str> = if existing.is_empty() {
        // Create CLAUDE.md by default.
        vec!["CLAUDE.md"]
    } else {
        existing
    };

    for name in targets {
        let path = root.join(name);
        if let Err(e) = ensure_argyph_block(&path) {
            eprintln!("init: failed to update {}: {}", path.display(), e);
            return ExitCode::FAILURE;
        }
        println!("init: updated {}", path.display());
    }
    ExitCode::SUCCESS
}

fn ensure_argyph_block(path: &Path) -> std::io::Result<()> {
    let mut content = if path.exists() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };
    let new_block = format!("{BEGIN}\n{BLOCK_BODY}{END}\n");
    if let (Some(b), Some(e)) = (content.find(BEGIN), content.find(END)) {
        // Replace in place. `e` is start-of-END; end-of-END = e + END.len().
        let end_full = e + END.len();
        let mut s = String::with_capacity(content.len() + new_block.len());
        s.push_str(&content[..b]);
        s.push_str(new_block.trim_end_matches('\n'));
        // Preserve trailing newline that follows the existing END if present.
        if end_full < content.len() {
            s.push_str(&content[end_full..]);
        } else {
            s.push('\n');
        }
        content = s;
    } else {
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        if !content.is_empty() {
            content.push('\n');
        }
        content.push_str(&new_block);
    }
    fs::write(path, content)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn creates_claude_md_when_none_exists() {
        let d = tempdir();
        let code = run(Some(d.path().to_str().unwrap()));
        assert_eq!(code, ExitCode::SUCCESS);
        let written = fs::read_to_string(d.path().join("CLAUDE.md")).unwrap();
        assert!(written.contains(BEGIN));
        assert!(written.contains("`ask`"));
        assert!(written.contains(END));
    }

    #[test]
    fn idempotent_on_rerun() {
        let d = tempdir();
        run(Some(d.path().to_str().unwrap()));
        run(Some(d.path().to_str().unwrap()));
        let written = fs::read_to_string(d.path().join("CLAUDE.md")).unwrap();
        let count = written.matches(BEGIN).count();
        assert_eq!(count, 1, "block should not duplicate");
    }

    #[test]
    fn preserves_surrounding_content() {
        let d = tempdir();
        let f = d.path().join("CLAUDE.md");
        let mut h = fs::File::create(&f).unwrap();
        writeln!(h, "# My project\n\nUnrelated content above.").unwrap();
        drop(h);
        run(Some(d.path().to_str().unwrap()));
        let written = fs::read_to_string(&f).unwrap();
        assert!(written.contains("Unrelated content above."));
        assert!(written.contains(BEGIN));
    }

    #[test]
    fn updates_all_existing_md_files() {
        let d = tempdir();
        fs::write(d.path().join("CLAUDE.md"), "# c\n").unwrap();
        fs::write(d.path().join("AGENTS.md"), "# a\n").unwrap();
        run(Some(d.path().to_str().unwrap()));
        assert!(fs::read_to_string(d.path().join("CLAUDE.md")).unwrap().contains(BEGIN));
        assert!(fs::read_to_string(d.path().join("AGENTS.md")).unwrap().contains(BEGIN));
    }
}
```

If `tempfile` is not already a dev-dependency of `argyph-cli`, add it:

```toml
[dev-dependencies]
tempfile = "3"
```

(Look at peer crates — `tempfile` is almost certainly already in the workspace.)

- [ ] **Step 2: Run the tests**

Run: `cargo test -p argyph-cli cmds::init::tests`
Expected: 4 passed.

- [ ] **Step 3: Commit**

```bash
git add -u
git commit -m "feat(cli): argyph init installs Argyph block into CLAUDE.md/AGENTS.md"
```

---

## Finalization

### Task F1: Documentation

**Files:**
- Modify: `README.md`, `ARCHITECTURE.md`, `ROADMAP.md`, `CHANGELOG.md`
- Create if absent: `docs/tools-reference.md`

- [ ] **Step 1: Update READMEs and reference docs**

- `README.md`: Add `ask` as the headline tool in the "What it does" section. Add a short "Quick start" line showing `argyph init && claude mcp add argyph -- npx @argyph/server`.
- `ARCHITECTURE.md`: Add a subsection under §2 titled "The meta-tool layer" describing the `ask` router, the `Span` contract, and the truncation policy.
- `docs/tools-reference.md`: Add full schemas for `ask` and `expand_span`. Update `search_text` / `search_semantic` / `find_*` / `get_*` entries to reflect the new `spans` response field.
- `ROADMAP.md`: Move Context Discipline items into "Shipped — v1.1" header (new section above v1.0.0-rc.1's Shipped section, or wherever the project convention dictates).
- `CHANGELOG.md`: Add a v1.1 section listing the six features.

- [ ] **Step 2: Commit**

```bash
git add README.md ARCHITECTURE.md docs/tools-reference.md ROADMAP.md CHANGELOG.md
git commit -m "docs: document ask, expand_span, span contract, and reranker for v1.1"
```

### Task F2: Full validation

- [ ] **Step 1: Full check + test**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green.

- [ ] **Step 2: Manual smoke test**

```bash
cargo build --release
cd /tmp && mkdir argyph-smoke && cd argyph-smoke
git init && echo "fn main() {}" > main.rs && git add . && git commit -m init
/path/to/target/release/argyph init
test -f CLAUDE.md && grep -q argyph:begin CLAUDE.md && echo "init OK"
/path/to/target/release/argyph serve &
SERVE_PID=$!
# (manually verify with an MCP client that `ask` returns spans capped at 80 lines)
kill $SERVE_PID
```

Expected: `init OK` printed; `argyph serve` exits cleanly on SIGTERM.

- [ ] **Step 3: Final commit if needed (formatting or doc fixes)**

```bash
git add -u && git commit -m "chore: post-validation fixes for v1.1"
```

---

## Self-Review Notes

- Every spec section (§3.1–§3.6) maps to at least one task: 3.1→B1+B2, 3.2→A1+A2+A4+A5, 3.3→C2, 3.4→C1, 3.5→D1+D2, 3.6→D3.
- Phase A is intentionally the foundation: A5 reshapes every retrieval tool's response, and B1/B2 build on it.
- The Phase B routing logic is exposed as `decide_strategy` so it can be unit-tested without booting a Supervisor.
- One known soft spot: D2 (reranker plumbing in `argyph-core`) depends on the symbol graph exposing 1-hop neighbors — verify `argyph-graph::graph::SymbolOutline` or sibling APIs offer this before writing the test; if not, add the helper there as part of D2 rather than splitting into a separate task.
- The `..Default::default()` shorthand in B2 (`parse_locate_request`, `parse_smart_request`) is a hazard — the plan flags it explicitly and instructs the implementer to verify field shapes before using it.
