//! End-to-end engine test against a hermetic local mock server (no network).

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use bru_engine::{run_request, RunContext};

/// Spin up a one-shot HTTP/1.1 server that captures the request and replies with
/// `body`. Returns the bound `127.0.0.1:port` address and a handle yielding the
/// raw request bytes the client sent.
fn mock_server(body: &'static str) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).unwrap_or(0);
        let request = String::from_utf8_lossy(&buf[..n]).into_owned();
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
        request
    });
    (format!("http://{addr}"), handle)
}

#[tokio::test]
async fn runs_request_interpolates_asserts_and_captures_var() {
    let (base, server) = mock_server(r#"{"token":"abc123","ok":true,"count":2}"#);

    // `{{base}}` resolves from context; assertions and a post-response capture.
    let src = "meta {\n  name: Login\n  type: http\n}\n\n\
         get {\n  url: {{base}}/login\n  auth: none\n}\n\n\
         assert {\n  res.status: 200\n  res.body.token: abc123\n  res.body.count: gt 1\n  res.body.ok: isTrue\n}\n\n\
         vars:post-response {\n  authToken: res.body.token\n}\n";
    let file = bru_lang::parse(src).expect("parse request");

    let mut ctx = RunContext::default();
    ctx.vars.insert("base".to_string(), base.clone());

    let outcome = run_request(&file, &mut ctx).await;
    let sent = server.join().unwrap();

    assert!(
        sent.starts_with("GET /login "),
        "request line was: {sent:?}"
    );
    assert!(outcome.error.is_none(), "error: {:?}", outcome.error);
    assert_eq!(outcome.response.as_ref().unwrap().status, 200);
    assert!(outcome.passed(), "assertions: {:?}", outcome.assertions);
    assert_eq!(outcome.assertions.len(), 4);
    assert_eq!(
        outcome.vars_set,
        vec![("authToken".to_string(), "abc123".to_string())]
    );
    assert_eq!(
        ctx.vars.get("authToken").map(String::as_str),
        Some("abc123")
    );
}

#[tokio::test]
async fn pre_and_post_scripts_run_and_share_vars() {
    let (base, server) = mock_server(r#"{"id":42,"ok":true}"#);
    let src = "meta {\n  name: S\n  type: http\n}\n\n\
        get {\n  url: {{base}}/{{path}}\n  auth: none\n}\n\n\
        script:pre-request {\n  bru.setVar('path', 'items/' + 42);\n}\n\n\
        script:post-response {\n  bru.setVar('captured', res.body.id);\n}\n\n\
        tests {\n  test('status ok', function(){ expect(res.status).to.equal(200); });\n  test('id is 42', function(){ expect(res.body.id).to.equal(42); });\n}\n";
    let file = bru_lang::parse(src).unwrap();

    let mut ctx = RunContext::default();
    ctx.vars.insert("base".to_string(), base.clone());

    let outcome = run_request(&file, &mut ctx).await;
    let sent = server.join().unwrap();

    assert!(sent.starts_with("GET /items/42 "), "request line: {sent:?}");
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert_eq!(outcome.tests.len(), 2);
    assert!(outcome.passed(), "tests: {:?}", outcome.tests);
    assert_eq!(ctx.vars.get("captured").map(String::as_str), Some("42"));
}

#[tokio::test]
async fn post_script_and_tests_combine_without_asi_breakage() {
    let (base, server) = mock_server(r#"{"id":1}"#);
    // post-response ends WITHOUT a semicolon; tests starts with `(` (an IIFE).
    // Under a bare `\n` join these merge into `setVar(...)(...)` and throw.
    let src = "meta {\n  name: A\n  type: http\n}\n\n\
        get {\n  url: URL\n  auth: none\n}\n\n\
        script:post-response {\n  bru.setVar('a', '1')\n}\n\n\
        tests {\n  (function(){ test('ok', function(){ expect(res.body.id).to.equal(1); }); })()\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/a"))).unwrap();

    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let _ = server.join();

    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert_eq!(outcome.tests.len(), 1, "tests: {:?}", outcome.tests);
    assert!(outcome.passed(), "tests: {:?}", outcome.tests);
    assert_eq!(ctx.vars.get("a").map(String::as_str), Some("1"));
}

#[tokio::test]
async fn content_type_not_duplicated_when_header_present() {
    let (base, server) = mock_server(r#"{"ok":true}"#);
    let src = "meta {\n  name: C\n  type: http\n}\n\n\
        post {\n  url: URL\n  body: json\n  auth: none\n}\n\n\
        headers {\n  content-type: application/json\n}\n\n\
        body:json {\n  {\n    \"a\": 1\n  }\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/c"))).unwrap();

    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let sent = server.join().unwrap();

    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    let count = sent.to_lowercase().matches("content-type:").count();
    assert_eq!(count, 1, "expected exactly one content-type:\n{sent}");
}

#[tokio::test]
async fn failed_assertion_marks_outcome_failed() {
    let (base, server) = mock_server(r#"{"status":"error"}"#);
    let src = "meta {\n  name: X\n  type: http\n}\n\nget {\n  url: URL\n  auth: none\n}\n\nassert {\n  res.status: 404\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/x"))).unwrap();

    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let _ = server.join();

    assert!(!outcome.passed());
    assert_eq!(outcome.assertions[0].actual, "200");
}
