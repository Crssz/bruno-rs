//! Unit coverage for the semantic layer: interpolation, assertions, projection.

use std::collections::HashMap;

use bru_core::{
    eval_response_expr, evaluate_assertions, interpolate, Assertion, Auth, Body, BruFile,
    ResponseFacts,
};

fn vars(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn interpolation_basics() {
    let v = vars(&[("base", "https://api.test"), ("id", "42")]);
    assert_eq!(
        interpolate("{{base}}/u/{{id}}", &v),
        "https://api.test/u/42"
    );
    // Whitespace inside the braces is trimmed.
    assert_eq!(interpolate("{{ base }}", &v), "https://api.test");
    // Unresolved placeholders are left verbatim.
    assert_eq!(interpolate("{{missing}}!", &v), "{{missing}}!");
    // A lone `{{` without a close is left as-is.
    assert_eq!(interpolate("a {{ b", &v), "a {{ b");
}

#[test]
fn interpolation_dynamic_vars() {
    let v = HashMap::new();
    let ts = interpolate("{{$timestamp}}", &v);
    assert!(ts.chars().all(|c| c.is_ascii_digit()) && !ts.is_empty());
    let guid = interpolate("{{$guid}}", &v);
    assert_eq!(guid.len(), 36);
    assert_eq!(guid.matches('-').count(), 4);
}

fn facts<'a>(
    status: u16,
    headers: &'a [(String, String)],
    body: &'a serde_json::Value,
) -> ResponseFacts<'a> {
    ResponseFacts {
        status,
        headers,
        body_json: Some(body),
        body_text: "",
        response_time_ms: 12,
    }
}

#[test]
fn assertion_operators_and_json_paths() {
    let headers = vec![("content-type".to_string(), "application/json".to_string())];
    let body = serde_json::json!({"user": {"name": "ada"}, "n": 5, "arr": [10, 20], "ok": true});
    let f = facts(200, &headers, &body);

    let a = |expr: &str, value: &str| Assertion {
        expr: expr.to_string(),
        value: value.to_string(),
        enabled: true,
    };
    let checks = vec![
        a("res.status", "200"),
        a("res.status", "eq 200"),
        a("res.body.user.name", "ada"),
        a("res.body.n", "gt 3"),
        a("res.body.n", "lte 5"),
        a("res.body.arr[1]", "20"),
        a("res.body.ok", "isTrue"),
        a("res.body.missing", "isUndefined"),
        a("res.headers.Content-Type", "contains json"),
        a("res.responseTime", "lt 1000"),
    ];
    let out = evaluate_assertions(&checks, &f);
    assert!(out.iter().all(|o| o.passed), "{out:#?}");

    // A failing assertion reports the actual value.
    let fail = evaluate_assertions(&[a("res.body.user.name", "bob")], &f);
    assert!(!fail[0].passed);
    assert_eq!(fail[0].actual, "ada");
}

#[test]
fn response_expr_resolves_nested_and_indexed() {
    let body = serde_json::json!({"data": {"items": [{"id": 7}]}});
    let f = facts(201, &[], &body);
    assert_eq!(
        eval_response_expr("res.body.data.items[0].id", &f).as_deref(),
        Some("7")
    );
    assert_eq!(eval_response_expr("res.status", &f).as_deref(), Some("201"));
    assert_eq!(eval_response_expr("res.body.nope", &f), None);
}

#[test]
fn request_projection_reads_method_url_headers_auth_body() {
    let src = "meta {\n  name: Create\n  type: http\n}\n\n\
        post {\n  url: https://api.test/u\n  body: json\n  auth: bearer\n}\n\n\
        headers {\n  x-trace: abc\n  ~x-off: skip\n}\n\n\
        auth:bearer {\n  token: secret\n}\n\n\
        body:json {\n  {\n    \"a\": 1\n  }\n}\n";
    let file: BruFile = bru_lang::parse(src).unwrap();
    let req = file.to_request().expect("is a request");

    assert_eq!(req.method, "POST");
    assert_eq!(req.url, "https://api.test/u");
    assert_eq!(req.headers.len(), 2);
    assert_eq!(req.headers[0].name, "x-trace");
    assert!(req.headers[0].enabled);
    assert!(!req.headers[1].enabled);
    assert_eq!(
        req.auth,
        Auth::Bearer {
            token: "secret".to_string()
        }
    );
    // Body is outdented back to the real payload.
    assert_eq!(req.body, Body::Json("{\n  \"a\": 1\n}".to_string()));
}
