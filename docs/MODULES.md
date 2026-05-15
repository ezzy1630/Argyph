# Modules Overview

Argyph is a Cargo workspace of 10 crates with strict, single-responsibility ownership. This document is the index. Each crate has a per-crate `MODULE.md` that is the source of truth for its boundaries.

The point of this strictness is not aesthetic. It is operational:

- It keeps each module small enough to fit in a single AI-agent chat session.
- It makes contribution scope obvious: "I am working in `argyph-fs`, so I cannot touch SQLite."
- It makes review fast: a reviewer reads only the touched crate's `MODULE.md` to evaluate scope creep.
- It makes refactors low-risk: the trait boundary between crates is the renegotiation surface.

When something is hard to fit into one crate, the right response is almost never "blur the boundary." It is "the trait surface needs a new method," or "this is genuinely a new responsibility and warrants its own crate."

---

## Crate map

| Crate            | Layer        | Owns (one-line summary)                                               |
|------------------|--------------|----------------------------------------------------------------------|
| `argyph`         | binary       | Entry point, dispatches to `serve` or CLI subcommands                 |
| `argyph-cli`     | UI           | CLI commands, output formatting, progress bars                        |
| `argyph-mcp`     | UI           | MCP server, thin tool handlers, schema validation                     |
| `argyph-core`    | orchestration| Supervisor lifecycle, three-tier orchestration, config, `Index` facade |
| `argyph-pack`    | domain       | Repo packing, format rendering, token-budgeted prioritization         |
| `argyph-graph`   | domain       | Symbol graph construction, edge resolution, graph queries             |
| `argyph-parse`   | domain       | Tree-sitter parsing, language packs, chunking, symbol extraction       |
| `argyph-embed`   | domain       | Embedding provider abstraction (local ONNX + remote HTTP)             |
| `argyph-store`   | infra        | LanceDB integration, SQLite metadata, schema, hybrid search           |
| `argyph-fs`      | infra        | Filesystem walking, ignore rules, watching, hashing                    |

Layer hierarchy (top depends on bottom):

```
binary (argyph)
   ↓
UI layer (argyph-cli, argyph-mcp)
   ↓
orchestration (argyph-core)
   ↓
domain (argyph-pack, argyph-graph, argyph-parse, argyph-embed)
   ↓
infra (argyph-store, argyph-fs)
```

A crate may depend on the layer below it. It must not depend on a sibling in the same layer except through a trait defined in the layer below or in `argyph-core`.

---

## Per-crate MODULE.md files

| Crate            | MODULE.md path                                |
|------------------|-----------------------------------------------|
| `argyph`         | [`crates/argyph/MODULE.md`](../crates/argyph/MODULE.md)             |
| `argyph-cli`     | [`crates/argyph-cli/MODULE.md`](../crates/argyph-cli/MODULE.md)     |
| `argyph-mcp`     | [`crates/argyph-mcp/MODULE.md`](../crates/argyph-mcp/MODULE.md)     |
| `argyph-core`    | [`crates/argyph-core/MODULE.md`](../crates/argyph-core/MODULE.md)   |
| `argyph-pack`    | [`crates/argyph-pack/MODULE.md`](../crates/argyph-pack/MODULE.md)   |
| `argyph-graph`   | [`crates/argyph-graph/MODULE.md`](../crates/argyph-graph/MODULE.md) |
| `argyph-parse`   | [`crates/argyph-parse/MODULE.md`](../crates/argyph-parse/MODULE.md) |
| `argyph-embed`   | [`crates/argyph-embed/MODULE.md`](../crates/argyph-embed/MODULE.md) |
| `argyph-store`   | [`crates/argyph-store/MODULE.md`](../crates/argyph-store/MODULE.md) |
| `argyph-fs`      | [`crates/argyph-fs/MODULE.md`](../crates/argyph-fs/MODULE.md)       |

---

## How to read a `MODULE.md`

Every `MODULE.md` follows the same structure:

1. **Purpose.** One paragraph: why this crate exists.
2. **Owns.** Specific responsibilities. If something is on this list, this crate is the only one that can implement it.
3. **Must never own.** Specific anti-responsibilities. If something is on this list, the agent rejects PRs that try to add it here.
4. **Public surface.** The exported traits, types, and functions other crates depend on. Changing this surface is a coordinated change across multiple PRs.
5. **Internal structure.** What modules live inside the crate, in one or two sentences each.
6. **Failure modes.** Known cases where AI agents tend to break the boundaries; how to recognize and prevent them.
7. **Honest limitations.** Things this crate intentionally does poorly so it can do its core job well.
8. **Stability.** What is locked, what is open to change.

If you are reviewing a PR, the fastest sanity check is: does the diff add anything that the touched crate's "Must never own" list forbids?

---

## When to add a new crate

Adding a new crate to the workspace is a meaningful decision that requires an issue and a design discussion. Add a new crate when:

- A new responsibility emerges that genuinely doesn't fit any existing `Owns` list.
- A subset of an existing crate has grown large and is independently testable.
- The new responsibility has a clear trait surface to its consumers.

Do not add a new crate just because:

- A file got long. Split the file inside the existing crate first.
- "It seems cleaner." Aesthetics are not a justification.
- An AI agent suggested it. Agents over-eagerly suggest crate splits.

Adding a new crate also means: writing its `MODULE.md`, updating this index, updating `ARCHITECTURE.md`, updating the workspace `Cargo.toml`, updating CI matrix paths, and adding it to the release artifacts list.

---

## Recipes

Step-by-step recipes for the most common contributions live next to this file:

- **Add a new MCP tool** — `recipes/add-tool.md` (Phase 1+)
- **Add a new language pack** — `recipes/add-language.md` (Phase 2+)
- **Add a new embedding provider** — `recipes/add-embed-provider.md` (Phase 3+)

These recipes exist so that contributions stay structurally consistent across the project.
