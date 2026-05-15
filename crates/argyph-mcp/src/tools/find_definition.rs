use std::sync::Arc;

use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use argyph_core::Supervisor;

use crate::error::{self, McpErrorBody};
use crate::handles::HandleStore;
use crate::span::{self, Span, SpanKind};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Request {
    pub name: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub language_hint: Option<String>,
    #[serde(default)]
    pub file_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SourceRange {
    pub file: String,
    pub range: (u64, u64),
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Definition {
    pub symbol_id: String,
    pub name: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub location: SourceRange,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docstring: Option<String>,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Response {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub definitions: Option<Vec<Definition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spans: Option<Vec<Span>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpErrorBody>,
}

impl Response {
    fn ok(defs: Vec<Definition>, spans: Vec<Span>, truncated: bool) -> Self {
        Self {
            definitions: Some(defs),
            spans: Some(spans),
            truncated: Some(truncated),
            error: None,
        }
    }

    fn err(body: McpErrorBody) -> Self {
        Self {
            definitions: None,
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

    let file_opt = request.file_hint.as_deref().map(camino::Utf8Path::new);

    let index = supervisor.index();
    match index.find_symbol(&request.name, file_opt).await {
        Ok(symbols) => {
            let defs = symbols
                .into_iter()
                .map(|s| {
                    let range = (s.range.start as u64, s.range.end as u64);
                    Definition {
                        symbol_id: s.id.as_str().to_string(),
                        name: s.name,
                        kind: format!("{:?}", s.kind).to_lowercase(),
                        signature: s.signature,
                        location: SourceRange {
                            file: s.file.to_string(),
                            range,
                        },
                        language: None,
                        docstring: None,
                    }
                })
                .collect::<Vec<_>>();
            let mut any_span_truncated = false;
            let spans = defs
                .iter()
                .map(|d| {
                    let (start, end) = span::byte_range_to_lines(
                        root.as_std_path(),
                        &d.location.file,
                        d.location.range,
                    );
                    let (text, byte_range) =
                        span::read_line_range(root.as_std_path(), &d.location.file, start, end);
                    let mut span = Span {
                        file: d.location.file.clone(),
                        start_line: start,
                        end_line: end,
                        byte_range,
                        text,
                        kind: SpanKind::Definition,
                        symbol: Some(d.name.clone()),
                        language: d.language.clone(),
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
            Response::ok(defs, spans, any_span_truncated || total_truncated)
        }
        Err(e) => Response::err(error::internal(e.to_string())),
    }
}
