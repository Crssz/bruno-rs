//! bru-script — a QuickJS Safe-Mode sandbox for pre-request, post-response, and
//! test scripts.
//!
//! The Rust↔JS boundary is deliberately tiny: the host marshals state in and out
//! as JSON (`__vars`/`__req`/`__res` in, `__vars`/`__tests`/`__console` out) and
//! a JS [prelude](prelude.js) builds the `bru` / `req` / `res` / `test` /
//! `expect` / `pm` API on top. The sandbox has no filesystem, network, process,
//! or `require()` — exactly Bruno's default Safe Mode.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rquickjs::{CatchResultExt, Context, Ctx, Function, Runtime, Value};
use serde_json::json;

const PRELUDE: &str = include_str!("prelude.js");
const REQUIRE_JS: &str = include_str!("require.js");

/// Read state back as JSON, resiliently: each field is stringified independently
/// with a BigInt-safe replacer and a try/catch, so a script poisoning one field
/// (a cycle or BigInt assigned directly to `__vars`) can't wipe the others.
const READBACK: &str = "(function(){\
  function rep(k,v){ return (typeof v === 'bigint') ? v.toString() : v; }\
  function safe(o){ try { return JSON.parse(JSON.stringify(o, rep)); } catch (e) { return undefined; } }\
  return JSON.stringify({ vars: safe(__vars), tests: safe(__tests), console: safe(__console) });\
})()";

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
#[derive(Debug, Clone, Default)]
pub struct ScriptInput {
    pub vars: HashMap<String, String>,
    pub request: ScriptRequest,
    /// `None` for pre-request scripts (no response yet).
    pub response: Option<ScriptResponse>,
    /// Directory of the request `.bru`, used to resolve `require('./x')` paths.
    /// `None` disables relative resolution (require errors if attempted).
    pub script_dir: Option<PathBuf>,
    /// Enable CommonJS `require()` of local files (Bruno's Developer Mode). When
    /// `false` (Safe Mode default) `require` is not defined at all.
    pub allow_require: bool,
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

    // Developer Mode: install CommonJS `require()` for local files. Safe Mode
    // (the default) skips this, so `require` stays undefined.
    if input.allow_require {
        install_require(ctx, input.script_dir.as_deref())?;
    }

    // Evaluate the user script as an async unit (`promise: true`) so top-level
    // `await` is valid — matching how Bruno wraps scripts. `finish()` pumps the
    // job queue until the resulting promise settles. A syntax error surfaces from
    // `eval_promise`; a runtime throw/rejection surfaces from `finish`. Both are
    // captured (non-fatal); test failures are recorded in `__tests` by `test()`.
    let script_error = match ctx.eval_promise(source).catch(ctx) {
        Ok(promise) => promise.finish::<Value>().catch(ctx).err().map(|e| e.to_string()),
        Err(e) => Some(e.to_string()),
    };
    // Drain any microtasks the script scheduled but did not await.
    while ctx.execute_pending_job() {}

    let readback: String = ctx
        .eval::<String, _>(READBACK)
        .catch(ctx)
        .map_err(|e| format!("readback error: {e}"))?;

    let mut outcome = parse_readback(&readback);
    outcome.error = script_error;
    Ok(outcome)
}

/// Install the Developer-Mode `require()`: a Rust loader host fn plus the JS shim
/// ([require.js]) that builds the CommonJS module wrapper on top of it.
fn install_require(ctx: &Ctx<'_>, script_dir: Option<&Path>) -> Result<(), String> {
    let g = ctx.globals();
    let dir = script_dir
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    g.set("__scriptDir", dir)
        .map_err(|e| format!("inject __scriptDir: {e}"))?;

    let loader = Function::new(
        ctx.clone(),
        |ctx: Ctx<'_>, from: String, spec: String| -> rquickjs::Result<String> {
            match load_module(&from, &spec) {
                Ok(json_str) => Ok(json_str),
                Err(msg) => {
                    let s = rquickjs::String::from_str(ctx.clone(), &msg)?;
                    Err(ctx.throw(s.into()))
                }
            }
        },
    )
    .map_err(|e| format!("define require loader: {e}"))?;
    g.set("__bru_load_module", loader)
        .map_err(|e| format!("inject require loader: {e}"))?;

    ctx.eval::<Value, _>(REQUIRE_JS)
        .catch(ctx)
        .map_err(|e| format!("require shim error: {e}"))?;
    Ok(())
}

/// Resolve and read a module for `require(spec)` issued from directory `from`.
/// Returns a JSON string `{path, dir, source, json}`. Only relative (`./`,`../`)
/// or absolute paths are honoured — bare specifiers (npm packages) are rejected,
/// since there is no `node_modules` resolution in this sandbox.
fn load_module(from: &str, spec: &str) -> Result<String, String> {
    let is_abs = Path::new(spec).is_absolute();
    if !(spec.starts_with("./") || spec.starts_with("../") || is_abs) {
        return Err(format!(
            "Cannot find module '{spec}': only relative ('./', '../') or absolute paths are supported (no npm packages)"
        ));
    }
    if from.is_empty() && !is_abs {
        return Err(format!(
            "Cannot resolve '{spec}': the request's directory is unknown"
        ));
    }
    let joined = Path::new(from).join(spec);
    let resolved =
        resolve_file(&joined).ok_or_else(|| format!("Cannot find module '{spec}' from '{from}'"))?;
    let source = std::fs::read_to_string(&resolved)
        .map_err(|e| format!("Cannot read module '{}': {e}", resolved.display()))?;
    let is_json = resolved.extension().and_then(|e| e.to_str()) == Some("json");
    let dir = resolved
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let path = resolved.to_string_lossy().into_owned();
    Ok(json!({ "path": path, "dir": dir, "source": source, "json": is_json }).to_string())
}

/// Node-style file resolution: exact path, then `.js`/`.json` appended, then
/// `index.js` in a directory. Canonicalizes so the cache key is stable.
fn resolve_file(p: &Path) -> Option<PathBuf> {
    let canon = |c: PathBuf| c.is_file().then(|| std::fs::canonicalize(&c).unwrap_or(c));
    if let Some(hit) = canon(p.to_path_buf()) {
        return Some(hit);
    }
    for ext in ["js", "json"] {
        let mut s = p.as_os_str().to_os_string();
        s.push(".");
        s.push(ext);
        if let Some(hit) = canon(PathBuf::from(s)) {
            return Some(hit);
        }
    }
    canon(p.join("index.js"))
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
            script_dir: None,
            allow_require: false,
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
    fn async_test_is_not_silently_passed() {
        let out = run_script(
            "test('a', async function(){ throw new Error('x'); });",
            &input(&[], None),
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.tests.len(), 1);
        assert!(!out.tests[0].passed, "a failing async test must not pass");
    }

    #[test]
    fn poisoned_vars_does_not_wipe_test_results() {
        // A circular value assigned straight to __vars must not lose test output.
        let out = run_script(
            "var o = {}; o.self = o; __vars.bad = o; test('t', function(){ expect(1).to.equal(1); });",
            &input(&[], None),
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.tests.len(), 1);
        assert!(out.tests[0].passed);
    }

    #[test]
    fn top_level_await_is_supported() {
        // The exact shape that produced `expecting ';'` before async evaluation.
        let src = "const v = await Promise.resolve('hi'); bru.setVar('out', v);";
        let out = run_script(src, &input(&[], None));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.vars.get("out").map(String::as_str), Some("hi"));
    }

    #[test]
    fn require_is_absent_in_safe_mode() {
        let out = run_script("var h = require('./hook');", &input(&[], None));
        assert!(out
            .error
            .as_deref()
            .unwrap_or("")
            .contains("require is not defined"));
    }

    #[test]
    fn require_loads_local_module_in_dev_mode() {
        // Write a throwaway module next to a fake request dir.
        let dir = std::env::temp_dir().join(format!("bru-req-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let hook = dir.join("hook.js");
        std::fs::write(
            &hook,
            "async function useOAPISetVar(){ bru.setVar('hooked', 'yes'); return 1; }\nmodule.exports = { useOAPISetVar };",
        )
        .unwrap();

        let mut inp = input(&[], None);
        inp.script_dir = Some(dir.clone());
        inp.allow_require = true;
        let src = "const { useOAPISetVar } = require('./hook');\nawait useOAPISetVar();";
        let out = run_script(src, &inp);

        let _ = std::fs::remove_file(&hook);
        let _ = std::fs::remove_dir(&dir);

        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.vars.get("hooked").map(String::as_str), Some("yes"));
    }

    #[test]
    fn require_rejects_bare_specifier() {
        let mut inp = input(&[], None);
        inp.allow_require = true;
        inp.script_dir = Some(std::env::temp_dir());
        let out = run_script("require('lodash');", &inp);
        assert!(
            out.error.as_deref().unwrap_or("").contains("npm packages"),
            "{:?}",
            out.error
        );
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
