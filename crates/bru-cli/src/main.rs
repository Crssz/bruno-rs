//! `bru` — headless CLI for running Bruno collections.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use bru_engine::{base_vars, run_request, RunContext, RunOutcome};
use bru_http::{HttpClient, SendOptions};
use clap::Parser;

#[derive(Parser)]
#[command(
    name = "bru",
    version,
    about = "Run Bruno API collections from the command line"
)]
struct RunArgs {
    /// Path to a `.bru` request file or a collection directory.
    path: PathBuf,
    /// Environment name to load from `<collection>/environments/<name>.bru`.
    #[arg(long)]
    env: Option<String>,
    /// Skip TLS certificate verification.
    #[arg(long)]
    insecure: bool,
    /// Per-request timeout in seconds.
    #[arg(long, default_value_t = 30)]
    timeout: u64,
    /// Data file (`.json` array of objects or `.csv`); run the targets once per
    /// row, with each row's fields injected as variables for that iteration.
    #[arg(long)]
    data: Option<PathBuf>,
    /// Number of iterations to run. Ignored when `--data` is given (the row
    /// count wins). Defaults to 1.
    #[arg(long)]
    iterations: Option<usize>,
    /// Developer Mode: let scripts `require()` local `.js` files (relative to the
    /// request). Off by default (Safe Mode — scripts have no filesystem access).
    #[arg(long)]
    developer: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    run(RunArgs::parse()).await
}

async fn run(args: RunArgs) -> ExitCode {
    let targets = match collect_targets(&args.path) {
        Ok(t) if !t.is_empty() => t,
        Ok(_) => {
            eprintln!("No requests found at {}", args.path.display());
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::FAILURE;
        }
    };

    if args.insecure {
        eprintln!(
            "WARNING: TLS certificate verification is DISABLED for all requests (--insecure)."
        );
    }
    let options = SendOptions {
        insecure: args.insecure,
        timeout: Duration::from_secs(args.timeout),
        ..SendOptions::default()
    };
    let client = match HttpClient::new(&options) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::FAILURE;
        }
    };

    // A dataset, if given, drives the iteration count and injects per-row vars.
    let dataset = match args.data.as_deref().map(load_dataset).transpose() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let iterations = match &dataset {
        Some(rows) => rows.len(),
        None => args.iterations.unwrap_or(1),
    };
    if let Some(rows) = &dataset {
        if rows.is_empty() {
            eprintln!("Error: data file has no rows");
            return ExitCode::FAILURE;
        }
    }

    let base = base_vars(&args.path, args.env.as_deref());
    let multi = iterations > 1 || dataset.is_some();

    let (mut passed, mut failed) = (0u32, 0u32);
    for iter in 0..iterations {
        if multi {
            println!("\n=== iteration {}/{iterations} ===", iter + 1);
        }
        // Each iteration starts from the base vars (collection + env), overlaid
        // with this row's data, in a fresh context so iterations don't leak vars.
        let mut vars = base.clone();
        if let Some(rows) = &dataset {
            for (k, v) in &rows[iter] {
                vars.insert(k.clone(), v.clone());
            }
        }
        let mut ctx = RunContext {
            vars,
            client: client.clone(),
            send_options: options.clone(),
            developer_mode: args.developer,
            ..Default::default()
        };

        for target in &targets {
            // Resolve each request's `require('./x')` relative to its own folder.
            ctx.script_dir = target.parent().map(Path::to_path_buf);
            let text = match std::fs::read_to_string(target) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("skip {}: {e}", target.display());
                    failed += 1;
                    continue;
                }
            };
            let file = match bru_lang::parse(&text) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("skip {}: parse error: {e}", target.display());
                    failed += 1;
                    continue;
                }
            };
            let outcome = run_request(&file, &mut ctx).await;
            if outcome.passed() {
                passed += 1;
            } else {
                failed += 1;
            }
            print_outcome(&outcome);
        }
    }

    let prefix = if multi {
        format!("{iterations} iterations, ")
    } else {
        String::new()
    };
    println!("\n{prefix}{passed} passed, {failed} failed");
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// A dataset is a list of iterations; each iteration is a list of key/value vars.
type Dataset = Vec<Vec<(String, String)>>;

/// Load a data file as a list of per-iteration variable rows. JSON files must be
/// an array of objects; CSV files use the header row as keys. All values are
/// stringified (JSON numbers/bools become their literal text; strings drop their
/// quotes).
fn load_dataset(path: &Path) -> Result<Dataset, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let is_csv = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("csv"));
    if is_csv {
        load_csv(&text)
    } else {
        load_json(&text)
    }
}

fn load_json(text: &str) -> Result<Dataset, String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("data: invalid JSON: {e}"))?;
    let arr = value
        .as_array()
        .ok_or("data: JSON must be an array of objects")?;
    let mut rows = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let obj = item
            .as_object()
            .ok_or_else(|| format!("data: row {i} is not an object"))?;
        let row = obj
            .iter()
            .map(|(k, v)| (k.clone(), json_to_string(v)))
            .collect();
        rows.push(row);
    }
    Ok(rows)
}

/// Stringify a JSON value for use as a variable: strings unquoted, scalars by
/// their literal text, and any nested object/array by its compact JSON.
fn json_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn load_csv(text: &str) -> Result<Dataset, String> {
    let mut reader = csv::Reader::from_reader(text.as_bytes());
    let headers = reader
        .headers()
        .map_err(|e| format!("data: invalid CSV header: {e}"))?
        .iter()
        .map(|h| h.to_string())
        .collect::<Vec<_>>();
    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(|e| format!("data: invalid CSV row: {e}"))?;
        let row = headers
            .iter()
            .cloned()
            .zip(record.iter().map(|s| s.to_string()))
            .collect();
        rows.push(row);
    }
    Ok(rows)
}

/// Resolve which `.bru` files to run: a single file, or every request in a
/// collection directory (in sidebar order).
fn collect_targets(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    if path.is_dir() {
        let tree = bru_lang::load_collection(path)?;
        let mut out = Vec::new();
        flatten(&tree.root, &mut out);
        Ok(out)
    } else {
        Ok(vec![path.to_path_buf()])
    }
}

fn flatten(folder: &bru_core::Folder, out: &mut Vec<PathBuf>) {
    for req in &folder.requests {
        out.push(req.path.clone());
    }
    for sub in &folder.folders {
        flatten(sub, out);
    }
}

fn print_outcome(o: &RunOutcome) {
    println!("\n> {}  {} {}", o.name, o.method, o.url);
    if let Some(err) = &o.error {
        println!("  ERROR: {err}");
        return;
    }
    if let Some(resp) = &o.response {
        println!(
            "  {} {}   {} ms   {} bytes",
            resp.status,
            resp.status_text,
            resp.duration_ms,
            resp.body.len()
        );
    }
    for a in &o.assertions {
        let mark = if a.passed { "PASS" } else { "FAIL" };
        let extra = if a.passed {
            String::new()
        } else {
            format!("  (actual: {})", a.actual)
        };
        println!("  [{mark}] {} {} {}{extra}", a.expr, a.operator, a.expected);
    }
    for t in &o.tests {
        let mark = if t.passed { "PASS" } else { "FAIL" };
        match &t.error {
            Some(e) if !t.passed => println!("  [{mark}] test: {}  ({e})", t.name),
            _ => println!("  [{mark}] test: {}", t.name),
        }
    }
    for line in &o.console {
        println!("  | {line}");
    }
    for (k, v) in &o.vars_set {
        println!("  set var {k} = {}", redact(k, v));
    }
}

/// Mask values captured into secret-looking variable names so tokens don't land
/// in CI logs or shell history.
fn redact(name: &str, value: &str) -> String {
    const SECRETISH: &[&str] = &["token", "secret", "password", "passwd", "key", "auth"];
    let lower = name.to_lowercase();
    if SECRETISH.iter().any(|s| lower.contains(s)) {
        "***".to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bru_core::AssertOutcome;
    use bru_engine::TestResult;
    use bru_http::HttpResponse;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A temp directory cleaned up on drop. Unique per test via pid + counter.
    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let p = std::env::temp_dir().join(format!(
                "bru-cli-unit-{tag}-{}-{}",
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

    // ---- load_json ----------------------------------------------------------

    #[test]
    fn load_json_valid_array_of_objects() {
        let rows = load_json(r#"[{"id":1,"name":"a"},{"id":2,"name":"b"}]"#).unwrap();
        assert_eq!(rows.len(), 2);
        // Order within a row is not guaranteed (serde_json object iteration), so
        // check by lookup rather than position.
        let find = |row: &Vec<(String, String)>, k: &str| {
            row.iter().find(|(rk, _)| rk == k).map(|(_, v)| v.clone())
        };
        assert_eq!(find(&rows[0], "id"), Some("1".to_string()));
        assert_eq!(find(&rows[0], "name"), Some("a".to_string()));
        assert_eq!(find(&rows[1], "id"), Some("2".to_string()));
        assert_eq!(find(&rows[1], "name"), Some("b".to_string()));
    }

    #[test]
    fn load_json_empty_array_ok() {
        let rows = load_json("[]").unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn load_json_invalid_json_errors() {
        let err = load_json("{not json").unwrap_err();
        assert!(err.contains("invalid JSON"), "{err}");
    }

    #[test]
    fn load_json_non_array_errors() {
        let err = load_json(r#"{"id":1}"#).unwrap_err();
        assert!(err.contains("must be an array"), "{err}");
    }

    #[test]
    fn load_json_row_not_object_errors() {
        let err = load_json(r#"[{"ok":1}, 42]"#).unwrap_err();
        assert!(err.contains("row 1 is not an object"), "{err}");
    }

    // ---- json_to_string -----------------------------------------------------

    #[test]
    fn json_to_string_each_kind() {
        use serde_json::json;
        assert_eq!(json_to_string(&json!("hello")), "hello");
        assert_eq!(json_to_string(&serde_json::Value::Null), "");
        assert_eq!(json_to_string(&json!(42)), "42");
        assert_eq!(json_to_string(&json!(3.5)), "3.5");
        assert_eq!(json_to_string(&json!(true)), "true");
        assert_eq!(json_to_string(&json!(false)), "false");
        // Nested object/array fall through to compact JSON text.
        assert_eq!(json_to_string(&json!({"a":1})), r#"{"a":1}"#);
        assert_eq!(json_to_string(&json!([1, 2])), "[1,2]");
    }

    // ---- load_csv -----------------------------------------------------------

    #[test]
    fn load_csv_header_and_rows() {
        let rows = load_csv("id,name\n10,alice\n20,bob\n").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            vec![
                ("id".to_string(), "10".to_string()),
                ("name".to_string(), "alice".to_string())
            ]
        );
        assert_eq!(
            rows[1],
            vec![
                ("id".to_string(), "20".to_string()),
                ("name".to_string(), "bob".to_string())
            ]
        );
    }

    #[test]
    fn load_csv_header_only_no_rows() {
        let rows = load_csv("id,name\n").unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn load_csv_malformed_row_errors() {
        // A record with more fields than the header makes the (non-flexible)
        // csv reader return an unequal-lengths error mid-iteration.
        let err = load_csv("id,name\n1,alice,extra\n").unwrap_err();
        assert!(err.contains("invalid CSV row"), "{err}");
    }

    // ---- collect_targets / flatten -----------------------------------------

    #[test]
    fn collect_targets_single_file() {
        let td = TempDir::new("single");
        let f = td.path().join("solo.bru");
        std::fs::write(&f, "meta {\n  name: Solo\n}\n").unwrap();
        let targets = collect_targets(&f).unwrap();
        assert_eq!(targets, vec![f]);
    }

    #[test]
    fn collect_targets_nonexistent_path_treated_as_file() {
        // A path that is not a dir is returned verbatim (existence is checked
        // later when the file is read).
        let p = PathBuf::from("does/not/exist.bru");
        let targets = collect_targets(&p).unwrap();
        assert_eq!(targets, vec![p]);
    }

    #[test]
    fn collect_targets_collection_dir_with_nested_folders() {
        let td = TempDir::new("coll");
        let root = td.path();
        std::fs::write(
            root.join("bruno.json"),
            r#"{"version":"1","name":"C","type":"collection"}"#,
        )
        .unwrap();
        // Top-level request (seq 1).
        std::fs::write(
            root.join("a.bru"),
            "meta {\n  name: A\n  type: http\n  seq: 1\n}\n\nget {\n  url: http://x/\n  auth: none\n}\n",
        )
        .unwrap();
        // Nested folder with one request.
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(
            sub.join("b.bru"),
            "meta {\n  name: B\n  type: http\n  seq: 1\n}\n\nget {\n  url: http://y/\n  auth: none\n}\n",
        )
        .unwrap();

        let targets = collect_targets(root).unwrap();
        // flatten emits this folder's requests first, then descends — so the
        // root request comes before the nested one.
        assert_eq!(targets.len(), 2);
        assert!(targets[0].ends_with("a.bru"), "{targets:?}");
        assert!(targets[1].ends_with("b.bru"), "{targets:?}");
    }

    #[test]
    fn flatten_walks_nested_folders_depth_first() {
        let leaf_req = |name: &str| bru_core::RequestItem {
            name: name.to_string(),
            path: PathBuf::from(format!("{name}.bru")),
            method: Some("GET".to_string()),
            seq: Some(1),
        };
        let inner = bru_core::Folder {
            name: "inner".to_string(),
            path: PathBuf::from("inner"),
            folders: vec![],
            requests: vec![leaf_req("deep")],
        };
        let root = bru_core::Folder {
            name: "root".to_string(),
            path: PathBuf::from("root"),
            folders: vec![inner],
            requests: vec![leaf_req("top")],
        };
        let mut out = Vec::new();
        flatten(&root, &mut out);
        assert_eq!(out.len(), 2);
        assert!(out[0].ends_with("top.bru"));
        assert!(out[1].ends_with("deep.bru"));
    }

    #[test]
    fn flatten_empty_folder_yields_nothing() {
        let folder = bru_core::Folder::default();
        let mut out = Vec::new();
        flatten(&folder, &mut out);
        assert!(out.is_empty());
    }

    // ---- load_dataset (dispatch by extension) -------------------------------

    #[test]
    fn load_dataset_dispatches_csv() {
        let td = TempDir::new("ds-csv");
        let f = td.path().join("rows.csv");
        std::fs::write(&f, "k\nv1\nv2\n").unwrap();
        let rows = load_dataset(&f).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], vec![("k".to_string(), "v1".to_string())]);
    }

    #[test]
    fn load_dataset_dispatches_csv_uppercase_ext() {
        let td = TempDir::new("ds-csv-up");
        let f = td.path().join("rows.CSV");
        std::fs::write(&f, "k\nv1\n").unwrap();
        let rows = load_dataset(&f).unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn load_dataset_dispatches_json() {
        let td = TempDir::new("ds-json");
        let f = td.path().join("rows.json");
        std::fs::write(&f, r#"[{"k":"v"}]"#).unwrap();
        let rows = load_dataset(&f).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], vec![("k".to_string(), "v".to_string())]);
    }

    #[test]
    fn load_dataset_no_extension_treated_as_json() {
        let td = TempDir::new("ds-noext");
        let f = td.path().join("data");
        std::fs::write(&f, r#"[{"k":"v"}]"#).unwrap();
        let rows = load_dataset(&f).unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn load_dataset_read_error_includes_path() {
        let missing = std::env::temp_dir().join(format!(
            "bru-cli-missing-{}-{}.json",
            std::process::id(),
            42
        ));
        let err = load_dataset(&missing).unwrap_err();
        // The error is prefixed with the path display.
        assert!(err.contains("bru-cli-missing"), "{err}");
    }

    // ---- redact -------------------------------------------------------------

    #[test]
    fn redact_masks_secretish_names() {
        for name in ["token", "secret", "password", "passwd", "key", "auth"] {
            assert_eq!(redact(name, "sensitive"), "***", "name = {name}");
        }
        // Case-insensitive and substring matching.
        assert_eq!(redact("API_TOKEN", "x"), "***");
        assert_eq!(redact("userPassword", "x"), "***");
        assert_eq!(redact("AuthHeader", "x"), "***");
    }

    #[test]
    fn redact_passes_through_normal_names() {
        assert_eq!(redact("username", "alice"), "alice");
        assert_eq!(redact("id", "42"), "42");
        assert_eq!(redact("", "anything"), "anything");
    }

    // ---- print_outcome (smoke: exercises every branch) ---------------------

    fn assertion(passed: bool) -> AssertOutcome {
        AssertOutcome {
            expr: "res.status".to_string(),
            operator: "eq".to_string(),
            expected: "200".to_string(),
            actual: if passed {
                "200".to_string()
            } else {
                "500".to_string()
            },
            passed,
        }
    }

    fn test_result(name: &str, passed: bool, error: Option<&str>) -> TestResult {
        TestResult {
            name: name.to_string(),
            passed,
            error: error.map(str::to_string),
        }
    }

    fn response() -> HttpResponse {
        HttpResponse {
            status: 200,
            status_text: "OK".to_string(),
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body: b"{\"ok\":true}".to_vec(),
            duration_ms: 12,
        }
    }

    #[test]
    fn print_outcome_error_branch_returns_early() {
        // An errored outcome prints the ERROR line and skips response/assertions.
        let mut o = RunOutcome::errored("Boom", "connection refused");
        o.method = "GET".to_string();
        o.url = "http://x/".to_string();
        // Even with assertions present, the error branch returns before them.
        o.assertions.push(assertion(false));
        print_outcome(&o); // must not panic
    }

    #[test]
    fn print_outcome_full_response_and_results() {
        let o = RunOutcome {
            name: "Full".to_string(),
            method: "POST".to_string(),
            url: "http://api/".to_string(),
            response: Some(response()),
            assertions: vec![assertion(true), assertion(false)],
            tests: vec![
                test_result("t-pass", true, None),
                test_result("t-fail", false, Some("expected 1 to be 2")),
                // A failed test with no error message hits the `_ =>` arm.
                test_result("t-fail-noerr", false, None),
                // A passed test that carries an error string also hits `_ =>`.
                test_result("t-pass-err", true, Some("ignored")),
            ],
            console: vec!["log line one".to_string(), "log line two".to_string()],
            vars_set: vec![
                ("userId".to_string(), "42".to_string()),
                ("authToken".to_string(), "should-be-masked".to_string()),
            ],
            error: None,
        };
        print_outcome(&o); // must not panic; exercises all loops + redact
    }

    #[test]
    fn print_outcome_no_response_no_results() {
        // No error, no response, empty assertion/test/console/vars lists: the
        // function should fall straight through without printing those blocks.
        let o = RunOutcome {
            name: "Bare".to_string(),
            method: "GET".to_string(),
            url: "http://bare/".to_string(),
            ..Default::default()
        };
        print_outcome(&o); // must not panic
    }

    #[test]
    fn outcome_passed_helper_reflects_state() {
        // Sanity-check the helper print_outcome leans on, across branches.
        let mut ok = RunOutcome::default();
        ok.assertions.push(assertion(true));
        ok.tests.push(test_result("t", true, None));
        assert!(ok.passed());

        let mut bad_assert = RunOutcome::default();
        bad_assert.assertions.push(assertion(false));
        assert!(!bad_assert.passed());

        let mut bad_test = RunOutcome::default();
        bad_test.tests.push(test_result("t", false, None));
        assert!(!bad_test.passed());

        let errored = RunOutcome::errored("n", "boom");
        assert!(!errored.passed());
    }
}
