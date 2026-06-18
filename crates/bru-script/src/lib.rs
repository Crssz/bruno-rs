//! bru-script — a QuickJS Safe-Mode sandbox for pre-request, post-response, and
//! test scripts.
//!
//! The Rust↔JS boundary is deliberately tiny: the host marshals state in and out
//! as JSON (`__vars`/`__req`/`__res` in, `__vars`/`__tests`/`__console` out) and
//! a JS [prelude](prelude.js) builds the `bru` / `req` / `res` / `test` /
//! `expect` / `pm` API on top. The sandbox has no filesystem, network, process,
//! or `require()` — exactly Bruno's default Safe Mode.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use rquickjs::{CatchResultExt, Context, Runtime, Value};
use serde_json::json;

const PRELUDE: &str = include_str!("prelude.js");

/// Resource limits enforced on a script run, so a runaway or hostile script
/// (infinite loop, unbounded allocation, deep recursion) can't hang or OOM the
/// host process.
#[derive(Debug, Clone, Copy)]
pub struct ScriptLimits {
    pub timeout: Duration,
    pub memory_bytes: usize,
    pub max_stack_bytes: usize,
}

impl Default for ScriptLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(5),
            memory_bytes: 64 * 1024 * 1024,
            max_stack_bytes: 512 * 1024,
        }
    }
}

/// The request fields a script can read via `req` / `pm`.
#[derive(Debug, Clone, Default)]
pub struct ScriptRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
}

/// The response fields a script can read via `res` / `pm.response` (post scripts).
#[derive(Debug, Clone)]
pub struct ScriptResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    /// Parsed JSON body when the response was JSON, else the body text.
    pub body: serde_json::Value,
    pub response_time_ms: u128,
}

/// Everything a script run starts from.
#[derive(Debug, Clone)]
pub struct ScriptInput {
    pub vars: HashMap<String, String>,
    pub request: ScriptRequest,
    /// `None` for pre-request scripts (no response yet).
    pub response: Option<ScriptResponse>,
}

/// One `test(name, fn)` result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub error: Option<String>,
}

/// What a script run produced.
#[derive(Debug, Clone, Default)]
pub struct ScriptOutcome {
    /// Variables after the script ran (with its mutations applied).
    pub vars: HashMap<String, String>,
    pub tests: Vec<TestResult>,
    pub console: Vec<String>,
    /// An uncaught error thrown by the script (test failures are in `tests`).
    pub error: Option<String>,
}

/// Run `source` in a fresh sandbox seeded from `input`, with default limits.
pub fn run_script(source: &str, input: &ScriptInput) -> ScriptOutcome {
    run_script_with_limits(source, input, ScriptLimits::default())
}

/// Run `source` with explicit resource limits.
pub fn run_script_with_limits(
    source: &str,
    input: &ScriptInput,
    limits: ScriptLimits,
) -> ScriptOutcome {
    let rt = match Runtime::new() {
        Ok(rt) => rt,
        Err(e) => return engine_error(input, format!("runtime init failed: {e}")),
    };
    rt.set_memory_limit(limits.memory_bytes);
    rt.set_max_stack_size(limits.max_stack_bytes);
    // Periodic interrupt check: abort once the deadline passes (guards against
    // infinite loops). QuickJS turns this into an uncatchable interrupt error.
    let deadline = Instant::now() + limits.timeout;
    rt.set_interrupt_handler(Some(Box::new(move || Instant::now() >= deadline)));

    let ctx = match Context::full(&rt) {
        Ok(ctx) => ctx,
        Err(e) => return engine_error(input, format!("context init failed: {e}")),
    };

    ctx.with(|ctx| match run_in_context(&ctx, source, input) {
        Ok(outcome) => outcome,
        Err(msg) => engine_error(input, msg),
    })
}

fn run_in_context(
    ctx: &rquickjs::Ctx<'_>,
    source: &str,
    input: &ScriptInput,
) -> Result<ScriptOutcome, String> {
    let g = ctx.globals();
    let set = |k: &str, v: String| g.set(k, v).map_err(|e| format!("inject {k}: {e}"));
    set("__varsJson", json!(input.vars).to_string())?;
    set("__reqJson", request_json(&input.request).to_string())?;
    set(
        "__resJson",
        response_json(input.response.as_ref()).to_string(),
    )?;

    ctx.eval::<Value, _>(PRELUDE)
        .catch(ctx)
        .map_err(|e| format!("prelude error: {e}"))?;

    // A thrown error from the user script is captured (not fatal); test failures
    // are recorded inside `__tests` by the prelude's `test()`.
    let script_error = ctx
        .eval::<Value, _>(source)
        .catch(ctx)
        .err()
        .map(|e| e.to_string());

    let readback: String = ctx
        .eval::<String, _>("JSON.stringify({vars:__vars,tests:__tests,console:__console})")
        .catch(ctx)
        .map_err(|e| format!("readback error: {e}"))?;

    let mut outcome = parse_readback(&readback);
    outcome.error = script_error;
    Ok(outcome)
}

fn request_json(req: &ScriptRequest) -> serde_json::Value {
    json!({
        "method": req.method,
        "url": req.url,
        "headers": req.headers.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
    })
}

fn response_json(res: Option<&ScriptResponse>) -> serde_json::Value {
    match res {
        None => serde_json::Value::Null,
        Some(r) => json!({
            "status": r.status,
            "statusText": r.status_text,
            "headers": r.headers.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
            "body": r.body,
            "responseTime": r.response_time_ms as u64,
        }),
    }
}

fn parse_readback(json_str: &str) -> ScriptOutcome {
    let root: serde_json::Value = serde_json::from_str(json_str).unwrap_or_default();

    let vars = root
        .get("vars")
        .and_then(|v| v.as_object())
        .map(|map| {
            map.iter()
                .map(|(k, v)| (k.clone(), var_to_string(v)))
                .collect()
        })
        .unwrap_or_default();

    let tests = root
        .get("tests")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .map(|t| TestResult {
                    name: t
                        .get("name")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string(),
                    passed: t.get("passed").and_then(|p| p.as_bool()).unwrap_or(false),
                    error: t.get("error").and_then(|e| e.as_str()).map(str::to_string),
                })
                .collect()
        })
        .unwrap_or_default();

    let console = root
        .get("console")
        .and_then(|c| c.as_array())
        .map(|arr| arr.iter().map(var_to_string).collect())
        .unwrap_or_default();

    ScriptOutcome {
        vars,
        tests,
        console,
        error: None,
    }
}

/// Coerce a read-back JSON value into the string form a variable holds.
fn var_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    }
}

fn engine_error(input: &ScriptInput, msg: String) -> ScriptOutcome {
    ScriptOutcome {
        vars: input.vars.clone(),
        tests: Vec::new(),
        console: Vec::new(),
        error: Some(msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(vars: &[(&str, &str)], response: Option<ScriptResponse>) -> ScriptInput {
        ScriptInput {
            vars: vars
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            request: ScriptRequest {
                method: "GET".into(),
                url: "https://api.test/x".into(),
                headers: vec![("accept".into(), "application/json".into())],
            },
            response,
        }
    }

    #[test]
    fn pre_request_sets_var() {
        let out = run_script("bru.setVar('token', 'abc' + 123);", &input(&[], None));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.vars.get("token").map(String::as_str), Some("abc123"));
    }

    #[test]
    fn tests_and_expect_pass_and_fail() {
        let res = ScriptResponse {
            status: 200,
            status_text: "OK".into(),
            headers: vec![("content-type".into(), "application/json".into())],
            body: serde_json::json!({"id": 7, "name": "ada"}),
            response_time_ms: 5,
        };
        let src = r#"
            test("status is 200", function () { expect(res.status).to.equal(200); });
            test("has id", function () { expect(res.body.id).to.equal(7); });
            test("this fails", function () { expect(res.body.name).to.equal("bob"); });
            console.log("done", res.body.id);
        "#;
        let out = run_script(src, &input(&[], Some(res)));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.tests.len(), 3);
        assert!(out.tests[0].passed && out.tests[1].passed);
        assert!(!out.tests[2].passed);
        assert!(out.console.iter().any(|l| l.contains("done 7")));
    }

    #[test]
    fn pm_shim_works() {
        let res = ScriptResponse {
            status: 201,
            status_text: "Created".into(),
            headers: vec![],
            body: serde_json::json!({"ok": true}),
            response_time_ms: 1,
        };
        let src = r#"
            pm.test("created", function () { pm.response.to.have.status(201); });
            pm.environment.set("flag", pm.response.json().ok);
        "#;
        let out = run_script(src, &input(&[], Some(res)));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert!(out.tests[0].passed, "{:?}", out.tests);
        assert_eq!(out.vars.get("flag").map(String::as_str), Some("true"));
    }

    #[test]
    fn uncaught_error_is_captured_not_fatal() {
        let out = run_script("throw new Error('boom');", &input(&[], None));
        assert!(out.error.as_deref().unwrap_or("").contains("boom"));
    }

    #[test]
    fn infinite_loop_is_interrupted() {
        let limits = ScriptLimits {
            timeout: Duration::from_millis(150),
            ..Default::default()
        };
        let start = Instant::now();
        let out = run_script_with_limits("while (true) {}", &input(&[], None), limits);
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "runaway script should be interrupted quickly, took {:?}",
            start.elapsed()
        );
        assert!(out.error.is_some(), "expected an interrupt error");
    }
}
