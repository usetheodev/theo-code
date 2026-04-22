//! Status line segment layout and rendering.

#![allow(dead_code)] // Scaffolded helpers — kept for upcoming TUI features.
use crate::render::style::{StyleCaps, accent, dim};

/// A single labeled segment in the status line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub label: String,
    pub value: String,
}

/// Full status line input.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatusLine {
    pub mode: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub llm_calls: u64,
}

impl StatusLine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    /// Build the ordered segment list.
    pub fn segments(&self) -> Vec<Segment> {
        let mut out = Vec::new();
        if let Some(m) = &self.mode {
            out.push(Segment {
                label: "mode".into(),
                value: m.clone(),
            });
        }
        if let Some(m) = &self.model {
            out.push(Segment {
                label: "model".into(),
                value: m.clone(),
            });
        }
        if let Some(p) = &self.provider {
            out.push(Segment {
                label: "provider".into(),
                value: p.clone(),
            });
        }
        out.push(Segment {
            label: "tokens".into(),
            value: format_tokens(self.total_tokens()),
        });
        if self.llm_calls > 0 {
            out.push(Segment {
                label: "calls".into(),
                value: self.llm_calls.to_string(),
            });
        }
        out
    }
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        format!("{tokens}")
    }
}

/// Render a status line to a single formatted string.
///
/// Shape: `[mode: plan] [model: gpt-4] [tokens: 1.2k]`
pub fn render_status(line: &StatusLine, caps: StyleCaps) -> String {
    let sep = dim(" · ", caps).to_string();
    line.segments()
        .iter()
        .map(|s| {
            format!(
                "{}: {}",
                dim(&s.label, caps),
                accent(&s.value, caps),
            )
        })
        .collect::<Vec<_>>()
        .join(&sep)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> StyleCaps {
        StyleCaps::plain()
    }

    #[test]
    fn test_new_is_empty() {
        let s = StatusLine::new();
        assert_eq!(s.total_tokens(), 0);
        // Tokens segment is always emitted
        assert_eq!(s.segments().len(), 1);
    }

    #[test]
    fn test_total_tokens_sum() {
        let s = StatusLine {
            input_tokens: 100,
            output_tokens: 250,
            ..Default::default()
        };
        assert_eq!(s.total_tokens(), 350);
    }

    #[test]
    fn test_segments_with_mode_and_model() {
        let s = StatusLine {
            mode: Some("plan".into()),
            model: Some("gpt-4".into()),
            ..Default::default()
        };
        let segs = s.segments();
        let labels: Vec<&str> = segs.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, vec!["mode", "model", "tokens"]);
    }

    #[test]
    fn test_segments_with_all_fields() {
        let s = StatusLine {
            mode: Some("agent".into()),
            model: Some("claude".into()),
            provider: Some("anthropic".into()),
            input_tokens: 500,
            output_tokens: 500,
            llm_calls: 3,
        };
        let segs = s.segments();
        assert_eq!(segs.len(), 5);
    }

    #[test]
    fn test_segments_omits_llm_calls_when_zero() {
        let s = StatusLine {
            llm_calls: 0,
            ..Default::default()
        };
        let segs = s.segments();
        assert!(!segs.iter().any(|s| s.label == "calls"));
    }

    #[test]
    fn test_format_tokens_small() {
        assert_eq!(format_tokens(42), "42");
    }

    #[test]
    fn test_format_tokens_thousands() {
        assert_eq!(format_tokens(1500), "1.5k");
    }

    #[test]
    fn test_format_tokens_millions() {
        assert_eq!(format_tokens(2_500_000), "2.5M");
    }

    #[test]
    fn test_render_plain_contains_labels() {
        let s = StatusLine {
            mode: Some("agent".into()),
            model: Some("gpt".into()),
            input_tokens: 1000,
            output_tokens: 500,
            llm_calls: 2,
            ..Default::default()
        };
        let out = render_status(&s, plain());
        assert!(out.contains("mode"));
        assert!(out.contains("agent"));
        assert!(out.contains("model"));
        assert!(out.contains("tokens"));
        assert!(out.contains("1.5k"));
    }

    #[test]
    fn test_render_empty_contains_zero_tokens() {
        let s = StatusLine::new();
        let out = render_status(&s, plain());
        assert!(out.contains("tokens"));
        assert!(out.contains('0'));
    }

    #[test]
    fn test_render_tty_contains_ansi() {
        let s = StatusLine {
            mode: Some("plan".into()),
            ..Default::default()
        };
        let out = render_status(&s, StyleCaps::full());
        assert!(out.contains("\x1b["));
    }

    #[test]
    fn test_render_is_deterministic() {
        let s = StatusLine {
            model: Some("x".into()),
            input_tokens: 5,
            ..Default::default()
        };
        assert_eq!(render_status(&s, plain()), render_status(&s, plain()));
    }
}
