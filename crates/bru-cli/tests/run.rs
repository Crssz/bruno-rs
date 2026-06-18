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

#[test]
fn single_bru_file_path_argument() {
    // Point the CLI at one .bru file directly (not a collection dir). The
    // collect_targets single-file branch returns just that file.
    let port = mock_server(r#"{"ok":true}"#, "200 OK");
    let dir = unique_dir(port);
    let bru = dir.join("solo.bru");
    std::fs::write(
        &bru,
        format!(
            "meta {{\n  name: Solo\n  type: http\n}}\n\nget {{\n  url: http://127.0.0.1:{port}/solo\n  auth: none\n}}\n\nassert {{\n  res.status: 200\n}}\n"
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bru"))
        .args([bru.to_str().unwrap()])
        .output()
        .expect("run bru");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(output.status.success(), "stdout:\n{stdout}");
    assert!(stdout.contains("1 passed, 0 failed"), "stdout:\n{stdout}");
    // Single run: no iteration header / prefix.
    assert!(!stdout.contains("iteration"), "stdout:\n{stdout}");
}

#[test]
fn env_flag_loads_environment_vars() {
    // The env var supplies the host:port the request interpolates, proving the
    // --env path resolved the environment file under <collection>/environments.
    let port = mock_server(r#"{"ok":true}"#, "200 OK");
    let dir = unique_dir(port);
    std::fs::write(
        dir.join("bruno.json"),
        r#"{"version":"1","name":"T","type":"collection"}"#,
    )
    .unwrap();
    let envs = dir.join("environments");
    std::fs::create_dir_all(&envs).unwrap();
    std::fs::write(
        envs.join("local.bru"),
        format!("vars {{\n  base: http://127.0.0.1:{port}\n}}\n"),
    )
    .unwrap();
    std::fs::write(
        dir.join("ping.bru"),
        "meta {\n  name: Ping\n  type: http\n  seq: 1\n}\n\nget {\n  url: {{base}}/ping\n  auth: none\n}\n\nassert {\n  res.status: 200\n}\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bru"))
        .args([dir.to_str().unwrap(), "--env", "local"])
        .output()
        .expect("run bru");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(output.status.success(), "stdout:\n{stdout}");
    assert!(stdout.contains("1 passed, 0 failed"), "stdout:\n{stdout}");
}

#[test]
fn insecure_flag_emits_warning() {
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
            "meta {{\n  name: Ping\n  type: http\n  seq: 1\n}}\n\nget {{\n  url: http://127.0.0.1:{port}/ping\n  auth: none\n}}\n\nassert {{\n  res.status: 200\n}}\n"
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bru"))
        .args([dir.to_str().unwrap(), "--insecure"])
        .output()
        .expect("run bru");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(
        output.status.success(),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("TLS certificate verification is DISABLED"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn iterations_flag_runs_multiple_times() {
    // --iterations N (no --data) drives N runs and prints the multi prefix.
    let (port, rx) = counting_mock_server(r#"{"ok":true}"#, 3);
    let dir = unique_dir(port);
    std::fs::write(
        dir.join("bruno.json"),
        r#"{"version":"1","name":"T","type":"collection"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("ping.bru"),
        format!(
            "meta {{\n  name: Ping\n  type: http\n  seq: 1\n}}\n\nget {{\n  url: http://127.0.0.1:{port}/ping\n  auth: none\n}}\n\nassert {{\n  res.status: 200\n}}\n"
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bru"))
        .args([dir.to_str().unwrap(), "--iterations", "3"])
        .output()
        .expect("run bru");

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let seen = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(output.status.success(), "stdout:\n{stdout}");
    assert_eq!(seen, 3, "stdout:\n{stdout}");
    assert!(
        stdout.contains("3 iterations, 3 passed, 0 failed"),
        "stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("=== iteration 1/3 ==="),
        "stdout:\n{stdout}"
    );
}

#[test]
fn iterations_zero_is_rejected() {
    // Regression: `--iterations 0` ran nothing yet exited success (a false green).
    // It must now be rejected at parse time.
    let output = Command::new(env!("CARGO_BIN_EXE_bru"))
        .args([".", "--iterations", "0"])
        .output()
        .expect("run bru");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(stderr.contains("iterations"), "stderr:\n{stderr}");
}

#[test]
fn empty_data_file_exits_nonzero() {
    // A JSON data file that is an empty array has no rows -> error, no requests.
    let dir = unique_dir(60001);
    std::fs::write(
        dir.join("bruno.json"),
        r#"{"version":"1","name":"T","type":"collection"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("item.bru"),
        "meta {\n  name: Item\n  type: http\n  seq: 1\n}\n\nget {\n  url: http://127.0.0.1:1/item\n  auth: none\n}\n",
    )
    .unwrap();
    let data = dir.join("data.json");
    std::fs::write(&data, "[]").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bru"))
        .args([dir.to_str().unwrap(), "--data", data.to_str().unwrap()])
        .output()
        .expect("run bru");

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(!output.status.success(), "stderr:\n{stderr}");
    assert!(stderr.contains("no rows"), "stderr:\n{stderr}");
}

#[test]
fn invalid_data_file_exits_nonzero() {
    // Malformed JSON data file -> load_dataset error -> nonzero exit.
    let dir = unique_dir(60002);
    std::fs::write(
        dir.join("bruno.json"),
        r#"{"version":"1","name":"T","type":"collection"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("item.bru"),
        "meta {\n  name: Item\n  type: http\n  seq: 1\n}\n\nget {\n  url: http://127.0.0.1:1/item\n  auth: none\n}\n",
    )
    .unwrap();
    let data = dir.join("data.json");
    std::fs::write(&data, "{ not valid json").unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bru"))
        .args([dir.to_str().unwrap(), "--data", data.to_str().unwrap()])
        .output()
        .expect("run bru");

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(!output.status.success(), "stderr:\n{stderr}");
    assert!(stderr.contains("invalid JSON"), "stderr:\n{stderr}");
}

#[test]
fn empty_collection_dir_exits_nonzero() {
    // A collection dir with no request files -> "No requests found" -> failure.
    let dir = unique_dir(60003);
    std::fs::write(
        dir.join("bruno.json"),
        r#"{"version":"1","name":"T","type":"collection"}"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_bru"))
        .args([dir.to_str().unwrap()])
        .output()
        .expect("run bru");

    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let _ = std::fs::remove_dir_all(&dir);

    assert!(!output.status.success(), "stderr:\n{stderr}");
    assert!(stderr.contains("No requests found"), "stderr:\n{stderr}");
}
