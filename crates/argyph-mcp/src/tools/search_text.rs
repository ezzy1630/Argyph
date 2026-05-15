use std::sync::Arc;

use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use argyph_core::{SearchFilter, Supervisor};

use crate::error::{self, McpErrorBody};
use crate::handles::HandleStore;
use crate::span::{self, Span, SpanKind};
use crate::types::Filter;
use crate::validate;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Request {
    pub pattern: String,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub case_sensitive: bool,
    #[serde(default = "default_max_results")]
    pub max_results: u64,
    #[serde(default)]
    pub filter: Option<Filter>,
}

fn default_max_results() -> u64 {
    100
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SearchHit {
    pub file: String,
    pub line: u64,
    pub column: u64,
    #[serde(rename = "match")]
    pub match_text: String,
    pub context_before: Vec<String>,
    pub context_after: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Response {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hits: Option<Vec<SearchHit>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spans: Option<Vec<Span>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpErrorBody>,
}

impl Response {
    fn ok(result: argyph_core::SearchResult, root: &Utf8PathBuf, handles: &HandleStore) -> Self {
        let mut any_span_truncated = false;
        let hits: Vec<SearchHit> = result
            .hits
            .into_iter()
            .map(|h| {
                let (ctx_before, ctx_after) = read_context(root, &h.file, h.line);
                SearchHit {
                    file: h.file.to_string(),
                    line: h.line,
                    column: h.column,
                    match_text: h.match_text,
                    context_before: ctx_before,
                    context_after: ctx_after,
                }
            })
            .collect();
        let mut spans: Vec<Span> = hits
            .iter()
            .map(|h| {
                let (text, byte_range) = span::read_line_range(
                    root.as_std_path(),
                    &h.file,
                    h.line as u32,
                    h.line as u32,
                );
                let mut span = Span {
                    file: h.file.clone(),
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
                span::apply_span_cap(&mut span, handles);
                any_span_truncated |= span.truncated;
                span
            })
            .collect();
        let (kept, total_truncated) = span::cap_total_lines(spans, span::max_total_lines());
        spans = kept;
        Self {
            hits: Some(hits),
            spans: Some(spans),
            truncated: Some(result.truncated || any_span_truncated || total_truncated),
            error: None,
        }
    }

    fn err(body: McpErrorBody) -> Self {
        Self {
            hits: None,
            spans: None,
            truncated: None,
            error: Some(body),
        }
    }
}

pub async fn handle(
    supervisor: &Arc<Supervisor>,
    handles: &Arc<HandleStore>,
    root: &Utf8PathBuf,
    request: Request,
) -> Response {
    if !supervisor.get_tier_state().await.is_ready() {
        return Response::err(error::index_not_ready());
    }

    let max_results = validate::clamp_u64(request.max_results, 1, 1000);
    let filter = request.filter.map(|f| SearchFilter {
        paths_glob: f.paths_glob,
        exclude_glob: f.exclude_glob,
    });

    let index = supervisor.index();
    match index
        .search_text(
            root,
            &request.pattern,
            request.regex,
            request.case_sensitive,
            max_results,
            filter,
        )
        .await
    {
        Ok(result) => Response::ok(result, root, handles),
        Err(e) => Response::err(error::internal(e.to_string())),
    }
}

fn read_context(
    root: &Utf8PathBuf,
    file: &camino::Utf8Path,
    line: u64,
) -> (Vec<String>, Vec<String>) {
    let full = root.join(file.as_str());
    let Ok(content) = std::fs::read_to_string(full.as_str()) else {
        return (vec![], vec![]);
    };
    let all_lines: Vec<&str> = content.lines().collect();
    let idx = (line as usize).saturating_sub(1);
    if idx >= all_lines.len() {
        return (vec![], vec![]);
    }
    let ctx_start = idx.saturating_sub(1);
    let before: Vec<String> = all_lines[ctx_start..idx]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let after_end = (idx + 2).min(all_lines.len());
    let after: Vec<String> = all_lines[idx + 1..after_end]
        .iter()
        .map(|s| s.to_string())
        .collect();
    (before, after)
}
