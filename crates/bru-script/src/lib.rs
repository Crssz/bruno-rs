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
        Ok(promise) => promise
            .finish::<Value>()
            .catch(ctx)
            .err()
            .map(|e| e.to_string()),
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
    let resolved = resolve_file(&joined)
        .ok_or_else(|| format!("Cannot find module '{spec}' from '{from}'"))?;
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

    // ---- temp-dir helper with a Drop guard (no `tempfile` crate available) ----

    use std::sync::atomic::{AtomicU32, Ordering};

    static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

    /// A unique temp directory cleaned up on drop.
    struct TempDir {
        path: PathBuf,
    }
    impl TempDir {
        fn new() -> Self {
            let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("bru-script-test-{}-{}", std::process::id(), n));
            std::fs::create_dir_all(&path).unwrap();
            TempDir { path }
        }
        fn file(&self, name: &str, contents: &str) -> PathBuf {
            let p = self.path.join(name);
            std::fs::write(&p, contents).unwrap();
            p
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    // ---- bru.* variable API -------------------------------------------------

    #[test]
    fn bru_get_has_delete_var() {
        let src = r#"
            bru.setVar('seeded', bru.getVar('initial'));
            bru.setVar('present', bru.hasVar('initial'));
            bru.setVar('absent', bru.hasVar('nope'));
            bru.deleteVar('initial');
            bru.setVar('afterDelete', bru.hasVar('initial'));
        "#;
        let out = run_script(src, &input(&[("initial", "hi")], None));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.vars.get("seeded").map(String::as_str), Some("hi"));
        assert_eq!(out.vars.get("present").map(String::as_str), Some("true"));
        assert_eq!(out.vars.get("absent").map(String::as_str), Some("false"));
        assert_eq!(
            out.vars.get("afterDelete").map(String::as_str),
            Some("false")
        );
        assert!(!out.vars.contains_key("initial"));
    }

    #[test]
    fn bru_env_and_process_aliases() {
        let src = r#"
            bru.setEnvVar('e', 'envval');
            bru.setVar('readEnv', bru.getEnvVar('e'));
            bru.setVar('readProc', bru.getProcessEnv('e'));
            bru.setVar('safe', bru.isSafeMode());
        "#;
        let out = run_script(src, &input(&[], None));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.vars.get("readEnv").map(String::as_str), Some("envval"));
        assert_eq!(out.vars.get("readProc").map(String::as_str), Some("envval"));
        assert_eq!(out.vars.get("safe").map(String::as_str), Some("true"));
    }

    #[test]
    fn set_var_coerces_undefined_null_and_types() {
        // undefined/null become '' ; other types stringify.
        let src = r#"
            bru.setVar('u');                 // undefined -> ''
            bru.setVar('n', null);           // null -> ''
            bru.setVar('b', true);           // bool -> 'true'
            bru.setVar('num', 42);           // number -> '42'
            bru.setVar('obj', {a:1});        // object -> String({a:1})
        "#;
        let out = run_script(src, &input(&[], None));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.vars.get("u").map(String::as_str), Some(""));
        assert_eq!(out.vars.get("n").map(String::as_str), Some(""));
        assert_eq!(out.vars.get("b").map(String::as_str), Some("true"));
        assert_eq!(out.vars.get("num").map(String::as_str), Some("42"));
        assert_eq!(
            out.vars.get("obj").map(String::as_str),
            Some("[object Object]")
        );
    }

    // ---- var_to_string read-back coercions ----------------------------------

    #[test]
    fn var_to_string_covers_all_json_kinds() {
        // Assign non-string JSON kinds directly onto __vars so read-back hits
        // every var_to_string arm: bool, number, null, array/object (`other`).
        let src = r#"
            __vars.bt = true;
            __vars.nm = 3.5;
            __vars.nl = null;
            __vars.arr = [1, 2, 3];
            __vars.objv = { k: 'v' };
            __vars.str = 'plain';
        "#;
        let out = run_script(src, &input(&[], None));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.vars.get("bt").map(String::as_str), Some("true"));
        assert_eq!(out.vars.get("nm").map(String::as_str), Some("3.5"));
        assert_eq!(out.vars.get("nl").map(String::as_str), Some(""));
        assert_eq!(out.vars.get("arr").map(String::as_str), Some("[1,2,3]"));
        assert_eq!(
            out.vars.get("objv").map(String::as_str),
            Some(r#"{"k":"v"}"#)
        );
        assert_eq!(out.vars.get("str").map(String::as_str), Some("plain"));
    }

    // ---- req / res getters --------------------------------------------------

    #[test]
    fn req_getters_and_header_lookup() {
        let src = r#"
            bru.setVar('url', req.getUrl());
            bru.setVar('m', req.getMethod());
            bru.setVar('acc', req.getHeader('ACCEPT'));        // case-insensitive
            bru.setVar('missing', String(req.getHeader('x-none')));
            bru.setVar('hcount', req.getHeaders().length);
            bru.setVar('purl', req.url);
            bru.setVar('pm', req.method);
        "#;
        let out = run_script(src, &input(&[], None));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(
            out.vars.get("url").map(String::as_str),
            Some("https://api.test/x")
        );
        assert_eq!(out.vars.get("m").map(String::as_str), Some("GET"));
        assert_eq!(
            out.vars.get("acc").map(String::as_str),
            Some("application/json")
        );
        assert_eq!(
            out.vars.get("missing").map(String::as_str),
            Some("undefined")
        );
        assert_eq!(out.vars.get("hcount").map(String::as_str), Some("1"));
        assert_eq!(
            out.vars.get("purl").map(String::as_str),
            Some("https://api.test/x")
        );
    }

    #[test]
    fn res_is_undefined_for_pre_request() {
        let out = run_script("bru.setVar('hasRes', String(res));", &input(&[], None));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(
            out.vars.get("hasRes").map(String::as_str),
            Some("undefined")
        );
    }

    #[test]
    fn res_getters_and_header_lookup() {
        let res = ScriptResponse {
            status: 200,
            status_text: "OK".into(),
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: serde_json::json!({"x": 1}),
            response_time_ms: 12,
        };
        let src = r#"
            bru.setVar('st', res.getStatus());
            bru.setVar('stp', res.status);
            bru.setVar('stt', res.statusText);
            bru.setVar('body', JSON.stringify(res.getBody()));
            bru.setVar('bodyp', JSON.stringify(res.body));
            bru.setVar('ct', res.getHeader('content-type'));
            bru.setVar('miss', String(res.getHeader('nope')));
            bru.setVar('rt', res.getResponseTime());
            bru.setVar('rtp', res.responseTime);
            bru.setVar('hc', res.getHeaders().length);
        "#;
        let out = run_script(src, &input(&[], Some(res)));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.vars.get("st").map(String::as_str), Some("200"));
        assert_eq!(out.vars.get("stt").map(String::as_str), Some("OK"));
        assert_eq!(out.vars.get("body").map(String::as_str), Some(r#"{"x":1}"#));
        assert_eq!(
            out.vars.get("ct").map(String::as_str),
            Some("application/json")
        );
        assert_eq!(out.vars.get("miss").map(String::as_str), Some("undefined"));
        assert_eq!(out.vars.get("rt").map(String::as_str), Some("12"));
    }

    #[test]
    fn header_get_with_no_headers_returns_undefined() {
        // res with empty header list: __headerGet loops zero times -> undefined.
        let res = ScriptResponse {
            status: 204,
            status_text: "No Content".into(),
            headers: vec![],
            body: serde_json::Value::Null,
            response_time_ms: 0,
        };
        let out = run_script(
            "bru.setVar('h', String(res.getHeader('any')));",
            &input(&[], Some(res)),
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.vars.get("h").map(String::as_str), Some("undefined"));
    }

    // ---- console capture ----------------------------------------------------

    #[test]
    fn console_levels_and_formatting() {
        let src = r#"
            console.log('a', 1, true);
            console.info('info-line');
            console.warn('warn-line');
            console.error('error-line');
            console.debug('debug-line');
            console.log({k: 'v'});            // object -> JSON
            var c = {}; c.self = c;
            console.log(c);                   // cyclic -> String() fallback
        "#;
        let out = run_script(src, &input(&[], None));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert!(out.console.iter().any(|l| l == "a 1 true"));
        assert!(out.console.iter().any(|l| l == "info-line"));
        assert!(out.console.iter().any(|l| l == "warn-line"));
        assert!(out.console.iter().any(|l| l == "error-line"));
        assert!(out.console.iter().any(|l| l == "debug-line"));
        assert!(out.console.iter().any(|l| l == r#"{"k":"v"}"#));
        assert!(out.console.iter().any(|l| l.contains("object")));
    }

    // ---- expect matchers ----------------------------------------------------

    #[test]
    fn expect_matchers_pass_paths() {
        let src = r#"
            test('eql', function(){ expect({a:1}).to.eql({a:1}); });
            test('equals', function(){ expect(2).to.equals(2); });
            test('above/gt', function(){ expect(5).to.be.above(3); expect(5).to.be.gt(3); });
            test('below/lt', function(){ expect(2).to.be.below(3); expect(2).to.be.lt(3); });
            test('least', function(){ expect(3).to.be.least(3); });
            test('most', function(){ expect(3).to.be.most(3); });
            test('include string', function(){ expect('hello').to.include('ell'); });
            test('include array', function(){ expect([1,2,3]).to.contain(2); });
            test('include obj key', function(){ expect({k:1}).to.include('k'); });
            test('include obj val', function(){ expect({k:9}).to.include(9); });
            test('match', function(){ expect('abc123').to.match(/\d+/); });
            test('property', function(){ expect({p:1}).to.have.property('p'); });
            test('lengthOf', function(){ expect([1,2]).to.have.lengthOf(2); });
            test('oneOf', function(){ expect(2).to.be.oneOf([1,2,3]); });
            test('true', function(){ expect(true).to.be.true; });
            test('false', function(){ expect(false).to.be.false; });
            test('null', function(){ expect(null).to.be.null; });
            test('undefined', function(){ expect(undefined).to.be.undefined; });
            test('ok', function(){ expect(1).to.be.ok; });
            test('empty arr', function(){ expect([]).to.be.empty; });
            test('empty obj', function(){ expect({}).to.be.empty; });
            test('empty null', function(){ expect(null).to.be.empty; });
            test('not equal', function(){ expect(1).to.not.equal(2); });
            test('chains', function(){ expect(1).to.be.been.is.that.which.has.have.with.and.a.an.equal(1); });
        "#;
        let out = run_script(src, &input(&[], None));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert!(
            out.tests.iter().all(|t| t.passed),
            "all should pass: {:?}",
            out.tests.iter().filter(|t| !t.passed).collect::<Vec<_>>()
        );
        assert!(out.tests.len() >= 24);
    }

    #[test]
    fn expect_matchers_fail_paths() {
        let src = r#"
            test('eql fail', function(){ expect({a:1}).to.eql({a:2}); });
            test('above fail', function(){ expect(1).to.be.above(3); });
            test('below fail', function(){ expect(5).to.be.below(3); });
            test('least fail', function(){ expect(1).to.be.least(3); });
            test('most fail', function(){ expect(5).to.be.most(3); });
            test('include str fail', function(){ expect('hi').to.include('xyz'); });
            test('include null throws', function(){ expect(null).to.include('x'); });
            test('include number throws', function(){ expect(5).to.include('x'); });
            test('match fail', function(){ expect('abc').to.match(/\d+/); });
            test('property fail', function(){ expect({}).to.have.property('p'); });
            test('property null fail', function(){ expect(null).to.have.property('p'); });
            test('lengthOf fail', function(){ expect([1]).to.have.lengthOf(3); });
            test('lengthOf null fail', function(){ expect(null).to.have.lengthOf(0); });
            test('oneOf fail', function(){ expect(9).to.be.oneOf([1,2]); });
            test('true fail', function(){ expect(false).to.be.true; });
            test('false fail', function(){ expect(true).to.be.false; });
            test('null fail', function(){ expect(1).to.be.null; });
            test('undefined fail', function(){ expect(1).to.be.undefined; });
            test('ok fail', function(){ expect(0).to.be.ok; });
            test('empty fail', function(){ expect([1]).to.be.empty; });
            test('not equal fail', function(){ expect(1).to.not.equal(1); });
        "#;
        let out = run_script(src, &input(&[], None));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert!(
            out.tests.iter().all(|t| !t.passed),
            "all should fail: {:?}",
            out.tests.iter().filter(|t| t.passed).collect::<Vec<_>>()
        );
        // The throwing-include cases must carry a descriptive message.
        assert!(out
            .tests
            .iter()
            .any(|t| t.error.as_deref().unwrap_or("").contains("null")));
    }

    // ---- pm.* shim ----------------------------------------------------------

    #[test]
    fn pm_variable_collections_and_response_helpers() {
        let res = ScriptResponse {
            status: 200,
            status_text: "OK".into(),
            headers: vec![("X-Token".into(), "t1".into())],
            body: serde_json::json!({"a": 1}),
            response_time_ms: 7,
        };
        let src = r#"
            pm.variables.set('v1', 'a');
            pm.globals.set('g1', pm.variables.get('v1'));
            pm.collectionVariables.set('c1', pm.variables.has('v1'));
            bru.setVar('vhas', pm.variables.has('v1'));
            bru.setVar('code', pm.response.code);
            bru.setVar('rstatus', pm.response.status);
            bru.setVar('rtime', pm.response.responseTime);
            bru.setVar('jb', JSON.stringify(pm.response.json()));
            bru.setVar('txt', pm.response.text());
            pm.test('be ok', function(){ pm.response.to.be.ok; });
            pm.test('status num', function(){ pm.response.to.have.status(200); });
            pm.test('status text', function(){ pm.response.to.have.status('OK'); });
            pm.test('header present', function(){ pm.response.to.have.header('x-token'); });
            pm.test('json body', function(){ pm.expect(pm.response.to.have.jsonBody().a).to.equal(1); });
        "#;
        let out = run_script(src, &input(&[], Some(res)));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.vars.get("g1").map(String::as_str), Some("a"));
        assert_eq!(out.vars.get("c1").map(String::as_str), Some("true"));
        assert_eq!(out.vars.get("code").map(String::as_str), Some("200"));
        assert_eq!(out.vars.get("rstatus").map(String::as_str), Some("OK"));
        assert_eq!(out.vars.get("jb").map(String::as_str), Some(r#"{"a":1}"#));
        assert_eq!(out.vars.get("txt").map(String::as_str), Some(r#"{"a":1}"#));
        assert!(out.tests.iter().all(|t| t.passed), "{:?}", out.tests);
    }

    #[test]
    fn pm_response_text_passthrough_for_string_body() {
        let res = ScriptResponse {
            status: 200,
            status_text: "OK".into(),
            headers: vec![],
            body: serde_json::Value::String("plain text".into()),
            response_time_ms: 1,
        };
        let out = run_script(
            "bru.setVar('t', pm.response.text());",
            &input(&[], Some(res)),
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.vars.get("t").map(String::as_str), Some("plain text"));
    }

    #[test]
    fn pm_response_failure_branches() {
        let res = ScriptResponse {
            status: 404,
            status_text: "Not Found".into(),
            headers: vec![],
            body: serde_json::Value::Null,
            response_time_ms: 1,
        };
        let src = r#"
            pm.test('not ok', function(){ pm.response.to.be.ok; });
            pm.test('wrong status num', function(){ pm.response.to.have.status(200); });
            pm.test('wrong status text', function(){ pm.response.to.have.status('OK'); });
            pm.test('missing header', function(){ pm.response.to.have.header('x-none'); });
        "#;
        let out = run_script(src, &input(&[], Some(res)));
        assert!(out.error.is_none(), "{:?}", out.error);
        assert!(out.tests.iter().all(|t| !t.passed), "{:?}", out.tests);
        assert!(out
            .tests
            .iter()
            .any(|t| t.error.as_deref().unwrap_or("").contains("404")));
    }

    #[test]
    fn pm_response_is_undefined_in_pre_request() {
        let out = run_script(
            "bru.setVar('hasPmRes', String(pm.response));",
            &input(&[], None),
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(
            out.vars.get("hasPmRes").map(String::as_str),
            Some("undefined")
        );
    }

    // ---- test() function edge cases -----------------------------------------

    #[test]
    fn sync_async_test_function_is_rejected() {
        // A fn returning a thenable from a SYNC test must be rejected (not awaited).
        let out = run_script(
            "test('thenable', function(){ return { then: function(){} }; });",
            &input(&[], None),
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.tests.len(), 1);
        assert!(!out.tests[0].passed);
        assert!(out.tests[0]
            .error
            .as_deref()
            .unwrap_or("")
            .contains("async test functions are not supported"));
    }

    #[test]
    fn test_error_without_message_stringifies_thrown_value() {
        // Throw a non-Error (no `.message`) so the catch uses String(e).
        let out = run_script(
            "test('throws string', function(){ throw 'raw failure'; });",
            &input(&[], None),
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.tests.len(), 1);
        assert!(!out.tests[0].passed);
        assert_eq!(out.tests[0].error.as_deref(), Some("raw failure"));
    }

    // ---- top-level await / async edge cases ---------------------------------

    #[test]
    fn rejected_top_level_await_is_captured() {
        let out = run_script(
            "await Promise.reject(new Error('rejected-tla'));",
            &input(&[], None),
        );
        assert!(out.error.as_deref().unwrap_or("").contains("rejected-tla"));
    }

    #[test]
    fn syntax_error_is_captured() {
        let out = run_script("this is not valid javascript ===", &input(&[], None));
        assert!(out.error.is_some(), "a syntax error must be captured");
    }

    #[test]
    fn unawaited_microtask_still_drains() {
        // A promise scheduled but not awaited must still run before read-back.
        let out = run_script(
            "Promise.resolve().then(function(){ bru.setVar('drained', 'yes'); });",
            &input(&[], None),
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.vars.get("drained").map(String::as_str), Some("yes"));
    }

    // ---- require() resolution -----------------------------------------------

    #[test]
    fn require_resolves_json_module() {
        let dir = TempDir::new();
        dir.file("data.json", r#"{"answer": 42}"#);
        let mut inp = input(&[], None);
        inp.script_dir = Some(dir.path.clone());
        inp.allow_require = true;
        let out = run_script(
            "var d = require('./data.json'); bru.setVar('a', d.answer);",
            &inp,
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.vars.get("a").map(String::as_str), Some("42"));
    }

    #[test]
    fn require_appends_js_extension() {
        let dir = TempDir::new();
        dir.file(
            "lib.js",
            "module.exports = { hi: function(){ return 'yo'; } };",
        );
        let mut inp = input(&[], None);
        inp.script_dir = Some(dir.path.clone());
        inp.allow_require = true;
        // Note: no extension on the specifier -> resolve_file appends `.js`.
        let out = run_script("var m = require('./lib'); bru.setVar('r', m.hi());", &inp);
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.vars.get("r").map(String::as_str), Some("yo"));
    }

    #[test]
    fn require_resolves_directory_index() {
        let dir = TempDir::new();
        let pkg = dir.path.join("pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("index.js"), "module.exports = { name: 'pkg' };").unwrap();
        let mut inp = input(&[], None);
        inp.script_dir = Some(dir.path.clone());
        inp.allow_require = true;
        let out = run_script("var p = require('./pkg'); bru.setVar('n', p.name);", &inp);
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.vars.get("n").map(String::as_str), Some("pkg"));
    }

    #[test]
    fn require_caches_and_resolves_nested_relative() {
        let dir = TempDir::new();
        let sub = dir.path.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        // dep.js increments a side-effect counter so a cache hit is observable.
        std::fs::write(
            sub.join("dep.js"),
            "globalThis.__depLoads = (globalThis.__depLoads||0)+1; module.exports = { v: 1 };",
        )
        .unwrap();
        // mid.js requires its sibling via a nested relative path.
        std::fs::write(
            sub.join("mid.js"),
            "var d = require('./dep'); module.exports = { dep: d };",
        )
        .unwrap();
        let mut inp = input(&[], None);
        inp.script_dir = Some(dir.path.clone());
        inp.allow_require = true;
        let src = r#"
            var a = require('./sub/mid');
            var b = require('./sub/dep');   // already cached by mid -> no re-exec
            bru.setVar('loads', String(globalThis.__depLoads));
            bru.setVar('same', String(a.dep === b));
        "#;
        let out = run_script(src, &inp);
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.vars.get("loads").map(String::as_str), Some("1"));
        assert_eq!(out.vars.get("same").map(String::as_str), Some("true"));
    }

    #[test]
    fn require_absolute_path() {
        let dir = TempDir::new();
        let modp = dir.file("abs.js", "module.exports = { x: 'absolute' };");
        let abs = modp.to_string_lossy().replace('\\', "\\\\");
        let mut inp = input(&[], None);
        // script_dir empty path but absolute specifier should still resolve.
        inp.script_dir = Some(dir.path.clone());
        inp.allow_require = true;
        let out = run_script(
            &format!("var m = require('{abs}'); bru.setVar('v', m.x);"),
            &inp,
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.vars.get("v").map(String::as_str), Some("absolute"));
    }

    #[test]
    fn require_missing_file_errors() {
        let dir = TempDir::new();
        let mut inp = input(&[], None);
        inp.script_dir = Some(dir.path.clone());
        inp.allow_require = true;
        let out = run_script("require('./does-not-exist');", &inp);
        assert!(
            out.error
                .as_deref()
                .unwrap_or("")
                .contains("Cannot find module"),
            "{:?}",
            out.error
        );
    }

    #[test]
    fn require_unknown_dir_errors_for_relative() {
        // allow_require but no script_dir -> `from` empty + relative spec rejected.
        let mut inp = input(&[], None);
        inp.script_dir = None;
        inp.allow_require = true;
        let out = run_script("require('./x');", &inp);
        assert!(
            out.error
                .as_deref()
                .unwrap_or("")
                .contains("directory is unknown"),
            "{:?}",
            out.error
        );
    }

    #[test]
    fn load_module_rejects_bare_specifier_directly() {
        let err = load_module("/some/dir", "express").unwrap_err();
        assert!(err.contains("npm packages"), "{err}");
    }

    #[test]
    fn load_module_unknown_dir_for_relative() {
        let err = load_module("", "./thing").unwrap_err();
        assert!(err.contains("directory is unknown"), "{err}");
    }

    #[test]
    fn load_module_reads_json_flag_and_paths() {
        let dir = TempDir::new();
        dir.file("cfg.json", r#"{"k":1}"#);
        let json_str = load_module(&dir.path.to_string_lossy(), "./cfg.json").unwrap();
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(v.get("json").and_then(|j| j.as_bool()), Some(true));
        assert!(v
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap()
            .ends_with("cfg.json"));
        assert!(v
            .get("source")
            .and_then(|s| s.as_str())
            .unwrap()
            .contains("\"k\":1"));
    }

    #[test]
    fn load_module_js_is_not_flagged_json() {
        let dir = TempDir::new();
        dir.file("a.js", "module.exports = 1;");
        let json_str = load_module(&dir.path.to_string_lossy(), "./a.js").unwrap();
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(v.get("json").and_then(|j| j.as_bool()), Some(false));
    }

    #[test]
    fn load_module_not_found() {
        let dir = TempDir::new();
        let err = load_module(&dir.path.to_string_lossy(), "./missing").unwrap_err();
        assert!(err.contains("Cannot find module"), "{err}");
    }

    // ---- resolve_file -------------------------------------------------------

    #[test]
    fn resolve_file_exact_extensions_and_index() {
        let dir = TempDir::new();
        // exact file
        let exact = dir.file("exact.txt", "x");
        assert_eq!(
            resolve_file(&exact).map(|p| p.is_file()),
            Some(true),
            "exact path must resolve"
        );
        // .js appended
        dir.file("withjs.js", "x");
        assert!(resolve_file(&dir.path.join("withjs")).is_some());
        // .json appended
        dir.file("withjson.json", "{}");
        assert!(resolve_file(&dir.path.join("withjson")).is_some());
        // index.js in a directory
        let sub = dir.path.join("idx");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("index.js"), "x").unwrap();
        assert!(resolve_file(&sub).is_some());
        // nothing found
        assert!(resolve_file(&dir.path.join("nope")).is_none());
    }

    // ---- request_json / response_json ---------------------------------------

    #[test]
    fn request_json_includes_headers() {
        let req = ScriptRequest {
            method: "POST".into(),
            url: "http://h/".into(),
            headers: vec![("a".into(), "b".into()), ("c".into(), "d".into())],
        };
        let v = request_json(&req);
        assert_eq!(v["method"], "POST");
        assert_eq!(v["headers"][0][0], "a");
        assert_eq!(v["headers"][1][1], "d");
    }

    #[test]
    fn response_json_none_is_null() {
        assert_eq!(response_json(None), serde_json::Value::Null);
    }

    #[test]
    fn response_json_some_maps_fields() {
        let r = ScriptResponse {
            status: 418,
            status_text: "I'm a teapot".into(),
            headers: vec![("h".into(), "v".into())],
            body: serde_json::json!([1, 2]),
            response_time_ms: 999,
        };
        let v = response_json(Some(&r));
        assert_eq!(v["status"], 418);
        assert_eq!(v["statusText"], "I'm a teapot");
        assert_eq!(v["headers"][0][0], "h");
        assert_eq!(v["responseTime"], 999u64);
        assert_eq!(v["body"], serde_json::json!([1, 2]));
    }

    // ---- parse_readback -----------------------------------------------------

    #[test]
    fn parse_readback_invalid_json_is_default() {
        let out = parse_readback("not json at all {");
        assert!(out.vars.is_empty());
        assert!(out.tests.is_empty());
        assert!(out.console.is_empty());
        assert!(out.error.is_none());
    }

    #[test]
    fn parse_readback_missing_fields_default_empty() {
        let out = parse_readback("{}");
        assert!(out.vars.is_empty());
        assert!(out.tests.is_empty());
        assert!(out.console.is_empty());
    }

    #[test]
    fn parse_readback_test_field_defaults() {
        // A test object missing name/passed/error must default sensibly.
        let json = r#"{
            "vars": {"k": "v"},
            "tests": [ {}, {"name":"n","passed":true,"error":"boom"} ],
            "console": ["line"]
        }"#;
        let out = parse_readback(json);
        assert_eq!(out.vars.get("k").map(String::as_str), Some("v"));
        assert_eq!(out.tests.len(), 2);
        assert_eq!(out.tests[0].name, "");
        assert!(!out.tests[0].passed);
        assert_eq!(out.tests[0].error, None);
        assert_eq!(out.tests[1].name, "n");
        assert!(out.tests[1].passed);
        assert_eq!(out.tests[1].error.as_deref(), Some("boom"));
        assert_eq!(out.console, vec!["line".to_string()]);
    }

    // ---- limits / engine_error ----------------------------------------------

    #[test]
    fn memory_limit_failure_is_captured_not_fatal() {
        let limits = ScriptLimits {
            memory_bytes: 1024 * 1024,
            ..Default::default()
        };
        // Allocate well beyond the cap; QuickJS surfaces an out-of-memory throw.
        let src = "var a = []; while (true) { a.push(new Array(100000).fill(0)); }";
        let out = run_script_with_limits(src, &input(&[("keep", "me")], limits_marker()), limits);
        // Either an OOM error or an interrupt; both must be non-fatal and captured.
        assert!(out.error.is_some(), "expected a captured error");
        // engine_error / readback both preserve nothing fatal; vars survive seeding.
        assert!(out.tests.is_empty());
    }

    /// Helper so the memory test still passes a response slot (None) clearly.
    fn limits_marker() -> Option<ScriptResponse> {
        None
    }

    #[test]
    fn engine_error_preserves_seeded_vars() {
        let inp = input(&[("preserved", "1")], None);
        let out = engine_error(&inp, "synthetic failure".into());
        assert_eq!(out.vars.get("preserved").map(String::as_str), Some("1"));
        assert_eq!(out.error.as_deref(), Some("synthetic failure"));
        assert!(out.tests.is_empty());
        assert!(out.console.is_empty());
    }

    #[test]
    fn run_script_with_explicit_default_limits_round_trips() {
        let out = run_script_with_limits(
            "bru.setVar('ok', 'yes');",
            &input(&[], None),
            ScriptLimits::default(),
        );
        assert!(out.error.is_none(), "{:?}", out.error);
        assert_eq!(out.vars.get("ok").map(String::as_str), Some("yes"));
    }
}
