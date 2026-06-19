// Stage 3 spike: a gpui view rendering JSON with real tree-sitter syntax
// highlighting — the actual reason for the gpui move. Proves the full path:
// tree-sitter parse -> highlight spans -> gpui StyledText, on Windows.
use std::ops::Range;

use gpui::{
    div, prelude::*, px, rgb, size, App, Bounds, Context, HighlightStyle, Hsla, SharedString,
    StyledText, Window, WindowBounds, WindowOptions,
};
use gpui_platform::application;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

/// Capture names we ask tree-sitter for. `configure` prefix-matches the grammar's
/// query captures against these, so more specific names (string.special.key) win.
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

/// Catppuccin-Mocha-ish color per capture name.
fn color_for(name: &str) -> Hsla {
    let rgb_hex = match name {
        "string.special.key" | "property" => 0x89b4fa, // blue (JSON keys)
        "string" => 0xa6e3a1,                          // green
        "number" => 0xfab387,                          // peach
        "constant.builtin" | "keyword" => 0xcba6f7,    // mauve (true/false/null)
        "escape" => 0xf5c2e7,                          // pink
        "comment" => 0x6c7086,                         // overlay (dim)
        _ => 0x9399b2,                                 // punctuation/other
    };
    rgb(rgb_hex).into()
}

/// Tree-sitter highlight spans for a JSON document.
fn highlight_json(code: &str) -> Vec<(Range<usize>, HighlightStyle)> {
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

const SAMPLE: &str = r#"{
  "total_count": 30822,
  "incomplete_results": false,
  "items": [
    {
      "id": 542284380,
      "name": "bruno",
      "full_name": "usebruno/bruno",
      "private": false,
      "owner": {
        "login": "usebruno",
        "id": 114530840,
        "type": "Organization"
      },
      "stargazers_count": 38211,
      "topics": ["api", "rust", "graphql"],
      "license": null
    }
  ]
}"#;

struct CodeView {
    code: SharedString,
}

impl Render for CodeView {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        // Seed the highlight base style from the window's current text style
        // (TextStyle isn't Default), then make it monospace.
        let mut base = window.text_style();
        base.font_family = "monospace".into();
        base.color = rgb(0xcdd6f4).into();
        base.font_size = px(14.).into();

        let spans = highlight_json(&self.code);

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x181825))
            .child(
                div()
                    .px_4()
                    .py_2()
                    .bg(rgb(0x1e1e2e))
                    .text_color(rgb(0xa6adc8))
                    .text_sm()
                    .child("bru-gpui \u{2014} tree-sitter JSON highlighting in gpui"),
            )
            .child(
                div()
                    .id("code")
                    .overflow_y_scroll()
                    .size_full()
                    .p_4()
                    .font_family("monospace")
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .child(StyledText::new(self.code.clone()).with_default_highlights(&base, spans)),
            )
    }
}

fn main() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(820.), px(620.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| CodeView {
                    code: SAMPLE.into(),
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
