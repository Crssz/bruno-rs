//! End-to-end `HttpClient::send` coverage against hermetic local mock servers.
//! Each body kind and content-type branch is verified by inspecting the exact
//! bytes the client put on the wire (captured by a one-shot TCP server).

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread;

use bru_core::{
    ApiKeyPlacement, Auth, Body, FileBodyItem, KeyVal, MultipartField, MultipartValue, Request,
};
use bru_http::{HttpClient, HttpError, SendOptions};

/// One-shot HTTP/1.1 server: captures the raw request bytes and replies 200 with
/// `resp_body`. Returns the bound base URL and a handle yielding what the client
/// sent. Reads until the headers terminator, then drains a Content-Length body.
fn capture_server(resp_body: &'static str) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = read_full_request(&mut stream);
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            resp_body.len(),
            resp_body
        );
        let _ = stream.write_all(resp.as_bytes());
        let _ = stream.flush();
        request
    });
    (format!("http://{addr}"), handle)
}

/// Read a full HTTP request: headers plus any Content-Length body, so multipart
/// and large bodies are captured in full rather than truncated at one recv().
fn read_full_request(stream: &mut std::net::TcpStream) -> String {
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        data.extend_from_slice(&buf[..n]);
        // Find header terminator.
        if let Some(pos) = find_subslice(&data, b"\r\n\r\n") {
            let header = String::from_utf8_lossy(&data[..pos]);
            let content_len = header
                .lines()
                .find_map(|l| {
                    let l = l.to_ascii_lowercase();
                    l.strip_prefix("content-length:")
                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                })
                .unwrap_or(0);
            let body_have = data.len() - (pos + 4);
            if body_have >= content_len {
                break;
            }
        }
    }
    String::from_utf8_lossy(&data).into_owned()
}

fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn client() -> HttpClient {
    HttpClient::new(&SendOptions::default()).unwrap()
}

fn post(base: &str, path: &str, body: Body) -> Request {
    Request {
        method: "POST".to_string(),
        url: format!("{base}{path}"),
        body,
        auth: Auth::None,
        ..Request::default()
    }
}

fn kv(name: &str, value: &str) -> KeyVal {
    KeyVal {
        name: name.into(),
        value: value.into(),
        enabled: true,
    }
}

/// A temp file with an RAII Drop guard (no `tempfile` crate available).
struct TmpFile {
    path: String,
}

impl TmpFile {
    fn new(prefix: &str, contents: &[u8]) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("{prefix}_{}_{}.bin", std::process::id(), n));
        std::fs::write(&path, contents).unwrap();
        TmpFile {
            path: path.to_string_lossy().into_owned(),
        }
    }
}

impl Drop for TmpFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn body_of(sent: &str) -> &str {
    sent.split("\r\n\r\n").nth(1).unwrap_or("")
}

// ---- Body kinds on the wire -------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn json_body_sets_default_content_type() {
    let (base, server) = capture_server(r#"{"ok":true}"#);
    let req = post(&base, "/j", Body::Json(r#"{"a":1}"#.into()));
    let resp = client().send(&req).await.unwrap();
    let sent = server.join().unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.status_text, "OK");
    let lower = sent.to_lowercase();
    assert!(lower.contains("content-type: application/json"), "{sent}");
    assert_eq!(body_of(&sent).trim(), r#"{"a":1}"#);
}

#[tokio::test(flavor = "current_thread")]
async fn text_body_sets_text_plain() {
    let (base, server) = capture_server("ok");
    let req = post(&base, "/t", Body::Text("hello".into()));
    client().send(&req).await.unwrap();
    let sent = server.join().unwrap();
    assert!(
        sent.to_lowercase().contains("content-type: text/plain"),
        "{sent}"
    );
    assert_eq!(body_of(&sent), "hello");
}

#[tokio::test(flavor = "current_thread")]
async fn xml_body_sets_application_xml() {
    let (base, server) = capture_server("ok");
    let req = post(&base, "/x", Body::Xml("<r/>".into()));
    client().send(&req).await.unwrap();
    let sent = server.join().unwrap();
    assert!(
        sent.to_lowercase()
            .contains("content-type: application/xml"),
        "{sent}"
    );
    assert_eq!(body_of(&sent), "<r/>");
}

#[tokio::test(flavor = "current_thread")]
async fn sparql_body_sets_sparql_content_type() {
    let (base, server) = capture_server("ok");
    let req = post(&base, "/s", Body::Sparql("SELECT * WHERE {}".into()));
    client().send(&req).await.unwrap();
    let sent = server.join().unwrap();
    assert!(
        sent.to_lowercase()
            .contains("content-type: application/sparql-query"),
        "{sent}"
    );
    assert_eq!(body_of(&sent), "SELECT * WHERE {}");
}

#[tokio::test(flavor = "current_thread")]
async fn form_urlencoded_body_default_ct() {
    let (base, server) = capture_server("ok");
    let req = post(
        &base,
        "/f",
        Body::FormUrlEncoded(vec![
            kv("a", "1"),
            kv("b", "two words"),
            KeyVal {
                name: "skip".into(),
                value: "x".into(),
                enabled: false,
            },
        ]),
    );
    client().send(&req).await.unwrap();
    let sent = server.join().unwrap();
    let lower = sent.to_lowercase();
    assert!(
        lower.contains("content-type: application/x-www-form-urlencoded"),
        "{sent}"
    );
    let body = body_of(&sent);
    assert!(body.contains("a=1"), "{body}");
    assert!(body.contains("b=two+words"), "{body}");
    assert!(!body.contains("skip"), "disabled field leaked: {body}");
}

#[tokio::test(flavor = "current_thread")]
async fn form_urlencoded_body_with_explicit_ct_encodes_manually() {
    let (base, server) = capture_server("ok");
    let mut req = post(
        &base,
        "/f",
        Body::FormUrlEncoded(vec![kv("a", "1"), kv("b", "2")]),
    );
    // An explicit content-type forces the manual serde_urlencoded path.
    req.headers = vec![kv(
        "Content-Type",
        "application/x-www-form-urlencoded; charset=utf-8",
    )];
    client().send(&req).await.unwrap();
    let sent = server.join().unwrap();
    // Exactly one content-type, the explicit one (charset preserved).
    let count = sent.to_lowercase().matches("content-type:").count();
    assert_eq!(count, 1, "{sent}");
    assert!(sent.to_lowercase().contains("charset=utf-8"), "{sent}");
    let body = body_of(&sent);
    assert!(body.contains("a=1") && body.contains("b=2"), "{body}");
}

#[tokio::test(flavor = "current_thread")]
async fn graphql_body_serializes_query_and_variables() {
    let (base, server) = capture_server("ok");
    let req = post(
        &base,
        "/g",
        Body::GraphQl {
            query: "query { me }".into(),
            variables: r#"{"id":7}"#.into(),
        },
    );
    client().send(&req).await.unwrap();
    let sent = server.join().unwrap();
    assert!(
        sent.to_lowercase()
            .contains("content-type: application/json"),
        "{sent}"
    );
    let json: serde_json::Value = serde_json::from_str(body_of(&sent).trim()).unwrap();
    assert_eq!(json["query"], "query { me }");
    assert_eq!(json["variables"]["id"], 7);
}

#[tokio::test(flavor = "current_thread")]
async fn graphql_body_blank_vars_uses_empty_object() {
    let (base, server) = capture_server("ok");
    let req = post(
        &base,
        "/g",
        Body::GraphQl {
            query: "{ me }".into(),
            variables: "".into(),
        },
    );
    client().send(&req).await.unwrap();
    let sent = server.join().unwrap();
    let json: serde_json::Value = serde_json::from_str(body_of(&sent).trim()).unwrap();
    assert_eq!(json["variables"], serde_json::json!({}));
}

#[tokio::test(flavor = "current_thread")]
async fn none_body_sends_no_content_type() {
    let (base, server) = capture_server("ok");
    let req = Request {
        method: "GET".to_string(),
        url: format!("{base}/n"),
        auth: Auth::None,
        ..Request::default()
    };
    client().send(&req).await.unwrap();
    let sent = server.join().unwrap();
    assert!(sent.starts_with("GET /n "), "{sent}");
    assert!(!sent.to_lowercase().contains("content-type:"), "{sent}");
}

#[tokio::test(flavor = "current_thread")]
async fn multipart_body_sends_text_and_file_parts() {
    let g = TmpFile::new("bru_http_cov_mp", b"the-file-contents");
    let (base, server) = capture_server("ok");
    let req = post(
        &base,
        "/m",
        Body::MultipartForm(vec![
            MultipartField {
                name: "field".into(),
                value: MultipartValue::Text("textval".into()),
                content_type: None,
                enabled: true,
            },
            MultipartField {
                name: "doc".into(),
                value: MultipartValue::File(g.path.clone()),
                content_type: Some("text/plain".into()),
                enabled: true,
            },
        ]),
    );
    client().send(&req).await.unwrap();
    let sent = server.join().unwrap();
    let lower = sent.to_lowercase();
    assert!(
        lower.contains("content-type: multipart/form-data; boundary="),
        "{sent}"
    );
    assert!(
        sent.contains("name=\"field\"") && sent.contains("textval"),
        "{sent}"
    );
    let basename = std::path::Path::new(&g.path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        sent.contains("name=\"doc\"") && sent.contains(&format!("filename=\"{basename}\"")),
        "{sent}"
    );
    assert!(sent.contains("the-file-contents"), "{sent}");
}

#[tokio::test(flavor = "current_thread")]
async fn file_body_sends_selected_file_bytes_with_content_type() {
    let g = TmpFile::new("bru_http_cov_file", b"binary-body-data");
    let (base, server) = capture_server("ok");
    let req = post(
        &base,
        "/file",
        Body::File(vec![FileBodyItem {
            path: g.path.clone(),
            content_type: Some("application/octet-stream".into()),
            selected: true,
        }]),
    );
    client().send(&req).await.unwrap();
    let sent = server.join().unwrap();
    assert!(
        sent.to_lowercase()
            .contains("content-type: application/octet-stream"),
        "{sent}"
    );
    assert_eq!(body_of(&sent), "binary-body-data");
}

#[tokio::test(flavor = "current_thread")]
async fn file_body_blank_path_sends_empty_body() {
    let (base, server) = capture_server("ok");
    let req = post(
        &base,
        "/file",
        Body::File(vec![FileBodyItem {
            path: "  ".into(),
            content_type: None,
            selected: true,
        }]),
    );
    let resp = client().send(&req).await.unwrap();
    let _ = server.join().unwrap();
    assert_eq!(resp.status, 200);
}

// ---- Content-type override / headers ----------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn explicit_content_type_not_duplicated_for_json_body() {
    let (base, server) = capture_server("ok");
    let mut req = post(&base, "/j", Body::Json(r#"{"a":1}"#.into()));
    req.headers = vec![kv("Content-Type", "application/vnd.custom+json")];
    client().send(&req).await.unwrap();
    let sent = server.join().unwrap();
    let count = sent.to_lowercase().matches("content-type:").count();
    assert_eq!(count, 1, "expected one content-type:\n{sent}");
    assert!(
        sent.to_lowercase().contains("application/vnd.custom+json"),
        "{sent}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn enabled_headers_are_sent_disabled_ones_skipped() {
    let (base, server) = capture_server("ok");
    let mut req = Request {
        method: "GET".to_string(),
        url: format!("{base}/h"),
        auth: Auth::None,
        ..Request::default()
    };
    req.headers = vec![
        kv("X-Custom", "yes"),
        KeyVal {
            name: "X-Off".into(),
            value: "no".into(),
            enabled: false,
        },
    ];
    client().send(&req).await.unwrap();
    let sent = server.join().unwrap();
    let lower = sent.to_lowercase();
    assert!(lower.contains("x-custom: yes"), "{sent}");
    assert!(!lower.contains("x-off"), "disabled header leaked:\n{sent}");
}

// ---- Auth on the wire -------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn basic_auth_sets_authorization_header() {
    let (base, server) = capture_server("ok");
    let mut req = Request {
        method: "GET".to_string(),
        url: format!("{base}/a"),
        ..Request::default()
    };
    req.auth = Auth::Basic {
        username: "user".into(),
        password: "pass".into(),
    };
    client().send(&req).await.unwrap();
    let sent = server.join().unwrap();
    assert!(
        sent.to_lowercase().contains("authorization: basic "),
        "{sent}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn bearer_auth_sets_authorization_header() {
    let (base, server) = capture_server("ok");
    let mut req = Request {
        method: "GET".to_string(),
        url: format!("{base}/a"),
        ..Request::default()
    };
    req.auth = Auth::Bearer {
        token: "tok-xyz".into(),
    };
    client().send(&req).await.unwrap();
    let sent = server.join().unwrap();
    assert!(
        sent.to_lowercase()
            .contains("authorization: bearer tok-xyz"),
        "{sent}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn api_key_header_placement_sends_custom_header() {
    let (base, server) = capture_server("ok");
    let mut req = Request {
        method: "GET".to_string(),
        url: format!("{base}/a"),
        ..Request::default()
    };
    req.auth = Auth::ApiKey {
        key: "X-Api-Key".into(),
        value: "secret".into(),
        placement: ApiKeyPlacement::Header,
    };
    client().send(&req).await.unwrap();
    let sent = server.join().unwrap();
    assert!(sent.to_lowercase().contains("x-api-key: secret"), "{sent}");
}

#[tokio::test(flavor = "current_thread")]
async fn api_key_query_placement_appends_to_url() {
    let (base, server) = capture_server("ok");
    let mut req = Request {
        method: "GET".to_string(),
        url: format!("{base}/a"),
        ..Request::default()
    };
    req.auth = Auth::ApiKey {
        key: "api_key".into(),
        value: "secret".into(),
        placement: ApiKeyPlacement::Query,
    };
    client().send(&req).await.unwrap();
    let sent = server.join().unwrap();
    assert!(sent.starts_with("GET /a?api_key=secret "), "{sent}");
}

// ---- Per-request timeout override -------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn per_request_timeout_override_still_succeeds_fast() {
    let (base, server) = capture_server("ok");
    let mut req = Request {
        method: "GET".to_string(),
        url: format!("{base}/q"),
        auth: Auth::None,
        ..Request::default()
    };
    // Generous override; the mock answers immediately, so this just exercises the
    // `settings.timeout_ms` branch in send().
    req.settings.timeout_ms = Some(5_000);
    let resp = client().send(&req).await.unwrap();
    let _ = server.join().unwrap();
    assert_eq!(resp.status, 200);
}

// ---- gzip response decoding -------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn gzip_encoded_response_is_decoded() {
    // Pre-gzipped "hello gzip" payload (gzip of the bytes below), produced by a
    // minimal deflate writer below so the test stays dependency-free.
    let plain = b"hello gzip body content";
    let gz = gzip(plain);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_full_request(&mut stream);
        let mut resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Encoding: gzip\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            gz.len()
        )
        .into_bytes();
        resp.extend_from_slice(&gz);
        let _ = stream.write_all(&resp);
        let _ = stream.flush();
    });
    let req = Request {
        method: "GET".to_string(),
        url: format!("http://{addr}/gz"),
        auth: Auth::None,
        ..Request::default()
    };
    let resp = client().send(&req).await.unwrap();
    server.join().unwrap();
    // reqwest's gzip feature transparently decodes the body back to plaintext.
    assert_eq!(resp.body, plain, "got {:?}", resp.text());
}

/// Minimal RFC 1952 gzip container around a single stored (uncompressed) DEFLATE
/// block — enough for reqwest to decode without pulling in flate2 as a dev-dep.
fn gzip(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    // gzip header: magic, deflate method, no flags, mtime=0, xfl=0, os=255.
    out.extend_from_slice(&[0x1f, 0x8b, 0x08, 0x00, 0, 0, 0, 0, 0x00, 0xff]);
    // DEFLATE stored blocks (BFINAL on the last). Each stored block holds up to
    // 65535 bytes: 1 header byte + LEN + NLEN (LE) + raw bytes.
    let mut chunks = data.chunks(0xffff).peekable();
    if data.is_empty() {
        out.push(0x01); // a single final, empty stored block
        out.extend_from_slice(&[0x00, 0x00, 0xff, 0xff]);
    }
    while let Some(chunk) = chunks.next() {
        let bfinal: u8 = if chunks.peek().is_none() { 1 } else { 0 };
        out.push(bfinal); // BTYPE=00 (stored) in the low bits, BFINAL above
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    // CRC32 (IEEE) of the uncompressed data, then ISIZE (length mod 2^32).
    out.extend_from_slice(&crc32(data).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xffff_ffff;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

// ---- Error variants ---------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn invalid_method_errors() {
    let req = Request {
        method: "BAD METHOD".to_string(),
        url: "http://127.0.0.1:0/x".to_string(),
        ..Request::default()
    };
    let err = client().send(&req).await.unwrap_err();
    match err {
        HttpError::InvalidMethod(m) => assert_eq!(m, "BAD METHOD"),
        other => panic!("expected InvalidMethod, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_url_errors() {
    let req = Request {
        method: "GET".to_string(),
        url: "::::not a url".to_string(),
        ..Request::default()
    };
    let err = client().send(&req).await.unwrap_err();
    assert!(matches!(err, HttpError::InvalidUrl(_, _)), "got {err:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn missing_multipart_file_errors_from_send() {
    let req = post(
        "http://127.0.0.1:1/m",
        "",
        Body::MultipartForm(vec![MultipartField {
            name: "doc".into(),
            value: MultipartValue::File("C:\\nope-bru-http\\missing.bin".into()),
            content_type: None,
            enabled: true,
        }]),
    );
    let err = client().send(&req).await.unwrap_err();
    assert!(matches!(err, HttpError::FileRead(_, _)), "got {err:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn connection_refused_is_request_error() {
    // Port 1 on loopback: nothing listens → connect fails → HttpError::Request.
    let req = Request {
        method: "GET".to_string(),
        url: "http://127.0.0.1:1/x".to_string(),
        auth: Auth::None,
        ..Request::default()
    };
    let err = client().send(&req).await.unwrap_err();
    assert!(matches!(err, HttpError::Request(_)), "got {err:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn response_headers_are_collected() {
    let (base, server) = capture_server(r#"{"ok":1}"#);
    let req = Request {
        method: "GET".to_string(),
        url: format!("{base}/h"),
        auth: Auth::None,
        ..Request::default()
    };
    let resp = client().send(&req).await.unwrap();
    let _ = server.join().unwrap();
    let has_ct = resp
        .headers
        .iter()
        .any(|(k, v)| k.eq_ignore_ascii_case("content-type") && v.contains("application/json"));
    assert!(has_ct, "headers: {:?}", resp.headers);
    assert!(resp.json().is_some());
}
