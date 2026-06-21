//! Tree-sitter syntax highlighting → colored spans.
//!
//! Two grammars are supported: JSON (request/response bodies) and JavaScript
//! (pre/post-request scripts + tests). Each grammar's `HighlightConfiguration`
//! is parsed once per thread and cached (the JS highlights query is large, and
//! `recompute_highlight` runs on every keystroke).
//!
//! Spans store a theme-independent capture index, not a resolved color, so the
//! editor resolves [`color`] fresh at paint time — a runtime light/dark switch
//! recolors syntax immediately, with no need to re-parse the buffer.

use std::cell::OnceCell;
use std::ops::Range;

use gpui::{rgb, Hsla};
use tree_sitter::{Node, Parser};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use crate::theme;

/// Capture names recognized across both grammars. `configure` prefix-matches the
/// grammar's query captures, so the most specific recognized name (e.g.
/// `string.special.key`) wins over its prefix (`string`).
const NAMES: &[&str] = &[
    "string",
    "string.special.key",
    "string.special",
    "number",
    "constant",
    "constant.builtin",
    "escape",
    "comment",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "keyword",
    "property",
    "variable",
    "variable.builtin",
    "function",
    "function.builtin",
    "function.method",
    "constructor",
    "operator",
];

/// Resolve a capture index (into [`NAMES`]) to a color for the active theme.
/// `None` means "no dedicated color" — the editor paints it in its default text
/// color, which keeps plain identifiers readable and theme-live.
pub fn color(kind: usize) -> Option<Hsla> {
    color_for(NAMES.get(kind)?)
}

fn color_for(name: &str) -> Option<Hsla> {
    // Catppuccin Mocha on dark; Latte (white-bg-safe) on light. Plain identifiers
    // (`variable`) return None so they fall through to the editor's text color.
    if name == "variable" {
        return None;
    }
    let hex = if theme::is_dark() {
        match name {
            "string.special.key" | "property" => 0x89b4fa, // blue (JSON keys / members)
            "string" => 0xa6e3a1,                          // green
            "string.special" | "escape" => 0xf5c2e7,       // pink (regex / escapes)
            "number" => 0xfab387,                          // peach
            "constant" => 0xf9e2af,                        // yellow
            "constant.builtin" | "keyword" => 0xcba6f7,    // mauve (true/false/null, kw)
            "function" | "function.builtin" | "function.method" => 0x89b4fa, // blue
            "constructor" => 0xf9e2af,                     // yellow (classes / types)
            "variable.builtin" => 0xeba0ac,                // maroon (this / super)
            "operator" => 0x89dceb,                        // sky
            "comment" => 0x6c7086,                         // dim
            _ => 0x9399b2,                                 // punctuation / other
        }
    } else {
        match name {
            "string.special.key" | "property" => 0x1e66f5,
            "string" => 0x40a02b,
            "string.special" | "escape" => 0xd01884,
            "number" => 0xc4520a,
            "constant" => 0xdf8e1d,
            "constant.builtin" | "keyword" => 0x8839ef,
            "function" | "function.builtin" | "function.method" => 0x1e66f5,
            "constructor" => 0xdf8e1d,
            "variable.builtin" => 0xe64553,
            "operator" => 0x209fb5,
            "comment" => 0x8c8fa1,
            _ => 0x5c5f77,
        }
    };
    Some(rgb(hex).into())
}

/// Build + configure a grammar's highlight config (injections/locals unused).
fn make_config(language: tree_sitter::Language, name: &str, query: &str) -> HighlightConfiguration {
    let mut config =
        HighlightConfiguration::new(language, name, query, "", "").expect("highlight config");
    config.configure(NAMES);
    config
}

/// Fold tree-sitter highlight events for `code` into `(byte range, capture
/// index)` spans. The index is resolved to a color at paint time via [`color`].
fn spans_for(config: &HighlightConfiguration, code: &str) -> Vec<(Range<usize>, usize)> {
    let mut hl = Highlighter::new();
    let mut spans = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let Ok(events) = hl.highlight(config, code.as_bytes(), None, |_| None) else {
        return spans;
    };
    for ev in events {
        let Ok(ev) = ev else { return spans };
        match ev {
            HighlightEvent::HighlightStart(h) => stack.push(h.0),
            HighlightEvent::HighlightEnd => {
                stack.pop();
            }
            HighlightEvent::Source { start, end } => {
                if let Some(&idx) = stack.last() {
                    spans.push((start..end, idx));
                }
            }
        }
    }
    spans
}

thread_local! {
    static JSON_CONFIG: OnceCell<HighlightConfiguration> = const { OnceCell::new() };
    static JS_CONFIG: OnceCell<HighlightConfiguration> = const { OnceCell::new() };
}

/// Compute tree-sitter highlight spans for a JSON document.
pub fn json(code: &str) -> Vec<(Range<usize>, usize)> {
    JSON_CONFIG.with(|c| {
        let config = c.get_or_init(|| {
            make_config(
                tree_sitter_json::LANGUAGE.into(),
                "json",
                tree_sitter_json::HIGHLIGHTS_QUERY,
            )
        });
        spans_for(config, code)
    })
}

/// Compute tree-sitter highlight spans for a JavaScript document (scripts/tests).
pub fn javascript(code: &str) -> Vec<(Range<usize>, usize)> {
    JS_CONFIG.with(|c| {
        let config = c.get_or_init(|| {
            make_config(
                tree_sitter_javascript::LANGUAGE.into(),
                "javascript",
                tree_sitter_javascript::HIGHLIGHT_QUERY,
            )
        });
        spans_for(config, code)
    })
}

/// The byte range of `symbol`'s definition in JavaScript `source`, located by a
/// real tree-sitter parse (so it ignores matches inside strings/comments and
/// prefers a declaration over an export reference). Used for "Go to
/// Implementation" to land on the exact line. `None` if no definition is found.
pub fn js_symbol_range(source: &str, symbol: &str) -> Option<Range<usize>> {
    let mut parser = Parser::new();
    let lang: tree_sitter::Language = tree_sitter_javascript::LANGUAGE.into();
    parser.set_language(&lang).ok()?;
    let tree = parser.parse(source, None)?;

    // DFS, keeping the highest-priority (lowest number) candidate.
    let mut best: Option<(u8, Range<usize>)> = None;
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if let Some((prio, range)) = def_in_node(node, source, symbol) {
            let better = match &best {
                Some((bp, _)) => prio < *bp,
                None => true,
            };
            if better {
                best = Some((prio, range));
            }
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            stack.push(child);
        }
    }
    best.map(|(_, r)| r)
}

/// If `node` defines `symbol`, return `(priority, name-range)`. Lower priority
/// wins: a `function`/`class`/`const` declaration beats an `exports.x =` or an
/// object-property/shorthand export reference.
fn def_in_node(node: Node, src: &str, symbol: &str) -> Option<(u8, Range<usize>)> {
    let text = |n: Node| src.get(n.byte_range());
    let (field, prio): (&str, u8) = match node.kind() {
        "function_declaration" | "generator_function_declaration" | "class_declaration" => {
            ("name", 0)
        }
        "variable_declarator" => ("name", 1),
        "method_definition" => ("name", 2),
        "pair" => ("key", 4),
        "assignment_expression" => {
            // exports.x = ... / module.exports.x = ...
            let left = node.child_by_field_name("left")?;
            let prop = (left.kind() == "member_expression")
                .then(|| left.child_by_field_name("property"))
                .flatten()?;
            return (text(prop) == Some(symbol)).then(|| (3, prop.byte_range()));
        }
        "shorthand_property_identifier" => {
            // `module.exports = { x }`
            return (text(node) == Some(symbol)).then(|| (4, node.byte_range()));
        }
        _ => return None,
    };
    let name = node.child_by_field_name(field)?;
    // Only plain names — skip destructuring patterns (`const { x } = ...`).
    if !matches!(name.kind(), "identifier" | "property_identifier") {
        return None;
    }
    (text(name) == Some(symbol)).then(|| (prio, name.byte_range()))
}

#[cfg(test)]
mod tests {
    // These guard the `make_config(...).expect(...)` path: a query that fails to
    // compile against its grammar would panic the first time an editor renders,
    // which no other test would catch. Each call forces config build + a parse.
    #[test]
    fn json_config_builds_and_highlights() {
        let spans = super::json("{\n  \"a\": 1,\n  \"b\": true\n}");
        assert!(!spans.is_empty(), "expected JSON highlight spans");
    }

    #[test]
    fn javascript_config_builds_and_highlights() {
        let code = "const x = 1; // comment\nfunction f() { return `${x}`; }";
        let spans = super::javascript(code);
        assert!(!spans.is_empty(), "expected JavaScript highlight spans");
    }

    #[test]
    fn empty_input_is_safe() {
        assert!(super::json("").is_empty());
        assert!(super::javascript("").is_empty());
    }

    #[test]
    fn js_symbol_range_prefers_declaration() {
        // The function declaration (early) should win over the export shorthand.
        let src = "async function useOAPISetVar(){ return 1; }\nmodule.exports = { useOAPISetVar };";
        let r = super::js_symbol_range(src, "useOAPISetVar").unwrap();
        assert_eq!(&src[r.clone()], "useOAPISetVar");
        assert!(r.start < 30, "should point at the decl, not the export line");
    }

    #[test]
    fn js_symbol_range_const_arrow_and_missing() {
        let src = "const helper = () => 1;\nmodule.exports = { helper };";
        assert_eq!(
            super::js_symbol_range(src, "helper").map(|r| &src[r]),
            Some("helper")
        );
        assert!(super::js_symbol_range(src, "nope").is_none());
    }
}
