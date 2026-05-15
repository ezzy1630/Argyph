use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use argyph_core::Supervisor;
use argyph_parse::SymbolId;

use crate::error::{self, McpErrorBody};
use crate::handles::HandleStore;
use crate::span::{self, Span, SpanKind};
use crate::tools::common;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Request {
    #[serde(default)]
    pub symbol_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub language_hint: Option<String>,
    #[serde(default)]
    pub file_hint: Option<String>,
    #[serde(default = "default_context_lines")]
    #[allow(dead_code)]
    pub context_lines: u64,
    #[serde(default = "default_max_results")]
    pub max_results: u64,
}

fn default_context_lines() -> u64 {
    2
}
fn default_max_results() -> u64 {
    100
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Reference {
    pub file: String,
    pub range: (u64, u64),
    pub snippet: String,
    pub context_before: Vec<String>,
    pub context_after: Vec<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Response {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references: Option<Vec<Reference>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spans: Option<Vec<Span>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpErrorBody>,
}

impl Response {
    fn ok(refs: Vec<Reference>, spans: Vec<Span>, truncated: bool) -> Self {
        Self {
            references: Some(refs),
            spans: Some(spans),
            truncated: Some(truncated),
            error: None,
        }
    }
    fn err(body: McpErrorBody) -> Self {
        Self {
            references: None,
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
    if supervisor.get_tier_state().await.tier_number() < 2 {
        return Response::err(error::index_not_ready());
    }

    let sel = match common::resolve_selector(&request.symbol_id, &request.name, &request.file_hint)
    {
        Ok(s) => s,
        Err(e) => return Response::err(e),
    };

    let max = request.max_results.clamp(1, 1000);
    let index = supervisor.index();
    match index.find_references(&sel).await {
        Ok(edges) => {
            let truncated = edges.len() > max as usize;
            let refs: Vec<Reference> = edges
                .into_iter()
                .take(max as usize)
                .map(|e| edge_to_reference(&e.from, root))
                .collect();
            let mut any_span_truncated = false;
            let spans = refs
                .iter()
                .map(|r| {
                    let start = r.range.0 as u32;
                    let end = r.range.1 as u32;
                    let (text, byte_range) =
                        span::read_line_range(root.as_std_path(), &r.file, start, end);
                    let mut span = Span {
                        file: r.file.clone(),
                        start_line: start,
                        end_line: end,
                        byte_range,
                        text,
                        kind: SpanKind::Reference,
                        symbol: request.name.clone(),
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
            let (spans, total_truncated) = span::cap_total_lines(spans, span::max_total_lines());
            Response::ok(
                refs,
                spans,
                truncated || any_span_truncated || total_truncated,
            )
        }
        Err(e) => Response::err(error::internal(e.to_string())),
    }
}

fn edge_to_reference(sid: &SymbolId, root: &Utf8Path) -> Reference {
    let id_str = sid.as_str();
    let (file_str, _, start) = common::parse_sid(id_str);
    let snippet = read_snippet(root, file_str, start);
    Reference {
        file: file_str.to_string(),
        range: (start as u64, start.saturating_add(1) as u64),
        snippet,
        context_before: vec![],
        context_after: vec![],
    }
}

fn read_snippet(root: &Utf8Path, file: &str, start: usize) -> String {
    let path = root.join(file);
    std::fs::read_to_string(path.as_str())
        .ok()
        .and_then(|c| {
            c.lines()
                .nth(start.saturating_sub(1))
                .map(|l| l.to_string())
        })
        .unwrap_or_default()
}
