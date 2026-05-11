# Argyph

> The local-first MCP server for serious codebases.
> Zero config, zero cloud, full context.

[![CI](https://github.com/Ezzy1630/argyph/actions/workflows/ci.yml/badge.svg)](https://github.com/Ezzy1630/argyph/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/argyph.svg)](https://crates.io/crates/argyph)
[![npm](https://img.shields.io/npm/v/@argyph/server.svg)](https://www.npmjs.com/package/@argyph/server)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![SafeSkill 93/100](https://img.shields.io/badge/SafeSkill-93%2F100_Verified%20Safe-brightgreen)](https://safeskill.dev/scan/ezzy1630-argyph)

Argyph is a single MCP server that gives any AI coding agent fast, structured, and semantic context over a codebase. It runs entirely on your machine, indexes incrementally, and is ready in under a second on previously-indexed repos.

The name is a portmanteau of **Argus** (the hundred-eyed watcher of Greek myth) and **Glyph** (a carved symbol with bound meaning) — a server that watches a codebase and gives the agent its symbols.

---

## What it does

Argyph replaces the half-dozen MCP servers most developers stitch together (one for grep, one for embeddings, one for symbol search, one for repo packing) with a single tool. It exposes three pillars of context behind one MCP endpoint:

1. **File and symbol intelligence** — a tree-sitter-driven symbol graph with `find_definition`, `find_references`, callers, callees, imports, and outline tools. Structural queries return in milliseconds.
2. **Semantic search** — hybrid (BM25 + vector) search over AST-aware chunks, backed by an embedded LanceDB store. Bundled local embedding model means no API key is required for full functionality.
3. **Repo packing** — token-budgeted, repomix-style flattening of a repo or subset for agents that need to absorb a codebase quickly.

Everything is read-only. Argyph never edits, commits, or executes code.

---

## Why local-first

Most existing context servers depend on a cloud vector database (Milvus, Pinecone) and a remote embedding API. That's a non-starter for proprietary code at most companies, and a tax on cold starts everywhere else. Argyph runs entirely on the developer's machine: a single binary, an embedded vector store, an optional bundled embedding model, no daemon, no account, no key required to get full functionality.

---

## Install

### Claude Code

```bash
claude mcp add argyph -- npx @argyph/server@latest
```

### npm / npx

```bash
npx @argyph/server
```

### Cargo

```bash
cargo install argyph
```

### Claude Desktop (DXT)

Download `argyph.dxt` from the [latest release](https://github.com/Ezzy1630/argyph/releases/latest) and double-click.

---

## Quick start

```bash
# In any repo
cd ~/code/your-repo
claude mcp add argyph -- npx @argyph/server@latest
claude
```

In the chat:

> What does this codebase do, and where is session expiration controlled?

Argyph indexes Tier 0 in under a second on first run, Tier 1 (symbol graph) in seconds, and Tier 2 (embeddings) in the background. You can query immediately — tools return what's available now plus an `index_coverage` field so the agent knows.

---

## How it works

Argyph builds the index in three tiers, each useful before the next completes:

| Tier | What it builds                                | Time on a 1M-LOC repo | Useful for                                  |
|------|-----------------------------------------------|------------------------|---------------------------------------------|
| 0    | File inventory, hashes, .gitignore-aware tree | <1 s                   | Tree views, ripgrep, packing                |
| 1    | Symbol graph (defs, refs, calls, imports)     | ~30 s                  | Go-to-def, find-references, call graphs     |
| 2    | Embeddings + hybrid index                     | minutes (background)   | Fuzzy semantic queries                      |

The 80/20 insight: most agent queries are structural (`where is parseConfig defined?`, `what calls validateUser?`) and don't need embeddings. Argyph serves those from Tier 1 in milliseconds, even while Tier 2 is still building.

After the first index, the on-disk `.argyph/` directory persists and only changed files are reprocessed on subsequent runs. A filesystem watcher keeps everything live.

---

## Tools

| Tool                  | Description                                              | Tier required |
|-----------------------|----------------------------------------------------------|---------------|
| `get_index_status`    | Tier readiness, embedding progress, watcher state        | 0             |
| `get_repo_overview`   | Languages, entry points, README excerpt, tree            | 0             |
| `search_text`         | Ripgrep-style regex / literal search                     | 0             |
| `find_definition`     | Locate the definition of a named symbol                  | 1             |
| `find_references`     | Reference sites with surrounding context                 | 1             |
| `get_callers`         | Functions that call a given function                     | 1             |
| `get_callees`         | Functions a given function calls                         | 1             |
| `get_imports`         | Imports of a file, and files that import it              | 1             |
| `get_symbol_outline`  | Hierarchical outline of a file                           | 1             |
| `search_semantic`     | Hybrid BM25 + vector over AST-aware chunks               | 2             |
| `pack_repo`           | Token-budgeted repo flattening (XML or markdown)         | 0+1           |
| `read_file_range`     | Bounded file read by symbol range                        | 0             |
| `reindex`             | Force a full or partial reindex                          | —             |

Full schema reference: [docs/tools-reference.md](docs/tools-reference.md).

---

## Configuration

Config is layered (highest priority first): env vars, `.argyph/config.toml` in the repo, built-in defaults. A config file is never required.

```bash
ARGYPH_LOG=info
ARGYPH_EMBED_PROVIDER=local        # local | openai | voyage
OPENAI_API_KEY=...                  # standard provider env vars
ARGYPH_DISABLE_WATCHER=true         # for sandboxed environments
```

Generate a starter config:

```bash
argyph init
```

---

## Why Argyph (vs alternatives)

| Tool                       | Symbol graph | Semantic search | Local-first | Single install | Incremental |
|----------------------------|:------------:|:---------------:|:-----------:|:--------------:|:-----------:|
| claude-context (Zilliz)    |              | yes             |             | yes            | yes         |
| GitNexus / CodeGraphContext| yes          |                 |             |                |             |
| repomix                    |              |                 | yes         | yes            |             |
| Serena                     | yes          |                 | yes         | yes            |             |
| **Argyph**                 | **yes**      | **yes**         | **yes**     | **yes**        | **yes**     |

---

## Benchmarks

Reproducible numbers, methodology in [`benches/README.md`](benches/README.md). Reported on M2 Max, macOS 14:

| Workload                                | Argyph     | claude-context | repomix    |
|-----------------------------------------|------------|----------------|------------|
| Cold index, 1M LOC TS monorepo          | _TBD_      | _TBD_          | n/a        |
| Warm start (already indexed)            | _<1 s_     | _TBD_          | n/a        |
| Search latency (semantic), p50          | _<50 ms_   | _TBD_          | n/a        |
| `find_definition`, p99                  | _<10 ms_   | n/a            | n/a        |

Numbers will be filled in as `benches/` is built out in Phase 2 of the build plan.

---

## Architecture

Argyph is a Rust workspace of nine focused crates with strict module ownership. The full architecture, including the Supervisor lifecycle, the three-tier indexing model, and per-crate responsibility boundaries, is documented in [ARCHITECTURE.md](ARCHITECTURE.md).

---

## Project status

**Pre-1.0.** This is alpha software. APIs may change. The build plan and milestones are in [`docs/BUILD_PLAN.md`](docs/BUILD_PLAN.md). Current milestone is tracked at the top of [ROADMAP.md](ROADMAP.md).

---

## Contributing

Argyph is built with substantial AI assistance, but human-architected and human-reviewed. Contribution guide and the (strict) AI agent rules are in [CONTRIBUTING.md](CONTRIBUTING.md). Commit conventions, including the project's attribution policy, are in [`docs/COMMIT_CONVENTIONS.md`](docs/COMMIT_CONVENTIONS.md).

---

## Author

Built by [Ezzy1630](https://github.com/Ezzy1630). See [AUTHORS.md](AUTHORS.md).

---

## License

Dual-licensed under either of:

- MIT License ([LICENSE-MIT](LICENSE-MIT))
- Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.
