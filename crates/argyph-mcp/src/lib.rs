#![forbid(unsafe_code)]

pub mod error;
pub mod handles;
pub mod prompts;
pub mod span;
pub mod types;
pub mod validate;

pub mod tools;

use std::sync::Arc;

use camino::Utf8PathBuf;
use rmcp::{
    handler::server::router::prompt::PromptRouter,
    handler::server::{wrapper::Json, wrapper::Parameters},
    model::{
        GetPromptRequestParams, GetPromptResult, ListPromptsResult, PaginatedRequestParams,
        PromptMessage,
    },
    prompt, prompt_handler, prompt_router,
    service::serve_server,
    service::RequestContext,
    tool, tool_handler, tool_router, RoleServer,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::handles::HandleStore;
use argyph_core::Supervisor;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
struct PrepareReviewPromptArgs {
    #[serde(default)]
    base_ref: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
struct TraceSymbolPromptArgs {
    symbol: String,
}

#[derive(Clone)]
struct ArgyphMcp {
    supervisor: Arc<Supervisor>,
    root: Arc<Utf8PathBuf>,
    handles: Arc<HandleStore>,
    prompt_router: PromptRouter<Self>,
}

#[prompt_router]
#[tool_router]
impl ArgyphMcp {
    #[prompt(
        name = "explore_codebase",
        description = "Orient on this repo using bounded Argyph lookup tools."
    )]
    async fn explore_codebase_prompt(&self) -> Vec<PromptMessage> {
        prompts::user_message(prompts::explore_codebase_body())
    }

    #[prompt(
        name = "trace_symbol",
        description = "Trace a symbol through definitions, callers, and callees."
    )]
    async fn trace_symbol_prompt(
        &self,
        Parameters(args): Parameters<TraceSymbolPromptArgs>,
    ) -> Vec<PromptMessage> {
        prompts::user_message(prompts::trace_symbol_body(&args.symbol))
    }

    #[prompt(
        name = "prepare_review",
        description = "Prepare a compact span-only code review packet."
    )]
    async fn prepare_review_prompt(
        &self,
        Parameters(args): Parameters<PrepareReviewPromptArgs>,
    ) -> Vec<PromptMessage> {
        prompts::user_message(prompts::prepare_review_body(args.base_ref.as_deref()))
    }

    #[tool(
        name = "ask",
        description = "Use this when doing any code, symbol, file, or content lookup in this repo. Returns minimal validated spans, never full files. Do not use grep, find, or read entire files when `ask` will answer the question. Tier requirement: 0/1/1.5/2."
    )]
    async fn ask(
        &self,
        Parameters(req): Parameters<tools::ask::Request>,
    ) -> Json<tools::ask::Response> {
        let response = tools::ask::handle(&self.supervisor, &self.handles, &self.root, req).await;
        Json(response)
    }

    #[tool(
        name = "get_index_status",
        description = "Use this when starting work in a fresh repo or after a long pause to confirm which tiers are ready. Returns tier readiness flags. Do not call on every query - it is cheap but redundant. Tier requirement: 0."
    )]
    async fn get_index_status(
        &self,
        Parameters(_req): Parameters<tools::get_index_status::Request>,
    ) -> Json<tools::get_index_status::Response> {
        let response = tools::get_index_status::handle(&self.supervisor, &self.root).await;
        Json(response)
    }

    #[tool(
        name = "get_repo_overview",
        description = "Use this when you need a high-level shape of the codebase (languages, entry points, README excerpt). Returns a structured overview. Do not use this as a substitute for `ask` when looking up specific code - it is broad, not deep. Tier requirement: 0."
    )]
    async fn get_repo_overview(
        &self,
        Parameters(req): Parameters<tools::get_repo_overview::Request>,
    ) -> Json<tools::get_repo_overview::Response> {
        let response = tools::get_repo_overview::handle(&self.supervisor, &self.root, req).await;
        Json(response)
    }

    #[tool(
        name = "search_text",
        description = "Use this for literal/regex text search when you specifically need pattern-matching semantics. Returns line-anchored spans, capped. Do not use this to find a symbol by name - use `ask` instead. Tier requirement: 0. Most callers should use `ask` instead."
    )]
    async fn search_text(
        &self,
        Parameters(req): Parameters<tools::search_text::Request>,
    ) -> Json<tools::search_text::Response> {
        let response =
            tools::search_text::handle(&self.supervisor, &self.handles, &self.root, req).await;
        Json(response)
    }

    #[tool(
        name = "search_semantic",
        description = "Use this only when you need fuzzy semantic matching and you have already tried `ask`. Returns chunk-level spans ranked by hybrid BM25+vector. Tier requirement: 2. Most callers should use `ask` instead."
    )]
    async fn search_semantic(
        &self,
        Parameters(req): Parameters<tools::search_semantic::Request>,
    ) -> Json<tools::search_semantic::Response> {
        let response =
            tools::search_semantic::handle(&self.supervisor, &self.handles, &self.root, req).await;
        Json(response)
    }

    #[tool(
        name = "find_definition",
        description = "Use this when you have a symbol id and need its definition only. Returns one or more definition spans. Do not use grep for a symbol name - use `ask` instead. Tier requirement: 1. Most callers should use `ask` instead."
    )]
    async fn find_definition(
        &self,
        Parameters(req): Parameters<tools::find_definition::Request>,
    ) -> Json<tools::find_definition::Response> {
        let response =
            tools::find_definition::handle(&self.supervisor, &self.handles, &self.root, req).await;
        Json(response)
    }

    #[tool(
        name = "find_references",
        description = "Use this when you need every reference site for a known symbol id. Returns reference spans. Do not use this for first-pass exploration. Tier requirement: 1. Most callers should use `ask` instead."
    )]
    async fn find_references(
        &self,
        Parameters(req): Parameters<tools::find_references::Request>,
    ) -> Json<tools::find_references::Response> {
        let response =
            tools::find_references::handle(&self.supervisor, &self.handles, &self.root, req).await;
        Json(response)
    }

    #[tool(
        name = "get_callers",
        description = "Use this when tracing a call graph upward from a known function (who calls X?). Returns caller spans grouped by caller. Do not use this before locating the symbol. Tier requirement: 1. Most callers should use `ask` instead."
    )]
    async fn get_callers(
        &self,
        Parameters(req): Parameters<tools::get_callers::Request>,
    ) -> Json<tools::get_callers::Response> {
        let response =
            tools::get_callers::handle(&self.supervisor, &self.handles, &self.root, req).await;
        Json(response)
    }

    #[tool(
        name = "get_callees",
        description = "Use this when tracing a call graph downward from a known function (what does X call?). Returns callee spans. Do not use this before locating the symbol. Tier requirement: 1. Most callers should use `ask` instead."
    )]
    async fn get_callees(
        &self,
        Parameters(req): Parameters<tools::get_callees::Request>,
    ) -> Json<tools::get_callees::Response> {
        let response =
            tools::get_callees::handle(&self.supervisor, &self.handles, &self.root, req).await;
        Json(response)
    }

    #[tool(
        name = "get_imports",
        description = "Use this to enumerate imports for a file in both directions. Returns import edges. Do not use this for code body lookup. Tier requirement: 1."
    )]
    async fn get_imports(
        &self,
        Parameters(req): Parameters<tools::get_imports::Request>,
    ) -> Json<tools::get_imports::Response> {
        let response = tools::get_imports::handle(&self.supervisor, &self.root, req).await;
        Json(response)
    }

    #[tool(
        name = "get_symbol_outline",
        description = "Use this to get a hierarchical outline of a single file's symbols. Returns a tree of symbols with line ranges, no bodies. Do not use this to fetch bodies. Tier requirement: 1."
    )]
    async fn get_symbol_outline(
        &self,
        Parameters(req): Parameters<tools::get_symbol_outline::Request>,
    ) -> Json<tools::get_symbol_outline::Response> {
        let response = tools::get_symbol_outline::handle(&self.supervisor, &self.root, req).await;
        Json(response)
    }

    #[tool(
        name = "pack_repo",
        description = "Use this only when you genuinely need a flat dump of many files at once. Returns a token-budgeted XML or Markdown bundle. Do not use it for a single-question lookup; `ask` is cheaper. Tier requirement: 0/1."
    )]
    async fn pack_repo(
        &self,
        Parameters(req): Parameters<tools::pack_repo::Request>,
    ) -> Json<tools::pack_repo::Response> {
        let response = tools::pack_repo::handle(&self.supervisor, &self.root, req).await;
        Json(response)
    }

    #[tool(
        name = "locate",
        description = "Use this when you have a structured locator (path/glob, file:symbol, file:Lnn) or need structural search over markdown/JSON/YAML/TOML/CSV. Returns smallest natural spans. Do not call directly for ordinary lookup. Tier requirement: 1.5. Most callers should use `ask` instead."
    )]
    async fn locate(
        &self,
        Parameters(req): Parameters<tools::locate::LocateRequest>,
    ) -> Json<tools::locate::LocateResponse> {
        let response =
            tools::locate::handle(&self.supervisor, &self.handles, &self.root, req).await;
        Json(response)
    }

    #[tool(
        name = "locate_smart",
        description = "Use this only when you have configured an LLM provider in argyph.toml and need multi-step retrieval. Returns bounded validated spans. Do not use as the default lookup path. Tier requirement: 1.5/2. Most callers should use `ask` instead."
    )]
    async fn locate_smart(
        &self,
        Parameters(req): Parameters<tools::locate_smart::LocateSmartRequest>,
    ) -> Json<tools::locate_smart::LocateSmartResponse> {
        let response =
            tools::locate_smart::handle(&self.supervisor, &self.handles, &self.root, req).await;
        Json(response)
    }

    #[tool(
        name = "expand_span",
        description = "Use this when a previous Argyph result returned an expand_handle and you genuinely need the elided middle. Returns one Span. Do not use without a handle from this session. Tier requirement: 0."
    )]
    async fn expand_span(
        &self,
        Parameters(req): Parameters<tools::expand_span::Request>,
    ) -> Json<tools::expand_span::Response> {
        let response = tools::expand_span::handle(&self.handles, &self.root, req).await;
        Json(response)
    }

    #[tool(
        name = "memory_save",
        description = "Use this for persistent agent memory across sessions (not code retrieval - that's `ask`). Persist a memory entry under a given scope with optional metadata."
    )]
    async fn memory_save(
        &self,
        Parameters(req): Parameters<tools::memory_save::Request>,
    ) -> Json<tools::memory_save::Response> {
        let response = tools::memory_save::handle(&self.supervisor, &self.root, req).await;
        Json(response)
    }

    #[tool(
        name = "memory_search",
        description = "Use this for persistent agent memory across sessions (not code retrieval - that's `ask`). Search memories by content using FTS5. Optionally filter by scope."
    )]
    async fn memory_search(
        &self,
        Parameters(req): Parameters<tools::memory_search::Request>,
    ) -> Json<tools::memory_search::Response> {
        let response = tools::memory_search::handle(&self.supervisor, &self.root, req).await;
        Json(response)
    }

    #[tool(
        name = "memory_list",
        description = "Use this for persistent agent memory across sessions (not code retrieval - that's `ask`). List all memories in a given scope."
    )]
    async fn memory_list(
        &self,
        Parameters(req): Parameters<tools::memory_list::Request>,
    ) -> Json<tools::memory_list::Response> {
        let response = tools::memory_list::handle(&self.supervisor, &self.root, req).await;
        Json(response)
    }

    #[tool(
        name = "memory_forget",
        description = "Use this for persistent agent memory across sessions (not code retrieval - that's `ask`). Delete a memory entry by its ID."
    )]
    async fn memory_forget(
        &self,
        Parameters(req): Parameters<tools::memory_forget::Request>,
    ) -> Json<tools::memory_forget::Response> {
        let response = tools::memory_forget::handle(&self.supervisor, &self.root, req).await;
        Json(response)
    }
}

#[prompt_handler(router = self.prompt_router)]
#[tool_handler]
impl rmcp::handler::server::ServerHandler for ArgyphMcp {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        let mut info = rmcp::model::ServerInfo::new(
            rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .build(),
        );
        info.server_info = rmcp::model::Implementation::new("argyph", env!("CARGO_PKG_VERSION"));
        info.instructions = Some("Argyph code indexer for AI agents".into());
        info
    }
}

pub async fn serve(
    supervisor: Arc<Supervisor>,
    root: Utf8PathBuf,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let service = ArgyphMcp {
        supervisor,
        root: Arc::new(root),
        handles: Arc::new(HandleStore::new()),
        prompt_router: ArgyphMcp::prompt_router(),
    };
    let transport = rmcp::transport::io::stdio();
    let running = serve_server(service, transport).await?;
    running.waiting().await?;
    Ok(())
}
