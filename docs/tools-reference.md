# Argyph MCP Tools Reference

This is the canonical reference for every MCP tool exposed by Argyph. Each entry covers the request schema, the response schema, the minimum index tier required, the error codes the tool may return, and at least one full example.

The schemas are normative. The Rust code in `crates/argyph-mcp/src/tools/` is generated against (or validated against) these contracts. If the code and this document disagree, the document is correct and the code is wrong.

---

## Conventions

- All requests and responses are JSON. Field names are `snake_case`.
- All paths are repo-relative, normalized to forward slashes, UTF-8.
- All ranges are `[start_line, end_line]` inclusive, 1-indexed (`[1, 1]` is the first line).
- All timestamps are ISO-8601 with timezone (`2026-05-09T14:21:00Z`).
- The `index_coverage` field on search responses is a float in `[0.0, 1.0]` indicating the fraction of the eligible corpus that was searchable at query time. `1.0` means the relevant tier is fully built.
- Errors use the shape in [§ Errors](#errors). Tools never throw raw panics across the MCP boundary.

### Common types

```ts
type Filter = {
  languages?: string[];        // e.g. ["rust", "typescript"]
  paths_glob?: string[];       // e.g. ["src/**", "!**/test/**"]
  exclude_glob?: string[];
};

type SymbolKind =
  | "function" | "method" | "struct" | "class"
  | "enum" | "trait" | "interface" | "constant"
  | "variable" | "type_alias" | "module" | "macro";

type SourceRange = {
  file: string;                // repo-relative path
  range: [number, number];     // [start_line, end_line], 1-indexed inclusive
};

type Symbol = {
  symbol_id: string;           // opaque, stable across runs (content-addressed)
  name: string;
  kind: SymbolKind;
  signature: string;           // human-readable, language-rendered
  location: SourceRange;
  language: string;
};

type Span = {
  file: string;
  start_line: number;          // 1-indexed inclusive
  end_line: number;            // 1-indexed inclusive
  byte_range: [number, number];
  text: string;                // capped by ARGYPH_MAX_SPAN_LINES
  kind: "definition" | "reference" | "match" | "outline" | "locate" | "call";
  symbol?: string;
  language?: string;
  score?: number;
  truncated: boolean;
  expand_handle?: string;      // pass to expand_span within this MCP session
};
```

Retrieval tools expose `Span` results additively beside their legacy fields.
Default caps are 80 lines per span and 400 total lines per response.
When a natural result exceeds the per-span cap, Argyph returns the first 40
and last 20 lines with an elision marker and issues an `expand_handle`.

---

## ask

Primary entry point for code, symbol, file, or content lookup. `ask` routes a
bare identifier to symbol definition lookup, a locator-like query to `locate`,
`mode=smart` to `locate_smart`, and natural-language queries to semantic
search. It returns bounded spans and reports the strategy that actually served
the response.

Request:

```json
{
  "query": "parseConfig",
  "focus": { "file": "src/config.rs", "symbol": "load_config", "line": 42 },
  "mode": "auto",
  "limit": 8
}
```

Response:

```json
{
  "spans": [
    {
      "file": "src/config.rs",
      "start_line": 10,
      "end_line": 24,
      "byte_range": [120, 480],
      "text": "fn parse_config(...) { ... }",
      "kind": "definition",
      "symbol": "parse_config",
      "language": "rust",
      "truncated": false
    }
  ],
  "strategy_used": "definition",
  "truncated": false
}
```

## expand_span

Fetches the full text behind an `expand_handle` issued earlier in the same MCP
session. Handles are in-memory and expire after 10 minutes.

Request:

```json
{ "handle": "eh_..." }
```

Response:

```json
{ "span": { "...": "full span text, not elided" } }
```

### Errors

Every tool returns either a result or:

```json
{
  "error": {
    "code": "INDEX_NOT_READY",
    "message": "Symbol index not yet built; available in ~3 seconds",
    "retryable": true,
    "retry_after_ms": 3000,
    "correlation_id": "01HZ7..."
  }
}
```

Stable error codes:

| Code                      | Meaning                                                                   |
|---------------------------|---------------------------------------------------------------------------|
| `INDEX_NOT_READY`         | Required tier hasn't built yet. `retryable: true`.                        |
| `INVALID_PATH`            | Path is malformed, traverses outside repo root, or doesn't exist.         |
| `OUT_OF_BUDGET`           | `pack_repo` couldn't fit the requested scope under the budget.            |
| `EMBED_PROVIDER_ERROR`    | Remote embedding provider returned an error (rate limit, auth, etc.).     |
| `LANGUAGE_UNSUPPORTED`    | A tool was given a language Argyph doesn't have a parser for.            |
| `SYMBOL_NOT_FOUND`        | A graph query was given a symbol that doesn't exist in the index.         |
| `SYMBOL_AMBIGUOUS`        | Multiple symbols match; caller must disambiguate via `language_hint`/`file`. |
| `INTERNAL`                | Unexpected internal error. `retryable: false`. File a bug.                |
| `LOCATE_SMART_DISABLED`  | `locate_smart` is not enabled in the configuration. `retryable: false`.     |
| `LOCATE_SMART_BUDGET_EXCEEDED` | The ReAct loop exceeded its step or token budget. `retryable: false`. |
| `PROVIDER_ERROR`          | LLM provider call failed (auth, rate limit, network). `retryable: true`.   |

---

## Status & lifecycle

### `get_index_status`

Reports the readiness of all index tiers and recent watcher activity. Cheap; safe to poll.

**Tier required:** none.

**Request:**

```json
{}
```

**Response:**

```json
{
  "root": "/Users/Ezzy1630/code/myrepo",
  "schema_version": 1,
  "tiers": {
    "files":       { "ready": true,  "count": 4321,  "duration_ms":   312 },
    "symbols":     { "ready": true,  "count": 18452, "duration_ms": 11240 },
    "structural":  { "ready": true,  "count":  1234, "duration_ms":  1800 },
    "embeddings":  { "ready": false, "embedded": 6234, "total": 18452, "progress": 0.34 }
  },
  "watcher": { "active": true, "events_last_hour": 14 },
  "last_change": "2026-05-09T14:21:00Z",
  "embed_provider": { "kind": "local", "model": "bge-small-en-v1.5" }
}
```

**Errors:** none expected.

---

## Repo overview

### `get_repo_overview`

A repo-shaped summary served from Tier 0 only. Useful for "what does this codebase do?" at the start of a session.

**Tier required:** 0.

**Request:**

```json
{ "max_tree_depth": 3 }
```

- `max_tree_depth`: optional int, clamps the rendered tree depth. Default 3, max 6.

**Response:**

```json
{
  "languages": [
    { "name": "rust", "files": 102, "loc": 18432 },
    { "name": "typescript", "files": 38, "loc": 4120 }
  ],
  "entry_points": ["src/main.rs", "src/bin/migrate.rs"],
  "readme_excerpt": "# myrepo\n\nA tiny example...",
  "tree": "src/\n  auth/\n    session.rs\n    middleware.rs\n  ...",
  "git": {
    "branch": "main",
    "head_short": "a1b2c3d",
    "dirty": false
  }
}
```

The `git` block is omitted if the repo is not a git checkout.

---

## Search

### `search_text`

Pure ripgrep-style regex/literal search. Available immediately at Tier 0.

**Tier required:** 0.

**Request:**

```json
{
  "pattern": "TODO\\(.*\\)",
  "regex": true,
  "case_sensitive": false,
  "max_results": 100,
  "filter": { "paths_glob": ["src/**"] }
}
```

- `pattern`: the search pattern. Required.
- `regex`: bool, default `false` (literal match).
- `case_sensitive`: bool, default `false`.
- `max_results`: int, clamped to `[1, 1000]`. Default 100.
- `filter`: optional `Filter`.

**Response:**

```json
{
  "hits": [
    {
      "file": "src/auth/session.rs",
      "line": 42,
      "column": 8,
      "match": "TODO(Ezzy1630): rotate keys",
      "context_before": ["fn rotate_session_key() {"],
      "context_after": ["    todo!()"]
    }
  ],
  "truncated": false
}
```

### `search_semantic`

Hybrid (BM25 + vector) search over AST-aware chunks. Returns whatever is currently embedded; reports `index_coverage` so the agent can decide whether to retry.

**Tier required:** 0 for partial results, 2 for `index_coverage = 1.0`.

**Request:**

```json
{
  "query": "session expiration timeout duration",
  "k": 10,
  "alpha": 0.5,
  "filter": {
    "languages": ["rust"],
    "paths_glob": ["src/**"]
  }
}
```

- `query`: natural-language or code-flavored query. Required.
- `k`: int, clamped to `[1, 100]`. Default 10.
- `alpha`: float in `[0.0, 1.0]`. `0.0` is pure BM25, `1.0` is pure vector. Default 0.5.
- `filter`: optional `Filter`.

**Response:**

```json
{
  "hits": [
    {
      "chunk_id": "src/auth/session.rs:38:52",
      "chunk_text": "pub struct SessionConfig { ttl: Duration, ... }",
      "file": "src/auth/session.rs",
      "byte_range": [840, 1210],
      "line_range": [38, 52],
      "score": 0.87,
      "source": "hybrid"
    }
  ],
  "spans": [
    {
      "file": "src/auth/session.rs",
      "start_line": 38,
      "end_line": 52,
      "byte_range": [840, 1210],
      "text": "pub struct SessionConfig { ttl: Duration, ... }",
      "kind": "match",
      "score": 0.87,
      "truncated": false
    }
  ],
  "truncated": false,
  "index_coverage": 1.0,
  "total_embedded": 18452,
  "total_chunks": 18452
}
```

**Errors:** `INDEX_NOT_READY` if Tier 0 hasn't completed. `EMBED_PROVIDER_ERROR` if the configured remote provider fails on the query embed.

---

## Precise locate

### `locate`

Return the smallest natural span containing the requested structured locator or natural-language query. Works on code, markdown, JSON, YAML, TOML, and CSV.

**Tier required:** 1.5 for structural path/search, 2 for semantic and hybrid queries.

**Request:**

```json
{
  "path": "database/host",
  "file": "config/app.json"
}
```

- `query`: optional natural-language query for semantic or hybrid search.
- `path`: optional structured locator (e.g. `"docs/billing.md > Enterprise"`, `"package.name"`, `"*"`).
- `file`: optional single file to scope the search to.
- `files`: optional glob patterns to filter candidate files (mutually exclusive with `file`).
- `max_results`: int, clamped to `[1, 10]`. Default 3.
- `max_bytes_per_span`: int, max bytes per returned span. Default 4096.

At least one of `query` or `path` is required. If both are provided with `file`, a scoped semantic search is performed.

**Strategy dispatch:**

| Inputs | Strategy |
|--------|----------|
| `path` only | `structural_path` |
| `query` only (short, ≤3 words) | `structural_search` |
| `query` only (long, with Tier 2) | `hybrid` |
| `query` only (long, without Tier 2) | `semantic` |
| `query` + `path` + `file` | `scoped_semantic` |

**Response:**

```json
{
  "spans": [
    {
      "file": "config/app.json",
      "byte_range": [120, 280],
      "line_range": [8, 12],
      "kind": "pair",
      "path": ["database", "host"],
      "content": "{\n  \"host\": \"localhost\",\n  \"port\": 5432\n}",
      "score": 1.0,
      "truncated": false,
      "expand_to": {
        "parent": { "node_id": "sn_42", "label": "database", "bytes": 512 },
        "file": null
      }
    }
  ],
  "strategy_used": "structural_path",
  "index_coverage": { "tier_1_5": "ready", "tier_2": "ready" }
}
```

On error, `spans` is null and `error` is populated:

```json
{
  "error": {
    "code": "INVALID_PATH",
    "message": "INVALID_ARGUMENT: query or path required",
    "retryable": false,
    "retry_after_ms": null
  }
}
```

**Errors:** `INVALID_PATH` for invalid arguments. `INDEX_NOT_READY` if the required tier isn't built. `INTERNAL` for unexpected errors.

### `locate_smart`

Retrieval subagent that runs a bounded multi-step search loop. Requires `[locate_smart]` configuration; returns `LOCATE_SMART_DISABLED` otherwise.

**Tier required:** 1.5 + configured LLM provider.

**Request:**

```json
{
  "query": "section about custom limits for enterprise pricing",
  "max_steps": 4,
  "max_output_tokens": 1024
}
```

- `query`: required. Natural-language query.
- `max_steps`: int, default 4, max 10. Maximum ReAct loop iterations.
- `max_output_tokens`: int, default 1024. Token budget for the model response.

**Response (success):**

```json
{
  "spans": [...],
  "strategy_used": "smart",
  "reasoning_summary": "Found enterprise pricing section via locate",
  "steps_taken": 2,
  "index_coverage": {"tier_1_5": "ready", "tier_2": "ready"}
}
```

**Response (disabled):**

```json
{
  "error": {
    "code": "LOCATE_SMART_DISABLED",
    "message": "locate_smart is disabled in this Argyph configuration",
    "retryable": false
  }
}
```

**Errors:** `LOCATE_SMART_DISABLED`, `LOCATE_SMART_BUDGET_EXCEEDED`, `PROVIDER_ERROR`, `INDEX_NOT_READY`, `INTERNAL`.

---

## Symbol graph

### `find_definition`

Locate the definition of a symbol by name. May return multiple definitions if the name is ambiguous.

**Tier required:** 1.

**Request:**

```json
{
  "name": "SessionConfig",
  "language_hint": "rust",
  "file_hint": "src/auth/session.rs"
}
```

- `name`: required.
- `language_hint`: optional; narrows results.
- `file_hint`: optional; preferred-file disambiguation.

**Response:**

```json
{
  "definitions": [
    {
      "symbol_id": "sym_8a3f...",
      "name": "SessionConfig",
      "kind": "struct",
      "signature": "pub struct SessionConfig { pub ttl: Duration, pub idle_timeout: Duration }",
      "location": { "file": "src/auth/session.rs", "range": [12, 30] },
      "language": "rust",
      "docstring": "Configuration for an authenticated session."
    }
  ]
}
```

**Errors:** `SYMBOL_NOT_FOUND` if no match. The tool does not error on ambiguity — it returns multiple definitions and lets the caller decide.

---

### `find_references`

Reference sites for a symbol, with surrounding context lines.

**Tier required:** 1.

**Request:**

```json
{
  "symbol_id": "sym_8a3f...",
  "context_lines": 2,
  "max_results": 100
}
```

Or, by name:

```json
{
  "name": "DEFAULT_SESSION_TTL",
  "language_hint": "rust",
  "context_lines": 2
}
```

**Response:**

```json
{
  "references": [
    {
      "file": "src/middleware/auth.rs",
      "range": [82, 82],
      "snippet": "let ttl = DEFAULT_SESSION_TTL;",
      "context_before": ["fn build_cookie() -> Cookie {"],
      "context_after": ["    Cookie::new(\"sid\", id).max_age(ttl)"]
    }
  ],
  "truncated": false
}
```

**Notes:** Cross-file resolution is best-effort, not LSP-precise. See [`crates/argyph-graph/MODULE.md`](../crates/argyph-graph/MODULE.md) for current accuracy bounds.

---

### `get_callers`

Functions that call a given function.

**Tier required:** 1.

**Request:**

```json
{ "symbol_id": "sym_8a3f..." }
```

Or by name:

```json
{ "name": "validateUser", "language_hint": "typescript" }
```

**Response:**

```json
{
  "callers": [
    {
      "caller": {
        "symbol_id": "sym_b1c2...",
        "name": "loginHandler",
        "kind": "function",
        "location": { "file": "src/api/login.ts", "range": [10, 45] }
      },
      "call_sites": [
        { "file": "src/api/login.ts", "range": [22, 22] }
      ]
    }
  ]
}
```

---

### `get_callees`

Functions a given function calls. Same shape as `get_callers`, with the roles flipped (`callee` and `call_sites`).

**Tier required:** 1.

---

### `get_imports`

Files imported by a given file, and files that import the given file.

**Tier required:** 1.

**Request:**

```json
{ "file": "src/auth/session.rs" }
```

**Response:**

```json
{
  "imports":     ["src/auth/types.rs", "src/util/clock.rs"],
  "imported_by": ["src/middleware/auth.rs", "src/main.rs"]
}
```

---

### `get_symbol_outline`

Hierarchical outline of a file: top-level symbols and their children.

**Tier required:** 1.

**Request:**

```json
{ "file": "src/auth/session.rs" }
```

**Response:**

```json
{
  "file": "src/auth/session.rs",
  "language": "rust",
  "outline": [
    {
      "symbol_id": "sym_8a3f...",
      "name": "SessionConfig",
      "kind": "struct",
      "range": [12, 30],
      "children": []
    },
    {
      "symbol_id": "sym_9d22...",
      "name": "Session",
      "kind": "struct",
      "range": [34, 90],
      "children": [
        {
          "symbol_id": "sym_9d23...",
          "name": "is_expired",
          "kind": "method",
          "range": [55, 62],
          "children": []
        }
      ]
    }
  ]
}
```

---

## Packing

### `pack_repo`

Token-budgeted, repomix-style flattening of a repo or subset. Multiple output formats. Prioritization heuristic: entry points → READMEs → recently modified → rest.

**Tier required:** 0 (1 helps prioritization).

**Request:**

```json
{
  "scope": { "paths": ["src/auth/", "tests/auth/"] },
  "format": "xml",
  "token_budget": 30000,
  "include": { "tests": false, "docs": true }
}
```

- `scope`:
  - `"all"` — entire repo (subject to budget)
  - `{ "paths": string[] }` — specific paths
  - `{ "symbol": string, "language_hint": string }` — pack everything connected to a symbol via the graph (definition file + immediate references + immediate callees)
- `format`: `"xml" | "markdown"`. Default `"xml"`.
- `token_budget`: int, clamped to `[1000, 200000]`.
- `include.tests`: bool, default `false`.
- `include.docs`: bool, default `true`.

**Response:**

```json
{
  "format": "xml",
  "content": "<repo>...</repo>",
  "token_count": 28341,
  "files_included": 14,
  "files_truncated": 2,
  "files_skipped": [
    { "path": "src/auth/legacy.rs", "reason": "over_budget" }
  ]
}
```

**Errors:** `OUT_OF_BUDGET` if even the priority-1 set exceeded the budget.

---

## File access

### `read_file_range`

A safer alternative to dumping whole files; agents are encouraged to read symbol-bounded ranges discovered via the graph rather than going via shell.

**Tier required:** 0.

**Request:**

```json
{
  "file": "src/auth/session.rs",
  "range": [38, 52]
}
```

- `file`: required, validated to be inside the indexed root.
- `range`: optional. If omitted, returns the whole file (subject to a hard size cap, default 5 MB).

**Response:**

```json
{
  "file": "src/auth/session.rs",
  "range": [38, 52],
  "content": "...",
  "language": "rust",
  "truncated": false
}
```

**Errors:** `INVALID_PATH` if the path is outside the indexed root, malformed, or doesn't exist.

---

## MCP resources

In addition to tools, Argyph exposes a small set of MCP resources. These are cacheable, agent-readable URIs that don't require an explicit tool call:

| URI                  | Equivalent to            |
|----------------------|--------------------------|
| `argyph://overview` | `get_repo_overview`      |
| `argyph://status`   | `get_index_status`       |
| `argyph://config`   | Effective merged config (no secrets) |

Resources are read-only and have the same content the equivalent tool would return.

---

## MCP prompts

A small set of opinionated prompts that demonstrate the tools well. These ship with the server and are surfaced in clients that support MCP prompts.

| Prompt              | Purpose                                                            |
|---------------------|--------------------------------------------------------------------|
| `explore_codebase`  | A guided "give me the lay of the land" walk through the repo.     |
| `trace_symbol`      | "Trace this function from definition through callers and refs."   |
| `prepare_review`    | "Pack relevant context for reviewing this PR diff."               |

Each prompt gives the client a compact retrieval recipe built around `ask`; the client still invokes the tools.

---

## Memory

### `memory_save`

**Request**

```ts
{
  scope: string;                          // any identifier; "repo" default in clients
  content: string;
  metadata?: Record<string, string>;
}
```

**Response**

```ts
{ id?: string; error?: McpError }
```

`id` is the memory's content-addressed identifier. Use it with `memory_forget`.

### `memory_search`

**Request**

```ts
{
  query: string;
  scope?: string;                         // omit to search all scopes
  k?: number;                             // default 10, clamped to [1, 100]
}
```

**Response**

```ts
{
  hits: Array<{
    id: string;
    scope: string;
    content: string;
    metadata: Record<string, string>;
    created_at: string;                   // ISO-8601
  }>;
  error?: McpError;
}
```

Backed by SQLite FTS5 over `content`.

### `memory_list`

**Request**

```ts
{ scope: string }
```

**Response**

```ts
{
  hits: Array<{ id; scope; content; metadata; created_at }>;
  error?: McpError;
}
```

### `memory_forget`

**Request**

```ts
{ id: string }
```

**Response**

```ts
{ forgotten: boolean; error?: McpError }
```

`forgotten: false` when no memory with that `id` existed.

---

## Validation rules (applied before any tool runs)

- All paths are normalized to repo-relative, UTF-8. `..` traversal outside the repo root is hard-rejected.
- `k` clamped to `[1, 100]`.
- `token_budget` clamped to `[1000, 200000]`.
- `max_results` clamped to `[1, 1000]` for `search_text`.
- Glob patterns are validated and bounded; pathological patterns are rejected.
- Strings have max lengths matching MCP input expectations (8 KB per string field unless documented otherwise).
- The schema version is reported in `get_index_status.schema_version`. Agents may use this to adapt to non-breaking schema additions.

---

## Versioning

Tool schemas are versioned with the project under SemVer:

- Adding a new tool is a **minor** version bump.
- Adding a new optional field to a request or a new field to a response is a **minor** bump.
- Renaming or removing a field, changing a field's type, or changing default behavior is a **major** bump.

Pre-1.0, schemas may evolve without strict SemVer. After 1.0, the contracts in this document are stable and any breaking change requires a new major version.
