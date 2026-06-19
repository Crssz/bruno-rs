//! Tree-sitter syntax highlighting → gpui highlight spans.
//!
//! Currently unused by the editable editor (single-color v1); kept for the
//! follow-up that adds per-line highlight runs to the editor.
#![allow(dead_code)]

use std::ops::Range;

use gpui::{rgb, HighlightStyle, Hsla};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use crate::theme;

/// Capture names requested from tree-sitter; `configure` prefix-matches the
/// grammar's query captures, so the most specific name wins.
const NAMES: &[&str] = &[
    "string",
    "string.special.key",
    "number",
    "constant.builtin",
    "escape",
    "comment",
    "punctuation.bracket",
    "punctuation.delimiter",
    "keyword",
    "property",
];

fn color_for(name: &str) -> Hsla {
    // Catppuccin Mocha on dark; Latte (darker, white-bg-safe) on light.
    let hex = if theme::is_dark() {
        match name {
            "string.special.key" | "property" => 0x89b4fa, // blue (JSON keys)
            "string" => 0xa6e3a1,                          // green
            "number" => 0xfab387,                          // peach
            "constant.builtin" | "keyword" => 0xcba6f7,    // mauve (true/false/null)
            "escape" => 0xf5c2e7,                          // pink
            "comment" => 0x6c7086,                         // dim
            _ => 0x9399b2,                                 // punctuation/other
        }
    } else {
        match name {
            "string.special.key" | "property" => 0x1e66f5,
            "string" => 0x40a02b,
            "number" => 0xc4520a,
            "constant.builtin" | "keyword" => 0x8839ef,
            "escape" => 0xd01884,
            "comment" => 0x8c8fa1,
            _ => 0x5c5f77,
        }
    };
    rgb(hex).into()
}

/// Compute tree-sitter highlight spans for a JSON document.
pub fn json(code: &str) -> Vec<(Range<usize>, HighlightStyle)> {
    let mut config = HighlightConfiguration::new(
        tree_sitter_json::LANGUAGE.into(),
        "json",
        tree_sitter_json::HIGHLIGHTS_QUERY,
        "",
        "",
    )
    .expect("json highlight config");
    config.configure(NAMES);

    let mut hl = Highlighter::new();
    let mut spans = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let events = hl
        .highlight(&config, code.as_bytes(), None, |_| None)
        .expect("highlight");
    for ev in events {
        match ev.expect("highlight event") {
            HighlightEvent::HighlightStart(h) => stack.push(h.0),
            HighlightEvent::HighlightEnd => {
                stack.pop();
            }
            HighlightEvent::Source { start, end } => {
                if let Some(&idx) = stack.last() {
                    spans.push((
                        start..end,
                        HighlightStyle {
                            color: Some(color_for(NAMES[idx])),
                            ..Default::default()
                        },
                    ));
                }
            }
        }
    }
    spans
}
