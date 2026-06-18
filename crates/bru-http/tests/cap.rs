//! Response-body size cap (DoS / decompression-bomb guard).

use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use bru_core::Request;
use bru_http::{HttpClient, HttpError, SendOptions};

fn mock(body_len: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let body = "x".repeat(body_len);
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    format!("http://{addr}")
}

#[tokio::test(flavor = "current_thread")]
async fn oversized_body_is_rejected() {
    let base = mock(10_000);
    let client = HttpClient::new(&SendOptions {
        max_response_bytes: 1000,
        ..SendOptions::default()
    })
    .unwrap();
    let req = Request {
        method: "GET".to_string(),
        url: base,
        ..Request::default()
    };
    let err = client.send(&req).await.unwrap_err();
    assert!(matches!(err, HttpError::BodyTooLarge(1000)), "got {err:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn within_cap_succeeds() {
    let base = mock(500);
    let client = HttpClient::new(&SendOptions {
        max_response_bytes: 1000,
        ..SendOptions::default()
    })
    .unwrap();
    let req = Request {
        method: "GET".to_string(),
        url: base,
        ..Request::default()
    };
    let resp = client.send(&req).await.unwrap();
    assert_eq!(resp.status, 200);
    assert_eq!(resp.body.len(), 500);
}
