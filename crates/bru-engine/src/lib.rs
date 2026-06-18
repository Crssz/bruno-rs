//! bru-engine — the request lifecycle orchestrator.
//!
//! For one request: merge vars → pre-request script → interpolate → send →
//! evaluate assertions → extract post-response vars → post-response + test
//! scripts. Scripts run in the `bru-script` QuickJS Safe-Mode sandbox.

mod context;
mod digest;
mod oauth;
mod sigv4;

pub use bru_script::TestResult;
pub use context::{base_vars, find_collection_root};

use std::collections::HashMap;
use std::path::PathBuf;

use bru_core::{
    eval_response_expr, evaluate_assertions, interpolate, AssertOutcome, Auth, Body, BruFile,
    KeyVal, MultipartValue, OAuth2, Request, ResponseFacts,
};
use bru_http::{HttpClient, HttpResponse, SendOptions};
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
    /// The options `client` was built from. Used as the base when a request's
    /// `settings` block needs a divergent client (e.g. redirect policy), which
    /// reqwest can only configure at client-build time.
    pub send_options: SendOptions,
    /// OAuth2 access tokens fetched this run, keyed per credentials.
    pub token_cache: HashMap<String, String>,
    /// Directory of the request being run, used to resolve `require('./x')` in
    /// scripts when `developer_mode` is on. `None` disables relative resolution.
    pub script_dir: Option<PathBuf>,
    /// Bruno "Developer Mode": let scripts `require()` local `.js`. Off (Safe
    /// Mode) by default — sandboxed scripts then have no filesystem reach.
    pub developer_mode: bool,
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
        let out = run_script(
            &src,
            &script_input(&ctx.vars, &req, None, ctx.script_dir.clone(), ctx.developer_mode),
        );
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

    // Resolve `auth: inherit` to the nearest configured folder/collection auth
    // before interpolation (so its {{vars}} resolve too) and before the
    // oauth2/sigv4/digest blocks (so an inherited scheme is fully handled).
    if matches!(req.auth, Auth::Inherit) {
        req.auth = resolve_inherited_auth(ctx.script_dir.as_deref());
    }

    interpolate_request(&mut req, &ctx.vars);

    // Finalize the URL (substitute `:path` params + append query) BEFORE auth
    // resolution, so SigV4/Digest sign the exact path+query that goes on the wire.
    // Path/query are then folded into `req.url`; clearing the vectors keeps the
    // send-time `build_url` from appending them twice.
    if let Ok(finalized) = bru_http::resolve_url(&req) {
        req.url = finalized;
        req.path_params.clear();
        req.query.clear();
    }

    // Validate GraphQL variables up front: a typo would otherwise be silently
    // dropped to `{}` by the body serializer, sending a variable-less request.
    if let Body::GraphQl { variables, .. } = &req.body {
        if !variables.trim().is_empty()
            && serde_json::from_str::<serde_json::Value>(variables).is_err()
        {
            let mut outcome = RunOutcome::errored(name, "graphql variables are not valid JSON");
            outcome.method = req.method;
            outcome.url = req.url;
            outcome.tests = tests;
            outcome.console = console;
            return outcome;
        }
    }

    // Resolve OAuth2: fetch a token from the token endpoint and place it on the
    // request (interactive grants are not supported and would project to None).
    if let Auth::OAuth2(cfg) = req.auth.clone() {
        match oauth::fetch_token(&ctx.client, &mut ctx.token_cache, &cfg).await {
            Ok(token) => oauth::apply_token(&mut req, &cfg, &token),
            Err(e) => {
                let mut outcome = RunOutcome::errored(name, format!("oauth2: {e}"));
                outcome.method = req.method;
                outcome.url = req.url;
                outcome.tests = tests;
                outcome.console = console;
                return outcome;
            }
        }
    }

    // Resolve AWS SigV4: pure computation, signed before send (like OAuth2 but
    // with no network round-trip). Pushes x-amz-date/Authorization headers.
    if matches!(req.auth, Auth::AwsV4 { .. }) {
        if let Err(e) = sigv4::resolve(&mut req, now_unix()) {
            let mut outcome = RunOutcome::errored(name, e);
            outcome.method = req.method;
            outcome.url = req.url;
            outcome.tests = tests;
            outcome.console = console;
            return outcome;
        }
    }

    // A request whose `settings` block overrides redirect behavior needs its own
    // client: reqwest fixes redirect policy at build time, so it can't be tweaked
    // per-send on the shared client. Timeout overrides, by contrast, are applied
    // per-send inside bru-http and need no separate client. On build failure we
    // fall back to the shared client rather than aborting the request.
    let req_client = if req.settings.follow_redirects.is_some()
        || req.settings.max_redirects.is_some()
    {
        let mut o = ctx.send_options.clone();
        if let Some(f) = req.settings.follow_redirects {
            o.follow_redirects = f;
        }
        if let Some(m) = req.settings.max_redirects {
            o.max_redirects = m as usize;
        }
        HttpClient::new(&o).ok()
    } else {
        None
    };
    let client = req_client.as_ref().unwrap_or(&ctx.client);

    // Digest auth is a challenge/response: stash the credentials, then clear the
    // auth so the first send goes out unauthenticated to elicit the 401 nonce.
    let digest_creds = match &req.auth {
        Auth::Digest { username, password } => {
            let creds = (username.clone(), password.clone());
            req.auth = Auth::None;
            Some(creds)
        }
        _ => None,
    };

    let resp = match client.send(&req).await {
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

    // If the server issued a Digest 401 challenge, recompute the Authorization
    // header from the nonce and re-send once.
    let resp = if let Some((username, password)) = digest_creds {
        match resp_digest_challenge(&resp) {
            Some(challenge) if resp.status == 401 => {
                let uri = digest_request_uri(&req.url);
                let header = digest::authorization_header(
                    &challenge,
                    &username,
                    &password,
                    &req.method.to_uppercase(),
                    &uri,
                    &digest::derive_cnonce(),
                );
                req.headers.push(KeyVal {
                    name: "Authorization".to_string(),
                    value: header,
                    enabled: true,
                });
                match client.send(&req).await {
                    Ok(resp2) => resp2,
                    Err(e) => {
                        let mut outcome = RunOutcome::errored(name, e.to_string());
                        outcome.method = req.method;
                        outcome.url = req.url;
                        outcome.tests = tests;
                        outcome.console = console;
                        return outcome;
                    }
                }
            }
            _ => resp,
        }
    } else {
        resp
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
        let out = run_script(
            &src,
            &script_input(
                &ctx.vars,
                &req,
                Some(sresp),
                ctx.script_dir.clone(),
                ctx.developer_mode,
            ),
        );
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

/// Resolve `auth: inherit` by walking from the request's directory up to the
/// collection root, returning the nearest folder.bru / collection.bru `auth`
/// block whose mode is a concrete scheme (else [`Auth::None`]).
fn resolve_inherited_auth(start: Option<&std::path::Path>) -> Auth {
    let Some(start) = start else {
        return Auth::None;
    };
    let root = find_collection_root(start);
    let mut dir = Some(start.to_path_buf());
    while let Some(d) = dir {
        let is_root = Some(d.as_path()) == root.as_deref();
        let candidate = if is_root {
            d.join("collection.bru")
        } else {
            d.join("folder.bru")
        };
        if let Ok(text) = std::fs::read_to_string(&candidate) {
            if let Ok(file) = bru_lang::parse(&text) {
                if let Some(mode) = file.dict_value("auth", "mode") {
                    if mode != "none" && mode != "inherit" {
                        return file.project_auth(mode);
                    }
                }
            }
        }
        if is_root {
            break;
        }
        dir = d.parent().map(|p| p.to_path_buf());
    }
    Auth::None
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
    script_dir: Option<PathBuf>,
    allow_require: bool,
) -> ScriptInput {
    ScriptInput {
        vars: vars.clone(),
        request: ScriptRequest {
            method: req.method.clone(),
            url: req.url.clone(),
            headers: enabled_pairs(&req.headers),
        },
        response,
        script_dir,
        allow_require,
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

/// Find and parse a `WWW-Authenticate: Digest ...` header from a response.
fn resp_digest_challenge(resp: &HttpResponse) -> Option<digest::Challenge> {
    resp.headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("www-authenticate"))
        .and_then(|(_, v)| digest::parse_challenge(v))
}

/// The request-URI used in the Digest `uri=` field and HA2: the path + query of
/// the request URL (Digest signs the path, not the full absolute URL).
fn digest_request_uri(url: &str) -> String {
    let after_scheme = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    match after_scheme.find('/') {
        Some(idx) => after_scheme[idx..].to_string(),
        None => "/".to_string(),
    }
}

/// Current wall-clock time as whole seconds since the Unix epoch (for SigV4's
/// `x-amz-date`). Falls back to 0 if the clock is before the epoch.
fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
        Body::GraphQl { query, variables } => Body::GraphQl {
            query: i(&query),
            variables: i(&variables),
        },
        Body::MultipartForm(mut fields) => {
            for f in &mut fields {
                f.value = match std::mem::replace(&mut f.value, MultipartValue::Text(String::new()))
                {
                    MultipartValue::Text(s) => MultipartValue::Text(i(&s)),
                    MultipartValue::File(p) => MultipartValue::File(i(&p)),
                };
                f.content_type = f.content_type.take().map(|c| i(&c));
            }
            Body::MultipartForm(fields)
        }
        Body::File(mut items) => {
            for it in &mut items {
                it.path = i(&it.path);
                it.content_type = it.content_type.take().map(|c| i(&c));
            }
            Body::File(items)
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
        Auth::OAuth2(cfg) => Auth::OAuth2(OAuth2 {
            access_token_url: i(&cfg.access_token_url),
            client_id: i(&cfg.client_id),
            client_secret: i(&cfg.client_secret),
            scope: i(&cfg.scope),
            username: i(&cfg.username),
            password: i(&cfg.password),
            ..cfg
        }),
        Auth::Digest { username, password } => Auth::Digest {
            username: i(&username),
            password: i(&password),
        },
        Auth::AwsV4 {
            access_key_id,
            secret_access_key,
            session_token,
            service,
            region,
            profile_name,
        } => Auth::AwsV4 {
            access_key_id: i(&access_key_id),
            secret_access_key: i(&secret_access_key),
            session_token: i(&session_token),
            service: i(&service),
            region: i(&region),
            profile_name: i(&profile_name),
        },
        other => other,
    };
}
