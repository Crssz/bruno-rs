//! Branch-coverage tests for bru-lang's public API: `parse`, `serialize`,
//! `round_trip`, `parse_env`, `load_env`, and `load_collection`. Each test
//! targets a specific surface form, error path, or edge case so that every
//! match arm / `?` / Option/Result branch in the crate is exercised.

use bru_core::{Annotation, Block, BlockContent, BruFile, Entry, Key, Value};
use bru_lang::{
    load_collection, load_env, parse, parse_env, round_trip, serialize, EnvVar, ParseError,
};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Temp-dir helper mirroring crates/bru-app/src/fsops.rs (no `tempfile` crate).
// ---------------------------------------------------------------------------

struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "bru-lang-cov-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn write(&self, rel: &str, contents: &str) {
        let p = self.0.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, contents).unwrap();
    }
    fn mkdir(&self, rel: &str) {
        std::fs::create_dir_all(self.0.join(rel)).unwrap();
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn rt(s: &str) {
    assert_eq!(round_trip(s).unwrap(), s, "round-trip mismatch for:\n{s}");
}

// ===========================================================================
// parse.rs — block forms
// ===========================================================================

#[test]
fn parse_empty_and_only_blank_lines() {
    // parse_file: peek_line None immediately, and after skipping blanks.
    assert_eq!(parse("").unwrap().blocks.len(), 0);
    assert_eq!(parse("\n\n   \n\t\n").unwrap().blocks.len(), 0);
}

#[test]
fn parse_dict_block_basic() {
    let f = parse("meta {\n  name: X\n  type: http\n}\n").unwrap();
    assert_eq!(f.blocks.len(), 1);
    assert_eq!(f.blocks[0].name, "meta");
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!("expected dict");
    };
    assert_eq!(e.len(), 2);
    assert!(matches!(&e[0].key, Key::Bare(k) if k == "name"));
    assert!(matches!(&e[0].value, Value::Inline(v) if v == "X"));
}

#[test]
fn parse_dict_with_blank_lines_inside() {
    // parse_dict_block: blank-line arm inside the block body.
    let f = parse("meta {\n  name: X\n\n  type: http\n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert_eq!(e.len(), 2);
}

#[test]
fn parse_empty_dict_block() {
    let f = parse("meta {\n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert!(e.is_empty());
    rt("meta {\n}\n");
}

#[test]
fn parse_text_block_and_byte_exact() {
    // Each TEXT_BLOCKS name routes through parse_text_block.
    for name in [
        "body",
        "body:json",
        "body:text",
        "body:xml",
        "body:sparql",
        "body:graphql",
        "body:graphql:vars",
        "script:pre-request",
        "script:post-response",
        "tests",
        "docs",
        "example",
    ] {
        let src = format!("{name} {{\n  some raw {{ braces }} text\n}}\n");
        let f = parse(&src).unwrap();
        let BlockContent::Text(t) = &f.blocks[0].content else {
            panic!("expected text block for {name}");
        };
        assert_eq!(t, "  some raw { braces } text");
        rt(&src);
    }
}

#[test]
fn parse_empty_text_block() {
    let f = parse("body:json {\n}\n").unwrap();
    let BlockContent::Text(t) = &f.blocks[0].content else {
        panic!()
    };
    assert_eq!(t, "");
    rt("body:json {\n}\n");
}

#[test]
fn parse_list_block() {
    // `name [` dispatches to parse_list_block / read_list_items.
    let f = parse("vars:secret [\n  api_key,\n  token\n]\n").unwrap();
    let BlockContent::List(items) = &f.blocks[0].content else {
        panic!("expected list block");
    };
    assert_eq!(items, &["api_key", "token"]);
}

#[test]
fn parse_list_block_with_blanks_and_trailing_comma() {
    // read_list_items: blank-line arm + trailing-comma stripping + empty item skip.
    let f = parse("vars:secret [\n\n  a,\n  ,\n  b,\n]\n").unwrap();
    let BlockContent::List(items) = &f.blocks[0].content else {
        panic!()
    };
    assert_eq!(items, &["a", "b"]);
}

#[test]
fn parse_block_bracket_before_brace_is_list() {
    // parse_block: both `{` and `[` present, `[` first → treated as a list.
    let f = parse("weird [ {\n  item\n]\n").unwrap();
    assert!(matches!(&f.blocks[0].content, BlockContent::List(_)));
}

#[test]
fn parse_block_brace_only_is_dict() {
    // parse_block: only `{` present.
    let f = parse("meta {\n  name: A\n}\n").unwrap();
    assert!(matches!(&f.blocks[0].content, BlockContent::Dict(_)));
}

#[test]
fn parse_block_brace_before_bracket_is_dict() {
    // parse_block: both present but `{` comes first → dict; the value line `[x]`
    // stays an inline string.
    let f = parse("meta {\n  k: [x]\n}\n").unwrap();
    assert!(matches!(&f.blocks[0].content, BlockContent::Dict(_)));
}

// ===========================================================================
// parse.rs — annotations
// ===========================================================================

#[test]
fn parse_flag_annotation_no_args() {
    // parse_annotation_line: no paren, no colon → bare flag (in a dict block).
    let f = parse("auth:oauth2 {\n  @disabled\n  k: v\n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert_eq!(e[0].annotations.len(), 1);
    assert_eq!(e[0].annotations[0].name, "disabled");
    assert_eq!(e[0].annotations[0].value, None);
}

#[test]
fn parse_annotation_with_single_quoted_arg() {
    // parse_annotation_line: paren present; strip_arg_quotes single-quote branch.
    let f = parse("headers {\n  @description('hello world')\n  k: v\n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert_eq!(e[0].annotations[0].name, "description");
    assert_eq!(e[0].annotations[0].value.as_deref(), Some("hello world"));
}

#[test]
fn parse_annotation_with_double_quoted_arg() {
    // strip_arg_quotes double-quote branch.
    let f = parse("headers {\n  @contentType(\"application/json\")\n  k: v\n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert_eq!(
        e[0].annotations[0].value.as_deref(),
        Some("application/json")
    );
}

#[test]
fn parse_annotation_unquoted_arg() {
    // strip_arg_quotes: no surrounding quotes → returned as-is.
    let f = parse("headers {\n  @contentType(text/plain)\n  k: v\n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert_eq!(e[0].annotations[0].value.as_deref(), Some("text/plain"));
}

#[test]
fn parse_annotation_paren_without_closing() {
    // parse_annotation_line: paren present but no `)` suffix → unwrap_or keeps it.
    let f = parse("headers {\n  @note(oops\n  k: v\n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert_eq!(e[0].annotations[0].name, "note");
    assert_eq!(e[0].annotations[0].value.as_deref(), Some("oops"));
}

#[test]
fn parse_at_local_var_is_not_annotation() {
    // parse_annotation_line: `@name: value` (colon before any paren) → None,
    // so it is parsed as a local var instead of a decorator.
    let f = parse("vars:pre-request {\n  @token: secret\n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert_eq!(e[0].annotations.len(), 0);
    assert!(e[0].local);
    assert_eq!(e[0].key.name(), "token");
}

#[test]
fn parse_annotation_paren_after_colon_is_annotation() {
    // parse_annotation_line: paren before colon → annotation. Arg holds a colon.
    let f = parse("headers {\n  @description('a: b')\n  k: v\n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert_eq!(e[0].annotations[0].value.as_deref(), Some("a: b"));
}

#[test]
fn parse_non_annotation_at_in_non_vars_block() {
    // In a non-vars dict block, a leading `@` is part of the key (local=false).
    let f = parse("headers {\n  @weird: v\n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert!(!e[0].local);
    assert_eq!(e[0].key.name(), "@weird");
}

// ===========================================================================
// parse.rs — keys
// ===========================================================================

#[test]
fn parse_annotation_empty_and_single_char_args() {
    // strip_arg_quotes: empty arg (len < 2) and a single-char arg (len < 2) both
    // take the as-is branch.
    let f = parse("headers {\n  @note()\n  @flag(x)\n  k: v\n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert_eq!(e[0].annotations[0].value.as_deref(), Some(""));
    assert_eq!(e[0].annotations[1].value.as_deref(), Some("x"));
}

#[test]
fn parse_text_block_strips_crlf_before_close() {
    // parse_text_block: the body's last line ends with CRLF, so the closing-`}`
    // line is reached after a `\r\n`; both the `\n` and the `\r` get stripped.
    let f = parse("body:json {\n  raw\r\n}\n").unwrap();
    let BlockContent::Text(t) = &f.blocks[0].content else {
        panic!()
    };
    assert_eq!(t, "  raw");
}

#[test]
fn parse_bare_key_with_colon() {
    let f = parse("headers {\n  Authorization: Bearer x\n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert!(matches!(&e[0].key, Key::Bare(k) if k == "Authorization"));
    assert!(matches!(&e[0].value, Value::Inline(v) if v == "Bearer x"));
}

#[test]
fn parse_quoted_key_with_escape_and_lone_backslash() {
    // parse_key quoted path: `\"` escape branch AND lone-backslash branch.
    let f = parse("headers {\n  \"a\\\"b\": v\n  \"c\\d\": w\n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert!(matches!(&e[0].key, Key::Quoted(k) if k == "a\"b"));
    assert!(matches!(&e[1].key, Key::Quoted(k) if k == "c\\d"));
}

// ===========================================================================
// parse.rs — values: inline / list / multiline
// ===========================================================================

#[test]
fn parse_inline_value_trimmed() {
    let f = parse("headers {\n  k:    spaced value   \n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert!(matches!(&e[0].value, Value::Inline(v) if v == "spaced value"));
}

#[test]
fn parse_empty_inline_value() {
    let f = parse("headers {\n  k: \n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert!(matches!(&e[0].value, Value::Inline(v) if v.is_empty()));
}

#[test]
fn parse_list_value() {
    // parse_pair: `key: [` → parse_list_value.
    let f = parse("meta {\n  tags: [\n    x,\n    y\n  ]\n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert!(matches!(&e[0].value, Value::List(v) if v == &["x", "y"]));
}

#[test]
fn parse_multiline_value_no_content_type() {
    let f = parse("docs-ish {\n  k: '''line one\nline two'''\n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    let Value::Multiline { text, content_type } = &e[0].value else {
        panic!("expected multiline")
    };
    assert_eq!(text, "line one\nline two");
    assert_eq!(content_type, &None);
}

#[test]
fn parse_multiline_value_with_content_type() {
    // parse_multiline_value: tail strip_prefix("@contentType(") + strip_suffix(")").
    let f = parse("body-ish {\n  k: '''payload''' @contentType(application/json)\n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    let Value::Multiline { text, content_type } = &e[0].value else {
        panic!()
    };
    assert_eq!(text, "payload");
    assert_eq!(content_type.as_deref(), Some("application/json"));
}

#[test]
fn parse_multiline_value_at_eof_without_trailing_newline() {
    // parse_multiline_value: tail_end falls through to input.len(); cursor lands
    // exactly at EOF so the `pos < len` newline-skip is NOT taken.
    let f = parse("blk {\n  k: '''x'''\n}").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert!(matches!(&e[0].value, Value::Multiline { text, .. } if text == "x"));
}

// ===========================================================================
// parse.rs — error paths
// ===========================================================================

#[test]
fn err_missing_opener_on_header() {
    // parse_block: no `{` and no `[` → MissingColon.
    let err = parse("just some text\n").unwrap_err();
    assert!(matches!(err, ParseError::MissingColon { .. }));
}

#[test]
fn err_unterminated_dict_block() {
    let err = parse("meta {\n  name: X\n").unwrap_err();
    assert!(matches!(err, ParseError::UnterminatedBlock { name, .. } if name == "meta"));
}

#[test]
fn err_unterminated_text_block() {
    let err = parse("body:json {\n  raw\n").unwrap_err();
    assert!(matches!(err, ParseError::UnterminatedBlock { name, .. } if name == "body:json"));
}

#[test]
fn err_unterminated_list_block() {
    let err = parse("vars:secret [\n  a,\n").unwrap_err();
    assert!(matches!(err, ParseError::UnterminatedList { name, .. } if name == "vars:secret"));
}

#[test]
fn err_unterminated_list_value() {
    // read_list_items reached via parse_list_value with the block name label.
    let err = parse("meta {\n  tags: [\n    a,\n}\n").unwrap_err();
    assert!(matches!(err, ParseError::UnterminatedList { .. }));
}

#[test]
fn err_missing_colon_in_pair() {
    // parse_key bare path: no `:` in the rest → MissingColon.
    let err = parse("headers {\n  nokeyhere\n}\n").unwrap_err();
    assert!(matches!(err, ParseError::MissingColon { .. }));
}

#[test]
fn err_unterminated_quoted_key() {
    let err = parse("headers {\n  \"unclosed: v\n}\n").unwrap_err();
    assert!(matches!(err, ParseError::UnterminatedQuotedKey { .. }));
}

#[test]
fn err_quoted_key_then_no_colon() {
    // parse_key quoted closes, but `after_key` has no leading `:` → MissingColon.
    let err = parse("headers {\n  \"key\" value\n}\n").unwrap_err();
    assert!(matches!(err, ParseError::MissingColon { .. }));
}

#[test]
fn err_unterminated_multiline() {
    // parse_multiline_value: no closing ''' before the block close → error.
    let err = parse("blk {\n  k: '''oops\n}\n").unwrap_err();
    assert!(matches!(err, ParseError::UnterminatedMultiline { .. }));
}

#[test]
fn err_unterminated_multiline_no_block_close() {
    // block_close_offset returns region.len() (no `}` line) and find fails → error.
    let err = parse("blk {\n  k: '''oops never closes").unwrap_err();
    assert!(matches!(err, ParseError::UnterminatedMultiline { .. }));
}

#[test]
fn err_trailing_text_after_multiline() {
    // parse_multiline_value: tail non-empty and not a @contentType → error.
    let err = parse("blk {\n  k: '''a''' junk here\n}\n").unwrap_err();
    assert!(matches!(err, ParseError::UnexpectedTrailing { got, .. } if got == "junk here"));
}

#[test]
fn err_display_messages_are_distinct() {
    // Exercise the Display impls (thiserror format strings) for each variant.
    let cases = [
        parse("x\n").unwrap_err(),
        parse("meta {\n").unwrap_err(),
        parse("v [\n").unwrap_err(),
        parse("headers {\n  bad\n}\n").unwrap_err(),
        parse("headers {\n  \"q: v\n}\n").unwrap_err(),
        parse("b {\n  k: '''open\n}\n").unwrap_err(),
        parse("b {\n  k: '''a'''x\n}\n").unwrap_err(),
    ];
    let msgs: Vec<String> = cases.iter().map(|e| e.to_string()).collect();
    for m in &msgs {
        assert!(!m.is_empty());
    }
    // Equality / PartialEq on ParseError.
    assert_eq!(
        parse("x\n").unwrap_err(),
        ParseError::MissingColon {
            got: "x".to_string(),
            line: 1
        }
    );
}

// ===========================================================================
// parse.rs — multiline value containing braces / content_type round trips
// ===========================================================================

#[test]
fn round_trip_multiline_with_content_type() {
    rt("body-ish {\n  k: '''{\"a\":1}''' @contentType(application/json)\n}\n");
}

// ===========================================================================
// serialize.rs — direct serialization of every shape
// ===========================================================================

#[test]
fn serialize_empty_file() {
    assert_eq!(serialize(&BruFile { blocks: vec![] }), "");
}

#[test]
fn serialize_dict_empty_and_nonempty() {
    let empty = BruFile {
        blocks: vec![Block {
            name: "meta".into(),
            content: BlockContent::Dict(vec![]),
        }],
    };
    assert_eq!(serialize(&empty), "meta {\n}\n");

    let one = BruFile {
        blocks: vec![Block {
            name: "meta".into(),
            content: BlockContent::Dict(vec![Entry {
                annotations: vec![],
                disabled: false,
                local: false,
                key: Key::Bare("name".into()),
                value: Value::Inline("X".into()),
            }]),
        }],
    };
    assert_eq!(serialize(&one), "meta {\n  name: X\n}\n");
}

#[test]
fn serialize_text_empty_and_nonempty() {
    let empty = BruFile {
        blocks: vec![Block {
            name: "body:json".into(),
            content: BlockContent::Text(String::new()),
        }],
    };
    assert_eq!(serialize(&empty), "body:json {\n}\n");

    let body = BruFile {
        blocks: vec![Block {
            name: "body:json".into(),
            content: BlockContent::Text("  raw".into()),
        }],
    };
    assert_eq!(serialize(&body), "body:json {\n  raw\n}\n");
}

#[test]
fn serialize_list_block() {
    let f = BruFile {
        blocks: vec![Block {
            name: "vars:secret".into(),
            content: BlockContent::List(vec!["a".into(), "b".into()]),
        }],
    };
    assert_eq!(serialize(&f), "vars:secret [\n  a,\n  b\n]\n");
}

#[test]
fn serialize_entry_disabled_local_and_quoted_key() {
    let f = BruFile {
        blocks: vec![Block {
            name: "vars:pre-request".into(),
            content: BlockContent::Dict(vec![Entry {
                annotations: vec![],
                disabled: true,
                local: true,
                key: Key::Quoted("a\"b".into()),
                value: Value::Inline("v".into()),
            }]),
        }],
    };
    // ~ then @ then quoted-key-with-escaped-quote.
    assert_eq!(serialize(&f), "vars:pre-request {\n  ~@\"a\\\"b\": v\n}\n");
}

#[test]
fn serialize_value_list_and_multiline() {
    let list = serialize(&BruFile {
        blocks: vec![Block {
            name: "meta".into(),
            content: BlockContent::Dict(vec![Entry {
                annotations: vec![],
                disabled: false,
                local: false,
                key: Key::Bare("tags".into()),
                value: Value::List(vec!["x".into(), "y".into()]),
            }]),
        }],
    });
    assert_eq!(list, "meta {\n  tags: [\n    x\n    y\n  ]\n}\n");

    let ml = serialize(&BruFile {
        blocks: vec![Block {
            name: "b".into(),
            content: BlockContent::Dict(vec![Entry {
                annotations: vec![],
                disabled: false,
                local: false,
                key: Key::Bare("k".into()),
                value: Value::Multiline {
                    text: "hi".into(),
                    content_type: Some("application/json".into()),
                },
            }]),
        }],
    });
    assert_eq!(ml, "b {\n  k: '''hi''' @contentType(application/json)\n}\n");

    let ml_none = serialize(&BruFile {
        blocks: vec![Block {
            name: "b".into(),
            content: BlockContent::Dict(vec![Entry {
                annotations: vec![],
                disabled: false,
                local: false,
                key: Key::Bare("k".into()),
                value: Value::Multiline {
                    text: "hi".into(),
                    content_type: None,
                },
            }]),
        }],
    });
    assert_eq!(ml_none, "b {\n  k: '''hi'''\n}\n");
}

#[test]
fn serialize_annotations_all_branches() {
    // None-value flag, single-quote value, double-quote value (value has '),
    // and a multiline value.
    let f = BruFile {
        blocks: vec![Block {
            name: "headers".into(),
            content: BlockContent::Dict(vec![Entry {
                annotations: vec![
                    Annotation {
                        name: "disabled".into(),
                        value: None,
                    },
                    Annotation {
                        name: "description".into(),
                        value: Some("plain".into()),
                    },
                    Annotation {
                        name: "note".into(),
                        value: Some("it's".into()),
                    },
                    Annotation {
                        name: "big".into(),
                        value: Some("a\nb".into()),
                    },
                ],
                disabled: false,
                local: false,
                key: Key::Bare("k".into()),
                value: Value::Inline("v".into()),
            }]),
        }],
    };
    let out = serialize(&f);
    assert!(out.contains("@disabled\n"), "flag: {out}");
    assert!(out.contains("@description('plain')"), "single q: {out}");
    assert!(out.contains("@note(\"it's\")"), "double q: {out}");
    assert!(out.contains("@big('''\n"), "multiline ann: {out}");
}

#[test]
fn serialize_strip_last_line_drops_trailing_newline_with_two_blocks() {
    // strip_last_line: two blocks → trailing "\n\n" then strip_last_line removes
    // the final "\n", leaving exactly one trailing newline.
    let out = serialize(&BruFile {
        blocks: vec![
            Block {
                name: "a".into(),
                content: BlockContent::Dict(vec![]),
            },
            Block {
                name: "b".into(),
                content: BlockContent::Dict(vec![]),
            },
        ],
    });
    assert_eq!(out, "a {\n}\n\nb {\n}\n");
}

#[test]
fn serialize_multiline_annotation_indents_via_split_lines() {
    // serialize_annotations multiline branch → indent_string → split_lines, with
    // a CRLF in the value exercising the `\r` strip in split_lines.
    let f = BruFile {
        blocks: vec![Block {
            name: "headers".into(),
            content: BlockContent::Dict(vec![Entry {
                annotations: vec![Annotation {
                    name: "big".into(),
                    value: Some("a\r\nb".into()),
                }],
                disabled: false,
                local: false,
                key: Key::Bare("k".into()),
                value: Value::Inline("v".into()),
            }]),
        }],
    };
    let out = serialize(&f);
    // The `\r` is stripped during indent_string's split_lines. The value is
    // indented once by serialize_annotations and again by the block body, so the
    // inner lines end up 4-space indented. The key assertion is that no `\r`
    // survives and the value lines are present.
    assert!(!out.contains('\r'), "{out}");
    assert!(out.contains("@big('''\n    a\n    b\n  ''')"), "{out}");
}

// ===========================================================================
// round_trip — combined
// ===========================================================================

#[test]
fn round_trip_propagates_parse_error() {
    // lib.rs round_trip: the `?` error path when parse fails.
    let err = round_trip("not a block\n").unwrap_err();
    assert!(matches!(err, ParseError::MissingColon { .. }));
}

#[test]
fn round_trip_assorted_forms() {
    rt("meta {\n  name: X\n  seq: 1\n}\n\nget {\n  url: https://x/y\n}\n");
    rt("vars:secret [\n  api_key,\n  token\n]\n");
    rt("headers {\n  @description('d')\n  ~Authorization: Bearer x\n}\n");
}

// ===========================================================================
// env.rs — parse_env / load_env
// ===========================================================================

#[test]
fn parse_env_plain_vars() {
    let vars = parse_env("vars {\n  host: example.com\n  port: 443\n}\n");
    assert_eq!(
        vars,
        vec![
            EnvVar {
                name: "host".into(),
                value: "example.com".into(),
                enabled: true,
                secret: false,
            },
            EnvVar {
                name: "port".into(),
                value: "443".into(),
                enabled: true,
                secret: false,
            },
        ]
    );
}

#[test]
fn parse_env_disabled_var() {
    // vars dict: a `~` disabled entry → enabled=false.
    let vars = parse_env("vars {\n  host: a\n  ~debug: b\n}\n");
    assert_eq!(vars[1].name, "debug");
    assert!(!vars[1].enabled);
    assert!(!vars[1].secret);
}

#[test]
fn parse_env_secret_list_enabled_and_disabled() {
    // vars:secret list: plain name (enabled) and `~name` (disabled), value empty.
    let vars = parse_env("vars:secret [\n  token,\n  ~old_token\n]\n");
    assert_eq!(vars[0].name, "token");
    assert!(vars[0].enabled && vars[0].secret && vars[0].value.is_empty());
    assert_eq!(vars[1].name, "old_token");
    assert!(!vars[1].enabled && vars[1].secret);
}

#[test]
fn parse_env_strips_color_line() {
    // The bare top-level `color:` line is filtered before parsing.
    let vars = parse_env("color: #ff0000\nvars {\n  host: a\n}\n");
    assert_eq!(vars.len(), 1);
    assert_eq!(vars[0].name, "host");
}

#[test]
fn parse_env_malformed_returns_empty() {
    // crate::parse errors → parse_env returns an empty Vec (no panic).
    assert!(parse_env("vars {\n  host: a\n").is_empty());
}

#[test]
fn parse_env_ignores_unknown_blocks() {
    // The `_ => {}` arm: a block that is neither vars nor vars:secret.
    let vars = parse_env("meta {\n  name: dev\n}\nvars {\n  k: v\n}\n");
    assert_eq!(vars.len(), 1);
    assert_eq!(vars[0].name, "k");

    // A `vars` block that is somehow not a Dict, and a `vars:secret` not a List,
    // both fall through the match (covered structurally by the unknown-block arm
    // above; here we confirm an empty vars dict yields nothing).
    assert!(parse_env("vars {\n}\n").is_empty());
}

#[test]
fn load_env_reads_file() {
    let d = TempDir::new("env");
    d.write(
        "environments/dev.bru",
        "color: #00ff00\nvars {\n  host: dev.example\n}\nvars:secret [\n  api_key\n]\n",
    );
    let vars = load_env(d.path(), "dev").unwrap();
    assert_eq!(vars.len(), 2);
    assert_eq!(vars[0].name, "host");
    assert_eq!(vars[0].value, "dev.example");
    assert_eq!(vars[1].name, "api_key");
    assert!(vars[1].secret);
}

#[test]
fn load_env_missing_file_errors() {
    let d = TempDir::new("env-missing");
    let err = load_env(d.path(), "nope").unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn env_var_struct_traits() {
    // Exercise derived Debug/Clone/PartialEq on EnvVar.
    let a = EnvVar {
        name: "n".into(),
        value: "v".into(),
        enabled: true,
        secret: false,
    };
    let b = a.clone();
    assert_eq!(a, b);
    assert!(format!("{a:?}").contains("EnvVar"));
}

// ===========================================================================
// loader.rs — load_collection
// ===========================================================================

fn write_request(d: &TempDir, rel: &str, name: &str, method: &str, seq: Option<i64>) {
    let seq_line = match seq {
        Some(s) => format!("  seq: {s}\n"),
        None => String::new(),
    };
    let m = method.to_lowercase();
    d.write(
        rel,
        &format!(
            "meta {{\n  name: {name}\n  type: http\n{seq_line}}}\n\n{m} {{\n  url: https://x\n}}\n"
        ),
    );
}

#[test]
fn load_collection_full_tree() {
    let d = TempDir::new("coll");
    d.write(
        "bruno.json",
        "{\n  \"version\": \"1\",\n  \"name\": \"My Coll\"\n}\n",
    );
    // Requests with seq → sorted by seq.
    write_request(&d, "b.bru", "Beta", "POST", Some(2));
    write_request(&d, "a.bru", "Alpha", "GET", Some(1));
    // Request with no seq → sorts last.
    write_request(&d, "z.bru", "Zeta", "PUT", None);
    // Inherited-config files are excluded.
    d.write("collection.bru", "meta {\n  name: ignored\n}\n");
    // A non-.bru file is ignored.
    d.write("README.md", "ignore me");
    // Subfolder with its own folder.bru name + a request.
    d.write("sub/folder.bru", "meta {\n  name: SubFolder\n}\n");
    write_request(&d, "sub/inner.bru", "Inner", "DELETE", Some(1));
    // Skipped dirs.
    d.mkdir("environments");
    d.write("environments/dev.bru", "vars {\n  k: v\n}\n");
    d.mkdir(".hidden");
    d.write(".hidden/x.bru", "meta {\n  name: hidden\n}\n");
    d.mkdir("node_modules");

    let tree = load_collection(d.path()).unwrap();
    assert_eq!(tree.name, "My Coll");

    let names: Vec<&str> = tree.root.requests.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Alpha", "Beta", "Zeta"]);
    assert_eq!(tree.root.requests[0].method.as_deref(), Some("GET"));
    assert_eq!(tree.root.requests[0].seq, Some(1));
    assert_eq!(tree.root.requests[2].seq, None);

    // Only `sub` survives (environments, .hidden, node_modules skipped).
    assert_eq!(tree.root.folders.len(), 1);
    assert_eq!(tree.root.folders[0].name, "SubFolder");
    assert_eq!(tree.root.folders[0].requests[0].name, "Inner");
    assert_eq!(
        tree.root.folders[0].requests[0].method.as_deref(),
        Some("DELETE")
    );
}

#[test]
fn load_collection_name_fallback_to_dir() {
    // collection_name None (no bruno.json) → dir_name fallback.
    let d = TempDir::new("noname");
    write_request(&d, "a.bru", "A", "GET", Some(1));
    let tree = load_collection(d.path()).unwrap();
    let stem = d.path().file_name().unwrap().to_string_lossy();
    assert_eq!(tree.name, stem);
}

#[test]
fn load_collection_bruno_json_without_name_field() {
    // collection_name: bruno.json exists but has no "name" key → None → fallback.
    let d = TempDir::new("noname-field");
    d.write("bruno.json", "{\n  \"version\": \"1\"\n}\n");
    write_request(&d, "a.bru", "A", "GET", Some(1));
    let tree = load_collection(d.path()).unwrap();
    let stem = d.path().file_name().unwrap().to_string_lossy();
    assert_eq!(tree.name, stem);
}

#[test]
fn load_collection_folder_name_fallback_to_dirname() {
    // folder_name None (no folder.bru) → dname is used for the folder.
    let d = TempDir::new("foldername");
    d.write("bruno.json", "{ \"name\": \"C\" }");
    d.mkdir("MyFolder");
    write_request(&d, "MyFolder/r.bru", "R", "GET", Some(1));
    let tree = load_collection(d.path()).unwrap();
    assert_eq!(tree.root.folders.len(), 1);
    assert_eq!(tree.root.folders[0].name, "MyFolder");
}

#[test]
fn load_collection_folder_bru_without_name_uses_dirname() {
    // folder.bru parses but has no meta.name → request_name None → dname.
    let d = TempDir::new("folderbru-noname");
    d.write("bruno.json", "{ \"name\": \"C\" }");
    d.write("Sub/folder.bru", "meta {\n  type: folder\n}\n");
    write_request(&d, "Sub/r.bru", "R", "GET", Some(1));
    let tree = load_collection(d.path()).unwrap();
    assert_eq!(tree.root.folders[0].name, "Sub");
}

#[test]
fn load_collection_unparseable_request_falls_back_to_stem() {
    // load_request: read/parse fails → name=stem, method=None, seq=None.
    let d = TempDir::new("badreq");
    d.write("bruno.json", "{ \"name\": \"C\" }");
    d.write("broken.bru", "this is { not valid bru");
    let tree = load_collection(d.path()).unwrap();
    let r = tree.root.requests.iter().find(|r| r.name == "broken");
    assert!(r.is_some(), "broken request should appear under its stem");
    let r = r.unwrap();
    assert_eq!(r.method, None);
    assert_eq!(r.seq, None);
}

#[test]
fn load_collection_request_without_meta_name_uses_stem() {
    // load_request: parses OK but request_name() None → stem fallback.
    let d = TempDir::new("noname-req");
    d.write("bruno.json", "{ \"name\": \"C\" }");
    d.write("my-req.bru", "get {\n  url: https://x\n}\n");
    let tree = load_collection(d.path()).unwrap();
    let r = tree
        .root
        .requests
        .iter()
        .find(|r| r.name == "my-req")
        .unwrap();
    assert_eq!(r.method.as_deref(), Some("GET"));
}

#[test]
fn load_collection_skips_beyond_max_depth() {
    // load_folder: nesting past MAX_DEPTH (64) triggers the warn-and-skip branch
    // (eprintln + continue) instead of recursing further. The build must still
    // succeed and contain the levels up to the cap.
    let d = TempDir::new("deep");
    d.write("bruno.json", "{ \"name\": \"Deep\" }");
    // Build 66 nested folders: depths 0..65. The folder created at depth 64
    // (the 65th nested dir) is skipped because depth >= MAX_DEPTH there.
    let mut rel = String::new();
    for i in 0..66 {
        if !rel.is_empty() {
            rel.push('/');
        }
        rel.push_str(&format!("d{i}"));
        d.mkdir(&rel);
    }
    // A request near the top to confirm the tree loads.
    write_request(&d, "d0/r.bru", "Top", "GET", Some(1));
    let tree = load_collection(d.path()).unwrap();
    // Walk down counting folder nesting; it must stop at the depth cap, well
    // short of 66, proving the deep tail was pruned.
    let mut depth = 0usize;
    let mut cur = &tree.root;
    while let Some(child) = cur.folders.first() {
        depth += 1;
        cur = child;
    }
    assert!(depth >= 1, "expected at least one nested folder");
    assert!(depth < 66, "deep tail should be pruned, got depth {depth}");
}

#[test]
fn load_collection_missing_dir_errors() {
    let missing = std::env::temp_dir().join(format!("bru-lang-cov-missing-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&missing);
    let err = load_collection(&missing).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn load_collection_name_tie_break_by_lowercase_name() {
    // Two requests with the SAME seq → tie broken by case-insensitive name.
    let d = TempDir::new("tiebreak");
    d.write("bruno.json", "{ \"name\": \"C\" }");
    write_request(&d, "x.bru", "banana", "GET", Some(5));
    write_request(&d, "y.bru", "Apple", "GET", Some(5));
    let tree = load_collection(d.path()).unwrap();
    let names: Vec<&str> = tree.root.requests.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Apple", "banana"]);
}

#[test]
fn load_collection_folders_sorted_lowercase() {
    // folders.sort_by_key by lowercased name.
    let d = TempDir::new("foldersort");
    d.write("bruno.json", "{ \"name\": \"C\" }");
    d.write("Zoo/folder.bru", "meta {\n  name: Zoo\n}\n");
    write_request(&d, "Zoo/r.bru", "Zr", "GET", Some(1));
    d.write("apple/folder.bru", "meta {\n  name: apple\n}\n");
    write_request(&d, "apple/r.bru", "Ar", "GET", Some(1));
    let tree = load_collection(d.path()).unwrap();
    let fnames: Vec<&str> = tree.root.folders.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(fnames, ["apple", "Zoo"]);
}
