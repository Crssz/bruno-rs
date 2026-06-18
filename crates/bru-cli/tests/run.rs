//! Integration test: invoke the real `bru` binary against a mock collection.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::thread;

fn mock_server(body: &'static str, status: &'static str) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        // Serve a couple of connections so a small collection run completes.
        for _ in 0..4 {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buf = [0u8; 2048];
                    let _ = stream.read(&mut buf);
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(resp.as_bytes());
                }
                Err(_) => break,
            }
        }
    });
    port
}

fn unique_dir(tag: u16) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("bru-cli-test-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn run_collection_reports_pass_and_exits_zero() {
    let port = mock_server(r#"{"ok":true}"#, "200 OK");
    let dir = unique_dir(port);

    std::fs::write(
        dir.join("bruno.json"),
        r#"{"version":"1","name":"T","type":"collection"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("ping.bru"),
        format!(
            "meta {{\n  name: Ping\n  type: http\n  seq: 1\n}}\n\nget {{\n  url: http://127.0.0.1:{port}/ping\n  auth: none\n}}\n\nassert {{\n  res.status: 200\n  res.body.ok: isTrue\n}}\n"
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bru"))
        .args(["run", dir.to_str().unwrap()])
        .output()
        .expect("run bru");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let _ = std::fs::remove_dir_all(&dir);

    assert!(output.status.success(), "stdout:\n{stdout}");
    assert!(stdout.contains("1 passed, 0 failed"), "stdout:\n{stdout}");
}

#[test]
fn failed_assertion_exits_nonzero() {
    let port = mock_server(r#"{"ok":false}"#, "500 Internal Server Error");
    let dir = unique_dir(port);

    std::fs::write(
        dir.join("bruno.json"),
        r#"{"version":"1","name":"T","type":"collection"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("boom.bru"),
        format!(
            "meta {{\n  name: Boom\n  type: http\n}}\n\nget {{\n  url: http://127.0.0.1:{port}/\n  auth: none\n}}\n\nassert {{\n  res.status: 200\n}}\n"
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bru"))
        .args(["run", dir.to_str().unwrap()])
        .output()
        .expect("run bru");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(!output.status.success());
    assert!(stdout.contains("0 passed, 1 failed"), "stdout:\n{stdout}");
}
