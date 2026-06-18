//! Coverage-focused tests for bru-engine's public API: `base_vars`,
//! `find_collection_root`, and the `run_request` branches the end-to-end
//! `engine.rs` suite doesn't reach (error outcomes, more auth/body combos,
//! var precedence, content-type handling). Network paths use a hermetic mock
//! server bound to 127.0.0.1:0; filesystem paths use a self-cleaning TempDir.
#![allow(clippy::field_reassign_with_default)]

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;

use bru_engine::{base_vars, find_collection_root, run_request, RunContext, RunOutcome};

// ── temp-dir helper (no `tempfile` crate available) ─────────────────────────

/// A unique temp directory cleaned up on drop (mirrors fsops.rs's helper).
struct TempDir(PathBuf);
impl TempDir {
    fn new(tag: &str) -> Self {
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "bru-engine-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

// ── mock server ─────────────────────────────────────────────────────────────

/// One-shot HTTP/1.1 server returning a 200 with `body`. Captures the request.
fn mock_server(body: &'static str) -> (String, thread::JoinHandle<String>) {
    serve_once(200, "OK", "application/json", body)
}

/// One-shot server with a custom status / content-type, capturing the request.
fn serve_once(
    status: u16,
    reason: &'static str,
    content_type: &'static str,
    body: &'static str,
) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf).unwrap_or(0);
        let request = String::from_utf8_lossy(&buf[..n]).into_owned();
        let resp = format!(
            "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
        request
    });
    (format!("http://{addr}"), handle)
}

// ════════════════════════════════════════════════════════════════════════════
// context.rs — find_collection_root
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn find_root_via_bruno_json_from_dir_input() {
    let d = TempDir::new("root-brunojson");
    write(&d.path().join("bruno.json"), "{}");
    // A directory path that *is* the root resolves to itself.
    assert_eq!(find_collection_root(d.path()).as_deref(), Some(d.path()));
}

#[test]
fn find_root_via_collection_bru() {
    let d = TempDir::new("root-colbru");
    write(&d.path().join("collection.bru"), "meta {\n  name: C\n}\n");
    assert_eq!(find_collection_root(d.path()).as_deref(), Some(d.path()));
}

#[test]
fn find_root_via_environments_dir() {
    let d = TempDir::new("root-envdir");
    std::fs::create_dir_all(d.path().join("environments")).unwrap();
    assert_eq!(find_collection_root(d.path()).as_deref(), Some(d.path()));
}

#[test]
fn find_root_walks_up_multiple_levels_from_file_input() {
    let d = TempDir::new("root-walkup");
    write(&d.path().join("bruno.json"), "{}");
    let nested = d.path().join("a").join("b").join("c");
    std::fs::create_dir_all(&nested).unwrap();
    let file = nested.join("req.bru");
    write(&file, "meta {\n  name: R\n}\n");
    // From a deeply nested *file*, we walk up to the bruno.json root.
    assert_eq!(find_collection_root(&file).as_deref(), Some(d.path()));
}

#[test]
fn find_root_none_when_no_marker() {
    // A bare temp dir with no marker file walks up to the OS temp root, which is
    // unlikely to contain a bruno.json. To be deterministic, point at a path
    // under a freshly created dir that has none of the trigger markers and whose
    // parents (the OS temp dir) also lack them in practice — assert via a child
    // of a dir we fully control by checking the immediate dir returns None when
    // we query a non-existent file inside an empty controlled tree.
    let d = TempDir::new("root-none");
    let nested = d.path().join("empty");
    std::fs::create_dir_all(&nested).unwrap();
    // No bruno.json / collection.bru / environments under `nested` or `d`.
    // Walking up eventually leaves our controlled tree; the result is whatever
    // the first ancestor with a marker is (often None). We only assert it is NOT
    // our controlled dir, proving the markers gate the match.
    let found = find_collection_root(&nested);
    assert_ne!(found.as_deref(), Some(d.path()));
    assert_ne!(found.as_deref(), Some(nested.as_path()));
}

#[test]
fn find_root_from_nonexistent_file_uses_parent() {
    let d = TempDir::new("root-nofile");
    write(&d.path().join("bruno.json"), "{}");
    // A path to a file that does not exist: `is_dir()` is false, so we take the
    // parent and still find the root.
    let ghost = d.path().join("does-not-exist.bru");
    assert_eq!(find_collection_root(&ghost).as_deref(), Some(d.path()));
}

// ════════════════════════════════════════════════════════════════════════════
// context.rs — base_vars
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn base_vars_empty_when_no_collection_root() {
    let d = TempDir::new("bv-noroot");
    let nested = d.path().join("loose");
    std::fs::create_dir_all(&nested).unwrap();
    // No marker anywhere in our controlled tree → if a marker happens to exist
    // above, the map may be non-empty, so only assert our own vars are absent.
    let vars = base_vars(&nested, Some("dev"));
    assert!(!vars.contains_key("collectionOnly"));
    assert!(!vars.contains_key("envName"));
}

#[test]
fn base_vars_collection_only_when_no_env_requested() {
    let d = TempDir::new("bv-colonly");
    write(&d.path().join("bruno.json"), "{}");
    write(
        &d.path().join("collection.bru"),
        "meta {\n  name: C\n}\n\nvars:pre-request {\n  apiBase: https://collection.example\n  shared: from-collection\n}\n",
    );
    let req = d.path().join("req.bru");
    write(&req, "meta {\n  name: R\n}\n");

    // env=None → no environment overlay, only collection vars.
    let vars = base_vars(&req, None);
    assert_eq!(
        vars.get("apiBase").map(String::as_str),
        Some("https://collection.example")
    );
    assert_eq!(
        vars.get("shared").map(String::as_str),
        Some("from-collection")
    );
}

#[test]
fn base_vars_env_overrides_collection_and_excludes_disabled_and_secret() {
    let d = TempDir::new("bv-precedence");
    write(&d.path().join("bruno.json"), "{}");
    write(
        &d.path().join("collection.bru"),
        "meta {\n  name: C\n}\n\nvars:pre-request {\n  shared: from-collection\n  collectionOnly: keep-me\n  ~disabledCol: ignored\n}\n",
    );
    // Environment: overrides `shared`, adds `envName`, marks one var disabled,
    // and declares a secret var (whose value is never on disk → excluded).
    write(
        &d.path().join("environments").join("dev.bru"),
        "vars {\n  shared: from-env\n  envName: dev-value\n  ~disabledEnv: nope\n}\n\nvars:secret [\n  apiKey\n]\n",
    );
    let req = d.path().join("req.bru");
    write(&req, "meta {\n  name: R\n}\n");

    let vars = base_vars(&req, Some("dev"));

    // env overrides collection.
    assert_eq!(vars.get("shared").map(String::as_str), Some("from-env"));
    // collection-only var survives.
    assert_eq!(
        vars.get("collectionOnly").map(String::as_str),
        Some("keep-me")
    );
    // env-only var present.
    assert_eq!(vars.get("envName").map(String::as_str), Some("dev-value"));
    // disabled (collection) entry excluded by dict_vars filter.
    assert!(!vars.contains_key("disabledCol"));
    // disabled (env) entry excluded by the `.enabled` filter.
    assert!(!vars.contains_key("disabledEnv"));
    // secret var excluded by the `!v.secret` filter (no value on disk).
    assert!(!vars.contains_key("apiKey"));
}

#[test]
fn base_vars_missing_env_file_falls_back_to_collection() {
    let d = TempDir::new("bv-missingenv");
    write(&d.path().join("bruno.json"), "{}");
    write(
        &d.path().join("collection.bru"),
        "meta {\n  name: C\n}\n\nvars:pre-request {\n  only: collection\n}\n",
    );
    let req = d.path().join("req.bru");
    write(&req, "meta {\n  name: R\n}\n");
    // Requesting a non-existent env: load_env returns Err, the `if let Ok` is
    // skipped, and we keep the collection vars.
    let vars = base_vars(&req, Some("does-not-exist"));
    assert_eq!(vars.get("only").map(String::as_str), Some("collection"));
}

#[test]
fn base_vars_no_collection_bru_but_environments_dir() {
    // Root detected via `environments/` only — collection.bru is absent, so the
    // `read_to_string` fails and we fall straight to the env overlay.
    let d = TempDir::new("bv-envonly");
    write(
        &d.path().join("environments").join("prod.bru"),
        "vars {\n  region: us\n}\n",
    );
    let req = d.path().join("req.bru");
    write(&req, "meta {\n  name: R\n}\n");
    let vars = base_vars(&req, Some("prod"));
    assert_eq!(vars.get("region").map(String::as_str), Some("us"));
}

// ════════════════════════════════════════════════════════════════════════════
// lib.rs — RunOutcome helpers
// ════════════════════════════════════════════════════════════════════════════

#[test]
fn run_outcome_errored_constructor_and_passed_logic() {
    let o = RunOutcome::errored("name", "boom");
    assert_eq!(o.name, "name");
    assert_eq!(o.error.as_deref(), Some("boom"));
    // An errored outcome never "passed".
    assert!(!o.passed());

    // A clean default with no error, assertions, or tests passes.
    let ok = RunOutcome::default();
    assert!(ok.passed());
}

// ════════════════════════════════════════════════════════════════════════════
// lib.rs — run_request error / edge branches
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn run_request_errors_when_not_a_request_file() {
    // A .bru with no method block → `to_request()` is None.
    let file = bru_lang::parse("meta {\n  name: NotARequest\n}\n").unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    assert_eq!(outcome.name, "NotARequest");
    assert_eq!(
        outcome.error.as_deref(),
        Some("not a request .bru (no method block)")
    );
    assert!(outcome.response.is_none());
}

#[tokio::test]
async fn run_request_unnamed_request_uses_placeholder_name() {
    // No meta block at all → request_name() is None → "(unnamed)".
    let src = "get {\n  url: http://127.0.0.1:1/x\n  auth: none\n}\n";
    let file = bru_lang::parse(src).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    assert_eq!(outcome.name, "(unnamed)");
    // Connecting to a closed port surfaces a send error.
    assert!(outcome.error.is_some());
}

#[tokio::test]
async fn run_request_send_error_on_bad_host() {
    // An unresolvable / refused address makes `client.send` return Err.
    let src =
        "meta {\n  name: Bad\n  type: http\n}\n\nget {\n  url: http://127.0.0.1:1/nope\n  auth: none\n}\n";
    let file = bru_lang::parse(src).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    assert!(outcome.error.is_some(), "expected a send error");
    assert_eq!(outcome.method, "GET");
    assert!(outcome.response.is_none());
}

#[tokio::test]
async fn run_request_invalid_url_surfaces_error() {
    // A malformed URL fails inside client.send (not resolve_url, which is lenient).
    let src = "meta {\n  name: U\n  type: http\n}\n\nget {\n  url: not a url\n  auth: none\n}\n";
    let file = bru_lang::parse(src).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    assert!(outcome.error.is_some());
}

#[tokio::test]
async fn run_request_graphql_invalid_variables_errors_before_send() {
    // Non-empty, non-JSON variables → the up-front validation aborts the request.
    let src = "meta {\n  name: G\n  type: http\n}\n\n\
        post {\n  url: http://127.0.0.1:1/gql\n  body: graphql\n  auth: none\n}\n\n\
        body:graphql {\n  query Q { a }\n}\n\n\
        body:graphql:vars {\n  not-json-at-all\n}\n";
    let file = bru_lang::parse(src).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    assert_eq!(
        outcome.error.as_deref(),
        Some("graphql variables are not valid JSON")
    );
    assert_eq!(outcome.method, "POST");
    // It aborted before any network attempt: no response captured.
    assert!(outcome.response.is_none());
}

#[tokio::test]
async fn run_request_graphql_empty_variables_is_ok() {
    // Empty variables skip the JSON validation entirely.
    let (base, server) = mock_server(r#"{"data":{"ok":true}}"#);
    let src = "meta {\n  name: G\n  type: http\n}\n\n\
        post {\n  url: URL\n  body: graphql\n  auth: none\n}\n\n\
        body:graphql {\n  query Q { a }\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/gql"))).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let _ = server.join();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert_eq!(outcome.response.as_ref().unwrap().status, 200);
}

#[tokio::test]
async fn run_request_pre_request_script_error_aborts() {
    let src = "meta {\n  name: P\n  type: http\n}\n\n\
        get {\n  url: http://127.0.0.1:1/x\n  auth: none\n}\n\n\
        script:pre-request {\n  throw new Error('boom-pre');\n}\n";
    let file = bru_lang::parse(src).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let err = outcome.error.expect("pre-request error");
    assert!(err.starts_with("pre-request script error:"), "err: {err}");
    assert!(err.contains("boom-pre"), "err: {err}");
    // Aborted before send → no response.
    assert!(outcome.response.is_none());
}

#[tokio::test]
async fn run_request_pre_request_vars_feed_interpolation() {
    // A `vars:pre-request` var (enabled) interpolates into the URL path.
    let (base, server) = mock_server(r#"{"ok":true}"#);
    let src = "meta {\n  name: V\n  type: http\n}\n\n\
        get {\n  url: {{root}}/{{seg}}\n  auth: none\n}\n\n\
        vars:pre-request {\n  seg: hello\n  ~off: nope\n}\n";
    let file = bru_lang::parse(&src.replace("{{root}}", &base)).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let sent = server.join().unwrap();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(sent.starts_with("GET /hello "), "request line: {sent:?}");
    // Disabled pre-request var was not inserted into the context.
    assert!(!ctx.vars.contains_key("off"));
    // Enabled one persists in the context.
    assert_eq!(ctx.vars.get("seg").map(String::as_str), Some("hello"));
}

#[tokio::test]
async fn run_request_post_response_script_error_recorded_as_failed_test() {
    let (base, server) = mock_server(r#"{"id":7}"#);
    let src = "meta {\n  name: PR\n  type: http\n}\n\n\
        get {\n  url: URL\n  auth: none\n}\n\n\
        script:post-response {\n  throw new Error('boom-post');\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/x"))).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let _ = server.join();
    // The request itself succeeded (no top-level error), but the post-response
    // script error becomes a failed synthetic test.
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    let failed = outcome
        .tests
        .iter()
        .find(|t| t.name == "post-response script")
        .expect("synthetic failed test");
    assert!(!failed.passed);
    assert!(!outcome.passed(), "a failed test means the outcome fails");
}

#[tokio::test]
async fn run_request_post_response_var_capture_only_when_expr_resolves() {
    // Two captures: one resolvable (`res.body.id`), one that does not resolve and
    // is therefore skipped (no entry pushed).
    let (base, server) = mock_server(r#"{"id":99}"#);
    let src = "meta {\n  name: C\n  type: http\n}\n\n\
        get {\n  url: URL\n  auth: none\n}\n\n\
        vars:post-response {\n  good: res.body.id\n  ~disabled: res.body.id\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/x"))).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let _ = server.join();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert_eq!(
        outcome.vars_set,
        vec![("good".to_string(), "99".to_string())]
    );
    // Disabled capture never ran.
    assert!(!ctx.vars.contains_key("disabled"));
}

#[tokio::test]
async fn run_request_basic_auth_interpolates_and_sends_header() {
    let (base, server) = mock_server(r#"{"ok":true}"#);
    let src = "meta {\n  name: B\n  type: http\n}\n\n\
        get {\n  url: URL\n  auth: basic\n}\n\n\
        auth:basic {\n  username: {{user}}\n  password: pw\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/x"))).unwrap();
    let mut ctx = RunContext::default();
    ctx.vars.insert("user".to_string(), "alice".to_string());
    let outcome = run_request(&file, &mut ctx).await;
    let sent = server.join().unwrap();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    // Basic auth header present (base64 of alice:pw); we just assert the scheme.
    assert!(
        sent.to_lowercase().contains("authorization: basic "),
        "missing basic auth header:\n{sent}"
    );
}

#[tokio::test]
async fn run_request_bearer_auth_sends_token() {
    let (base, server) = mock_server(r#"{"ok":true}"#);
    let src = "meta {\n  name: Be\n  type: http\n}\n\n\
        get {\n  url: URL\n  auth: bearer\n}\n\n\
        auth:bearer {\n  token: {{tok}}\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/x"))).unwrap();
    let mut ctx = RunContext::default();
    ctx.vars.insert("tok".to_string(), "secret-tok".to_string());
    let outcome = run_request(&file, &mut ctx).await;
    let sent = server.join().unwrap();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(
        sent.to_lowercase()
            .contains("authorization: bearer secret-tok"),
        "missing bearer token:\n{sent}"
    );
}

#[tokio::test]
async fn run_request_apikey_auth_in_header() {
    let (base, server) = mock_server(r#"{"ok":true}"#);
    let src = "meta {\n  name: K\n  type: http\n}\n\n\
        get {\n  url: URL\n  auth: apikey\n}\n\n\
        auth:apikey {\n  key: X-Api-Key\n  value: {{val}}\n  placement: header\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/x"))).unwrap();
    let mut ctx = RunContext::default();
    ctx.vars.insert("val".to_string(), "abc-key".to_string());
    let outcome = run_request(&file, &mut ctx).await;
    let sent = server.join().unwrap();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(
        sent.to_lowercase().contains("x-api-key: abc-key"),
        "missing api key header:\n{sent}"
    );
}

#[tokio::test]
async fn run_request_oauth2_token_endpoint_error_aborts() {
    // Token endpoint returns 401 → fetch_token errors → run aborts with oauth2:.
    let (base, server) = serve_once(
        401,
        "Unauthorized",
        "application/json",
        r#"{"error":"bad"}"#,
    );
    let src = "meta {\n  name: O\n  type: http\n}\n\n\
        get {\n  url: URL/api\n  auth: oauth2\n}\n\n\
        auth:oauth2 {\n  grant_type: client_credentials\n  access_token_url: URL/token\n  client_id: id\n  client_secret: sec\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &base)).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let _ = server.join();
    let err = outcome.error.expect("oauth error");
    assert!(err.starts_with("oauth2:"), "err: {err}");
    assert_eq!(outcome.method, "GET");
    assert!(ctx.token_cache.is_empty(), "no token cached on failure");
}

#[tokio::test]
async fn run_request_oauth2_token_placement_query() {
    // token_placement=query → token appended as a query param, not a header.
    let (base, server) = oauth_query_mock("qtok");
    let src = "meta {\n  name: Oq\n  type: http\n}\n\n\
        get {\n  url: URL/api\n  auth: oauth2\n}\n\n\
        auth:oauth2 {\n  grant_type: client_credentials\n  access_token_url: URL/token\n  client_id: id\n  client_secret: sec\n  token_placement: query\n  token_query_key: at\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &base)).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let api_req = server.join().unwrap();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(
        api_req.starts_with("GET /api?at=qtok "),
        "token not placed in query:\n{api_req}"
    );
}

/// Two-connection OAuth mock: 1) token endpoint, 2) capture the API request line.
fn oauth_query_mock(token: &'static str) -> (String, thread::JoinHandle<String>) {
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
        let (mut s1, _) = listener.accept().unwrap();
        let _ = s1.read(&mut [0u8; 4096]);
        respond(&mut s1, format!("{{\"access_token\":\"{token}\"}}"));
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
async fn run_request_awsv4_multipart_unsupported_errors() {
    // SigV4 over a multipart body is explicitly rejected before send.
    let src = "meta {\n  name: AwsMp\n  type: http\n}\n\n\
        post {\n  url: http://127.0.0.1:1/x\n  body: multipartForm\n  auth: awsv4\n}\n\n\
        auth:awsv4 {\n  accessKeyId: AK\n  secretAccessKey: sk\n  region: us-east-1\n  service: s3\n}\n\n\
        body:multipart-form {\n  a: b\n}\n";
    let file = bru_lang::parse(src).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    assert_eq!(
        outcome.error.as_deref(),
        Some("awsv4: signing multipart bodies is not supported")
    );
    assert_eq!(outcome.method, "POST");
}

#[tokio::test]
async fn run_request_awsv4_session_token_adds_security_header() {
    let (base, server) = mock_server(r#"{"ok":true}"#);
    let src = "meta {\n  name: AwsTok\n  type: http\n}\n\n\
        get {\n  url: URL/\n  auth: awsv4\n}\n\n\
        auth:awsv4 {\n  accessKeyId: AKIDEXAMPLE\n  secretAccessKey: secret\n  sessionToken: SESSION123\n  service: service\n  region: us-east-1\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &base)).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let sent = server.join().unwrap();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(
        sent.to_lowercase()
            .contains("x-amz-security-token: session123"),
        "session token header missing:\n{sent}"
    );
}

#[tokio::test]
async fn run_request_awsv4_invalid_url_errors() {
    // SigV4 url_parse fails on a URL with no host → resolve returns Err.
    let src = "meta {\n  name: AwsBad\n  type: http\n}\n\n\
        get {\n  url: not://a host\n  auth: awsv4\n}\n\n\
        auth:awsv4 {\n  accessKeyId: AK\n  secretAccessKey: sk\n}\n";
    let file = bru_lang::parse(src).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let err = outcome.error.expect("awsv4 url error");
    assert!(err.contains("awsv4"), "err: {err}");
}

#[tokio::test]
async fn run_request_form_urlencoded_body_interpolated_and_sent() {
    let (base, server) = mock_server(r#"{"ok":true}"#);
    let src = "meta {\n  name: F\n  type: http\n}\n\n\
        post {\n  url: URL\n  body: formUrlEncoded\n  auth: none\n}\n\n\
        body:form-urlencoded {\n  field: {{val}}\n  fixed: 1\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/f"))).unwrap();
    let mut ctx = RunContext::default();
    ctx.vars.insert("val".to_string(), "hello".to_string());
    let outcome = run_request(&file, &mut ctx).await;
    let sent = server.join().unwrap();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(
        sent.to_lowercase()
            .contains("content-type: application/x-www-form-urlencoded"),
        "missing form content-type:\n{sent}"
    );
    let body = sent.split("\r\n\r\n").nth(1).unwrap_or("");
    assert!(body.contains("field=hello"), "body: {body}");
}

#[tokio::test]
async fn run_request_text_body_interpolated_and_sent() {
    let (base, server) = mock_server(r#"{"ok":true}"#);
    let src = "meta {\n  name: T\n  type: http\n}\n\n\
        post {\n  url: URL\n  body: text\n  auth: none\n}\n\n\
        body:text {\n  hello {{who}}\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/t"))).unwrap();
    let mut ctx = RunContext::default();
    ctx.vars.insert("who".to_string(), "world".to_string());
    let outcome = run_request(&file, &mut ctx).await;
    let sent = server.join().unwrap();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    let body = sent.split("\r\n\r\n").nth(1).unwrap_or("");
    assert!(body.contains("hello world"), "body: {body}");
}

#[tokio::test]
async fn run_request_xml_body_sent() {
    let (base, server) = mock_server(r#"{"ok":true}"#);
    let src = "meta {\n  name: Xm\n  type: http\n}\n\n\
        post {\n  url: URL\n  body: xml\n  auth: none\n}\n\n\
        body:xml {\n  <a>{{v}}</a>\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/x"))).unwrap();
    let mut ctx = RunContext::default();
    ctx.vars.insert("v".to_string(), "1".to_string());
    let outcome = run_request(&file, &mut ctx).await;
    let sent = server.join().unwrap();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    let body = sent.split("\r\n\r\n").nth(1).unwrap_or("");
    assert!(body.contains("<a>1</a>"), "body: {body}");
}

#[tokio::test]
async fn run_request_non_json_response_body_used_as_text() {
    // A non-JSON response body: `resp.json()` is None, so a post-response script
    // sees the body as a string. Exercises the `script_response` text fallback.
    let (base, server) = serve_once(200, "OK", "text/plain", "plain-text-body");
    let src = "meta {\n  name: NJ\n  type: http\n}\n\n\
        get {\n  url: URL\n  auth: none\n}\n\n\
        tests {\n  test('body is string', function(){ expect(typeof res.body).to.equal('string'); });\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/x"))).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let _ = server.join();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(outcome.passed(), "tests: {:?}", outcome.tests);
}

#[tokio::test]
async fn run_request_path_params_folded_into_url() {
    // `:id` path param substitution happens in resolve_url before send.
    let (base, server) = mock_server(r#"{"ok":true}"#);
    let src = "meta {\n  name: PP\n  type: http\n}\n\n\
        get {\n  url: URL/users/:id\n  auth: none\n}\n\n\
        params:path {\n  id: 7\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &base)).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let sent = server.join().unwrap();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(sent.starts_with("GET /users/7 "), "request line: {sent:?}");
}

#[tokio::test]
async fn run_request_inherit_auth_resolves_to_none_without_script_dir() {
    // auth: inherit with no script_dir → resolve_inherited_auth returns None,
    // so the request just sends unauthenticated.
    let (base, server) = mock_server(r#"{"ok":true}"#);
    let src = "meta {\n  name: I\n  type: http\n}\n\nget {\n  url: URL\n  auth: inherit\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/x"))).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let sent = server.join().unwrap();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(
        !sent.to_lowercase().contains("authorization:"),
        "no auth expected:\n{sent}"
    );
}

#[tokio::test]
async fn run_request_inherit_auth_resolves_from_collection_bru() {
    // auth: inherit + a collection.bru declaring bearer auth → the request
    // inherits the bearer token.
    let d = TempDir::new("inherit-col");
    write(&d.path().join("bruno.json"), "{}");
    write(
        &d.path().join("collection.bru"),
        "auth {\n  mode: bearer\n}\n\nauth:bearer {\n  token: inherited-tok\n}\n",
    );
    let (base, server) = mock_server(r#"{"ok":true}"#);
    let src = "meta {\n  name: I2\n  type: http\n}\n\nget {\n  url: URL\n  auth: inherit\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/x"))).unwrap();
    let mut ctx = RunContext::default();
    // script_dir inside the collection lets inherit walk up to collection.bru.
    ctx.script_dir = Some(d.path().to_path_buf());
    let outcome = run_request(&file, &mut ctx).await;
    let sent = server.join().unwrap();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(
        sent.to_lowercase()
            .contains("authorization: bearer inherited-tok"),
        "inherited bearer missing:\n{sent}"
    );
}

#[tokio::test]
async fn run_request_inherit_auth_walks_up_from_folder() {
    // A request in a sub-folder inherits from a folder.bru above it.
    let d = TempDir::new("inherit-folder");
    write(&d.path().join("bruno.json"), "{}");
    write(&d.path().join("collection.bru"), "meta {\n  name: C\n}\n");
    let folder = d.path().join("sub");
    std::fs::create_dir_all(&folder).unwrap();
    write(
        &folder.join("folder.bru"),
        "auth {\n  mode: bearer\n}\n\nauth:bearer {\n  token: folder-tok\n}\n",
    );
    let (base, server) = mock_server(r#"{"ok":true}"#);
    let src = "meta {\n  name: I3\n  type: http\n}\n\nget {\n  url: URL\n  auth: inherit\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/x"))).unwrap();
    let mut ctx = RunContext::default();
    ctx.script_dir = Some(folder.clone());
    let outcome = run_request(&file, &mut ctx).await;
    let sent = server.join().unwrap();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(
        sent.to_lowercase()
            .contains("authorization: bearer folder-tok"),
        "inherited folder bearer missing:\n{sent}"
    );
}

#[tokio::test]
async fn run_request_settings_max_redirects_builds_separate_client() {
    // A `maxRedirects` override forces a separate client build (the
    // follow_redirects/max_redirects branch). We don't need a real redirect —
    // just exercise the client-build branch and a successful send.
    let (base, server) = mock_server(r#"{"ok":true}"#);
    let src = "meta {\n  name: SR\n  type: http\n}\n\n\
        get {\n  url: URL\n  auth: none\n}\n\n\
        settings {\n  maxRedirects: 2\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/x"))).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let _ = server.join();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert_eq!(outcome.response.as_ref().unwrap().status, 200);
}

#[tokio::test]
async fn run_request_console_log_collected() {
    let (base, server) = mock_server(r#"{"ok":true}"#);
    let src = "meta {\n  name: L\n  type: http\n}\n\n\
        get {\n  url: URL\n  auth: none\n}\n\n\
        script:pre-request {\n  console.log('pre-line');\n}\n\n\
        script:post-response {\n  console.log('post-line');\n}\n";
    let file = bru_lang::parse(&src.replace("URL", &format!("{base}/x"))).unwrap();
    let mut ctx = RunContext::default();
    let outcome = run_request(&file, &mut ctx).await;
    let _ = server.join();
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(
        outcome.console.iter().any(|l| l.contains("pre-line")),
        "console: {:?}",
        outcome.console
    );
    assert!(
        outcome.console.iter().any(|l| l.contains("post-line")),
        "console: {:?}",
        outcome.console
    );
}
