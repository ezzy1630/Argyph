# @argyph/server

Local-first MCP server giving AI coding agents fast, structured, and semantic context over any codebase. No API keys, no cloud, no daemon — a single Rust binary with an embedded vector store and a bundled ONNX embedder.

This npm package is a thin shim: on `npm install`, it downloads the
prebuilt `argyph` binary for your platform from the matching GitHub
release, verifies the SHA256, and places it on your `PATH`.

## Install

```bash
# One-shot (no global install)
npx @argyph/server

# Or, persistent
npm install -g @argyph/server
```

## Quick start

```bash
# Inside any repo
argyph init                                  # writes agent instructions
claude mcp add argyph -- npx @argyph/server  # register with Claude Code
```

Then ask your agent any structural ("where is `parseConfig` defined?")
or semantic ("how does session expiry work?") question — Argyph indexes
the repo on first run and serves queries while higher tiers continue
building in the background.

## Supported platforms

- macOS arm64 / x64
- Linux x64 / arm64 (glibc)
- Windows x64

## More info

- Repository, docs, full tool reference: [github.com/Ezzy1630/argyph](https://github.com/Ezzy1630/argyph)
- Tool schema: [docs/tools-reference.md](https://github.com/Ezzy1630/argyph/blob/main/docs/tools-reference.md)
- Architecture: [ARCHITECTURE.md](https://github.com/Ezzy1630/argyph/blob/main/ARCHITECTURE.md)

## License

MIT OR Apache-2.0
