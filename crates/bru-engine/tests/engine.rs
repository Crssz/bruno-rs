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
