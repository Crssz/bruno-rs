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

/// A two-request mock: first connection answers the OAuth2 token endpoint,
/// second captures the actual API request (to inspect its Authorization header).
fn oauth_mock(token: &'static str) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let respond = |stream: &mut std::net::TcpStream, body: String| {
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
        };
        // 1) token endpoint
        let (mut s1, _) = listener.accept().unwrap();
        let _ = s1.read(&mut [0u8; 4096]);
        respond(
            &mut s1,
            format!("{{\"access_token\":\"{token}\",\"token_type\":\"Bearer\"}}"),
        );
        // 2) actual API request
        let (mut s2, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let n = s2.read(&mut buf).unwrap_or(0);
        let api_req = String::from_utf8_lossy(&buf[..n]).into_owned();
        respond(&mut s2, "{\"ok\":true}".to_string());
        api_req
    });
    (format!("http://{addr}"), handle)
}

#[tokio::test]
async fn oauth2_client_credentials_fetches_and_attaches_bearer() {
    let (base, server) = oauth_mock("tok-123");
    let src = "meta {\n  name: O\n  type: http\n}\n\n\
        get {\n  url: {{base}}/api\n  auth: oauth2\n}\n\n\
        auth:oauth2 {\n  grant_type: client_credentials\n  access_token_url: {{base}}/token\n  client_id: id\n  client_secret: sec\n  scope: read\n}\n";
    let file = bru_lang::parse(src).unwrap();

    let mut ctx = RunContext::default();
    ctx.vars.insert("base".to_string(), base.clone());

    let outcome = run_request(&file, &mut ctx).await;
    let api_req = server.join().unwrap();

    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert_eq!(outcome.response.as_ref().unwrap().status, 200);
    assert!(
        api_req
            .to_lowercase()
            .contains("authorization: bearer tok-123"),
        "API request missing bearer token:\n{api_req}"
    );
    assert_eq!(ctx.token_cache.len(), 1, "token should be cached");
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
async fn graphql_body_sends_json_with_query_and_variables() {
    let (base, server) = mock_server(r#"{"data":{"ok":true}}"#);
    // `{{id}}` interpolates into the variables block; query is outdented text.
    let src = "meta {\n  name: G\n  type: http\n}\n\n\
        post {\n  url: URL\n  body: graphql\n  auth: none\n}\n\n\
        body:graphql {\n  query Q($id: ID!) {\n    user(id: $id) { name }\n  }\n}\n\n\
        body:graphql:vars {\n  {\n    \"id\": \"{{id}}\"\n  }\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/graphql"))).unwrap();

    let mut ctx = RunContext::default();
    ctx.vars.insert("id".to_string(), "42".to_string());

    let outcome = run_request(&file, &mut ctx).await;
    let sent = server.join().unwrap();

    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(
        sent.to_lowercase()
            .contains("content-type: application/json"),
        "missing json content-type:\n{sent}"
    );
    // Body is the last line after the blank-line header separator.
    let body = sent.split("\r\n\r\n").nth(1).unwrap_or("");
    let json: serde_json::Value = serde_json::from_str(body.trim()).expect("body is JSON");
    assert!(
        json["query"].as_str().unwrap().contains("user(id: $id)"),
        "query missing:\n{json}"
    );
    assert_eq!(json["variables"]["id"], "42");
}

#[tokio::test]
async fn multipart_body_sends_text_and_file_parts() {
    use std::io::Write as _;

    // Write a temp file to act as the file part.
    let path = std::env::temp_dir().join("bru_rs_multipart_test.txt");
    {
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"hello-from-file").unwrap();
    }
    let path_str = path.to_string_lossy().replace('\\', "\\\\");

    let (base, server) = mock_server(r#"{"ok":true}"#);
    let src = format!(
        "meta {{\n  name: M\n  type: http\n}}\n\n\
        post {{\n  url: {base}/upload\n  body: multipartForm\n  auth: none\n}}\n\n\
        body:multipart-form {{\n  field1: {{{{val}}}}\n  doc: @file({path_str})\n}}\n"
    );
    let file = bru_lang::parse(&src).unwrap();

    let mut ctx = RunContext::default();
    ctx.vars.insert("val".to_string(), "textvalue".to_string());

    let outcome = run_request(&file, &mut ctx).await;
    let sent = server.join().unwrap();

    let _ = std::fs::remove_file(&path);

    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(
        sent.to_lowercase()
            .contains("content-type: multipart/form-data; boundary="),
        "missing multipart content-type:\n{sent}"
    );
    assert!(
        sent.contains("name=\"field1\"") && sent.contains("textvalue"),
        "text field missing:\n{sent}"
    );
    assert!(
        sent.contains("name=\"doc\"")
            && sent.contains("filename=\"bru_rs_multipart_test.txt\"")
            && sent.contains("hello-from-file"),
        "file part missing:\n{sent}"
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

/// A two-connection Digest mock: the first request gets a 401 + Digest challenge,
/// the second (with the recomputed Authorization) gets a 200. Returns the raw
/// bytes of the SECOND (retried) request so the test can inspect its header.
fn digest_mock() -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        // 1) unauthenticated request → 401 challenge
        let (mut s1, _) = listener.accept().unwrap();
        let _ = s1.read(&mut [0u8; 4096]);
        let challenge = "WWW-Authenticate: Digest realm=\"x\", nonce=\"abc\", qop=\"auth\"";
        let resp401 = format!(
            "HTTP/1.1 401 Unauthorized\r\n{challenge}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = s1.write_all(resp401.as_bytes());
        let _ = s1.flush();

        // 2) retried request carrying Authorization → 200
        let (mut s2, _) = listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let n = s2.read(&mut buf).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]).into_owned();
        let body = "{\"ok\":true}";
        let resp200 = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = s2.write_all(resp200.as_bytes());
        let _ = s2.flush();
        req
    });
    (format!("http://{addr}"), handle)
}

#[tokio::test]
async fn digest_challenge_response_resends_with_authorization() {
    let (base, server) = digest_mock();
    let src = "meta {\n  name: D\n  type: http\n}\n\n\
        get {\n  url: URL\n  auth: digest\n}\n\n\
        auth:digest {\n  username: Mufasa\n  password: secret\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/dir/index.html"))).unwrap();

    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let retried = server.join().unwrap();

    assert!(outcome.error.is_none(), "error: {:?}", outcome.error);
    assert_eq!(
        outcome.response.as_ref().unwrap().status,
        200,
        "digest retry should land 200"
    );
    let lower = retried.to_lowercase();
    assert!(
        lower.contains("authorization: digest "),
        "retry missing Digest Authorization header:\n{retried}"
    );
    assert!(
        lower.contains("response="),
        "retry Authorization missing response= field:\n{retried}"
    );
    assert!(
        retried.contains("username=\"Mufasa\""),
        "retry Authorization missing username:\n{retried}"
    );
    assert!(
        retried.contains("uri=\"/dir/index.html\""),
        "retry Authorization should sign the request path:\n{retried}"
    );
}

#[tokio::test]
async fn awsv4_signs_request_before_send() {
    let (base, server) = mock_server(r#"{"ok":true}"#);
    let src = "meta {\n  name: A\n  type: http\n}\n\n\
        get {\n  url: URL\n  auth: awsv4\n}\n\n\
        auth:awsv4 {\n  accessKeyId: AKIDEXAMPLE\n  secretAccessKey: secret\n  service: service\n  region: us-east-1\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/"))).unwrap();

    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let sent = server.join().unwrap();

    assert!(outcome.error.is_none(), "error: {:?}", outcome.error);
    assert_eq!(outcome.response.as_ref().unwrap().status, 200);
    let lower = sent.to_lowercase();
    assert!(
        lower.contains("authorization: aws4-hmac-sha256 "),
        "request missing SigV4 Authorization header:\n{sent}"
    );
    assert!(
        lower.contains("x-amz-date:"),
        "request missing x-amz-date header:\n{sent}"
    );
    assert!(
        sent.contains("Credential=AKIDEXAMPLE/")
            && sent.contains("/us-east-1/service/aws4_request"),
        "SigV4 credential scope wrong:\n{sent}"
    );
    assert!(
        sent.contains("SignedHeaders=") && sent.contains("Signature="),
        "SigV4 Authorization missing SignedHeaders/Signature:\n{sent}"
    );
    // No session token configured → no security-token header.
    assert!(
        !lower.contains("x-amz-security-token:"),
        "unexpected security-token header:\n{sent}"
    );
}

#[tokio::test]
async fn settings_block_projects_into_request() {
    let src = "meta {\n  name: S\n  type: http\n}\n\n\
        get {\n  url: https://api.test/x\n  auth: none\n}\n\n\
        settings {\n  followRedirects: false\n  maxRedirects: 3\n  timeout: 5000\n  encodeUrl: true\n}\n";
    let file = bru_lang::parse(src).unwrap();
    let req = file.to_request().expect("request projects");
    assert_eq!(req.settings.follow_redirects, Some(false));
    assert_eq!(req.settings.max_redirects, Some(3));
    assert_eq!(req.settings.timeout_ms, Some(5000));
    assert_eq!(req.settings.encode_url, Some(true));
}

#[tokio::test]
async fn settings_disables_redirect_following() {
    // Destination server: a one-shot 200 that should NEVER be reached when
    // followRedirects is off. It signals on `tx` if it ever accepts a connection.
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let dest_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let dest_addr = dest_listener.local_addr().unwrap();
    // Intentionally not joined: with the redirect suppressed, `accept()` blocks
    // forever, so the thread is abandoned and reaped on process exit.
    thread::spawn(move || {
        if let Ok((mut s, _)) = dest_listener.accept() {
            let _ = tx.send(());
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let body = "{\"reached\":true}";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });

    // Redirect server: replies 302 pointing at the destination.
    let redir_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let redir_addr = redir_listener.local_addr().unwrap();
    let location = format!("http://{dest_addr}/dest");
    let redir = thread::spawn(move || {
        let (mut s, _) = redir_listener.accept().unwrap();
        let mut buf = [0u8; 4096];
        let _ = s.read(&mut buf);
        let resp = format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        let _ = s.write_all(resp.as_bytes());
        let _ = s.flush();
    });

    let src = "meta {\n  name: R\n  type: http\n}\n\n\
        get {\n  url: URL\n  auth: none\n}\n\n\
        settings {\n  followRedirects: false\n}\n";
    let file =
        bru_lang::parse(&src.replace("URL", &format!("http://{redir_addr}/start"))).unwrap();

    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    redir.join().unwrap();

    assert!(outcome.error.is_none(), "error: {:?}", outcome.error);
    assert_eq!(
        outcome.response.as_ref().unwrap().status,
        302,
        "302 must be surfaced, not followed, when followRedirects:false"
    );
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(400)).is_err(),
        "destination was contacted — redirect was followed despite followRedirects:false"
    );
}
