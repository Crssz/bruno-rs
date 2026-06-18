//! Integration test: invoke the real `bru` binary against a mock collection.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;
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

/// A mock server that serves `n` connections and reports how many requests it
/// actually saw over the returned channel (so a test can assert the iteration
/// count drove the right number of sends).
fn counting_mock_server(body: &'static str, n: usize) -> (u16, mpsc::Receiver<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut seen = 0usize;
        for _ in 0..n {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut buf = [0u8; 2048];
                    let _ = stream.read(&mut buf);
                    seen += 1;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(resp.as_bytes());
                }
                Err(_) => break,
            }
        }
        let _ = tx.send(seen);
    });
    (port, rx)
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
        .args([dir.to_str().unwrap()])
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
        .args([dir.to_str().unwrap()])
        .output()
        .expect("run bru");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(!output.status.success());
    assert!(stdout.contains("0 passed, 1 failed"), "stdout:\n{stdout}");
}

/// Write a one-request collection whose URL interpolates `{{id}}` so each data
/// row hits a distinct path. Returns the collection dir.
fn data_collection(port: u16) -> PathBuf {
    let dir = unique_dir(port);
    std::fs::write(
        dir.join("bruno.json"),
        r#"{"version":"1","name":"T","type":"collection"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("item.bru"),
        format!(
            "meta {{\n  name: Item\n  type: http\n  seq: 1\n}}\n\nget {{\n  url: http://127.0.0.1:{port}/item/{{{{id}}}}\n  auth: none\n}}\n\nassert {{\n  res.status: 200\n}}\n"
        ),
    )
    .unwrap();
    dir
}

#[test]
fn json_data_runs_once_per_row() {
    let (port, rx) = counting_mock_server(r#"{"ok":true}"#, 3);
    let dir = data_collection(port);
    let data = dir.join("data.json");
    std::fs::write(&data, r#"[{"id":1},{"id":2},{"id":3}]"#).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bru"))
        .args([dir.to_str().unwrap(), "--data", data.to_str().unwrap()])
        .output()
        .expect("run bru");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let seen = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(output.status.success(), "stdout:\n{stdout}");
    assert_eq!(
        seen, 3,
        "mock should have seen 3 requests; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("3 iterations, 3 passed, 0 failed"),
        "stdout:\n{stdout}"
    );
}

#[test]
fn csv_data_runs_once_per_row() {
    let (port, rx) = counting_mock_server(r#"{"ok":true}"#, 2);
    let dir = data_collection(port);
    let data = dir.join("data.csv");
    std::fs::write(&data, "id,name\n10,alice\n20,bob\n").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bru"))
        .args([dir.to_str().unwrap(), "--data", data.to_str().unwrap()])
        .output()
        .expect("run bru");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let seen = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(output.status.success(), "stdout:\n{stdout}");
    assert_eq!(
        seen, 2,
        "mock should have seen 2 requests; stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("2 iterations, 2 passed, 0 failed"),
        "stdout:\n{stdout}"
    );
}
