//! bru-engine — the request lifecycle orchestrator.
//!
//! For one request: merge vars → pre-request script → interpolate → send →
//! evaluate assertions → extract post-response vars → post-response + test
//! scripts. Scripts run in the `bru-script` QuickJS Safe-Mode sandbox.

mod context;

pub use bru_script::TestResult;
pub use context::{base_vars, find_collection_root};

use std::collections::HashMap;

use bru_core::{
    eval_response_expr, evaluate_assertions, interpolate, AssertOutcome, Auth, Body, BruFile,
    KeyVal, Request, ResponseFacts,
};
use bru_http::{HttpClient, HttpResponse};
use bru_script::{run_script, ScriptInput, ScriptRequest, ScriptResponse};

/// Mutable run state: the merged variable map plus a reusable HTTP client.
///
/// Variable precedence, lowest to highest: collection vars → environment vars →
/// request pre-request vars → runtime/post-response captures (each inserted later
/// overrides earlier ones in `vars`).
#[derive(Debug, Clone, Default)]
pub struct RunContext {
    pub vars: HashMap<String, String>,
    pub client: HttpClient,
}

/// The result of running one request.
#[derive(Debug, Clone, Default)]
pub struct RunOutcome {
    pub name: String,
    pub method: String,
    pub url: String,
    pub response: Option<HttpResponse>,
    pub assertions: Vec<AssertOutcome>,
    /// Results from `test(...)` / `pm.test(...)` in scripts.
    pub tests: Vec<TestResult>,
    /// `console.log` output collected from scripts.
    pub console: Vec<String>,
    /// Variables captured from the response by `vars:post-response`.
    pub vars_set: Vec<(String, String)>,
    pub error: Option<String>,
}

impl RunOutcome {
    /// True when the request sent and every assertion and test passed.
    pub fn passed(&self) -> bool {
        self.error.is_none()
            && self.assertions.iter().all(|a| a.passed)
            && self.tests.iter().all(|t| t.passed)
    }

    /// An outcome representing a failure before the request could run.
    pub fn errored(name: impl Into<String>, message: impl Into<String>) -> Self {
        RunOutcome {
            name: name.into(),
            error: Some(message.into()),
            ..Default::default()
        }
    }
}

/// Run a single request file against the context, mutating its variable map with
/// any pre-request and post-response bindings (so chained requests see them).
pub async fn run_request(file: &BruFile, ctx: &mut RunContext) -> RunOutcome {
    let name = file.request_name().unwrap_or("(unnamed)").to_string();

    let Some(mut req) = file.to_request() else {
        return RunOutcome::errored(name, "not a request .bru (no method block)");
    };

    let mut tests = Vec::new();
    let mut console = Vec::new();

    // Pre-request vars feed interpolation of this very request. The `local`
    // (@) flag is preserved on the model but is a no-op here in M1: nothing is
    // persisted back to environment files, so every var is already run-scoped.
    for v in &req.vars_pre {
        if v.enabled {
            ctx.vars.insert(v.name.clone(), v.value.clone());
        }
    }

    // Pre-request script: may mutate vars before interpolation. An uncaught
    // error aborts the request (mirrors Bruno).
    if let Some(src) = file.script_pre() {
        let out = run_script(&src, &script_input(&ctx.vars, &req, None));
        ctx.vars = out.vars;
        console.extend(out.console);
        tests.extend(out.tests);
        if let Some(e) = out.error {
            let mut outcome = RunOutcome::errored(name, format!("pre-request script error: {e}"));
            outcome.tests = tests;
            outcome.console = console;
            return outcome;
        }
    }

    interpolate_request(&mut req, &ctx.vars);

    let resp = match ctx.client.send(&req).await {
        Ok(resp) => resp,
        Err(e) => {
            let mut outcome = RunOutcome::errored(name, e.to_string());
            outcome.method = req.method;
            outcome.url = req.url;
            outcome.tests = tests;
            outcome.console = console;
            return outcome;
        }
    };

    let json = resp.json();
    let text = resp.text();
    let facts = ResponseFacts {
        status: resp.status,
        headers: &resp.headers,
        body_json: json.as_ref(),
        body_text: &text,
        response_time_ms: resp.duration_ms,
    };
    let assertions = evaluate_assertions(&req.assertions, &facts);

    // Declarative post-response variable captures (`res.body.*` expressions).
    let mut vars_set = Vec::new();
    for v in &req.vars_post {
        if v.enabled {
            if let Some(value) = eval_response_expr(&v.value, &facts) {
                ctx.vars.insert(v.name.clone(), value.clone());
                vars_set.push((v.name.clone(), value));
            }
        }
    }

    // Post-response script + tests run together in one sandbox so they share scope.
    if let Some(src) = combine_post_scripts(file) {
        let sresp = script_response(&resp, &json, &text);
        let out = run_script(&src, &script_input(&ctx.vars, &req, Some(sresp)));
        ctx.vars = out.vars;
        console.extend(out.console);
        tests.extend(out.tests);
        if let Some(e) = out.error {
            tests.push(TestResult {
                name: "post-response script".to_string(),
                passed: false,
                error: Some(e),
            });
        }
    }

    RunOutcome {
        name,
        method: req.method,
        url: req.url,
        response: Some(resp),
        assertions,
        tests,
        console,
        vars_set,
        error: None,
    }
}

/// Combine `script:post-response` and `tests` into one source (Bruno runs them
/// in sequence in the same context). The `\n;\n` separator forces a statement
/// boundary so a missing trailing semicolon in the first block can't merge it
/// with the second (JS automatic-semicolon-insertion is not reliable here).
fn combine_post_scripts(file: &BruFile) -> Option<String> {
    match (file.script_post(), file.tests_script()) {
        (Some(a), Some(b)) => Some(format!("{a}\n;\n{b}")),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
    }
}

fn script_input(
    vars: &HashMap<String, String>,
    req: &Request,
    response: Option<ScriptResponse>,
) -> ScriptInput {
    ScriptInput {
        vars: vars.clone(),
        request: ScriptRequest {
            method: req.method.clone(),
            url: req.url.clone(),
            headers: enabled_pairs(&req.headers),
        },
        response,
    }
}

fn script_response(
    resp: &HttpResponse,
    json: &Option<serde_json::Value>,
    text: &str,
) -> ScriptResponse {
    ScriptResponse {
        status: resp.status,
        status_text: resp.status_text.clone(),
        headers: resp.headers.clone(),
        body: json
            .clone()
            .unwrap_or_else(|| serde_json::Value::String(text.to_string())),
        response_time_ms: resp.duration_ms,
    }
}

fn enabled_pairs(kvs: &[KeyVal]) -> Vec<(String, String)> {
    kvs.iter()
        .filter(|k| k.enabled)
        .map(|k| (k.name.clone(), k.value.clone()))
        .collect()
}

/// Apply `{{var}}` interpolation to every outgoing string field of the request.
fn interpolate_request(req: &mut Request, vars: &HashMap<String, String>) {
    let i = |s: &str| interpolate(s, vars);
    req.url = i(&req.url);
    for kv in req
        .headers
        .iter_mut()
        .chain(req.query.iter_mut())
        .chain(req.path_params.iter_mut())
    {
        kv.value = i(&kv.value);
    }
    req.body = match std::mem::take(&mut req.body) {
        Body::None => Body::None,
        Body::Json(s) => Body::Json(i(&s)),
        Body::Text(s) => Body::Text(i(&s)),
        Body::Xml(s) => Body::Xml(i(&s)),
        Body::Sparql(s) => Body::Sparql(i(&s)),
        Body::FormUrlEncoded(mut fields) => {
            for f in &mut fields {
                f.value = i(&f.value);
            }
            Body::FormUrlEncoded(fields)
        }
    };
    req.auth = match std::mem::take(&mut req.auth) {
        Auth::Basic { username, password } => Auth::Basic {
            username: i(&username),
            password: i(&password),
        },
        Auth::Bearer { token } => Auth::Bearer { token: i(&token) },
        Auth::ApiKey {
            key,
            value,
            placement,
        } => Auth::ApiKey {
            key: i(&key),
            value: i(&value),
            placement,
        },
        other => other,
    };
}
