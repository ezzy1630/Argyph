use std::sync::Arc;

use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use argyph_core::Supervisor;
use argyph_store::search::SearchFilter;

use crate::error::{self, McpErrorBody};
use crate::handles::HandleStore;
use crate::span::{self, Span, SpanKind};
use crate::types::Filter;
use crate::validate;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Request {
    pub query: String,
    #[serde(default = "default_k")]
    pub k: usize,
    #[serde(default = "default_alpha")]
    #[allow(dead_code)]
    pub alpha: f64,
    #[serde(default)]
    pub filter: Option<Filter>,
}

fn default_k() -> usize {
    10
}

fn default_alpha() -> f64 {
    0.5
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SemanticHit {
    pub chunk_id: String,
    pub chunk_text: String,
    pub file: String,
    pub byte_range: (u32, u32),
    pub line_range: (u32, u32),
    pub score: f32,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Response {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hits: Option<Vec<SemanticHit>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spans: Option<Vec<Span>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_coverage: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_embedded: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_chunks: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpErrorBody>,
}

impl Response {
    fn ok(result: &argyph_core::SemanticResult, handles: &HandleStore) -> Self {
        let coverage = if result.total_chunks > 0 {
            result.total_embedded as f64 / result.total_chunks as f64
        } else {
            0.0
        };
        let mut hits: Vec<SemanticHit> = result
            .hits
            .iter()
            .map(|h| SemanticHit {
                chunk_id: h.chunk_id.clone(),
                chunk_text: h.chunk_text.clone(),
                file: h.file.clone(),
                byte_range: h.byte_range,
                line_range: h.line_range,
                score: h.score,
                source: h.source.clone(),
            })
            .collect();
        let mut any_span_truncated = false;
        let spans: Vec<Span> = hits
            .iter()
            .map(|h| {
                let mut span = Span {
                    file: h.file.clone(),
                    start_line: h.line_range.0,
                    end_line: h.line_range.1,
                    byte_range: (h.byte_range.0 as u64, h.byte_range.1 as u64),
                    text: h.chunk_text.clone(),
                    kind: SpanKind::Match,
                    symbol: None,
                    language: None,
                    score: Some(h.score),
                    truncated: false,
                    expand_handle: None,
                };
                span::apply_span_cap(&mut span, handles);
                any_span_truncated |= span.truncated;
                span
            })
            .collect();
        let (spans, total_truncated) = span::cap_total_lines(spans, span::max_total_lines());
        hits.truncate(spans.len());
        for (hit, capped_span) in hits.iter_mut().zip(spans.iter()) {
            hit.chunk_text.clone_from(&capped_span.text);
        }
        Self {
            hits: Some(hits),
            spans: Some(spans),
            truncated: Some(any_span_truncated || total_truncated),
            index_coverage: Some(coverage),
            total_embedded: Some(result.total_embedded),
            total_chunks: Some(result.total_chunks),
            error: None,
        }
    }

    fn err(body: McpErrorBody) -> Self {
        Self {
            hits: None,
            spans: None,
            truncated: None,
            index_coverage: None,
            total_embedded: None,
            total_chunks: None,
            error: Some(body),
        }
    }
}

pub async fn handle(
    supervisor: &Arc<Supervisor>,
    handles: &Arc<HandleStore>,
    _root: &Utf8PathBuf,
    request: Request,
) -> Response {
    if !supervisor.get_tier_state().await.is_ready() {
        return Response::err(error::index_not_ready());
    }

    let k = validate::clamp_u64(request.k as u64, 1, 100) as usize;
    let filter = request.filter.map(|f| SearchFilter {
        language: f.languages.and_then(|v| v.into_iter().next()),
        paths_glob: f.paths_glob.and_then(|v| v.into_iter().next()),
        exclude_glob: f.exclude_glob.and_then(|v| v.into_iter().next()),
        file_ids: None,
    });

    let index = supervisor.index();
    match index
        .search_semantic(&request.query, k, filter.as_ref())
        .await
    {
        Ok(result) => Response::ok(&result, handles),
        Err(e) => Response::err(error::internal(e.to_string())),
    }
}
