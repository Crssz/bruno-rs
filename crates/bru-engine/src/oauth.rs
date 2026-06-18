//! OAuth 2.0 token acquisition for the non-interactive grants
//! (`client_credentials`, `password`). Tokens are fetched via the shared HTTP
//! client and cached per run so a collection doesn't re-authenticate per request.

use std::collections::HashMap;

use bru_core::{Auth, Body, KeyVal, OAuth2, Request};
use bru_http::HttpClient;

fn kv(name: &str, value: &str) -> KeyVal {
    KeyVal {
        name: name.to_string(),
        value: value.to_string(),
        enabled: true,
    }
}

fn cache_key(cfg: &OAuth2) -> String {
    format!(
        "{}|{}|{}|{}|{}",
        cfg.grant_type, cfg.access_token_url, cfg.client_id, cfg.scope, cfg.username
    )
}

/// Fetch (or reuse a cached) access token for `cfg`.
pub async fn fetch_token(
    client: &HttpClient,
    cache: &mut HashMap<String, String>,
    cfg: &OAuth2,
) -> Result<String, String> {
    let key = cache_key(cfg);
    if let Some(tok) = cache.get(&key) {
        return Ok(tok.clone());
    }

    let mut form = vec![kv("grant_type", &cfg.grant_type)];
    if !cfg.scope.is_empty() {
        form.push(kv("scope", &cfg.scope));
    }
    if cfg.grant_type == "password" {
        form.push(kv("username", &cfg.username));
        form.push(kv("password", &cfg.password));
    }

    // Client credentials go either in the form body or as HTTP Basic auth.
    let auth = if cfg.credentials_placement == "basic_auth_header" {
        Auth::Basic {
            username: cfg.client_id.clone(),
            password: cfg.client_secret.clone(),
        }
    } else {
        form.push(kv("client_id", &cfg.client_id));
        form.push(kv("client_secret", &cfg.client_secret));
        Auth::None
    };

    let token_req = Request {
        method: "POST".to_string(),
        url: cfg.access_token_url.clone(),
        body: Body::FormUrlEncoded(form),
        auth,
        ..Default::default()
    };

    let resp = client
        .send(&token_req)
        .await
        .map_err(|e| format!("token request failed: {e}"))?;
    if resp.status >= 400 {
        return Err(format!(
            "token endpoint returned {} {}",
            resp.status,
            resp.text()
        ));
    }
    let json = resp
        .json()
        .ok_or_else(|| "token response was not JSON".to_string())?;
    let token = json
        .get("access_token")
        .and_then(|t| t.as_str())
        .ok_or_else(|| "token response had no `access_token`".to_string())?
        .to_string();

    cache.insert(key, token.clone());
    Ok(token)
}

/// Place the obtained token on the request, then clear the auth (it's resolved).
pub fn apply_token(req: &mut Request, cfg: &OAuth2, token: &str) {
    if cfg.token_placement == "query" {
        let key = if cfg.token_query_key.is_empty() {
            "access_token"
        } else {
            &cfg.token_query_key
        };
        req.query.push(kv(key, token));
    } else {
        let prefix = if cfg.token_header_prefix.is_empty() {
            "Bearer"
        } else {
            &cfg.token_header_prefix
        };
        let value = if prefix.is_empty() {
            token.to_string()
        } else {
            format!("{prefix} {token}")
        };
        req.headers.push(kv("Authorization", &value));
    }
    req.auth = Auth::None;
}
