//! bru-http — execute a [`Request`] over HTTP (async, reqwest + rustls).
//!
//! Pure transport: it consumes an already-interpolated [`Request`] and returns a
//! raw [`HttpResponse`]. Variable resolution, scripting, and assertions live in
//! `bru-engine`; auth schemes beyond basic/bearer/api-key arrive later.

use std::time::{Duration, Instant};

use bru_core::{ApiKeyPlacement, Auth, Body, Request};
use reqwest::{Method, Url};
use thiserror::Error;

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
}

/// Transport-level options.
#[derive(Debug, Clone)]
pub struct SendOptions {
    /// Skip TLS certificate verification (`--insecure`).
    pub insecure: bool,
    pub timeout: Duration,
}

impl Default for SendOptions {
    fn default() -> Self {
        Self {
            insecure: false,
            timeout: Duration::from_secs(30),
        }
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

/// Send `req` and return the response.
pub async fn send(req: &Request, opts: &SendOptions) -> Result<HttpResponse, HttpError> {
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(opts.insecure)
        .timeout(opts.timeout)
        .build()
        .map_err(HttpError::Client)?;

    let method = Method::from_bytes(req.method.to_uppercase().as_bytes())
        .map_err(|_| HttpError::InvalidMethod(req.method.clone()))?;

    let url = build_url(req)?;
    let mut builder = client.request(method, url);

    for h in req.headers.iter().filter(|h| h.enabled) {
        builder = builder.header(&h.name, &h.value);
    }

    builder = apply_auth(builder, &req.auth);
    builder = apply_body(builder, &req.body);

    let started = Instant::now();
    let resp = builder.send().await.map_err(HttpError::Request)?;
    let status = resp.status();
    let headers = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
        .collect();
    let body = resp.bytes().await.map_err(HttpError::Request)?.to_vec();
    let duration_ms = started.elapsed().as_millis();

    Ok(HttpResponse {
        status: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("").to_string(),
        headers,
        body,
        duration_ms,
    })
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

fn apply_body(builder: reqwest::RequestBuilder, body: &Body) -> reqwest::RequestBuilder {
    const CT: &str = "content-type";
    match body {
        Body::None => builder,
        Body::Json(s) => builder.header(CT, "application/json").body(s.clone()),
        Body::Text(s) => builder.header(CT, "text/plain").body(s.clone()),
        Body::Xml(s) => builder.header(CT, "application/xml").body(s.clone()),
        Body::Sparql(s) => builder
            .header(CT, "application/sparql-query")
            .body(s.clone()),
        Body::FormUrlEncoded(fields) => {
            let pairs: Vec<(&str, &str)> = fields
                .iter()
                .filter(|f| f.enabled)
                .map(|f| (f.name.as_str(), f.value.as_str()))
                .collect();
            builder.form(&pairs)
        }
    }
}
