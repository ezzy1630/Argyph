use rmcp::model::{PromptMessage, PromptMessageRole};

pub const STANDING_INSTRUCTION: &str = "For any lookup of code, symbols, files, or content in this repo, prefer the `ask` tool over grep, find, or reading files directly. Argyph returns minimal validated spans, not full files.";

pub fn explore_codebase_body() -> String {
    format!(
        "{STANDING_INSTRUCTION}\n\nOrient on this codebase:\n1. Call `get_repo_overview` once.\n2. Use `ask` with focused questions about likely entry points (build commands, main modules, public API).\n3. Use `get_symbol_outline` on any file the overview points to."
    )
}

pub fn trace_symbol_body(symbol: &str) -> String {
    format!(
        "{STANDING_INSTRUCTION}\n\nTrace the symbol `{symbol}`:\n1. `ask` for `{symbol}` with mode=structural.\n2. `find_definition` if you need the exact symbol id.\n3. `get_callers` and `get_callees` on the definition.\nReturn a span-only call graph fragment."
    )
}

pub fn prepare_review_body(base_ref: Option<&str>) -> String {
    let base = base_ref
        .map(|base| format!(" Base reference supplied by caller: `{base}`."))
        .unwrap_or_default();
    format!(
        "{STANDING_INSTRUCTION}\n\nPrepare a code review:{base}\n1. Caller supplies changed files or a diff summary.\n2. For each changed file, call `get_symbol_outline`.\n3. For changed symbols, call `ask` to surface related code elsewhere in the repo.\nProduce a compact span-only review packet."
    )
}

pub fn user_message(body: String) -> Vec<PromptMessage> {
    vec![PromptMessage::new_text(PromptMessageRole::User, body)]
}
