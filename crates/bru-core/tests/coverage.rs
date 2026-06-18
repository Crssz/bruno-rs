//! Exhaustive public-API coverage for interp.rs, assert.rs, model.rs, request.rs.
//!
//! Aims to exercise every function, every match arm and every error/edge path
//! reachable through the crate's public surface (and through `bru_lang::parse`,
//! which builds the `BruFile`s the projection code consumes).

use std::collections::HashMap;

use bru_core::{
    eval_response_expr, evaluate_assertions, interpolate, ApiKeyPlacement, Assertion, Auth,
    BlockContent, Body, BruFile, FileBodyItem, Key, KeyVal, MultipartField, MultipartValue, OAuth2,
    Request, RequestSettings, ResponseFacts, Value, Var, HTTP_VERBS,
};

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn assertion(expr: &str, value: &str) -> Assertion {
    Assertion {
        expr: expr.to_string(),
        value: value.to_string(),
        enabled: true,
    }
}

fn facts<'a>(
    status: u16,
    headers: &'a [(String, String)],
    body: Option<&'a serde_json::Value>,
    body_text: &'a str,
    rt: u128,
) -> ResponseFacts<'a> {
    ResponseFacts {
        status,
        headers,
        body_json: body,
        body_text,
        response_time_ms: rt,
    }
}

// ---------------------------------------------------------------------------
// interp.rs
// ---------------------------------------------------------------------------

#[test]
fn interpolate_plain_and_missing() {
    let v = vars(&[("base", "https://x"), ("id", "7")]);
    assert_eq!(interpolate("{{base}}/{{id}}", &v), "https://x/7");
    // whitespace trim inside braces
    assert_eq!(interpolate("{{  base  }}/{{ id }}", &v), "https://x/7");
    // unresolved left verbatim, preserving original interior (no trimming)
    assert_eq!(interpolate("[{{ missing }}]", &v), "[{{ missing }}]");
    // text with no placeholders passes through unchanged
    assert_eq!(interpolate("nothing here", &v), "nothing here");
    // empty template
    assert_eq!(interpolate("", &v), "");
}

#[test]
fn interpolate_unterminated_open_brace() {
    let v = vars(&[("a", "1")]);
    // a `{{` without a closing `}}` keeps the `{{` and the trailing text.
    assert_eq!(interpolate("pre {{a", &v), "pre {{a");
    assert_eq!(interpolate("{{a}} then {{b", &v), "1 then {{b");
}

#[test]
fn interpolate_adjacent_and_nested_text() {
    let v = vars(&[("a", "X"), ("b", "Y")]);
    assert_eq!(interpolate("{{a}}{{b}}", &v), "XY");
    // surrounding text on both sides
    assert_eq!(interpolate("<{{a}}>", &v), "<X>");
    // a resolved var whose VALUE contains braces is not re-interpolated
    let v2 = vars(&[("a", "{{b}}"), ("b", "Y")]);
    assert_eq!(interpolate("{{a}}", &v2), "{{b}}");
}

#[test]
fn interpolate_dynamic_timestamp_and_randomint() {
    let v = HashMap::new();
    let ts = interpolate("{{$timestamp}}", &v);
    assert!(!ts.is_empty() && ts.chars().all(|c| c.is_ascii_digit()));

    let ri = interpolate("{{$randomInt}}", &v);
    let n: u64 = ri.parse().expect("randomInt is numeric");
    assert!(n < 1000);
}

#[test]
fn interpolate_dynamic_guid_and_uuid() {
    let v = HashMap::new();
    for token in ["{{$guid}}", "{{$randomUUID}}"] {
        let id = interpolate(token, &v);
        assert_eq!(id.len(), 36, "{token}");
        assert_eq!(id.matches('-').count(), 4, "{token}");
        // version nibble is 4, variant nibble in {8,9,a,b}
        let bytes: Vec<&str> = id.split('-').collect();
        assert!(bytes[2].starts_with('4'));
        let variant = bytes[3].chars().next().unwrap();
        assert!(['8', '9', 'a', 'b'].contains(&variant), "variant={variant}");
        // all non-dash chars are hex
        assert!(id.chars().all(|c| c == '-' || c.is_ascii_hexdigit()));
    }
}

#[test]
fn interpolate_dynamic_iso_timestamp_shape() {
    let v = HashMap::new();
    let iso = interpolate("{{$isoTimestamp}}", &v);
    // YYYY-MM-DDTHH:MM:SSZ -> length 20, ends with Z, has the T separator.
    assert_eq!(iso.len(), 20, "{iso}");
    assert!(iso.ends_with('Z'));
    assert_eq!(&iso[4..5], "-");
    assert_eq!(&iso[7..8], "-");
    assert_eq!(&iso[10..11], "T");
    assert_eq!(&iso[13..14], ":");
    assert_eq!(&iso[16..17], ":");
    // year should be >= 2025 (we are well past the epoch)
    let year: i64 = iso[..4].parse().unwrap();
    assert!(year >= 2025, "{iso}");
}

#[test]
fn interpolate_unknown_dynamic_var_left_verbatim() {
    let v = HashMap::new();
    // an unknown `$`-prefixed token resolves to None -> kept verbatim.
    assert_eq!(interpolate("{{$nope}}", &v), "{{$nope}}");
    // a `$`-token that also is not in vars stays verbatim too.
    assert_eq!(interpolate("{{$randomThing}}", &v), "{{$randomThing}}");
}

#[test]
fn interpolate_empty_placeholder_name() {
    // `{{}}` trims to empty -> not a dynamic var, not in vars -> verbatim.
    let v = HashMap::new();
    assert_eq!(interpolate("{{}}", &v), "{{}}");
}

// ---------------------------------------------------------------------------
// assert.rs — operators, parse_operator defaulting, eval_response_expr paths
// ---------------------------------------------------------------------------

#[test]
fn assert_eq_neq_default_and_explicit() {
    let body = serde_json::json!({"name": "ada", "count": "3"});
    let f = facts(200, &[], Some(&body), "", 5);

    // bare value defaults to eq
    assert!(evaluate_assertions(&[assertion("res.body.name", "ada")], &f)[0].passed);
    // explicit eq
    assert!(evaluate_assertions(&[assertion("res.body.name", "eq ada")], &f)[0].passed);
    // eq fail
    assert!(!evaluate_assertions(&[assertion("res.body.name", "bob")], &f)[0].passed);
    // neq pass / fail
    assert!(evaluate_assertions(&[assertion("res.body.name", "neq bob")], &f)[0].passed);
    assert!(!evaluate_assertions(&[assertion("res.body.name", "neq ada")], &f)[0].passed);
}

#[test]
fn assert_contains_notcontains() {
    let body = serde_json::json!({"s": "hello world"});
    let f = facts(200, &[], Some(&body), "", 5);
    assert!(evaluate_assertions(&[assertion("res.body.s", "contains world")], &f)[0].passed);
    assert!(!evaluate_assertions(&[assertion("res.body.s", "contains nope")], &f)[0].passed);
    assert!(evaluate_assertions(&[assertion("res.body.s", "notContains nope")], &f)[0].passed);
    assert!(!evaluate_assertions(&[assertion("res.body.s", "notContains world")], &f)[0].passed);
}

#[test]
fn assert_numeric_gt_gte_lt_lte() {
    let body = serde_json::json!({"n": 5});
    let f = facts(200, &[], Some(&body), "", 5);
    assert!(evaluate_assertions(&[assertion("res.body.n", "gt 4")], &f)[0].passed);
    assert!(!evaluate_assertions(&[assertion("res.body.n", "gt 5")], &f)[0].passed);
    assert!(evaluate_assertions(&[assertion("res.body.n", "gte 5")], &f)[0].passed);
    assert!(!evaluate_assertions(&[assertion("res.body.n", "gte 6")], &f)[0].passed);
    assert!(evaluate_assertions(&[assertion("res.body.n", "lt 6")], &f)[0].passed);
    assert!(!evaluate_assertions(&[assertion("res.body.n", "lt 5")], &f)[0].passed);
    assert!(evaluate_assertions(&[assertion("res.body.n", "lte 5")], &f)[0].passed);
    assert!(!evaluate_assertions(&[assertion("res.body.n", "lte 4")], &f)[0].passed);
}

#[test]
fn assert_numeric_non_numeric_operands_fail() {
    let body = serde_json::json!({"s": "abc"});
    let f = facts(200, &[], Some(&body), "", 5);
    // actual not parseable as f64 -> gt path returns false
    assert!(!evaluate_assertions(&[assertion("res.body.s", "gt 1")], &f)[0].passed);
    // expected not parseable -> false
    let body2 = serde_json::json!({"n": 5});
    let f2 = facts(200, &[], Some(&body2), "", 5);
    assert!(!evaluate_assertions(&[assertion("res.body.n", "gt notanumber")], &f2)[0].passed);
}

#[test]
fn assert_unary_null_operators() {
    let body = serde_json::json!({"nil": null, "v": "x"});
    let f = facts(200, &[], Some(&body), "", 5);
    // serde null stringifies to "null"
    assert!(evaluate_assertions(&[assertion("res.body.nil", "isNull")], &f)[0].passed);
    assert!(!evaluate_assertions(&[assertion("res.body.v", "isNull")], &f)[0].passed);
    // missing path -> actual None -> isNull true
    assert!(evaluate_assertions(&[assertion("res.body.missing", "isNull")], &f)[0].passed);

    assert!(evaluate_assertions(&[assertion("res.body.v", "isNotNull")], &f)[0].passed);
    assert!(!evaluate_assertions(&[assertion("res.body.nil", "isNotNull")], &f)[0].passed);
    assert!(!evaluate_assertions(&[assertion("res.body.missing", "isNotNull")], &f)[0].passed);
}

#[test]
fn assert_unary_defined_undefined() {
    let body = serde_json::json!({"v": "x"});
    let f = facts(200, &[], Some(&body), "", 5);
    assert!(evaluate_assertions(&[assertion("res.body.v", "isDefined")], &f)[0].passed);
    assert!(!evaluate_assertions(&[assertion("res.body.missing", "isDefined")], &f)[0].passed);
    assert!(evaluate_assertions(&[assertion("res.body.missing", "isUndefined")], &f)[0].passed);
    assert!(!evaluate_assertions(&[assertion("res.body.v", "isUndefined")], &f)[0].passed);
}

#[test]
fn assert_unary_empty_notempty() {
    let body = serde_json::json!({"empty": "", "full": "x"});
    let f = facts(200, &[], Some(&body), "", 5);
    assert!(evaluate_assertions(&[assertion("res.body.empty", "isEmpty")], &f)[0].passed);
    // missing -> None counts as empty
    assert!(evaluate_assertions(&[assertion("res.body.missing", "isEmpty")], &f)[0].passed);
    assert!(!evaluate_assertions(&[assertion("res.body.full", "isEmpty")], &f)[0].passed);

    assert!(evaluate_assertions(&[assertion("res.body.full", "isNotEmpty")], &f)[0].passed);
    assert!(!evaluate_assertions(&[assertion("res.body.empty", "isNotEmpty")], &f)[0].passed);
    assert!(!evaluate_assertions(&[assertion("res.body.missing", "isNotEmpty")], &f)[0].passed);
}

#[test]
fn assert_unary_true_false() {
    let body = serde_json::json!({"t": true, "f": false, "s": "true"});
    let f = facts(200, &[], Some(&body), "", 5);
    assert!(evaluate_assertions(&[assertion("res.body.t", "isTrue")], &f)[0].passed);
    assert!(evaluate_assertions(&[assertion("res.body.s", "isTrue")], &f)[0].passed);
    assert!(!evaluate_assertions(&[assertion("res.body.f", "isTrue")], &f)[0].passed);
    // missing -> None -> isTrue false
    assert!(!evaluate_assertions(&[assertion("res.body.missing", "isTrue")], &f)[0].passed);

    assert!(evaluate_assertions(&[assertion("res.body.f", "isFalse")], &f)[0].passed);
    assert!(!evaluate_assertions(&[assertion("res.body.t", "isFalse")], &f)[0].passed);
    assert!(!evaluate_assertions(&[assertion("res.body.missing", "isFalse")], &f)[0].passed);
}

#[test]
fn assert_binary_on_missing_actual_is_false() {
    let body = serde_json::json!({});
    let f = facts(200, &[], Some(&body), "", 5);
    // a binary operator (eq) against a missing path -> actual None -> false,
    // and the outcome.actual reports "undefined".
    let out = evaluate_assertions(&[assertion("res.body.x", "eq 1")], &f);
    assert!(!out[0].passed);
    assert_eq!(out[0].actual, "undefined");
}

#[test]
fn assert_unknown_operator_is_treated_as_eq_value() {
    // "bogus" is not in the OPS list, so parse_operator returns ("eq", "bogus ...").
    // With a single token it becomes the expected value for an eq compare.
    let body = serde_json::json!({"s": "weird"});
    let f = facts(200, &[], Some(&body), "", 5);
    // value "weird" with default eq -> passes
    assert!(evaluate_assertions(&[assertion("res.body.s", "weird")], &f)[0].passed);
}

#[test]
fn assert_operator_prefix_not_split_on_boundary() {
    // "contains" must match as a whole op; "containsX" (no space) must NOT be
    // taken as the `contains` operator — it falls through to eq.
    let body = serde_json::json!({"s": "containsX"});
    let f = facts(200, &[], Some(&body), "", 5);
    let out = evaluate_assertions(&[assertion("res.body.s", "containsX")], &f);
    assert_eq!(out[0].operator, "eq");
    assert!(out[0].passed);
}

#[test]
fn assert_disabled_assertions_skipped() {
    let body = serde_json::json!({"n": 1});
    let f = facts(200, &[], Some(&body), "", 5);
    let disabled = Assertion {
        expr: "res.body.n".to_string(),
        value: "eq 999".to_string(),
        enabled: false,
    };
    // disabled assertion is filtered out entirely -> empty result.
    assert!(evaluate_assertions(&[disabled], &f).is_empty());
}

#[test]
fn assert_outcome_fields_populated() {
    let body = serde_json::json!({"n": 5});
    let f = facts(200, &[], Some(&body), "", 5);
    let out = evaluate_assertions(&[assertion("res.body.n", "gt 4")], &f);
    assert_eq!(out[0].expr, "res.body.n");
    assert_eq!(out[0].operator, "gt");
    assert_eq!(out[0].expected, "4");
    assert_eq!(out[0].actual, "5");
    assert!(out[0].passed);

    // AssertOutcome derives Clone/PartialEq/Debug — exercise them.
    let cloned = out[0].clone();
    assert_eq!(cloned, out[0]);
    assert!(format!("{cloned:?}").contains("res.body.n"));
}

#[test]
fn eval_response_expr_status_and_response_time() {
    let f = facts(404, &[], None, "", 99);
    assert_eq!(eval_response_expr("res.status", &f).as_deref(), Some("404"));
    assert_eq!(
        eval_response_expr("res.responseTime", &f).as_deref(),
        Some("99")
    );
}

#[test]
fn eval_response_expr_prefix_variants() {
    let body = serde_json::json!({"a": 1});
    let f = facts(200, &[], Some(&body), "", 5);
    // `$res` prefix
    assert_eq!(
        eval_response_expr("$res.status", &f).as_deref(),
        Some("200")
    );
    // `res` prefix
    assert_eq!(eval_response_expr("res.status", &f).as_deref(), Some("200"));
    // no prefix at all — strips nothing, head = "status"
    assert_eq!(eval_response_expr("status", &f).as_deref(), Some("200"));
    // a bare path with no recognized head -> None
    assert_eq!(eval_response_expr("res.unknown", &f), None);
}

#[test]
fn eval_response_expr_headers_lookup() {
    let headers = vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        ("X-Trace".to_string(), "abc".to_string()),
    ];
    let f = facts(200, &headers, None, "", 5);
    // case-insensitive header lookup
    assert_eq!(
        eval_response_expr("res.headers.content-type", &f).as_deref(),
        Some("application/json")
    );
    assert_eq!(
        eval_response_expr("res.headers.X-TRACE", &f).as_deref(),
        Some("abc")
    );
    // missing header -> None
    assert_eq!(eval_response_expr("res.headers.nope", &f), None);
    // `res.headers` with no key part -> rest is None -> `?` returns None.
    assert_eq!(eval_response_expr("res.headers", &f), None);
}

#[test]
fn eval_response_expr_body_whole_json_and_text_fallback() {
    // whole body, JSON present -> serialized json
    let body = serde_json::json!({"a": 1});
    let f = facts(200, &[], Some(&body), "ignored", 5);
    assert_eq!(
        eval_response_expr("res.body", &f).as_deref(),
        Some(r#"{"a":1}"#)
    );

    // whole body, no JSON -> falls back to body_text
    let f2 = facts(200, &[], None, "raw text body", 5);
    assert_eq!(
        eval_response_expr("res.body", &f2).as_deref(),
        Some("raw text body")
    );
}

#[test]
fn eval_response_expr_body_path_no_json_returns_none() {
    // body with a path but no JSON document -> `facts.body_json?` is None.
    let f = facts(200, &[], None, "text", 5);
    assert_eq!(eval_response_expr("res.body.a.b", &f), None);
}

#[test]
fn eval_response_expr_json_string_vs_other() {
    // a JSON string value returns the raw string (no surrounding quotes),
    // while non-string values are serialized.
    let body = serde_json::json!({"s": "hi", "n": 7, "b": true, "obj": {"k": 1}});
    let f = facts(200, &[], Some(&body), "", 5);
    assert_eq!(eval_response_expr("res.body.s", &f).as_deref(), Some("hi"));
    assert_eq!(eval_response_expr("res.body.n", &f).as_deref(), Some("7"));
    assert_eq!(
        eval_response_expr("res.body.b", &f).as_deref(),
        Some("true")
    );
    assert_eq!(
        eval_response_expr("res.body.obj", &f).as_deref(),
        Some(r#"{"k":1}"#)
    );
}

#[test]
fn eval_response_expr_navigate_indices_and_missing() {
    let body = serde_json::json!({
        "roles": [["admin", "user"], ["guest"]],
        "items": [{"id": 7}],
    });
    let f = facts(200, &[], Some(&body), "", 5);
    // nested double index roles[0][1]
    assert_eq!(
        eval_response_expr("res.body.roles[0][1]", &f).as_deref(),
        Some("user")
    );
    // key then index then key
    assert_eq!(
        eval_response_expr("res.body.items[0].id", &f).as_deref(),
        Some("7")
    );
    // out-of-range index -> None
    assert_eq!(eval_response_expr("res.body.roles[9]", &f), None);
    // index into a non-array -> None
    assert_eq!(eval_response_expr("res.body.items[0].id[0]", &f), None);
    // missing key -> None
    assert_eq!(eval_response_expr("res.body.nope.deep", &f), None);
}

#[test]
fn eval_response_expr_navigate_leading_index_segment() {
    // A path segment that begins with `[` (empty key) navigates the array
    // directly without a get(key) first.
    let body = serde_json::json!({"arr": [10, 20, 30]});
    let f = facts(200, &[], Some(&body), "", 5);
    // `arr.[2]` -> key "arr", then a segment "[2]" with empty key part.
    assert_eq!(
        eval_response_expr("res.body.arr.[2]", &f).as_deref(),
        Some("30")
    );
}

#[test]
fn eval_response_expr_navigate_malformed_index() {
    let body = serde_json::json!({"arr": [1, 2, 3]});
    let f = facts(200, &[], Some(&body), "", 5);
    // non-numeric index content -> parse fails -> None
    assert_eq!(eval_response_expr("res.body.arr[x]", &f), None);
    // unclosed bracket: no `]` found, loop never runs, returns the array value.
    assert!(eval_response_expr("res.body.arr[0", &f).is_some());
}

// ---------------------------------------------------------------------------
// model.rs — Block / Entry / Key / Value / BruFile / Annotation methods
// ---------------------------------------------------------------------------

#[test]
fn model_key_name() {
    assert_eq!(Key::Bare("k".into()).name(), "k");
    assert_eq!(Key::Quoted("with space".into()).name(), "with space");
}

#[test]
fn model_value_as_inline() {
    assert_eq!(Value::Inline("v".into()).as_inline(), "v");
    assert_eq!(Value::Inline(String::new()).as_inline(), "");
    // non-inline variants return ""
    assert_eq!(Value::List(vec!["a".into()]).as_inline(), "");
    assert_eq!(
        Value::Multiline {
            text: "t".into(),
            content_type: None
        }
        .as_inline(),
        ""
    );
}

#[test]
fn model_brufile_default_and_block_lookup() {
    let empty = BruFile::default();
    assert!(empty.blocks.is_empty());
    assert_eq!(empty.block("meta"), None);
    assert_eq!(empty.request_name(), None);
    assert_eq!(empty.seq(), None);
    assert_eq!(empty.request_method(), None);
    assert_eq!(empty.dict_value("meta", "name"), None);
    // an empty BruFile is not a request
    assert!(empty.to_request().is_none());
}

#[test]
fn model_brufile_block_and_dict_value() {
    let src = "meta {\n  name: Hello\n  seq: 3\n}\n";
    let f: BruFile = bru_lang::parse(src).unwrap();
    assert!(f.block("meta").is_some());
    assert_eq!(f.block("absent"), None);
    assert_eq!(f.dict_value("meta", "name"), Some("Hello"));
    assert_eq!(f.dict_value("meta", "absent"), None);
    // dict_value on a non-existent block returns None (the `?` early-out)
    assert_eq!(f.dict_value("absent", "name"), None);
    assert_eq!(f.request_name(), Some("Hello"));
    assert_eq!(f.seq(), Some(3));
}

#[test]
fn model_dict_value_on_non_dict_block_returns_none() {
    // body:json is a Text block, not a Dict, so dict_value hits the `_ => None` arm.
    let src = "body:json {\n  {\n    \"a\": 1\n  }\n}\n";
    let f: BruFile = bru_lang::parse(src).unwrap();
    assert!(matches!(
        f.block("body:json").unwrap().content,
        BlockContent::Text(_)
    ));
    assert_eq!(f.dict_value("body:json", "a"), None);
}

#[test]
fn model_seq_unparseable_returns_none() {
    let src = "meta {\n  name: X\n  seq: notanumber\n}\n";
    let f: BruFile = bru_lang::parse(src).unwrap();
    assert_eq!(f.seq(), None);
}

#[test]
fn model_request_method_verb_and_uppercase() {
    let src = "get {\n  url: https://x\n}\n";
    let f: BruFile = bru_lang::parse(src).unwrap();
    assert_eq!(f.request_method(), Some("GET".to_string()));
}

#[test]
fn model_request_method_http_block_with_method() {
    let src = "http {\n  method: report\n  url: https://x\n}\n";
    let f: BruFile = bru_lang::parse(src).unwrap();
    // custom http block with explicit method -> uppercased that value
    assert_eq!(f.request_method(), Some("REPORT".to_string()));
}

#[test]
fn model_request_method_http_block_without_method() {
    let src = "http {\n  url: https://x\n}\n";
    let f: BruFile = bru_lang::parse(src).unwrap();
    // http block but no method key -> falls back to "HTTP"
    assert_eq!(f.request_method(), Some("HTTP".to_string()));
}

#[test]
fn model_request_method_none_when_no_method_block() {
    let src = "meta {\n  name: X\n}\n";
    let f: BruFile = bru_lang::parse(src).unwrap();
    assert_eq!(f.request_method(), None);
}

#[test]
fn model_http_verbs_constant() {
    assert!(HTTP_VERBS.contains(&"get"));
    assert!(HTTP_VERBS.contains(&"connect"));
    assert!(!HTTP_VERBS.contains(&"report"));
    assert_eq!(HTTP_VERBS.len(), 9);
}

#[test]
fn model_types_clone_eq_debug() {
    // exercise the derived Clone/PartialEq/Debug on the model types.
    let blk = bru_core::Block {
        name: "meta".into(),
        content: BlockContent::List(vec!["a".into()]),
    };
    let blk2 = blk.clone();
    assert_eq!(blk, blk2);
    assert!(format!("{blk:?}").contains("meta"));

    let ann = bru_core::Annotation {
        name: "contentType".into(),
        value: Some("application/json".into()),
    };
    let ann2 = ann.clone();
    assert_eq!(ann, ann2);
    assert!(format!("{ann:?}").contains("contentType"));
    let bare = bru_core::Annotation {
        name: "secret".into(),
        value: None,
    };
    assert_ne!(ann, bare);

    let entry = bru_core::Entry {
        annotations: vec![ann.clone()],
        disabled: true,
        local: true,
        key: Key::Bare("k".into()),
        value: Value::Inline("v".into()),
    };
    let entry2 = entry.clone();
    assert_eq!(entry, entry2);
    assert!(entry.disabled && entry.local);

    let f = BruFile { blocks: vec![blk] };
    let f2 = f.clone();
    assert_eq!(f, f2);
}

#[test]
fn model_blockcontent_variants_eq() {
    let d = BlockContent::Dict(vec![]);
    let t = BlockContent::Text("x".into());
    let l = BlockContent::List(vec!["a".into()]);
    assert_ne!(d, t);
    assert_ne!(t, l);
    assert_eq!(t.clone(), t);
}

// ---------------------------------------------------------------------------
// request.rs — to_request projection across every Auth/Body/settings/param path
// ---------------------------------------------------------------------------

#[test]
fn request_full_projection_query_path_headers_vars_assert() {
    let src = "\
meta {
  name: Full
  type: http
}

post {
  url: https://api.test/{id}
  body: json
  auth: none
}

params:query {
  q: search
  ~off: x
}

params:path {
  id: 42
}

headers {
  x-a: 1
  ~x-b: 2
}

vars:pre-request {
  pre: 1
  @localpre: 2
}

vars:post-response {
  post: 3
  ~offvar: 4
}

assert {
  res.status: eq 200
  ~res.body.x: eq 1
}

body:json {
  {
    \"k\": 1
  }
}
";
    let f: BruFile = bru_lang::parse(src).unwrap();
    let req = f.to_request().expect("request");
    assert_eq!(req.method, "POST");
    assert_eq!(req.url, "https://api.test/{id}");

    assert_eq!(req.query.len(), 2);
    assert_eq!(req.query[0], KeyVal::new("q", "search"));
    assert!(req.query[0].enabled);
    assert!(!req.query[1].enabled);

    assert_eq!(req.path_params.len(), 1);
    assert_eq!(req.path_params[0].name, "id");
    assert_eq!(req.path_params[0].value, "42");

    assert_eq!(req.headers.len(), 2);
    assert!(req.headers[0].enabled);
    assert!(!req.headers[1].enabled);

    assert_eq!(req.vars_pre.len(), 2);
    assert!(req.vars_pre[0].enabled && !req.vars_pre[0].local);
    assert!(req.vars_pre[1].local);

    assert_eq!(req.vars_post.len(), 2);
    assert!(req.vars_post[0].enabled);
    assert!(!req.vars_post[1].enabled);

    assert_eq!(req.assertions.len(), 2);
    assert_eq!(req.assertions[0].expr, "res.status");
    assert_eq!(req.assertions[0].value, "eq 200");
    assert!(req.assertions[0].enabled);
    assert!(!req.assertions[1].enabled);

    assert_eq!(req.body, Body::Json("{\n  \"k\": 1\n}".to_string()));
    assert_eq!(req.auth, Auth::None);
}

#[test]
fn request_missing_optional_blocks_yield_empty_vectors() {
    // only a method block — every key_vals/vars/assertions hits the `_ => Vec::new()` arm.
    let src = "get {\n  url: https://x\n}\n";
    let f: BruFile = bru_lang::parse(src).unwrap();
    let req = f.to_request().unwrap();
    assert!(req.query.is_empty());
    assert!(req.path_params.is_empty());
    assert!(req.headers.is_empty());
    assert!(req.vars_pre.is_empty());
    assert!(req.vars_post.is_empty());
    assert!(req.assertions.is_empty());
    assert_eq!(req.body, Body::None);
    assert_eq!(req.auth, Auth::None);
    assert_eq!(req.url, "https://x");
}

#[test]
fn request_url_defaults_to_empty_when_absent() {
    // method block with no url key -> url default "".
    let src = "get {\n  body: none\n}\n";
    let f: BruFile = bru_lang::parse(src).unwrap();
    let req = f.to_request().unwrap();
    assert_eq!(req.url, "");
}

fn project(src: &str) -> Request {
    let f: BruFile = bru_lang::parse(src).unwrap();
    f.to_request().expect("request")
}

#[test]
fn request_auth_basic() {
    let req = project(
        "get {\n  url: u\n  auth: basic\n}\n\nauth:basic {\n  username: alice\n  password: pw\n}\n",
    );
    assert_eq!(
        req.auth,
        Auth::Basic {
            username: "alice".into(),
            password: "pw".into()
        }
    );
}

#[test]
fn request_auth_basic_missing_fields_default_empty() {
    // auth: basic but no auth:basic block -> empty strings via unwrap_or("").
    let req = project("get {\n  url: u\n  auth: basic\n}\n");
    assert_eq!(
        req.auth,
        Auth::Basic {
            username: String::new(),
            password: String::new()
        }
    );
}

#[test]
fn request_auth_bearer_and_inherit_and_none_and_unknown() {
    let bearer = project("get {\n  url: u\n  auth: bearer\n}\n\nauth:bearer {\n  token: t\n}\n");
    assert_eq!(bearer.auth, Auth::Bearer { token: "t".into() });

    let inherit = project("get {\n  url: u\n  auth: inherit\n}\n");
    assert_eq!(inherit.auth, Auth::Inherit);

    let none = project("get {\n  url: u\n  auth: none\n}\n");
    assert_eq!(none.auth, Auth::None);

    // an unrecognized auth mode falls through to Auth::None.
    let weird = project("get {\n  url: u\n  auth: digest-md5-weird\n}\n");
    assert_eq!(weird.auth, Auth::None);
}

#[test]
fn request_auth_apikey_header_and_query() {
    let header = project(
        "get {\n  url: u\n  auth: apikey\n}\n\nauth:apikey {\n  key: X-Key\n  value: secret\n}\n",
    );
    assert_eq!(
        header.auth,
        Auth::ApiKey {
            key: "X-Key".into(),
            value: "secret".into(),
            placement: ApiKeyPlacement::Header,
        }
    );

    let query = project(
        "get {\n  url: u\n  auth: apikey\n}\n\nauth:apikey {\n  key: k\n  value: v\n  placement: queryparams\n}\n",
    );
    assert_eq!(
        query.auth,
        Auth::ApiKey {
            key: "k".into(),
            value: "v".into(),
            placement: ApiKeyPlacement::Query,
        }
    );
    // ApiKeyPlacement is Copy/Eq/Debug
    let p = ApiKeyPlacement::Query;
    let p2 = p;
    assert_eq!(p, p2);
    assert_ne!(ApiKeyPlacement::Header, ApiKeyPlacement::Query);
}

#[test]
fn request_auth_oauth2_full_and_defaults() {
    let full = project(
        "get {\n  url: u\n  auth: oauth2\n}\n\nauth:oauth2 {\n  grant_type: password\n  access_token_url: https://t\n  client_id: cid\n  client_secret: csec\n  scope: read\n  username: bob\n  password: pw\n  credentials_placement: basic_auth_header\n  token_placement: query\n  token_header_prefix: Bearer\n  token_query_key: tok\n}\n",
    );
    let Auth::OAuth2(o) = full.auth else {
        panic!("oauth2")
    };
    assert_eq!(o.grant_type, "password");
    assert_eq!(o.access_token_url, "https://t");
    assert_eq!(o.client_id, "cid");
    assert_eq!(o.client_secret, "csec");
    assert_eq!(o.scope, "read");
    assert_eq!(o.username, "bob");
    assert_eq!(o.password, "pw");
    assert_eq!(o.credentials_placement, "basic_auth_header");
    assert_eq!(o.token_placement, "query");
    assert_eq!(o.token_header_prefix, "Bearer");
    assert_eq!(o.token_query_key, "tok");

    // minimal oauth2 block -> the with_default fallbacks kick in.
    let min = project("get {\n  url: u\n  auth: oauth2\n}\n\nauth:oauth2 {\n  grant_type: client_credentials\n}\n");
    let Auth::OAuth2(o2) = min.auth else {
        panic!("oauth2")
    };
    assert_eq!(o2.credentials_placement, "body");
    assert_eq!(o2.token_placement, "header");
    assert_eq!(o2.token_query_key, "access_token");
    assert_eq!(o2.token_header_prefix, "");
}

#[test]
fn request_auth_digest_and_awsv4() {
    let dig = project(
        "get {\n  url: u\n  auth: digest\n}\n\nauth:digest {\n  username: u1\n  password: p1\n}\n",
    );
    assert_eq!(
        dig.auth,
        Auth::Digest {
            username: "u1".into(),
            password: "p1".into()
        }
    );

    let aws = project(
        "get {\n  url: u\n  auth: awsv4\n}\n\nauth:awsv4 {\n  accessKeyId: ak\n  secretAccessKey: sk\n  sessionToken: st\n  service: s3\n  region: us-east-1\n  profileName: prof\n}\n",
    );
    assert_eq!(
        aws.auth,
        Auth::AwsV4 {
            access_key_id: "ak".into(),
            secret_access_key: "sk".into(),
            session_token: "st".into(),
            service: "s3".into(),
            region: "us-east-1".into(),
            profile_name: "prof".into(),
        }
    );
}

#[test]
fn request_project_auth_public_api() {
    // project_auth is public — callable directly with any mode.
    let src = "get {\n  url: u\n}\n\nauth:bearer {\n  token: zz\n}\n";
    let f: BruFile = bru_lang::parse(src).unwrap();
    assert_eq!(
        f.project_auth("bearer"),
        Auth::Bearer { token: "zz".into() }
    );
    assert_eq!(f.project_auth("none"), Auth::None);
    assert_eq!(f.project_auth("inherit"), Auth::Inherit);
}

#[test]
fn request_body_text_xml_sparql() {
    let text = project("get {\n  url: u\n  body: text\n}\n\nbody:text {\n  hello world\n}\n");
    assert_eq!(text.body, Body::Text("hello world".to_string()));

    let xml = project("get {\n  url: u\n  body: xml\n}\n\nbody:xml {\n  <a>1</a>\n}\n");
    assert_eq!(xml.body, Body::Xml("<a>1</a>".to_string()));

    let sparql = project("get {\n  url: u\n  body: sparql\n}\n\nbody:sparql {\n  SELECT *\n}\n");
    assert_eq!(sparql.body, Body::Sparql("SELECT *".to_string()));
}

#[test]
fn request_body_text_missing_block_defaults_empty() {
    // body: json but no body:json block -> text() returns default "".
    let req = project("get {\n  url: u\n  body: json\n}\n");
    assert_eq!(req.body, Body::Json(String::new()));
}

#[test]
fn request_body_form_urlencoded() {
    let req = project(
        "get {\n  url: u\n  body: formUrlEncoded\n}\n\nbody:form-urlencoded {\n  a: 1\n  ~b: 2\n}\n",
    );
    let Body::FormUrlEncoded(kvs) = req.body else {
        panic!("form")
    };
    assert_eq!(kvs.len(), 2);
    assert_eq!(kvs[0], KeyVal::new("a", "1"));
    assert!(kvs[0].enabled);
    assert!(!kvs[1].enabled);
}

#[test]
fn request_body_graphql() {
    let req = project(
        "get {\n  url: u\n  body: graphql\n}\n\nbody:graphql {\n  query { me }\n}\n\nbody:graphql:vars {\n  {\n    \"x\": 1\n  }\n}\n",
    );
    let Body::GraphQl { query, variables } = req.body else {
        panic!("graphql")
    };
    assert_eq!(query, "query { me }");
    assert_eq!(variables, "{\n  \"x\": 1\n}");
}

#[test]
fn request_body_multipart_text_and_file() {
    let req = project(
        "get {\n  url: u\n  body: multipartForm\n}\n\nbody:multipart-form {\n  field1: plain text\n  upload: @file(/tmp/a.bin) @contentType(application/octet-stream)\n  ~off: nope\n}\n",
    );
    let Body::MultipartForm(fields) = req.body else {
        panic!("multipart")
    };
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].name, "field1");
    assert_eq!(
        fields[0].value,
        MultipartValue::Text("plain text".to_string())
    );
    assert_eq!(fields[0].content_type, None);
    assert!(fields[0].enabled);

    assert_eq!(fields[1].name, "upload");
    assert_eq!(
        fields[1].value,
        MultipartValue::File("/tmp/a.bin".to_string())
    );
    assert_eq!(
        fields[1].content_type,
        Some("application/octet-stream".to_string())
    );

    // disabled field
    assert!(!fields[2].enabled);
    assert_eq!(fields[2].value, MultipartValue::Text("nope".to_string()));
}

#[test]
fn request_body_file_selected_and_unselected() {
    let req = project(
        "get {\n  url: u\n  body: file\n}\n\nbody:file {\n  file: @file(/tmp/a.pdf) @contentType(application/pdf)\n  ~file: @file(/tmp/b.pdf)\n}\n",
    );
    let Body::File(items) = req.body else {
        panic!("file body")
    };
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].path, "/tmp/a.pdf");
    assert_eq!(items[0].content_type, Some("application/pdf".to_string()));
    assert!(items[0].selected);
    assert_eq!(items[1].path, "/tmp/b.pdf");
    assert_eq!(items[1].content_type, None);
    assert!(!items[1].selected);
}

#[test]
fn request_body_none_explicit_and_unknown_mode() {
    let none = project("get {\n  url: u\n  body: none\n}\n");
    assert_eq!(none.body, Body::None);
    // an unrecognized body mode falls through to Body::None.
    let weird = project("get {\n  url: u\n  body: somethingelse\n}\n");
    assert_eq!(weird.body, Body::None);
}

#[test]
fn request_body_missing_optional_collections_empty() {
    // formUrlEncoded with no block -> empty Vec; multipart/file likewise.
    let form = project("get {\n  url: u\n  body: formUrlEncoded\n}\n");
    assert_eq!(form.body, Body::FormUrlEncoded(vec![]));
    let multi = project("get {\n  url: u\n  body: multipartForm\n}\n");
    assert_eq!(multi.body, Body::MultipartForm(vec![]));
    let file = project("get {\n  url: u\n  body: file\n}\n");
    assert_eq!(file.body, Body::File(vec![]));
}

#[test]
fn request_settings_all_present() {
    let req = project(
        "get {\n  url: u\n}\n\nsettings {\n  followRedirects: true\n  maxRedirects: 5\n  timeout: 3000\n  encodeUrl: false\n}\n",
    );
    assert_eq!(
        req.settings,
        RequestSettings {
            follow_redirects: Some(true),
            max_redirects: Some(5),
            timeout_ms: Some(3000),
            encode_url: Some(false),
        }
    );
}

#[test]
fn request_settings_absent_and_invalid_stay_none() {
    // no settings block at all -> all None.
    let none = project("get {\n  url: u\n}\n");
    assert_eq!(none.settings, RequestSettings::default());

    // invalid values: non-bool followRedirects, non-numeric maxRedirects/timeout.
    let bad = project(
        "get {\n  url: u\n}\n\nsettings {\n  followRedirects: maybe\n  maxRedirects: lots\n  timeout: soon\n  encodeUrl: nope\n}\n",
    );
    assert_eq!(bad.settings, RequestSettings::default());
}

#[test]
fn request_settings_disabled_entries_ignored() {
    // a disabled settings entry is skipped (find filters on !disabled).
    let req =
        project("get {\n  url: u\n}\n\nsettings {\n  ~followRedirects: true\n  timeout: 100\n}\n");
    assert_eq!(req.settings.follow_redirects, None);
    assert_eq!(req.settings.timeout_ms, Some(100));
}

#[test]
fn request_method_via_http_block() {
    // a custom http block drives method_block() through its `b.name == "http"` arm.
    let req = project("http {\n  method: purge\n  url: https://x\n}\n");
    assert_eq!(req.method, "PURGE");
    assert_eq!(req.url, "https://x");
}

#[test]
fn request_script_and_tests_blocks() {
    let src = "\
get {
  url: u
}

script:pre-request {
  console.log('pre')
}

script:post-response {
  console.log('post')
}

tests {
  test('ok', () => {})
}
";
    let f: BruFile = bru_lang::parse(src).unwrap();
    assert_eq!(f.script_pre().as_deref(), Some("console.log('pre')"));
    assert_eq!(f.script_post().as_deref(), Some("console.log('post')"));
    assert_eq!(f.tests_script().as_deref(), Some("test('ok', () => {})"));

    // absent blocks -> None (text_block returns None via the `?`).
    let bare: BruFile = bru_lang::parse("get {\n  url: u\n}\n").unwrap();
    assert_eq!(bare.script_pre(), None);
    assert_eq!(bare.script_post(), None);
    assert_eq!(bare.tests_script(), None);
}

#[test]
fn request_types_clone_eq_debug_default() {
    // exercise derives across the request types.
    let kv = KeyVal::new("a", "b");
    assert_eq!(kv.clone(), kv);
    assert!(format!("{kv:?}").contains("a"));

    let var = Var {
        name: "n".into(),
        value: "v".into(),
        enabled: true,
        local: false,
    };
    assert_eq!(var.clone(), var);

    let mpf = MultipartField {
        name: "f".into(),
        value: MultipartValue::File("p".into()),
        content_type: None,
        enabled: true,
    };
    assert_eq!(mpf.clone(), mpf);
    assert_ne!(
        MultipartValue::Text("p".into()),
        MultipartValue::File("p".into())
    );

    let fbi = FileBodyItem {
        path: "p".into(),
        content_type: Some("ct".into()),
        selected: true,
    };
    assert_eq!(fbi.clone(), fbi);

    let oa = OAuth2::default();
    assert_eq!(oa.clone(), oa);
    assert_eq!(oa.grant_type, "");

    assert_eq!(Body::default(), Body::None);
    assert_eq!(Auth::default(), Auth::None);
    assert_eq!(Request::default(), Request::default());
    let req = Request::default();
    assert!(req.method.is_empty() && req.url.is_empty());
    assert!(format!("{req:?}").contains("Request"));
}
