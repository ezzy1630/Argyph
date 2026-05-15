# AI Agent Workflow

Argyph is built primarily by a single human (Ezzy1630) using AI coding agents (Claude Code and similar) as a force multiplier. This document captures the workflow that keeps the codebase small, well-bounded, and reviewable when AI is doing significant code generation.

If you read nothing else, read § 1 (operating principles) and § 6 (when to start a fresh chat).

---

## 1. Operating principles

### Principle 1 — One module per session

Never give an agent more than one crate's scope in a single conversation. If a feature spans two crates (e.g. adding a tool requires a new method in `argyph-store` and a new handler in `argyph-mcp`), do it in two passes: first the trait + impl in the lower crate, ship and test, then the handler that consumes it.

### Principle 2 — Trait first, implementation second

When introducing new behavior, the prompt order is:

1. Define the trait.
2. Write the failing test against the trait.
3. Implement the trait.
4. Wire it up.

This naturally constrains the agent's scope and prevents it from wandering into adjacent files.

### Principle 3 — Generation size cap

Aim for **100–300 lines of new Rust per agent generation**. Not because more doesn't compile, but because review fatigue is real and bugs hide in long generations. If the agent wants to write 800 lines, that is a signal the task is too big — split it.

### Principle 4 — Test before merge, every time

Every agent-produced PR must include unit tests for new behavior plus an integration test if it touched MCP tools. CI enforces this via a coverage diff check.

### Principle 5 — Fresh chat at module boundaries

When moving from `argyph-fs` to `argyph-parse`, start a new chat. Carry over only the artifact summary (the relevant `MODULE.md` plus the trait signatures the new module will use). Don't carry the whole conversation; context drift is real and cumulative.

### Principle 6 — Human-authored decisions

Architecture, design, trade-offs, cuts, and the call on what to ship are human work. The AI is a code generator and a sounding board, not an architect. Don't outsource the parts that matter most.

---

## 2. The prompt template

The canonical agent task prompt template lives at [`agent-prompts/template.md`](agent-prompts/template.md). Use it verbatim for every milestone task. It encodes:

- The role and architecture context.
- The crate's `OWNS` and `MUST NEVER OWN` boundaries from `MODULE.md`.
- Allowed and forbidden dependencies.
- The specific milestone task.
- Hard constraints (no new top-level deps, no `unwrap()`, module size cap, etc.).
- A two-step process where the agent proposes signatures *first*, the human approves, and only then does the agent implement.

The two-step process is the most important part of the template. Most AI failures happen because the agent dives into implementation with the wrong mental model. Catching it at the signatures stage is cheap.

---

## 3. Review checkpoints

For every PR, run this mental checklist (or rely on the PR template, which encodes it):

- [ ] Module ownership respected — no leakage into adjacent crates?
- [ ] Public surface documented (rustdoc on every new pub item)?
- [ ] Tests cover the new behavior?
- [ ] No new top-level dependencies added unilaterally?
- [ ] No `unwrap()` outside tests?
- [ ] No `unsafe` outside the ONNX FFI module?
- [ ] Errors typed at the crate boundary with `thiserror`?
- [ ] No regression in `criterion` benchmarks?
- [ ] Module files stay under 600 lines?
- [ ] Commit message follows Conventional Commits?
- [ ] Attribution trailer (if any) complies with `COMMIT_CONVENTIONS.md` § 2?

---

## 4. Anti-spaghetti safeguards (encoded as lints + CI)

These exist because AI agents reliably violate them without enforcement.

- `clippy::pedantic` selectively enabled.
- `clippy::unwrap_used` — error.
- `clippy::expect_used` — warn (allowed with `// SAFETY:` comment).
- `#![forbid(unsafe_code)]` per crate, with one allowlisted exception in `argyph-embed` for ONNX FFI.
- Module length limit: enforced via a custom CI script that fails on `*.rs > 600 lines`.
- `cargo deny` enforces no banned crates and license compatibility.
- A custom CI script that fails if `argyph-mcp` imports any non-`argyph-core` / `argyph-{domain}` symbols directly — handlers stay thin.

---

## 5. Rules AI agents must NEVER violate

These are the same rules listed in `CONTRIBUTING.md` § 4. They are non-negotiable. If your AI agent generates a PR that violates one of these, reject the PR and re-prompt.

1. **NEVER** edit `crates/argyph-core/src/supervisor.rs` without an issue + design discussion.
2. **NEVER** edit `crates/argyph-store/src/schema.rs` or `migrations/` directly. Schema changes are new migration files.
3. **NEVER** add a new top-level workspace dependency without prior approval.
4. **NEVER** call `unsafe` outside the ONNX FFI module.
5. **NEVER** spawn long-running tasks outside `Supervisor::spawn`.
6. **NEVER** mix concerns across crate boundaries.
7. **NEVER** put business logic in MCP tool handlers — handlers dispatch only.
8. **NEVER** assume the index is fully built; tools handle partial-index states.
9. **NEVER** log file contents or query strings at INFO level.
10. **NEVER** make breaking changes to MCP tool schemas without bumping the major version.

---

## 6. When to start a fresh chat

- ✅ Moving to a new crate.
- ✅ After a milestone is merged.
- ✅ When the agent starts repeating itself or contradicting earlier decisions.
- ✅ Before any "let's refactor" task.
- ✅ After ~50–80 turns in the same chat, regardless of progress.

When you start a fresh chat, paste:

1. The relevant `MODULE.md`.
2. The trait signatures the new module will consume from other crates.
3. The current milestone task.
4. The prompt template from `agent-prompts/template.md`.

That is enough context. Don't paste the whole previous conversation; the agent will read intent into stale context.

---

## 7. When to refactor

- After every milestone, before tagging.
- Never mid-feature.
- Refactor PRs are *non-functional by definition* — they touch zero behavior. CI runs the full integration suite to prove it.

---

## 8. Working with multiple AI tools

You may use multiple AI tools (Claude Code for implementation, ChatGPT for design discussion, Cursor inline for tab completion). The attribution policy in [`COMMIT_CONVENTIONS.md`](COMMIT_CONVENTIONS.md) § 2 applies the same way to all of them: attribution trailers are reserved for substantial generative contributions, not routine assistance.

Do not stack multiple `Co-authored-by:` trailers for AI tools. If two tools meaningfully contributed, pick the one whose contribution was most substantial and attribute that one.

---

## 9. What does NOT belong in this workflow

- Asking an AI to make architectural decisions for you.
- Asking an AI to choose what to ship in a milestone.
- Pasting the whole codebase into the context window — by design, individual modules fit in a single context.
- Letting the AI write the commit message wholesale. Curate it.
- Letting the AI generate the changelog wholesale. Curate it.
- Letting the AI write the README's pitch. Curate it. The voice of the project is yours.

These are the things that, if outsourced to an AI, dilute the engineering signal of the project. Don't outsource them.

---

## 10. Honest self-assessment

A useful end-of-week question: *if a senior engineer reviewed every PR I shipped this week, what would they think?* If the answer is "this is thoughtful, well-bounded systems work," you are using the workflow correctly. If the answer is "this person is shipping AI output without judgment," reconsider.

The workflow is intentionally designed so that the answer should be the first thing.
