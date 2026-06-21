//! Blocking request/folder runners executed on worker threads over a fresh
//! tokio runtime. The caller bridges results back to gpui's foreground.

use crate::RunResult;
use bru_core::BruFile;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
/// Run a request to completion on a fresh tokio runtime (called on a worker
/// thread). Returns the formatted response or an error string.
#[allow(clippy::too_many_arguments)]
pub fn run_blocking(
    file: BruFile,
    dir: PathBuf,
    script_dir: Option<PathBuf>,
    opts: bru_http::SendOptions,
    global_vars: HashMap<String, String>,
    env: Option<String>,
    developer: bool,
) -> bru_engine::RunOutcome {
    let errout = |e: String| bru_engine::RunOutcome::errored("request".to_string(), e);
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => return errout(e.to_string()),
    };
    rt.block_on(async move {
        let client = match bru_http::HttpClient::new(&opts) {
            Ok(c) => c,
            Err(e) => return errout(e.to_string()),
        };
        // Vault secrets are the base layer; collection + selected-env vars override.
        let mut vars = global_vars;
        for (k, v) in bru_engine::base_vars(&dir, env.as_deref()) {
            vars.insert(k, v);
        }
        let mut ctx = bru_engine::RunContext {
            vars,
            client,
            send_options: opts,
            script_dir,
            developer_mode: developer,
            ..Default::default()
        };
        bru_engine::run_request(&file, &mut ctx).await
    })
}
/// Run request files sequentially through one shared RunContext (Bruno's folder
/// runner) on a fresh tokio runtime. Worker thread.
pub fn run_folder_blocking(
    files: Vec<PathBuf>,
    vars_base: PathBuf,
    opts: bru_http::SendOptions,
    global_vars: HashMap<String, String>,
    env: Option<String>,
    developer: bool,
) -> Vec<RunResult> {
    let err_row = |name: &str, e: String| RunResult {
        name: name.to_string(),
        passed: false,
        status: 0,
        ms: 0,
        error: Some(e),
    };
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => return vec![err_row("runtime", e.to_string())],
    };
    rt.block_on(async move {
        let client = match bru_http::HttpClient::new(&opts) {
            Ok(c) => c,
            Err(e) => return vec![err_row("client", e.to_string())],
        };
        let mut vars = global_vars;
        for (k, v) in bru_engine::base_vars(&vars_base, env.as_deref()) {
            vars.insert(k, v);
        }
        let mut ctx = bru_engine::RunContext {
            vars,
            client,
            send_options: opts,
            developer_mode: developer,
            ..Default::default()
        };
        let mut results = Vec::new();
        for path in files {
            ctx.script_dir = path.parent().map(Path::to_path_buf);
            let fname = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("request")
                .to_string();
            let file = match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|t| bru_lang::parse(&t).map_err(|e| e.to_string()))
            {
                Ok(f) => f,
                Err(e) => {
                    results.push(err_row(&fname, e));
                    continue;
                }
            };
            if file.to_request().is_none() {
                continue; // skip non-HTTP .bru
            }
            let outcome = bru_engine::run_request(&file, &mut ctx).await;
            let status = outcome.response.as_ref().map(|r| r.status).unwrap_or(0);
            let ms = outcome
                .response
                .as_ref()
                .map(|r| r.duration_ms)
                .unwrap_or(0);
            let passed = outcome.error.is_none()
                && outcome.assertions.iter().all(|a| a.passed)
                && outcome.tests.iter().all(|t| t.passed);
            results.push(RunResult {
                name: outcome.name.clone(),
                passed,
                status,
                ms,
                error: outcome.error.clone(),
            });
        }
        results
    })
}

#[cfg(test)]
mod cov_tests {
    use super::*;

    /// Parse a `.bru` source into a `BruFile`. The runner consumes a parsed file.
    fn parse(src: &str) -> BruFile {
        bru_lang::parse(src).expect("test .bru parses")
    }

    /// A request `.bru` whose URL is malformed, so `client.send` errors inside the
    /// runtime with no network round-trip (matches the engine's own
    /// `run_request_invalid_url_surfaces_error` fixture).
    const INVALID_URL_REQ: &str =
        "meta {\n  name: U\n  type: http\n}\n\nget {\n  url: not a url\n  auth: none\n}\n";

    /// A `.bru` with only a `meta` block (no method block) is not an HTTP request.
    const NON_REQUEST: &str = "meta {\n  name: NotARequest\n}\n";

    #[test]
    fn run_blocking_non_request_returns_errored_outcome() {
        let file = parse(NON_REQUEST);
        let out = run_blocking(
            file,
            std::env::temp_dir(),
            None,
            bru_http::SendOptions::default(),
            HashMap::new(),
            None,
            false,
        );
        // No method block -> errored without touching the network.
        assert!(out.error.is_some());
        assert_eq!(out.name, "NotARequest");
        assert!(out.response.is_none());
    }

    #[test]
    fn run_blocking_invalid_url_errors_without_network() {
        let file = parse(INVALID_URL_REQ);
        let out = run_blocking(
            file,
            std::env::temp_dir(),
            None,
            bru_http::SendOptions::default(),
            HashMap::new(),
            None,
            false,
        );
        assert!(out.error.is_some());
        assert_eq!(out.method, "GET");
        assert!(out.response.is_none());
    }

    #[test]
    fn run_blocking_honours_developer_flag_and_global_vars() {
        // developer=true and a populated global_vars map exercise the var-merge
        // and context-construction branches; the malformed URL still aborts pre-send.
        let mut vars = HashMap::new();
        vars.insert("token".to_string(), "abc".to_string());
        let file = parse(INVALID_URL_REQ);
        let out = run_blocking(
            file,
            std::env::temp_dir(),
            Some(std::env::temp_dir()),
            bru_http::SendOptions::default(),
            vars,
            Some("dev".to_string()),
            true,
        );
        assert!(out.error.is_some());
    }

    #[test]
    fn run_folder_blocking_empty_list_yields_no_rows() {
        let out = run_folder_blocking(
            Vec::new(),
            std::env::temp_dir(),
            bru_http::SendOptions::default(),
            HashMap::new(),
            None,
            false,
        );
        assert!(out.is_empty());
    }

    #[test]
    fn run_folder_blocking_missing_file_is_error_row() {
        let missing = std::env::temp_dir().join("bru-runner-cov-does-not-exist.bru");
        let out = run_folder_blocking(
            vec![missing],
            std::env::temp_dir(),
            bru_http::SendOptions::default(),
            HashMap::new(),
            None,
            false,
        );
        assert_eq!(out.len(), 1);
        assert!(!out[0].passed);
        assert!(out[0].error.is_some());
        assert_eq!(out[0].status, 0);
        assert_eq!(out[0].ms, 0);
        // The row is named from the file stem.
        assert_eq!(out[0].name, "bru-runner-cov-does-not-exist");
    }

    #[test]
    fn run_folder_blocking_unparseable_file_is_error_row() {
        let dir = std::env::temp_dir().join(format!(
            "bru-runner-cov-bad-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Broken.bru");
        // An unterminated block is a parse error.
        std::fs::write(&path, "meta {\n  name: Broken\n").unwrap();

        let out = run_folder_blocking(
            vec![path],
            dir.clone(),
            bru_http::SendOptions::default(),
            HashMap::new(),
            None,
            false,
        );
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(out.len(), 1);
        assert!(!out[0].passed);
        assert!(out[0].error.is_some());
        assert_eq!(out[0].name, "Broken");
    }

    #[test]
    fn run_folder_blocking_skips_non_request_files() {
        let dir = std::env::temp_dir().join(format!(
            "bru-runner-cov-skip-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("VarsOnly.bru");
        // A vars-only / meta-only file parses but is not an HTTP request, so it's
        // silently skipped (no result row).
        std::fs::write(&path, NON_REQUEST).unwrap();

        let out = run_folder_blocking(
            vec![path],
            dir.clone(),
            bru_http::SendOptions::default(),
            HashMap::new(),
            None,
            false,
        );
        let _ = std::fs::remove_dir_all(&dir);

        assert!(out.is_empty());
    }

    #[test]
    fn run_folder_blocking_invalid_url_request_is_failed_row() {
        let dir = std::env::temp_dir().join(format!(
            "bru-runner-cov-req-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("BadUrl.bru");
        std::fs::write(&path, INVALID_URL_REQ).unwrap();

        let out = run_folder_blocking(
            vec![path],
            dir.clone(),
            bru_http::SendOptions::default(),
            HashMap::new(),
            None,
            false,
        );
        let _ = std::fs::remove_dir_all(&dir);

        // One executed request that errored before the wire: failed row, no status.
        assert_eq!(out.len(), 1);
        assert!(!out[0].passed);
        assert!(out[0].error.is_some());
        assert_eq!(out[0].status, 0);
        assert_eq!(out[0].name, "U");
    }

    #[test]
    fn run_folder_blocking_mixed_batch_orders_skip_error_and_run() {
        let dir = std::env::temp_dir().join(format!(
            "bru-runner-cov-mixed-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let skip = dir.join("Skip.bru");
        std::fs::write(&skip, NON_REQUEST).unwrap();
        let bad = dir.join("Missing.bru"); // never written -> read error row
        let req = dir.join("Req.bru");
        std::fs::write(&req, INVALID_URL_REQ).unwrap();

        let out = run_folder_blocking(
            vec![skip, bad, req],
            dir.clone(),
            bru_http::SendOptions::default(),
            HashMap::new(),
            None,
            true,
        );
        let _ = std::fs::remove_dir_all(&dir);

        // Skip produces no row; the read-error and the invalid-url request each do.
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|r| !r.passed && r.error.is_some()));
        assert_eq!(out[0].name, "Missing");
        assert_eq!(out[1].name, "U");
    }
}
