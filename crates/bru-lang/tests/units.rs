//! Focused unit tests for the trickier `.bru` surface forms, pinning behavior
//! that the broad corpus exercises but doesn't isolate.

use bru_core::{BlockContent, Key, Value};
use bru_lang::{parse, round_trip};

fn rt(s: &str) {
    assert_eq!(round_trip(s).unwrap(), s, "round-trip mismatch for:\n{s}");
}

#[test]
fn disabled_and_local_prefixes() {
    // `~` disabled, `@` local, `~@` both — order is ~ before @.
    let src = "vars:post-response {\n  token: a\n  @local: b\n  ~off: c\n  ~@localoff: d\n}\n";
    rt(src);
    let f = parse(src).unwrap();
    let BlockContent::Dict(entries) = &f.blocks[0].content else {
        panic!("dict")
    };
    assert_eq!((entries[0].disabled, entries[0].local), (false, false));
    assert_eq!((entries[1].disabled, entries[1].local), (false, true));
    assert_eq!((entries[2].disabled, entries[2].local), (true, false));
    assert_eq!((entries[3].disabled, entries[3].local), (true, true));
}

#[test]
fn quoted_keys_and_escapes_and_colons_in_values() {
    let src = "headers {\n  content-type: application/json\n  \"key with spaces\": is allowed\n  \"colon:header\": is allowed\n  \"nested escaped \\\"quote\\\"\": is allowed\n}\n";
    rt(src);
    let f = parse(src).unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert!(matches!(&e[0].key, Key::Bare(k) if k == "content-type"));
    assert!(matches!(&e[1].key, Key::Quoted(k) if k == "key with spaces"));
    assert!(matches!(&e[2].key, Key::Quoted(k) if k == "colon:header"));
    assert!(matches!(&e[3].key, Key::Quoted(k) if k == "nested escaped \"quote\""));
}

#[test]
fn empty_value_keeps_trailing_space() {
    // `key:` with nothing after serializes back as `key: ` (colon-space).
    let src = "auth:oauth2 {\n  refresh_token_url: \n}\n";
    rt(src);
}

#[test]
fn list_value_vs_bracketed_string() {
    // `tags: [` + newline => a list; `[inline]` on the value line => a string.
    rt("meta {\n  name: X\n  tags: [\n    foo\n    bar\n  ]\n}\n");
    let f = parse("meta {\n  name: X\n  tags: [\n    foo\n    bar\n  ]\n}\n").unwrap();
    let BlockContent::Dict(e) = &f.blocks[0].content else {
        panic!()
    };
    assert!(matches!(&e[1].value, Value::List(v) if v == &["foo", "bar"]));

    let f2 = parse("headers {\n  x: [inline value]\n}\n").unwrap();
    let BlockContent::Dict(e2) = &f2.blocks[0].content else {
        panic!()
    };
    assert!(matches!(&e2[0].value, Value::Inline(v) if v == "[inline value]"));
}

#[test]
fn verbatim_text_body_is_byte_exact() {
    let src = "body:json {\n  {\n    \"hello\": \"world\"\n  }\n}\n";
    rt(src);
    let f = parse(src).unwrap();
    let BlockContent::Text(t) = &f.blocks[0].content else {
        panic!()
    };
    assert_eq!(t, "  {\n    \"hello\": \"world\"\n  }");
}

#[test]
fn empty_input_and_single_block() {
    assert_eq!(round_trip("").unwrap(), "");
    rt("meta {\n  type: collection\n}\n");
}
