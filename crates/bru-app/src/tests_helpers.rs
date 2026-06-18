//! Coverage tests for the pure helper functions in `main.rs`.
#![allow(unused_imports, clippy::field_reassign_with_default)]
use super::*;

use bru_core::{
    ApiKeyPlacement, Assertion, Auth, Body, FileBodyItem, KeyVal, MultipartField, MultipartValue,
    OAuth2, Request, Var,
};
use bru_engine::RunOutcome;
use bru_http::HttpResponse;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// ── temp-dir helper (Drop-guard cleanup) ─────────────────────────────────────
struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> Self {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "bru-helpers-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn parse(s: &str) -> BruFile {
    bru_lang::parse(s).unwrap()
}

const SRC: &str = "meta {\n  name: X\n  type: http\n}\n\nget {\n  url: https://a.test\n  body: none\n  auth: none\n}\n";

fn req(method: &str, url: &str) -> Request {
    Request {
        method: method.to_string(),
        url: url.to_string(),
        ..Default::default()
    }
}

fn resp(status: u16, ct: &str, body: &[u8]) -> HttpResponse {
    HttpResponse {
        status,
        status_text: "OK".to_string(),
        headers: vec![("content-type".to_string(), ct.to_string())],
        body: body.to_vec(),
        duration_ms: 7,
    }
}

// ── human_size: every boundary ───────────────────────────────────────────────
#[test]
fn human_size_buckets() {
    assert_eq!(human_size(0), "0 B");
    assert_eq!(human_size(512), "512 B");
    assert_eq!(human_size(1023), "1023 B");
    assert_eq!(human_size(1024), "1.0 KB");
    assert_eq!(human_size(1536), "1.5 KB");
    assert_eq!(human_size(1024 * 1024 - 1), "1024.0 KB");
    assert_eq!(human_size(1024 * 1024), "1.00 MB");
    assert_eq!(human_size(3 * 1024 * 1024), "3.00 MB");
}

// ── short_method: each verb branch ───────────────────────────────────────────
#[test]
fn short_method_all() {
    assert_eq!(short_method("delete"), "DEL");
    assert_eq!(short_method("OPTIONS"), "OPT");
    assert_eq!(short_method(""), "?");
    assert_eq!(short_method("get"), "GET");
    assert_eq!(short_method("post"), "POST");
    // >4 chars is truncated to 4.
    assert_eq!(short_method("connect"), "CONN");
    assert_eq!(short_method("PATCH"), "PATC");
}

// ── hex_dump: empty, short, and truncation path ──────────────────────────────
#[test]
fn hex_dump_basic_and_ascii() {
    assert_eq!(hex_dump(&[]), "");
    let d = hex_dump(b"AB\x00\xff");
    assert!(d.starts_with("00000000  "));
    // Printable -> char, non-printable -> '.'.
    assert!(d.contains("AB.."));
    assert!(d.contains("41 42 00 ff"));
}

#[test]
fn hex_dump_truncates_huge() {
    // 4096 chunks * 16 bytes = exactly the cap; one more byte triggers truncation.
    let big = vec![0u8; 4096 * 16 + 1];
    let d = hex_dump(&big);
    assert!(d.contains("... (truncated)"));
}

// ── pretty_body: json vs non-json ────────────────────────────────────────────
#[test]
fn pretty_body_json_and_text() {
    let j = resp(200, "application/json", b"{\"a\":1}");
    let p = pretty_body(&j);
    assert!(p.contains("\"a\": 1")); // pretty-printed
    let t = resp(200, "text/plain", b"not json {");
    assert_eq!(pretty_body(&t), "not json {");
}

// ── summarize: error / response / no-response ────────────────────────────────
#[test]
fn summarize_error() {
    let mut o = RunOutcome::default();
    o.error = Some("boom".to_string());
    assert_eq!(summarize(&o), "Error: boom");
}

#[test]
fn summarize_no_response() {
    let o = RunOutcome::default();
    assert_eq!(summarize(&o), "No response");
}

#[test]
fn summarize_with_response_and_checks() {
    let mut o = RunOutcome::default();
    o.response = Some(resp(200, "application/json", b"{}"));
    o.assertions = vec![bru_core::AssertOutcome {
        expr: "res.status".to_string(),
        operator: "eq".to_string(),
        expected: "200".to_string(),
        actual: "200".to_string(),
        passed: true,
    }];
    let s = summarize(&o);
    assert!(s.starts_with("200 OK"));
    assert!(s.contains("1/1 checks"));
}

// ── content_type_contains / is_image / is_html / resp_syntax ─────────────────
#[test]
fn content_type_helpers() {
    let img = vec![("Content-Type".to_string(), "image/png".to_string())];
    assert!(content_type_contains(&img, "image/"));
    assert!(is_image_response(&img));
    assert!(!is_html_response(&img));
    assert_eq!(resp_syntax(&img), "txt");

    let html = vec![(
        "content-type".to_string(),
        "text/html; charset=utf-8".to_string(),
    )];
    assert!(is_html_response(&html));
    assert_eq!(resp_syntax(&html), "html");

    let none: Vec<(String, String)> = vec![];
    assert!(!content_type_contains(&none, "json"));
    assert!(!is_image_response(&none));
}

#[test]
fn resp_syntax_all_branches() {
    let mk = |ct: &str| vec![("content-type".to_string(), ct.to_string())];
    assert_eq!(resp_syntax(&mk("application/json")), "json");
    assert_eq!(resp_syntax(&mk("text/html")), "html");
    assert_eq!(resp_syntax(&mk("application/xml")), "xml");
    assert_eq!(resp_syntax(&mk("application/javascript")), "js");
    assert_eq!(resp_syntax(&mk("text/css")), "css");
    assert_eq!(resp_syntax(&mk("text/plain")), "txt");
}

// ── distinct_vars ────────────────────────────────────────────────────────────
#[test]
fn distinct_vars_dedups_and_orders() {
    assert_eq!(
        distinct_vars("{{a}}/{{b}}/{{a}}"),
        vec!["a".to_string(), "b".to_string()]
    );
    // Whitespace trimmed; empty tokens skipped.
    assert_eq!(distinct_vars("{{ x }}{{}}"), vec!["x".to_string()]);
    // Unbalanced: stops at the dangling `{{`.
    assert_eq!(distinct_vars("{{a}} and {{ broken"), vec!["a".to_string()]);
    assert!(distinct_vars("no vars here").is_empty());
}

// ── var_preview ──────────────────────────────────────────────────────────────
#[test]
fn var_preview_none_when_no_braces() {
    assert!(var_preview("plain text", |_| None).is_none());
}

#[test]
fn var_preview_returns_element_when_token_present() {
    // Leading + trailing literals, resolved + unresolved pills.
    let e = var_preview("pre {{host}} mid {{missing}} post", |n| {
        if n == "host" {
            Some("example.com".to_string())
        } else {
            None
        }
    });
    assert!(e.is_some());
    drop(e);
}

#[test]
fn var_preview_unbalanced_returns_none() {
    // `{{` present but never closed -> no resolved token -> None.
    assert!(var_preview("dangling {{ never closes", |_| None).is_none());
}

// ── var_pill (constructs an Element; both color branches) ─────────────────────
#[test]
fn var_pill_builds() {
    drop(var_pill("name", Some("v".to_string())));
    drop(var_pill("name", None));
}

// ── body_syntax ──────────────────────────────────────────────────────────────
#[test]
fn body_syntax_all() {
    assert_eq!(body_syntax("body:json"), "json");
    assert_eq!(body_syntax("body:xml"), "xml");
    assert_eq!(body_syntax("body:sparql"), "sql");
    assert_eq!(body_syntax("body:text"), "txt");
    assert_eq!(body_syntax("whatever"), "txt");
}

// ── body_block_name / body_mode_value (every Body variant) ───────────────────
#[test]
fn body_block_name_variants() {
    assert_eq!(body_block_name(&Body::Json(String::new())), "body:json");
    assert_eq!(body_block_name(&Body::Text(String::new())), "body:text");
    assert_eq!(body_block_name(&Body::Xml(String::new())), "body:xml");
    assert_eq!(body_block_name(&Body::Sparql(String::new())), "body:sparql");
    assert_eq!(body_block_name(&Body::None), "");
    assert_eq!(body_block_name(&Body::File(vec![])), "");
}

#[test]
fn body_mode_value_variants() {
    assert_eq!(body_mode_value(&Body::None), "none");
    assert_eq!(body_mode_value(&Body::Json(String::new())), "json");
    assert_eq!(body_mode_value(&Body::Text(String::new())), "text");
    assert_eq!(body_mode_value(&Body::Xml(String::new())), "xml");
    assert_eq!(body_mode_value(&Body::Sparql(String::new())), "sparql");
    assert_eq!(
        body_mode_value(&Body::FormUrlEncoded(vec![])),
        "formUrlEncoded"
    );
    assert_eq!(
        body_mode_value(&Body::MultipartForm(vec![])),
        "multipartForm"
    );
    assert_eq!(
        body_mode_value(&Body::GraphQl {
            query: String::new(),
            variables: String::new()
        }),
        "graphql"
    );
    assert_eq!(body_mode_value(&Body::File(vec![])), "file");
}

// ── auth_mode_value (every Auth variant) ─────────────────────────────────────
#[test]
fn auth_mode_value_variants() {
    assert_eq!(auth_mode_value(&Auth::None), "none");
    assert_eq!(auth_mode_value(&Auth::Inherit), "inherit");
    assert_eq!(
        auth_mode_value(&Auth::Basic {
            username: String::new(),
            password: String::new()
        }),
        "basic"
    );
    assert_eq!(
        auth_mode_value(&Auth::Bearer {
            token: String::new()
        }),
        "bearer"
    );
    assert_eq!(
        auth_mode_value(&Auth::ApiKey {
            key: String::new(),
            value: String::new(),
            placement: ApiKeyPlacement::Header
        }),
        "apikey"
    );
    assert_eq!(auth_mode_value(&Auth::OAuth2(OAuth2::default())), "oauth2");
    assert_eq!(
        auth_mode_value(&Auth::Digest {
            username: String::new(),
            password: String::new()
        }),
        "digest"
    );
    assert_eq!(
        auth_mode_value(&Auth::AwsV4 {
            access_key_id: String::new(),
            secret_access_key: String::new(),
            session_token: String::new(),
            service: String::new(),
            region: String::new(),
            profile_name: String::new(),
        }),
        "awsv4"
    );
}

// ── auth_view (every Auth variant builds an element) ─────────────────────────
#[test]
fn auth_view_all_variants() {
    drop(auth_view(&Auth::None));
    drop(auth_view(&Auth::Inherit));
    drop(auth_view(&Auth::Basic {
        username: "u".into(),
        password: "p".into(),
    }));
    drop(auth_view(&Auth::Bearer { token: "t".into() }));
    drop(auth_view(&Auth::ApiKey {
        key: "k".into(),
        value: "v".into(),
        placement: ApiKeyPlacement::Header,
    }));
    drop(auth_view(&Auth::ApiKey {
        key: "k".into(),
        value: "v".into(),
        placement: ApiKeyPlacement::Query,
    }));
    drop(auth_view(&Auth::Digest {
        username: "u".into(),
        password: "p".into(),
    }));
    drop(auth_view(&Auth::AwsV4 {
        access_key_id: "a".into(),
        secret_access_key: "s".into(),
        session_token: "t".into(),
        service: "svc".into(),
        region: "r".into(),
        profile_name: "p".into(),
    }));
    // OAuth2 client_credentials (no username/password rows).
    let mut o = OAuth2::default();
    o.grant_type = "client_credentials".into();
    drop(auth_view(&Auth::OAuth2(o)));
    // OAuth2 password grant (extra username/password rows).
    let mut o2 = OAuth2::default();
    o2.grant_type = "password".into();
    drop(auth_view(&Auth::OAuth2(o2)));
}

// ── is_unary_op / split_assert / combine_assert ──────────────────────────────
#[test]
fn unary_op_membership() {
    assert!(is_unary_op("isEmpty"));
    assert!(is_unary_op("isArray"));
    assert!(!is_unary_op("eq"));
    assert!(!is_unary_op("contains"));
}

#[test]
fn split_assert_branches() {
    // Leading known operator + operand.
    assert_eq!(
        split_assert("eq 200"),
        ("eq".to_string(), "200".to_string())
    );
    assert_eq!(split_assert("gt 5"), ("gt".to_string(), "5".to_string()));
    // Lone known operator (unary) keeps identity, empty operand.
    assert_eq!(
        split_assert("isNumber"),
        ("isNumber".to_string(), String::new())
    );
    // Lone known binary operator before an operand is typed.
    assert_eq!(
        split_assert("contains"),
        ("contains".to_string(), String::new())
    );
    // Bare value -> defaults to eq.
    assert_eq!(split_assert("200"), ("eq".to_string(), "200".to_string()));
    // First token unknown -> whole thing is eq operand.
    assert_eq!(
        split_assert("foo bar"),
        ("eq".to_string(), "foo bar".to_string())
    );
}

#[test]
fn combine_assert_branches() {
    assert_eq!(combine_assert("isEmpty", "ignored"), "isEmpty");
    assert_eq!(combine_assert("eq", "200"), "eq 200");
    // Empty operand trims the trailing space.
    assert_eq!(combine_assert("eq", ""), "eq");
}

// ── pairs ────────────────────────────────────────────────────────────────────
#[test]
fn pairs_maps_table() {
    let t = [("a", "Alpha"), ("b", "Beta")];
    let p = pairs(&t);
    assert_eq!(
        p,
        vec![
            ("a".to_string(), "Alpha".to_string()),
            ("b".to_string(), "Beta".to_string())
        ]
    );
    assert!(pairs(&[]).is_empty());
}

// ── file_stem ────────────────────────────────────────────────────────────────
#[test]
fn file_stem_variants() {
    assert_eq!(file_stem(Path::new("/a/b/req.bru")), "req");
    assert_eq!(file_stem(Path::new("plain")), "plain");
    // No usable stem -> fallback.
    assert_eq!(file_stem(Path::new("/")), "request");
}

// ── kv_rows / var_rows_local / assert_rows ───────────────────────────────────
#[test]
fn kv_rows_maps() {
    let items = vec![
        KeyVal {
            name: "a".into(),
            value: "1".into(),
            enabled: true,
        },
        KeyVal {
            name: "b".into(),
            value: "2".into(),
            enabled: false,
        },
    ];
    let rows = kv_rows(&items);
    assert_eq!(
        rows,
        vec![
            ("a".to_string(), "1".to_string(), true),
            ("b".to_string(), "2".to_string(), false),
        ]
    );
    assert!(kv_rows(&[]).is_empty());
}

#[test]
fn var_rows_local_maps() {
    let items = vec![
        Var {
            name: "x".into(),
            value: "1".into(),
            enabled: true,
            local: false,
        },
        Var {
            name: "y".into(),
            value: "2".into(),
            enabled: false,
            local: true,
        },
    ];
    let rows = var_rows_local(&items);
    assert_eq!(
        rows,
        vec![
            ("x".to_string(), "1".to_string(), true, false),
            ("y".to_string(), "2".to_string(), false, true),
        ]
    );
}

#[test]
fn assert_rows_maps() {
    let items = vec![Assertion {
        expr: "res.status".into(),
        value: "eq 200".into(),
        enabled: true,
    }];
    assert_eq!(
        assert_rows(&items),
        vec![("res.status".to_string(), "eq 200".to_string(), true)]
    );
}

// ── block_kv_rows / block_var_rows ───────────────────────────────────────────
#[test]
fn block_kv_rows_reads_dict() {
    let f = parse("headers {\n  X-A: 1\n  ~X-B: 2\n}\n");
    let rows = block_kv_rows(&f, "headers");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0], ("X-A".to_string(), "1".to_string(), true));
    assert!(!rows[1].2); // disabled
                         // Missing block -> empty.
    assert!(block_kv_rows(&f, "nope").is_empty());
}

#[test]
fn block_var_rows_reads_local_flag() {
    let f = parse("vars:pre-request {\n  @tok: secret\n  plain: 1\n}\n");
    let rows = block_var_rows(&f, "vars:pre-request");
    assert_eq!(rows.len(), 2);
    // @tok is local.
    let tok = rows.iter().find(|r| r.0 == "tok").unwrap();
    assert!(tok.3);
    let plain = rows.iter().find(|r| r.0 == "plain").unwrap();
    assert!(!plain.3);
    assert!(block_var_rows(&f, "absent").is_empty());
}

// ── bulk_text / parse_bulk round-trip ────────────────────────────────────────
#[test]
fn bulk_text_serializes() {
    let f = parse("headers {\n  A: 1\n  ~B: 2\n}\n");
    let t = bulk_text(&f, "headers");
    assert!(t.contains("A: 1"));
    assert!(t.contains("~B: 2"));
}

#[test]
fn bulk_text_local_var_prefix() {
    let f = parse("vars:pre-request {\n  @tok: s\n  ~@dis: x\n}\n");
    let t = bulk_text(&f, "vars:pre-request");
    assert!(t.contains("@tok: s"));
    assert!(t.contains("~@dis: x"));
}

#[test]
fn parse_bulk_branches() {
    let text = "A: 1\n~B: 2\n@tok: s\n~@dis: x\n\nno colon line\n  C : 3  ";
    let rows = parse_bulk(text);
    // Blank line and "no colon line" skipped.
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0], ("A".to_string(), "1".to_string(), true, false));
    assert_eq!(rows[1], ("B".to_string(), "2".to_string(), false, false));
    assert_eq!(rows[2], ("tok".to_string(), "s".to_string(), true, true));
    assert_eq!(rows[3], ("dis".to_string(), "x".to_string(), false, true));
    assert_eq!(rows[4], ("C".to_string(), "3".to_string(), true, false));
}

#[test]
fn parse_bulk_empty() {
    assert!(parse_bulk("").is_empty());
    assert!(parse_bulk("\n\n  \n").is_empty());
}

// ── docs_text ────────────────────────────────────────────────────────────────
#[test]
fn docs_text_outdents() {
    let f = parse("docs {\n  line one\n  line two\n}\n");
    let d = docs_text(&f);
    assert_eq!(d, "line one\nline two");
    // No docs block -> empty.
    let f2 = parse(SRC);
    assert_eq!(docs_text(&f2), "");
}

// ── example_count / request_examples / build_example_text ────────────────────
#[test]
fn example_count_and_examples() {
    let f = parse(SRC);
    assert_eq!(example_count(&f), 0);
    assert!(request_examples(&f).is_empty());

    let req = req("GET", "https://a.test");
    let r = resp(200, "application/json", b"{\"a\":1}");
    let mut f2 = parse(SRC);
    edit::push_text_block(&mut f2, "example", build_example_text("Ex1", &req, &r));
    let reparsed = parse(&bru_lang::serialize(&f2));
    assert_eq!(example_count(&reparsed), 1);
    let ex = request_examples(&reparsed);
    assert_eq!(ex.len(), 1);
    assert_eq!(ex[0].0, "Ex1");
}

#[test]
fn build_example_text_text_body_with_headers() {
    // Non-json response -> "text" kind; enabled header included.
    let mut req = req("post", "https://a.test/x");
    req.headers = vec![
        KeyVal {
            name: "X-On".into(),
            value: "1".into(),
            enabled: true,
        },
        KeyVal {
            name: "X-Off".into(),
            value: "2".into(),
            enabled: false,
        },
    ];
    req.body = Body::Text("hi".into());
    let r = resp(404, "text/plain", b"nope");
    let text = build_example_text("My Ex", &req, &r);
    assert!(text.contains("name: My Ex"));
    assert!(text.contains("method: post"));
    assert!(text.contains("mode: text"));
    assert!(text.contains("X-On: 1"));
    assert!(!text.contains("X-Off")); // disabled header dropped
    assert!(text.contains("code: 404"));
    assert!(text.contains("type: text"));
}

#[test]
fn request_examples_named_via_top_level_name() {
    // An example whose nested request: block also has a `name:` must use the
    // top-level one ("Outer"), not the nested field.
    let f = parse(SRC);
    let mut f = f;
    let ex = "  name: Outer\n  request: {\n    name: Inner\n    url: x\n  }";
    edit::push_text_block(&mut f, "example", ex.to_string());
    let reparsed = parse(&bru_lang::serialize(&f));
    let examples = request_examples(&reparsed);
    assert_eq!(examples.len(), 1);
    assert_eq!(examples[0].0, "Outer");
}

#[test]
fn request_examples_falls_back_to_default_name() {
    let mut f = parse(SRC);
    // No `name:` line -> falls back to "example".
    edit::push_text_block(
        &mut f,
        "example",
        "  request: {\n    url: x\n  }".to_string(),
    );
    let reparsed = parse(&bru_lang::serialize(&f));
    let examples = request_examples(&reparsed);
    assert_eq!(examples.len(), 1);
    assert_eq!(examples[0].0, "example");
}

// ── timeline_text ────────────────────────────────────────────────────────────
#[test]
fn timeline_text_with_response() {
    // Build a real Tab to exercise to_request projection inside timeline_text.
    let mut app = App::default();
    let file = parse("meta {\n  name: T\n  type: http\n}\n\nget {\n  url: https://a.test\n  body: none\n  auth: none\n}\n\nheaders {\n  Accept: text/plain\n}\n");
    let saved = bru_lang::serialize(&file);
    let tab = app.blank_tab(None, file, saved);
    let mut o = RunOutcome::default();
    o.response = Some(resp(200, "application/json", b"{}"));
    let t = timeline_text(&tab, &o);
    assert!(t.contains("> GET https://a.test"));
    assert!(t.contains("> Accept: text/plain"));
    assert!(t.contains("< 200 OK"));
    assert!(t.contains("ms,"));
}

#[test]
fn timeline_text_no_response() {
    let mut app = App::default();
    let file = parse(SRC);
    let saved = bru_lang::serialize(&file);
    let tab = app.blank_tab(None, file, saved);
    let o = RunOutcome::default();
    let t = timeline_text(&tab, &o);
    assert!(t.contains("> GET https://a.test"));
    // No `<` response lines.
    assert!(!t.contains('<'));
}

// ── scan_envs ────────────────────────────────────────────────────────────────
#[test]
fn scan_envs_lists_sorted() {
    let d = TempDir::new("envs");
    // No environments dir yet -> empty.
    assert!(scan_envs(&d.0).is_empty());
    let envdir = d.0.join("environments");
    std::fs::create_dir_all(&envdir).unwrap();
    std::fs::write(envdir.join("prod.bru"), "vars {\n}\n").unwrap();
    std::fs::write(envdir.join("dev.bru"), "vars {\n}\n").unwrap();
    std::fs::write(envdir.join("notes.txt"), "ignored").unwrap();
    let envs = scan_envs(&d.0);
    assert_eq!(envs, vec!["dev".to_string(), "prod".to_string()]); // sorted, .txt ignored
}

// ── folder tree helpers ──────────────────────────────────────────────────────
fn folder(name: &str, path: &str) -> Folder {
    Folder {
        name: name.into(),
        path: PathBuf::from(path),
        folders: vec![],
        requests: vec![],
    }
}

fn request_item(name: &str, path: &str) -> bru_core::RequestItem {
    bru_core::RequestItem {
        name: name.into(),
        path: PathBuf::from(path),
        method: Some("GET".into()),
        seq: Some(1),
    }
}

fn sample_tree() -> Folder {
    let mut root = folder("root", "/c");
    let mut sub = folder("Sub", "/c/sub");
    sub.requests.push(request_item("R2", "/c/sub/r2.bru"));
    root.folders.push(sub);
    root.requests.push(request_item("R1", "/c/r1.bru"));
    root
}

#[test]
fn collect_folder_paths_recurses() {
    let root = sample_tree();
    let mut out = Vec::new();
    collect_folder_paths(&root, &mut out);
    assert_eq!(out, vec![PathBuf::from("/c/sub")]);
}

#[test]
fn collect_request_index_flattens() {
    let root = sample_tree();
    let mut out = Vec::new();
    collect_request_index(&root, &mut out);
    // root request first, then sub-folder requests.
    let names: Vec<&str> = out.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, vec!["R1", "R2"]);
}

#[test]
fn collect_folder_requests_subfolders_first() {
    let root = sample_tree();
    let mut out = Vec::new();
    collect_folder_requests(&root, &mut out);
    // Sub-folder requests come before this level's requests.
    assert_eq!(
        out,
        vec![PathBuf::from("/c/sub/r2.bru"), PathBuf::from("/c/r1.bru")]
    );
}

#[test]
fn find_folder_found_and_missing() {
    let root = sample_tree();
    assert!(find_folder(&root, Path::new("/c/sub")).is_some());
    assert!(find_folder(&root, Path::new("/c/nope")).is_none());
    // Nested find: add a deeper folder.
    let mut root2 = sample_tree();
    let deep = folder("Deep", "/c/sub/deep");
    root2.folders[0].folders.push(deep);
    assert!(find_folder(&root2, Path::new("/c/sub/deep")).is_some());
}

#[test]
fn folder_matches_name_request_descendant() {
    let root = sample_tree();
    // Matches own name.
    assert!(folder_matches(&root, "root"));
    // Matches a request name.
    assert!(folder_matches(&root, "r1"));
    // Matches a descendant folder/request.
    assert!(folder_matches(&root, "r2"));
    assert!(folder_matches(&root, "sub"));
    // No match.
    assert!(!folder_matches(&root, "zzz"));
}

// ── gen_curl: each auth / body / method / query combination ──────────────────
#[test]
fn gen_curl_basic_get() {
    let r = req("GET", "https://a.test/x");
    let c = gen_curl(&r);
    assert!(c.contains("curl -X GET 'https://a.test/x'"));
}

#[test]
fn gen_curl_query_apikey_appends() {
    let mut r = req("GET", "https://a.test/x");
    r.auth = Auth::ApiKey {
        key: "k".into(),
        value: "v".into(),
        placement: ApiKeyPlacement::Query,
    };
    let c = gen_curl(&r);
    assert!(c.contains("k=v"));
    // With existing query -> uses `&`.
    let mut r2 = req("GET", "https://a.test/x?p=1");
    r2.auth = Auth::ApiKey {
        key: "k".into(),
        value: "v".into(),
        placement: ApiKeyPlacement::Query,
    };
    let c2 = gen_curl(&r2);
    assert!(c2.contains("&k=v"));
}

#[test]
fn gen_curl_header_apikey_and_enabled_headers() {
    let mut r = req("GET", "https://a.test");
    r.auth = Auth::ApiKey {
        key: "X-Key".into(),
        value: "secret".into(),
        placement: ApiKeyPlacement::Header,
    };
    r.headers = vec![
        KeyVal {
            name: "X-On".into(),
            value: "1".into(),
            enabled: true,
        },
        KeyVal {
            name: "X-Off".into(),
            value: "2".into(),
            enabled: false,
        },
    ];
    let c = gen_curl(&r);
    assert!(c.contains("-H 'X-On: 1'"));
    assert!(!c.contains("X-Off"));
    assert!(c.contains("-H 'X-Key: secret'"));
}

#[test]
fn gen_curl_basic_and_bearer_auth() {
    let mut r = req("GET", "https://a.test");
    r.auth = Auth::Basic {
        username: "u".into(),
        password: "p".into(),
    };
    assert!(gen_curl(&r).contains("-u 'u:p'"));

    let mut r2 = req("GET", "https://a.test");
    r2.auth = Auth::Bearer {
        token: "tok".into(),
    };
    assert!(gen_curl(&r2).contains("-H 'Authorization: Bearer tok'"));
}

#[test]
fn gen_curl_body_variants() {
    let mut json = req("POST", "https://a.test");
    json.body = Body::Json("{\"a\":1}".into());
    let c = gen_curl(&json);
    assert!(c.contains("-H 'Content-Type: application/json'"));
    assert!(c.contains("-d '{\"a\":1}'"));

    let mut text = req("POST", "https://a.test");
    text.body = Body::Text("hello".into());
    assert!(gen_curl(&text).contains("-H 'Content-Type: text/plain'"));

    let mut xml = req("POST", "https://a.test");
    xml.body = Body::Xml("<x/>".into());
    assert!(gen_curl(&xml).contains("-H 'Content-Type: application/xml'"));

    let mut sparql = req("POST", "https://a.test");
    sparql.body = Body::Sparql("SELECT *".into());
    assert!(gen_curl(&sparql).contains("application/sparql-query"));

    // None body -> no -d.
    let none = req("GET", "https://a.test");
    assert!(!gen_curl(&none).contains("-d"));
}

#[test]
fn gen_curl_graphql_empty_and_populated_vars() {
    let mut g = req("POST", "https://a.test");
    g.body = Body::GraphQl {
        query: "{ me }".into(),
        variables: String::new(),
    };
    let c = gen_curl(&g);
    assert!(c.contains("\"query\":\"{ me }\""));
    assert!(c.contains("\"variables\":{}"));

    // Populated (valid JSON) variables.
    let mut g2 = req("POST", "https://a.test");
    g2.body = Body::GraphQl {
        query: "q".into(),
        variables: "{\"id\":5}".into(),
    };
    assert!(gen_curl(&g2).contains("\"id\":5"));

    // Invalid JSON variables -> falls back to empty object.
    let mut g3 = req("POST", "https://a.test");
    g3.body = Body::GraphQl {
        query: "q".into(),
        variables: "not json".into(),
    };
    assert!(gen_curl(&g3).contains("\"variables\":{}"));
}

#[test]
fn gen_curl_form_and_multipart() {
    let mut form = req("POST", "https://a.test");
    form.body = Body::FormUrlEncoded(vec![
        KeyVal {
            name: "a".into(),
            value: "1".into(),
            enabled: true,
        },
        KeyVal {
            name: "b".into(),
            value: "2".into(),
            enabled: false,
        },
    ]);
    let c = gen_curl(&form);
    assert!(c.contains("--data-urlencode 'a=1'"));
    assert!(!c.contains("b=2"));

    let mut mp = req("POST", "https://a.test");
    mp.body = Body::MultipartForm(vec![
        MultipartField {
            name: "t".into(),
            value: MultipartValue::Text("v".into()),
            content_type: None,
            enabled: true,
        },
        MultipartField {
            name: "f".into(),
            value: MultipartValue::File("/tmp/x".into()),
            content_type: None,
            enabled: true,
        },
    ]);
    let c2 = gen_curl(&mp);
    assert!(c2.contains("-F 't=v'"));
    assert!(c2.contains("-F 'f=@/tmp/x'"));
}

#[test]
fn gen_curl_file_body() {
    let mut r = req("PUT", "https://a.test");
    r.body = Body::File(vec![
        FileBodyItem {
            path: "/tmp/a.bin".into(),
            content_type: Some("application/octet-stream".into()),
            selected: true,
        },
        FileBodyItem {
            path: "/tmp/b.bin".into(),
            content_type: None,
            selected: false,
        },
    ]);
    let c = gen_curl(&r);
    assert!(c.contains("-H 'Content-Type: application/octet-stream'"));
    assert!(c.contains("--data-binary '@/tmp/a.bin'"));

    // No selected -> uses first; empty content_type skips the CT header.
    let mut r2 = req("PUT", "https://a.test");
    r2.body = Body::File(vec![FileBodyItem {
        path: "/tmp/c.bin".into(),
        content_type: Some(String::new()),
        selected: false,
    }]);
    let c2 = gen_curl(&r2);
    assert!(c2.contains("--data-binary '@/tmp/c.bin'"));

    // Empty File list -> no --data-binary.
    let mut r3 = req("PUT", "https://a.test");
    r3.body = Body::File(vec![]);
    assert!(!gen_curl(&r3).contains("--data-binary"));
}

#[test]
fn gen_curl_no_duplicate_content_type() {
    // Explicit content-type header means the default is NOT added.
    let mut r = req("POST", "https://a.test");
    r.headers = vec![KeyVal {
        name: "Content-Type".into(),
        value: "application/custom".into(),
        enabled: true,
    }];
    r.body = Body::Json("{}".into());
    let c = gen_curl(&r);
    assert!(c.contains("application/custom"));
    assert!(!c.contains("application/json"));
}

#[test]
fn gen_curl_single_quote_escaping() {
    let mut r = req("POST", "https://a.test");
    r.body = Body::Text("it's".into());
    let c = gen_curl(&r);
    // Single quotes are escaped with the '\'' idiom.
    assert!(c.contains("'\\''"));
}

// ── new helpers (Wave 1/2) ───────────────────────────────────────────────────

#[test]
fn host_of_strips_scheme_path_port_userinfo() {
    assert_eq!(
        host_of("https://api.github.com/search?q=1"),
        "api.github.com"
    );
    assert_eq!(host_of("http://user:pw@example.com:8080/x"), "example.com");
    assert_eq!(host_of("nohost"), "nohost");
}

#[test]
fn parse_set_cookie_name_value_and_attrs() {
    let c = parse_set_cookie("sid=abc; Path=/api; Domain=.example.com; HttpOnly", "h.com")
        .expect("parses");
    assert_eq!(c.name, "sid");
    assert_eq!(c.value, "abc");
    assert_eq!(c.path, "/api");
    assert_eq!(c.domain, "example.com"); // leading dot stripped
                                         // No Domain attr → falls back to the responding host.
    let d = parse_set_cookie("t=1", "host.test").unwrap();
    assert_eq!(d.domain, "host.test");
    // Garbage (no `=`) is rejected.
    assert!(parse_set_cookie("nonsense", "h").is_none());
}

#[test]
fn upsert_cookie_replaces_same_key() {
    let mut jar = Vec::new();
    upsert_cookie(&mut jar, parse_set_cookie("a=1", "h").unwrap());
    upsert_cookie(&mut jar, parse_set_cookie("a=2", "h").unwrap());
    upsert_cookie(&mut jar, parse_set_cookie("b=9", "h").unwrap());
    assert_eq!(jar.len(), 2);
    assert_eq!(jar.iter().find(|c| c.name == "a").unwrap().value, "2");
}

#[test]
fn json_path_navigates_keys_indexes_wildcards() {
    let v: serde_json::Value =
        serde_json::from_str(r#"{"items":[{"name":"a"},{"name":"b"}],"n":3}"#).unwrap();
    assert_eq!(json_path(&v, "$.n"), Some(serde_json::json!(3)));
    assert_eq!(json_path(&v, "items[0].name"), Some(serde_json::json!("a")));
    assert_eq!(
        json_path(&v, "$.items[*].name"),
        Some(serde_json::json!(["a", "b"]))
    );
    assert_eq!(json_path(&v, "$.missing"), None);
    assert_eq!(json_path(&v, "items[9]"), None);
}

#[test]
fn git_branch_reads_head_ref_and_detached() {
    let td = TempDir::new("git");
    let git = td.0.join(".git");
    std::fs::create_dir_all(&git).unwrap();
    std::fs::write(git.join("HEAD"), "ref: refs/heads/feature/x\n").unwrap();
    assert_eq!(git_branch(&td.0).as_deref(), Some("feature/x"));
    // Detached HEAD → short hash.
    std::fs::write(git.join("HEAD"), "0123456789abcdef\n").unwrap();
    assert_eq!(git_branch(&td.0).as_deref(), Some("0123456"));
    // A subdirectory resolves the ancestor repo.
    let sub = td.0.join("a/b");
    std::fs::create_dir_all(&sub).unwrap();
    assert_eq!(git_branch(&sub).as_deref(), Some("0123456"));
}

#[test]
fn git_branch_none_outside_repo() {
    let td = TempDir::new("nogit");
    assert_eq!(git_branch(&td.0), None);
}
