//! Blocking request/folder runners executed on worker threads over a fresh
//! tokio runtime. The caller bridges results back to gpui's foreground.

use crate::RunResult;
use bru_core::BruFile;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
/// Run a request to completion on a fresh tokio runtime (called on a worker
/// thread). Returns the formatted response or an error string.
pub fn run_blocking(
    file: BruFile,
    dir: PathBuf,
    script_dir: Option<PathBuf>,
    opts: bru_http::SendOptions,
    global_vars: HashMap<String, String>,
    env: Option<String>,
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
