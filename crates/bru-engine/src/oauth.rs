//! OAuth 2.0 token acquisition for the non-interactive grants
//! (`client_credentials`, `password`). Tokens are fetched via the shared HTTP
//! client and cached per run so a collection doesn't re-authenticate per request.

use std::collections::HashMap;

use bru_core::{Auth, Body, KeyVal, OAuth2, Request};
use bru_http::HttpClient;

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

    let mut form = vec![KeyVal::new("grant_type", &cfg.grant_type)];
    if !cfg.scope.is_empty() {
        form.push(KeyVal::new("scope", &cfg.scope));
    }
    if cfg.grant_type == "password" {
        form.push(KeyVal::new("username", &cfg.username));
        form.push(KeyVal::new("password", &cfg.password));
    }

    // Client credentials go either in the form body or as HTTP Basic auth.
    let auth = if cfg.credentials_placement == "basic_auth_header" {
        Auth::Basic {
            username: cfg.client_id.clone(),
            password: cfg.client_secret.clone(),
        }
    } else {
        form.push(KeyVal::new("client_id", &cfg.client_id));
        form.push(KeyVal::new("client_secret", &cfg.client_secret));
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
        req.query.push(KeyVal::new(key, token));
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
        req.headers.push(KeyVal::new("Authorization", &value));
    }
    req.auth = Auth::None;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> OAuth2 {
        OAuth2 {
            grant_type: "client_credentials".to_string(),
            access_token_url: "https://auth.example/token".to_string(),
            client_id: "id".to_string(),
            client_secret: "sec".to_string(),
            scope: "read".to_string(),
            username: String::new(),
            password: String::new(),
            credentials_placement: "body".to_string(),
            token_placement: "header".to_string(),
            token_header_prefix: String::new(),
            token_query_key: String::new(),
        }
    }

    #[test]
    fn cache_key_includes_distinguishing_fields() {
        let a = cfg();
        let mut b = cfg();
        b.scope = "write".to_string();
        // Different scope → different cache key (independent tokens).
        assert_ne!(cache_key(&a), cache_key(&b));
        // Same config → identical key (cache hit).
        assert_eq!(cache_key(&a), cache_key(&cfg()));
        // The key embeds the five identity fields.
        let k = cache_key(&a);
        assert!(k.contains("client_credentials"));
        assert!(k.contains("https://auth.example/token"));
        assert!(k.contains("|id|"));
        assert!(k.contains("|read|"));
    }

    #[test]
    fn apply_token_header_default_bearer_prefix() {
        let mut req = Request::default();
        apply_token(&mut req, &cfg(), "tok");
        let auth = req
            .headers
            .iter()
            .find(|h| h.name == "Authorization")
            .unwrap();
        assert_eq!(auth.value, "Bearer tok");
        // Auth is cleared after the token is baked in.
        assert!(matches!(req.auth, Auth::None));
        assert!(req.query.is_empty());
    }

    #[test]
    fn apply_token_header_custom_prefix() {
        let mut c = cfg();
        c.token_header_prefix = "Token".to_string();
        let mut req = Request::default();
        apply_token(&mut req, &c, "tok");
        assert_eq!(req.headers[0].value, "Token tok");
    }

    #[test]
    fn apply_token_header_empty_prefix_uses_bare_token() {
        // An explicitly-empty prefix is not the same as the default: the header
        // carries the raw token with no scheme word.
        let mut c = cfg();
        c.token_header_prefix = " ".to_string(); // non-empty so default skipped
        let mut req = Request::default();
        apply_token(&mut req, &c, "tok");
        assert_eq!(req.headers[0].value, "  tok");
    }

    #[test]
    fn apply_token_query_default_key() {
        let mut c = cfg();
        c.token_placement = "query".to_string();
        c.token_query_key = String::new(); // → defaults to access_token
        let mut req = Request::default();
        apply_token(&mut req, &c, "tok");
        assert_eq!(req.query[0].name, "access_token");
        assert_eq!(req.query[0].value, "tok");
        assert!(req.headers.is_empty());
    }

    #[test]
    fn apply_token_query_custom_key() {
        let mut c = cfg();
        c.token_placement = "query".to_string();
        c.token_query_key = "at".to_string();
        let mut req = Request::default();
        apply_token(&mut req, &c, "tok");
        assert_eq!(req.query[0].name, "at");
        assert_eq!(req.query[0].value, "tok");
    }
}
