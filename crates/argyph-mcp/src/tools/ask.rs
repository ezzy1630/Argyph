use std::sync::Arc;

use camino::Utf8PathBuf;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use argyph_core::Supervisor;

use crate::error::{ErrorCode, McpErrorBody};
use crate::handles::HandleStore;
use crate::span::{self, Span, SpanKind};

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Focus {
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Auto,
    Structural,
    Semantic,
    Smart,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Request {
    pub query: String,
    #[serde(default)]
    pub focus: Option<Focus>,
    #[serde(default)]
    pub mode: Mode,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    std::env::var("ARGYPH_ASK_DEFAULT_K")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8)
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct Response {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spans: Option<Vec<Span>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy_used: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpErrorBody>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    Definition,
    Locate,
    Semantic,
    Smart,
}

pub fn decide_strategy(req: &Request) -> Strategy {
    match req.mode {
        Mode::Smart => Strategy::Smart,
        Mode::Structural => {
            if is_bare_identifier(&req.query) {
                Strategy::Definition
            } else {
                Strategy::Locate
            }
        }
        Mode::Semantic => Strategy::Semantic,
        Mode::Auto => {
            if is_bare_identifier(&req.query) {
                Strategy::Definition
            } else if looks_like_locator(&req.query) {
                Strategy::Locate
            } else {
                Strategy::Semantic
            }
        }
    }
}

fn is_bare_identifier(q: &str) -> bool {
    let q = q.trim();
    !q.is_empty()
        && q.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && q.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn looks_like_locator(q: &str) -> bool {
    q.contains(":L") || q.contains('/') || q.contains('*') || q.contains('.')
}

pub async fn handle(
    supervisor: &Arc<Supervisor>,
    handles: &Arc<HandleStore>,
    root: &Utf8PathBuf,
    req: Request,
) -> Response {
    let strategy = decide_strategy(&req);
    let result = match strategy {
        Strategy::Definition => dispatch_definition(supervisor, handles, root, &req).await,
        Strategy::Locate => dispatch_locate(supervisor, handles, root, &req).await,
        Strategy::Semantic => dispatch_semantic(supervisor, handles, root, &req).await,
        Strategy::Smart => dispatch_smart(supervisor, handles, root, &req).await,
    };

    match result {
        Ok((label, spans)) => {
            let (spans, truncated) = span::cap_total_lines(spans, span::max_total_lines());
            Response {
                spans: Some(spans),
                strategy_used: Some(label.into()),
                truncated: Some(truncated),
                error: None,
            }
        }
        Err(error) => Response {
            spans: None,
            strategy_used: None,
            truncated: None,
            error: Some(error),
        },
    }
}

async fn dispatch_definition(
    supervisor: &Arc<Supervisor>,
    handles: &Arc<HandleStore>,
    root: &Utf8PathBuf,
    req: &Request,
) -> Result<(&'static str, Vec<Span>), McpErrorBody> {
    let inner = super::find_definition::Request {
        name: req.query.clone(),
        language_hint: None,
        file_hint: req.focus.as_ref().and_then(|f| f.file.clone()),
    };
    let resp = super::find_definition::handle(supervisor, handles, root, inner).await;
    if let Some(error) = resp.error {
        if matches!(
            error.code,
            ErrorCode::IndexNotReady | ErrorCode::SymbolNotFound
        ) {
            return dispatch_semantic(supervisor, handles, root, req).await;
        }
        return Err(error);
    }
    let spans = resp.spans.unwrap_or_default();
    if spans.is_empty() {
        return dispatch_semantic(supervisor, handles, root, req).await;
    }
    Ok((
        "definition",
        spans.into_iter().take(req.limit as usize).collect(),
    ))
}

async fn dispatch_locate(
    supervisor: &Arc<Supervisor>,
    handles: &Arc<HandleStore>,
    root: &Utf8PathBuf,
    req: &Request,
) -> Result<(&'static str, Vec<Span>), McpErrorBody> {
    let locate_req = if looks_like_locator(&req.query) {
        argyph_locate::Request {
            path: Some(req.query.clone()),
            query: None,
            file: req.focus.as_ref().and_then(|f| f.file.clone()),
            files: None,
            max_results: req.limit.min(u8::MAX as u32) as u8,
            max_bytes_per_span: 4096,
        }
    } else {
        argyph_locate::Request {
            query: Some(req.query.clone()),
            path: None,
            file: req.focus.as_ref().and_then(|f| f.file.clone()),
            files: None,
            max_results: req.limit.min(u8::MAX as u32) as u8,
            max_bytes_per_span: 4096,
        }
    };
    let resp = super::locate::handle(supervisor, handles, root, locate_req).await;
    if let Some(error) = resp.error {
        return Err(error);
    }
    Ok(("locate", resp.spans_v2.unwrap_or_default()))
}

async fn dispatch_semantic(
    supervisor: &Arc<Supervisor>,
    handles: &Arc<HandleStore>,
    root: &Utf8PathBuf,
    req: &Request,
) -> Result<(&'static str, Vec<Span>), McpErrorBody> {
    let inner = super::search_semantic::Request {
        query: req.query.clone(),
        k: req.limit as usize,
        alpha: 0.5,
        filter: None,
    };
    let resp = super::search_semantic::handle(supervisor, handles, root, inner).await;
    if let Some(error) = resp.error {
        return Err(error);
    }
    Ok(("semantic", resp.spans.unwrap_or_default()))
}

async fn dispatch_smart(
    supervisor: &Arc<Supervisor>,
    handles: &Arc<HandleStore>,
    root: &Utf8PathBuf,
    req: &Request,
) -> Result<(&'static str, Vec<Span>), McpErrorBody> {
    let inner = super::locate_smart::LocateSmartRequest {
        query: req.query.clone(),
        max_steps: 4,
        max_output_tokens: 1024,
    };
    let resp = super::locate_smart::handle(supervisor, handles, root, inner).await;
    if let Some(error) = resp.error {
        if error.code == ErrorCode::LocateSmartDisabled {
            return dispatch_semantic(supervisor, handles, root, req).await;
        }
        return Err(error);
    }
    Ok(("smart", resp.spans_v2.unwrap_or_default()))
}

pub fn span_from_locate_span(s: super::locate::SpanData, handles: &HandleStore) -> Span {
    let mut span = Span {
        file: s.file,
        start_line: s.line_range.0,
        end_line: s.line_range.1,
        byte_range: (s.byte_range.0 as u64, s.byte_range.1 as u64),
        text: s.content,
        kind: SpanKind::Locate,
        symbol: s.path.last().cloned(),
        language: None,
        score: Some(s.score),
        truncated: s.truncated,
        expand_handle: None,
    };
    span::apply_span_cap(&mut span, handles);
    span
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(query: &str, mode: Mode) -> Request {
        Request {
            query: query.into(),
            focus: None,
            mode,
            limit: 8,
        }
    }

    #[test]
    fn auto_bare_identifier_picks_definition() {
        assert_eq!(
            decide_strategy(&req("parseConfig", Mode::Auto)),
            Strategy::Definition
        );
    }

    #[test]
    fn auto_path_glob_picks_locate() {
        assert_eq!(
            decide_strategy(&req("src/**/foo.rs", Mode::Auto)),
            Strategy::Locate
        );
    }

    #[test]
    fn auto_natural_language_picks_semantic() {
        assert_eq!(
            decide_strategy(&req("where do we handle auth failures", Mode::Auto)),
            Strategy::Semantic
        );
    }

    #[test]
    fn explicit_smart_overrides_auto() {
        assert_eq!(
            decide_strategy(&req("parseConfig", Mode::Smart)),
            Strategy::Smart
        );
    }

    #[test]
    fn explicit_semantic_overrides_identifier_heuristic() {
        assert_eq!(
            decide_strategy(&req("parseConfig", Mode::Semantic)),
            Strategy::Semantic
        );
    }
}
