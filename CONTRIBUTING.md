# Contributing to Argyph

Thanks for considering a contribution. Argyph is built with substantial AI assistance but human-architected, human-reviewed, and human-maintained. The rules below exist so the codebase stays small, well-bounded, and reviewable.

If you read nothing else, read these three sections:

- [§ 1 — Before you open a PR](#1-before-you-open-a-pr)
- [§ 4 — AI Agent Rules](#4-ai-agent-rules)
- [§ 5 — Commit attribution policy](#5-commit-attribution-policy)

---

## 1. Before you open a PR

1. Open an issue first for anything beyond a typo or one-line fix. We'd rather discuss the design than reject a finished PR.
2. Read [`ARCHITECTURE.md`](ARCHITECTURE.md). If your change isn't described there, the architecture probably needs to be updated first.
3. Read the [`MODULE.md`](docs/MODULES.md) for the crate you're touching. Each crate has a strict ownership list and a "must never own" list.
4. Run `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test` locally before pushing.

---

## 2. Setup

```bash
git clone https://github.com/Ezzy1630/argyph.git
cd argyph
rustup show                  # ensure toolchain matches rust-toolchain.toml
cargo build
cargo test
```

Optional dev tooling:

```bash
cargo install cargo-deny     # license + advisory checks
cargo install cargo-nextest  # faster test runs
cargo install cargo-dist     # release pipeline (maintainers only)
```

---

## 3. Code style and lint rules

- `cargo fmt` is enforced in CI.
- `cargo clippy -- -D warnings` is enforced in CI.
- `unwrap()` is forbidden outside tests (lint-enforced). Use `expect("…")` with a comment if you must.
- `unsafe` is forbidden outside the ONNX FFI module (lint-enforced via `#![forbid(unsafe_code)]` per crate, with one allowlisted exception in `argyph-embed`).
- Errors are typed with `thiserror` at every crate boundary. `anyhow` is allowed only in the binary entry point and tests.
- Public functions are documented with rustdoc and at least one example.
- Module files are capped at 600 lines (CI-enforced via a custom script). Split when you exceed.

---

## 4. AI Agent Rules

Argyph welcomes AI-assisted contributions, but the project's design depends on tight constraints that AI agents reliably violate when given too much rope. The following rules are non-negotiable. Violating any of them is grounds for rejecting a PR, regardless of how good the code looks.

1. **NEVER** edit `crates/argyph-core/src/supervisor.rs` without a linked issue and design discussion.
2. **NEVER** edit `crates/argyph-store/src/schema.rs` or `crates/argyph-store/src/migrations/` directly. Schema changes are new migration files, never edits to existing ones.
3. **NEVER** add a new top-level workspace dependency without prior approval in an issue.
4. **NEVER** call `unsafe` outside the ONNX FFI module.
5. **NEVER** spawn long-running tasks outside `Supervisor::spawn`.
6. **NEVER** mix concerns across crate boundaries — see each crate's `MODULE.md`.
7. **NEVER** put business logic in MCP tool handlers. Handlers are <100 lines and dispatch only; logic lives in the relevant domain crate.
8. **NEVER** assume the index is fully built. Tools must handle partial-index states and return `INDEX_NOT_READY` when appropriate.
9. **NEVER** log file contents, search query strings, or API keys at INFO level. INFO-level logs are public-shareable.
10. **NEVER** make breaking changes to MCP tool schemas without bumping the major version and updating `docs/tools-reference.md`.

If you're prompting an AI to write code for this repo, paste these rules into the prompt. The full prompt template is in [`docs/agent-prompts/template.md`](docs/agent-prompts/template.md). Read [`docs/AGENT_WORKFLOW.md`](docs/AGENT_WORKFLOW.md) for the broader workflow guidance.

---

## 5. Commit attribution policy

Argyph is the personal work of [Ezzy1630](https://github.com/Ezzy1630), built with AI assistance. We are honest about that, but we are also clear-eyed about credit. The detailed commit message format and the rules for when (and when not) to attribute Claude or any other tool are in [`docs/COMMIT_CONVENTIONS.md`](docs/COMMIT_CONVENTIONS.md).

The short version:

- The default `Author:` of every commit is the human contributor. There is no scenario where an AI agent is the named author of a commit.
- `Co-authored-by:` trailers for AI tools are reserved for commits where the AI did substantial generative work that the human reviewed and integrated. Most commits should not carry such a trailer. Routine assistance (autocomplete, small refactors, doc polish, syntax fixes) does not warrant attribution.
- Decisions, architecture, design, code review, and the choice of what to ship are human work. The repo, the README, the `AUTHORS.md`, and the public-facing record reflect that.

This is not just style. The point of building Argyph is partly to demonstrate engineering judgment. Diluting that signal with reflexive AI attribution undermines the whole exercise.

---

## 6. PR review process

Every PR runs through the following checks (most automated):

- [ ] Module ownership respected — no leakage into adjacent crates
- [ ] Public surface documented (rustdoc on new pub items)
- [ ] Tests cover new behavior (unit + integration if MCP-touching)
- [ ] No new top-level dependencies without prior approval
- [ ] No `unwrap()` / no `unsafe` (lint-enforced)
- [ ] Errors typed at crate boundary with `thiserror`
- [ ] No regression in `criterion` benchmarks > 20%
- [ ] Module files stay under 600 lines
- [ ] Commit message follows the [Conventional Commits](https://www.conventionalcommits.org) format
- [ ] Attribution trailers comply with [`docs/COMMIT_CONVENTIONS.md`](docs/COMMIT_CONVENTIONS.md)

PRs are squash-merged. The final commit message is curated by the maintainer.

---

## 7. Extension points

Argyph's common extension points each live in one crate. Use the
existing implementations next to the one you're adding as the template,
and consult [`docs/MODULES.md`](docs/MODULES.md) and
[`ARCHITECTURE.md`](ARCHITECTURE.md) for the surrounding contracts.

- **Add a new MCP tool** — `crates/argyph-mcp`. Implement the tool
  handler and register it in the tool list; mirror an existing tool of
  the same tier.
- **Add a new language pack** — `crates/argyph-parse` (tree-sitter
  grammar, symbol/chunk queries) and the `Language` enum in
  `crates/argyph-fs`. Copy an existing language end-to-end.
- **Add a new embedding provider** — `crates/argyph-embed`. Implement
  the provider trait alongside the `local`, `openai`, and `voyage`
  providers.

Dedicated step-by-step recipe docs are planned; until they land, the
existing implementations are the reference.

---

## 8. Reporting bugs and security issues

- Bugs: open an issue using the bug report template. Include OS, Argyph version, repo size, and a correlation ID from the logs.
- Security: see [`SECURITY.md`](SECURITY.md). Do not file public issues for security vulnerabilities.

---

## 9. Code of Conduct

We follow the Contributor Covenant. See [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).
