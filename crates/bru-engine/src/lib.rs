//! bru-engine — the request lifecycle orchestrator.
//!
//! For one request: merge variables → interpolate → send → evaluate assertions →
//! extract post-response variables. Pre/post JS scripts are a later milestone;
//! the declarative pipeline (vars, interpolation, assertions) runs today.

mod context;

pub use context::{base_vars, find_collection_root};

use std::collections::HashMap;

use bru_core::{
    eval_response_expr, evaluate_assertions, interpolate, AssertOutcome, Auth, Body, BruFile,
    Request, ResponseFacts,
};
use bru_http::{send, HttpResponse, SendOptions};

/// Mutable run state: the merged variable map (env → collection → request →
/// runtime, highest last) plus transport options.
#[derive(Debug, Clone, Default)]
pub struct RunContext {
    pub vars: HashMap<String, String>,
    pub options: SendOptions,
}

/// The result of running one request.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub name: String,
    pub method: String,
    pub url: String,
    pub response: Option<HttpResponse>,
    pub assertions: Vec<AssertOutcome>,
    /// Variables captured from the response by `vars:post-response`.
    pub vars_set: Vec<(String, String)>,
    pub error: Option<String>,
}

impl RunOutcome {
    /// True when the request sent successfully and every assertion passed.
    pub fn passed(&self) -> bool {
        self.error.is_none() && self.assertions.iter().all(|a| a.passed)
    }

    /// An outcome representing a failure before the request could run.
    pub fn errored(name: impl Into<String>, message: impl Into<String>) -> Self {
        RunOutcome {
            name: name.into(),
            method: String::new(),
            url: String::new(),
            response: None,
            assertions: Vec::new(),
            vars_set: Vec::new(),
            error: Some(message.into()),
        }
    }
}

/// Run a single request file against the context, mutating its variable map with
/// any pre-request and post-response bindings (so chained requests see them).
pub async fn run_request(file: &BruFile, ctx: &mut RunContext) -> RunOutcome {
    let name = file.request_name().unwrap_or("(unnamed)").to_string();

    let Some(mut req) = file.to_request() else {
        return RunOutcome {
            name,
            method: String::new(),
            url: String::new(),
            response: None,
            assertions: Vec::new(),
            vars_set: Vec::new(),
            error: Some("not a request .bru (no method block)".to_string()),
        };
    };

    // Pre-request vars feed interpolation of this very request.
    for v in &req.vars_pre {
        if v.enabled {
            ctx.vars.insert(v.name.clone(), v.value.clone());
        }
    }
    interpolate_request(&mut req, &ctx.vars);

    match send(&req, &ctx.options).await {
        Ok(resp) => {
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

            let mut vars_set = Vec::new();
            for v in &req.vars_post {
                if v.enabled {
                    if let Some(value) = eval_response_expr(&v.value, &facts) {
                        ctx.vars.insert(v.name.clone(), value.clone());
                        vars_set.push((v.name.clone(), value));
                    }
                }
            }

            RunOutcome {
                name,
                method: req.method,
                url: req.url,
                response: Some(resp),
                assertions,
                vars_set,
                error: None,
            }
        }
        Err(e) => RunOutcome {
            name,
            method: req.method,
            url: req.url,
            response: None,
            assertions: Vec::new(),
            vars_set: Vec::new(),
            error: Some(e.to_string()),
        },
    }
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
