use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::handles::{ExpandTarget, HandleStore};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    Definition,
    Reference,
    Match,
    Outline,
    Locate,
    Call,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Span {
    pub file: String,
    pub start_line: u32,
    pub end_line: u32,
    pub byte_range: (u64, u64),
    pub text: String,
    pub kind: SpanKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expand_handle: Option<String>,
}

pub const DEFAULT_MAX_SPAN_LINES: u32 = 80;
pub const DEFAULT_MAX_TOTAL_LINES: u32 = 400;
pub const HEAD_LINES: usize = 40;
pub const TAIL_LINES: usize = 20;

pub fn max_span_lines() -> u32 {
    std::env::var("ARGYPH_MAX_SPAN_LINES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_MAX_SPAN_LINES)
}

pub fn max_total_lines() -> u32 {
    std::env::var("ARGYPH_MAX_TOTAL_LINES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_MAX_TOTAL_LINES)
}

pub fn truncate_lines(text: &str, cap: u32) -> (String, bool) {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    if lines.len() as u32 <= cap {
        return (text.to_string(), false);
    }

    let budget = cap.max(1) as usize;
    if budget == 1 {
        return (format!("[...{} lines elided...]\n", lines.len()), true);
    }

    let head_lines = HEAD_LINES.min(budget.saturating_sub(1));
    let tail_lines = TAIL_LINES.min(budget.saturating_sub(1 + head_lines));
    let head: String = lines.iter().take(head_lines).copied().collect();
    let tail_start = lines.len().saturating_sub(tail_lines);
    let tail: String = lines.iter().skip(tail_start).copied().collect();
    let elided = lines.len().saturating_sub(head_lines + tail_lines);
    (format!("{head}[...{elided} lines elided...]\n{tail}"), true)
}

pub fn cap_total_lines(mut spans: Vec<Span>, cap: u32) -> (Vec<Span>, bool) {
    let mut total = 0u32;
    let mut keep = 0usize;

    for span in &spans {
        let lines = returned_line_count(&span.text);
        if total.saturating_add(lines) > cap {
            break;
        }
        total = total.saturating_add(lines);
        keep += 1;
    }

    let truncated = keep < spans.len();
    spans.truncate(keep);
    (spans, truncated)
}

fn returned_line_count(text: &str) -> u32 {
    if text.is_empty() {
        0
    } else {
        text.lines().count().max(1) as u32
    }
}

pub fn apply_span_cap(span: &mut Span, handles: &HandleStore) {
    let (text, truncated) = truncate_lines(&span.text, max_span_lines());
    if truncated {
        span.expand_handle = Some(handles.issue(ExpandTarget {
            file: span.file.clone(),
            byte_range: span.byte_range,
            start_line: span.start_line,
            end_line: span.end_line,
        }));
    }
    span.text = text;
    span.truncated = span.truncated || truncated;
}

pub fn read_line_range(
    root: &Path,
    file: &str,
    start_line: u32,
    end_line: u32,
) -> (String, (u64, u64)) {
    let full = root.join(file);
    let Ok(content) = std::fs::read_to_string(&full) else {
        return (String::new(), (0, 0));
    };

    let mut byte_end = content.len() as u64;
    let mut current_line = 1u32;
    let mut cursor = 0usize;
    let bytes = content.as_bytes();

    while cursor < bytes.len() && current_line < start_line {
        if bytes[cursor] == b'\n' {
            current_line += 1;
        }
        cursor += 1;
    }
    let byte_start = cursor as u64;

    while cursor < bytes.len() {
        if bytes[cursor] == b'\n' {
            if current_line >= end_line {
                byte_end = (cursor + 1) as u64;
                break;
            }
            current_line += 1;
        }
        cursor += 1;
    }

    let text = String::from_utf8_lossy(&bytes[byte_start as usize..byte_end as usize]).into_owned();
    (text, (byte_start, byte_end))
}

pub fn byte_range_to_lines(root: &Path, file: &str, byte_range: (u64, u64)) -> (u32, u32) {
    let full = root.join(file);
    let Ok(content) = std::fs::read(&full) else {
        return (1, 1);
    };
    let count_to = |limit: usize| -> u32 {
        let upto = limit.min(content.len());
        content[..upto].iter().filter(|&&b| b == b'\n').count() as u32 + 1
    };
    (
        count_to(byte_range.0 as usize),
        count_to(byte_range.1 as usize),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_lines_under_cap_is_noop() {
        let text = "a\nb\nc\n";
        let (out, truncated) = truncate_lines(text, 80);
        assert_eq!(out, text);
        assert!(!truncated);
    }

    #[test]
    fn truncate_lines_over_cap_elides_middle() {
        let big: String = (0..200).map(|i| format!("L{i}\n")).collect();
        let (out, truncated) = truncate_lines(&big, 80);
        assert!(truncated);
        assert!(out.contains("[...140 lines elided...]"));
        assert!(out.starts_with("L0\n"));
        assert!(out.contains("L199\n"));
    }

    #[test]
    fn truncate_lines_respects_custom_low_cap() {
        let big: String = (0..200).map(|i| format!("L{i}\n")).collect();
        let (out, truncated) = truncate_lines(&big, 20);
        assert!(truncated);
        assert!(out.lines().count() <= 20);
    }

    #[test]
    fn cap_total_lines_counts_returned_text_not_original_range() {
        let mk = |s: u32, e: u32| Span {
            file: "f".into(),
            start_line: s,
            end_line: e,
            byte_range: (0, 0),
            text: "capped\ntext\n".into(),
            kind: SpanKind::Match,
            symbol: None,
            language: None,
            score: None,
            truncated: false,
            expand_handle: None,
        };
        let (kept, truncated) = cap_total_lines(vec![mk(1, 1_000)], 250);
        assert!(!truncated);
        assert_eq!(kept.len(), 1);
    }

    #[test]
    fn cap_total_lines_drops_trailing_when_returned_text_budget_exceeded() {
        let mk = |text: &str| Span {
            file: "f".into(),
            start_line: 1,
            end_line: 1,
            byte_range: (0, 0),
            text: text.into(),
            kind: SpanKind::Match,
            symbol: None,
            language: None,
            score: None,
            truncated: false,
            expand_handle: None,
        };
        let text = (0..100).map(|i| format!("L{i}\n")).collect::<String>();
        let (kept, truncated) = cap_total_lines(vec![mk(&text), mk(&text), mk(&text)], 250);
        assert!(truncated);
        assert_eq!(kept.len(), 2);
    }
}
