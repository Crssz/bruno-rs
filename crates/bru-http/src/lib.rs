//! bru-http — execute a [`Request`] over HTTP (async, reqwest + rustls).
//!
//! Pure transport: it consumes an already-interpolated [`Request`] and returns a
//! raw [`HttpResponse`]. Variable resolution, scripting, and assertions live in
//! `bru-engine`; auth schemes beyond basic/bearer/api-key arrive later.
//!
//! Build one [`HttpClient`] per run and reuse it — it owns a connection pool, so
//! a collection run keeps connections alive across requests.

use std::time::{Duration, Instant};

use bru_core::{ApiKeyPlacement, Auth, Body, MultipartValue, Request};
use reqwest::{Method, Url};
use thiserror::Error;

/// Default cap on a (decoded) response body: 100 MiB. Protects against
/// unbounded buffering and decompression bombs.
pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 100 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum HttpError {
    #[error("invalid HTTP method `{0}`")]
    InvalidMethod(String),
    #[error("invalid URL `{0}`: {1}")]
    InvalidUrl(String, String),
    #[error("failed to build HTTP client: {0}")]
    Client(#[source] reqwest::Error),
    #[error("request failed: {0}")]
    Request(#[source] reqwest::Error),
    #[error("response body exceeded the {0}-byte limit")]
    BodyTooLarge(usize),
    #[error("failed to read multipart file `{0}`: {1}")]
    FileRead(String, String),
    #[error("invalid multipart content-type: {0}")]
    InvalidMultipart(String),
}

/// Options used to construct an [`HttpClient`].
#[derive(Debug, Clone)]
pub struct SendOptions {
    /// Skip TLS certificate verification (`--insecure`). Dangerous: disables
    /// chain *and* hostname checks for every request this client makes.
    pub insecure: bool,
    /// Per-request timeout. `Duration::ZERO` means no timeout.
    pub timeout: Duration,
    /// Maximum decoded response body size before the request errors.
    pub max_response_bytes: usize,
}

impl Default for SendOptions {
    fn default() -> Self {
        Self {
            insecure: false,
            timeout: Duration::from_secs(30),
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        }
    }
}

/// A reusable HTTP client (connection pool + TLS config).
#[derive(Debug, Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    max_response_bytes: usize,
}

impl Default for HttpClient {
    fn default() -> Self {
        Self {
            inner: reqwest::Client::new(),
            max_response_bytes: DEFAULT_MAX_RESPONSE_BYTES,
        }
    }
}

impl HttpClient {
    /// Build a client from options. Reuse the result across a run.
    pub fn new(opts: &SendOptions) -> Result<Self, HttpError> {
        let mut builder = reqwest::Client::builder().danger_accept_invalid_certs(opts.insecure);
        // reqwest treats a zero Duration as an immediate deadline; only set a
        // timeout when one was actually requested.
        if !opts.timeout.is_zero() {
            builder = builder.timeout(opts.timeout);
        }
        Ok(Self {
            inner: builder.build().map_err(HttpError::Client)?,
            max_response_bytes: opts.max_response_bytes,
        })
    }

    /// Send `req` and return the response.
    pub async fn send(&self, req: &Request) -> Result<HttpResponse, HttpError> {
        let method = Method::from_bytes(req.method.to_uppercase().as_bytes())
            .map_err(|_| HttpError::InvalidMethod(req.method.clone()))?;

        let url = build_url(req)?;
        let mut builder = self.inner.request(method, url);

        let has_ct = req
            .headers
            .iter()
            .any(|h| h.enabled && h.name.eq_ignore_ascii_case("content-type"));
        for h in req.headers.iter().filter(|h| h.enabled) {
            builder = builder.header(&h.name, &h.value);
        }

        builder = apply_auth(builder, &req.auth);
        // Multipart needs async file reads, so it is built here in `send`; every
        // other body is applied by the sync `apply_body` helper.
        if let Body::MultipartForm(fields) = &req.body {
            let form = build_multipart_form(fields).await?;
            builder = builder.multipart(form);
        } else {
            builder = apply_body(builder, &req.body, has_ct);
        }

        let started = Instant::now();
        let resp = builder.send().await.map_err(HttpError::Request)?;
        let status = resp.status();
        let headers = resp
            .headers()
            .iter()
            .map(|(k, v)| {
                // Preserve non-ASCII header bytes (lossily) rather than dropping them.
                (
                    k.to_string(),
                    String::from_utf8_lossy(v.as_bytes()).into_owned(),
                )
            })
            .collect();
        let body = self.read_capped(resp).await?;
        let duration_ms = started.elapsed().as_millis();

        Ok(HttpResponse {
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or("").to_string(),
            headers,
            body,
            duration_ms,
        })
    }

    /// Stream the body, aborting if it exceeds the configured cap (enforced on
    /// *decoded* bytes, so a decompression bomb can't blow past the limit).
    async fn read_capped(&self, mut resp: reqwest::Response) -> Result<Vec<u8>, HttpError> {
        let mut body = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(HttpError::Request)? {
            if body.len() + chunk.len() > self.max_response_bytes {
                return Err(HttpError::BodyTooLarge(self.max_response_bytes));
            }
            body.extend_from_slice(&chunk);
        }
        Ok(body)
    }
}

/// A raw HTTP response plus timing.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub duration_ms: u128,
}

impl HttpResponse {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// Parse the body as JSON, if it is valid JSON.
    pub fn json(&self) -> Option<serde_json::Value> {
        serde_json::from_slice(&self.body).ok()
    }
}

/// Substitute `:path` params, then parse the URL and append enabled query params
/// (and an api-key credential placed in the query).
fn build_url(req: &Request) -> Result<Url, HttpError> {
    let mut raw = req.url.clone();
    for p in req.path_params.iter().filter(|p| p.enabled) {
        raw = replace_path_param(&raw, &p.name, &p.value);
    }

    let mut url =
        Url::parse(&raw).map_err(|e| HttpError::InvalidUrl(raw.clone(), e.to_string()))?;
    {
        let mut pairs = url.query_pairs_mut();
        for q in req.query.iter().filter(|q| q.enabled) {
            pairs.append_pair(&q.name, &q.value);
        }
        if let Auth::ApiKey {
            key,
            value,
            placement: ApiKeyPlacement::Query,
        } = &req.auth
        {
            pairs.append_pair(key, value);
        }
    }
    // Url leaves a trailing `?` when no pairs were added; normalize it away.
    if url.query() == Some("") {
        url.set_query(None);
    }
    Ok(url)
}

/// Replace a `:name` path segment, respecting segment boundaries so `:id` does
/// not match inside `:identity`.
fn replace_path_param(url: &str, name: &str, value: &str) -> String {
    let needle = format!(":{name}");
    let mut out = String::with_capacity(url.len());
    let mut rest = url;
    while let Some(idx) = rest.find(&needle) {
        out.push_str(&rest[..idx]);
        let after = &rest[idx + needle.len()..];
        let boundary = after
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
        if boundary {
            out.push_str(value);
        } else {
            out.push_str(&needle);
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

fn apply_auth(builder: reqwest::RequestBuilder, auth: &Auth) -> reqwest::RequestBuilder {
    match auth {
        Auth::Basic { username, password } => builder.basic_auth(username, Some(password)),
        Auth::Bearer { token } => builder.bearer_auth(token),
        Auth::ApiKey {
            key,
            value,
            placement: ApiKeyPlacement::Header,
        } => builder.header(key.as_str(), value.as_str()),
        // Query api-key handled in build_url; None/Inherit/query do nothing here.
        _ => builder,
    }
}

/// Apply the body. A default `Content-Type` is only added when the request did
/// not already declare one (`has_ct`), so an explicit header is never duplicated.
fn apply_body(
    builder: reqwest::RequestBuilder,
    body: &Body,
    has_ct: bool,
) -> reqwest::RequestBuilder {
    const CT: &str = "content-type";
    let with_default_ct = |b: reqwest::RequestBuilder, ct: &str| {
        if has_ct {
            b
        } else {
            b.header(CT, ct)
        }
    };
    match body {
        Body::None => builder,
        Body::Json(s) => with_default_ct(builder.body(s.clone()), "application/json"),
        Body::Text(s) => with_default_ct(builder.body(s.clone()), "text/plain"),
        Body::Xml(s) => with_default_ct(builder.body(s.clone()), "application/xml"),
        Body::Sparql(s) => with_default_ct(builder.body(s.clone()), "application/sparql-query"),
        Body::FormUrlEncoded(fields) => {
            let pairs: Vec<(&str, &str)> = fields
                .iter()
                .filter(|f| f.enabled)
                .map(|f| (f.name.as_str(), f.value.as_str()))
                .collect();
            if has_ct {
                // Encode manually so reqwest's `.form()` doesn't re-set Content-Type.
                match serde_urlencoded::to_string(&pairs) {
                    Ok(encoded) => builder.body(encoded),
                    Err(_) => builder.form(&pairs),
                }
            } else {
                builder.form(&pairs)
            }
        }
        Body::GraphQl { query, variables } => {
            // Parse the variables text as JSON; fall back to an empty object when
            // it is blank or not valid JSON.
            let vars: serde_json::Value = if variables.trim().is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(variables).unwrap_or_else(|_| serde_json::json!({}))
            };
            let payload = serde_json::json!({ "query": query, "variables": vars });
            let body = serde_json::to_string(&payload).unwrap_or_default();
            with_default_ct(builder.body(body), "application/json")
        }
        // Multipart is built asynchronously in `send`; nothing to do here.
        Body::MultipartForm(_) => builder,
    }
}

/// Build a `reqwest::multipart::Form` from the enabled fields, reading any file
/// parts off disk. File parts carry a filename (the path's basename) and, when
/// declared, a per-part content-type.
async fn build_multipart_form(
    fields: &[bru_core::MultipartField],
) -> Result<reqwest::multipart::Form, HttpError> {
    let mut form = reqwest::multipart::Form::new();
    for f in fields.iter().filter(|f| f.enabled) {
        let part = match &f.value {
            MultipartValue::Text(text) => {
                let mut part = reqwest::multipart::Part::text(text.clone());
                if let Some(ct) = &f.content_type {
                    part = part
                        .mime_str(ct)
                        .map_err(|e| HttpError::InvalidMultipart(e.to_string()))?;
                }
                part
            }
            MultipartValue::File(path) => {
                let bytes = tokio::fs::read(path)
                    .await
                    .map_err(|e| HttpError::FileRead(path.clone(), e.to_string()))?;
                let filename = std::path::Path::new(path)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone());
                let mut part = reqwest::multipart::Part::bytes(bytes).file_name(filename);
                if let Some(ct) = &f.content_type {
                    part = part
                        .mime_str(ct)
                        .map_err(|e| HttpError::InvalidMultipart(e.to_string()))?;
                }
                part
            }
        };
        form = form.part(f.name.clone(), part);
    }
    Ok(form)
}
