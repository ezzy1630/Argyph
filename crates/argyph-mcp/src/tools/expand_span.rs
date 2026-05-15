use std::sync::Arc;

use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{ErrorCode, McpErrorBody};
use crate::handles::HandleStore;
use crate::span::{Span, SpanKind};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Request {
    pub handle: String,
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Response {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpErrorBody>,
}

pub async fn handle(handles: &Arc<HandleStore>, root: &Utf8PathBuf, req: Request) -> Response {
    let Some(target) = handles.lookup(&req.handle) else {
        return Response {
            span: None,
            error: Some(McpErrorBody {
                code: ErrorCode::InvalidPath,
                message: "unknown or expired expand handle".into(),
                retryable: false,
                retry_after_ms: None,
                correlation_id: None,
            }),
        };
    };

    let full = root.join(&target.file);
    let content = match std::fs::read(full.as_str()) {
        Ok(content) => content,
        Err(e) => {
            return Response {
                span: None,
                error: Some(crate::error::internal(format!("read: {e}"))),
            };
        }
    };

    let (start, end) = target.byte_range;
    let text = content
        .get(start as usize..end as usize)
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
        .unwrap_or_default();

    Response {
        span: Some(Span {
            file: target.file,
            start_line: target.start_line,
            end_line: target.end_line,
            byte_range: target.byte_range,
            text,
            kind: SpanKind::Match,
            symbol: None,
            language: None,
            score: None,
            truncated: false,
            expand_handle: None,
        }),
        error: None,
    }
}
