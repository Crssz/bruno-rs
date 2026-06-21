//! Importers from other API-client formats into Bruno `.bru`.
//!
//! Currently: Postman v2.1 collections (→ a new on-disk collection) and curl
//! commands (→ a single request's `.bru` text). OpenAPI / Insomnia are not yet
//! supported. Output is plain `.bru` text built directly (no model round-trip),
//! so it stays close to what `fsops`/the serializer would write.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::envfs;

/// Scaffold a new collection dir (bruno.json + environments/) under `parent`.
fn create_collection(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let n = name.trim();
    let dir = parent.join(envfs::sanitize(if n.is_empty() {
        "Imported Collection"
    } else {
        n
    }));
    if dir.exists() {
        return Err(format!("\"{}\" already exists", dir.display()));
    }
    std::fs::create_dir_all(dir.join("environments")).map_err(|e| e.to_string())?;
    let esc = n.replace('\\', "\\\\").replace('"', "\\\"");
    let json = format!(
        "{{\n  \"version\": \"1\",\n  \"name\": \"{esc}\",\n  \"type\": \"collection\"\n}}\n"
    );
    std::fs::write(dir.join("bruno.json"), json).map_err(|e| e.to_string())?;
    Ok(dir)
}

// ── Postman v2.1 ─────────────────────────────────────────────────────────────

/// Import a Postman v2.1 collection JSON into a new bru collection under
/// `parent`. Returns the new collection directory.
pub fn import_postman(json: &str, parent: &Path) -> Result<PathBuf, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;
    let name = v
        .get("info")
        .and_then(|i| i.get("name"))
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Imported Collection");
    let dir = create_collection(parent, name)?;
    let items = v
        .get("item")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    import_items(&items, &dir)?;
    Ok(dir)
}

/// Recursively write Postman items: an item with a nested `item` array is a
/// folder, one with a `request` is a request file.
fn import_items(items: &[Value], dir: &Path) -> Result<(), String> {
    let mut seq = 1i64;
    for item in items {
        let name = item
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("item");
        if let Some(sub) = item.get("item").and_then(Value::as_array) {
            let fdir = dir.join(envfs::sanitize(name));
            std::fs::create_dir_all(&fdir).map_err(|e| e.to_string())?;
            std::fs::write(
                fdir.join("folder.bru"),
                format!("meta {{\n  name: {name}\n  type: folder\n}}\n"),
            )
            .map_err(|e| e.to_string())?;
            import_items(sub, &fdir)?;
        } else if let Some(req) = item.get("request") {
            let text = postman_request_bru(name, req, seq);
            let path = unique_path(dir, &envfs::sanitize(name));
            std::fs::write(&path, text).map_err(|e| e.to_string())?;
            seq += 1;
        }
    }
    Ok(())
}

/// `<dir>/<stem>.bru`, appending ` N` to the stem until it doesn't collide.
fn unique_path(dir: &Path, stem: &str) -> PathBuf {
    let stem = if stem.is_empty() { "request" } else { stem };
    let mut p = dir.join(format!("{stem}.bru"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{stem} {n}.bru"));
        n += 1;
    }
    p
}

fn postman_request_bru(name: &str, req: &Value, seq: i64) -> String {
    let method = req
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or("GET")
        .to_lowercase();
    let url = postman_url(req.get("url"));
    let (mode, body_block) = postman_body(req.get("body"));
    let auth = req.get("auth");
    let auth_mode = postman_auth_mode(auth);

    let mut s = format!("meta {{\n  name: {name}\n  type: http\n  seq: {seq}\n}}\n\n");
    s.push_str(&format!(
        "{method} {{\n  url: {url}\n  body: {mode}\n  auth: {auth_mode}\n}}\n"
    ));
    if let Some(q) = postman_kv_block("params:query", req.get("url").and_then(|u| u.get("query"))) {
        s.push('\n');
        s.push_str(&q);
    }
    if let Some(h) = postman_kv_block("headers", req.get("header")) {
        s.push('\n');
        s.push_str(&h);
    }
    if let Some(a) = postman_auth_block(auth) {
        s.push('\n');
        s.push_str(&a);
    }
    if let Some(b) = body_block {
        s.push('\n');
        s.push_str(&b);
    }
    s
}

fn postman_url(url: Option<&Value>) -> String {
    match url {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(_)) => url
            .and_then(|u| u.get("raw"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

/// Emit a dict block (`params:query` / `headers`) from a Postman array of
/// `{key, value, disabled}`. A `disabled` entry gets the `~` prefix bru uses.
fn postman_kv_block(block: &str, arr: Option<&Value>) -> Option<String> {
    let arr = arr?.as_array()?;
    let mut lines = Vec::new();
    for e in arr {
        let key = e.get("key").and_then(Value::as_str).unwrap_or("");
        if key.is_empty() {
            continue;
        }
        let value = e.get("value").and_then(Value::as_str).unwrap_or("");
        let disabled = e.get("disabled").and_then(Value::as_bool).unwrap_or(false);
        let prefix = if disabled { "~" } else { "" };
        lines.push(format!("  {prefix}{key}: {value}"));
    }
    if lines.is_empty() {
        return None;
    }
    Some(format!("{block} {{\n{}\n}}\n", lines.join("\n")))
}

/// Map a Postman body to a `(mode, Option<block-text>)`.
fn postman_body(body: Option<&Value>) -> (&'static str, Option<String>) {
    let Some(body) = body else {
        return ("none", None);
    };
    match body.get("mode").and_then(Value::as_str) {
        Some("raw") => {
            let raw = body.get("raw").and_then(Value::as_str).unwrap_or("");
            let lang = body
                .get("options")
                .and_then(|o| o.get("raw"))
                .and_then(|r| r.get("language"))
                .and_then(Value::as_str)
                .unwrap_or("text");
            let mode = match lang {
                "json" => "json",
                "xml" => "xml",
                _ => "text",
            };
            (
                mode,
                Some(format!("body:{mode} {{\n{}\n}}\n", indent_block(raw))),
            )
        }
        Some("urlencoded") => (
            "form-urlencoded",
            postman_kv_block("body:form-urlencoded", body.get("urlencoded")),
        ),
        Some("formdata") => (
            "multipart-form",
            postman_kv_block("body:multipart-form", body.get("formdata")),
        ),
        Some("graphql") => {
            let q = body
                .get("graphql")
                .and_then(|g| g.get("query"))
                .and_then(Value::as_str)
                .unwrap_or("");
            (
                "graphql",
                Some(format!("body:graphql {{\n{}\n}}\n", indent_block(q))),
            )
        }
        _ => ("none", None),
    }
}

/// Indent each line of a raw body by two spaces (bru block bodies are indented).
fn indent_block(s: &str) -> String {
    s.lines()
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn postman_auth_mode(auth: Option<&Value>) -> &'static str {
    match auth.and_then(|a| a.get("type")).and_then(Value::as_str) {
        Some("bearer") => "bearer",
        Some("basic") => "basic",
        Some("apikey") => "apikey",
        _ => "none",
    }
}

/// Look up a parameter value inside a Postman auth section
/// (`auth[kind]` is an array of `{key, value}`).
fn pm_auth_param(auth: &Value, kind: &str, key: &str) -> String {
    auth.get(kind)
        .and_then(Value::as_array)
        .and_then(|arr| {
            arr.iter()
                .find(|e| e.get("key").and_then(Value::as_str) == Some(key))
        })
        .and_then(|e| e.get("value").and_then(Value::as_str))
        .unwrap_or("")
        .to_string()
}

fn postman_auth_block(auth: Option<&Value>) -> Option<String> {
    let auth = auth?;
    match auth.get("type").and_then(Value::as_str)? {
        "bearer" => Some(format!(
            "auth:bearer {{\n  token: {}\n}}\n",
            pm_auth_param(auth, "bearer", "token")
        )),
        "basic" => Some(format!(
            "auth:basic {{\n  username: {}\n  password: {}\n}}\n",
            pm_auth_param(auth, "basic", "username"),
            pm_auth_param(auth, "basic", "password"),
        )),
        "apikey" => {
            let placement = match pm_auth_param(auth, "apikey", "in").as_str() {
                "query" => "query",
                _ => "header",
            };
            Some(format!(
                "auth:apikey {{\n  key: {}\n  value: {}\n  placement: {placement}\n}}\n",
                pm_auth_param(auth, "apikey", "key"),
                pm_auth_param(auth, "apikey", "value"),
            ))
        }
        _ => None,
    }
}

// ── curl ─────────────────────────────────────────────────────────────────────

/// Parse a curl command into `(name, bru_text)` for a single request, or None
/// if no URL could be found.
pub fn curl_to_bru(cmd: &str) -> Option<(String, String)> {
    let toks = tokenize(cmd);
    let mut method: Option<String> = None;
    let mut url: Option<String> = None;
    let mut headers: Vec<(String, String)> = Vec::new();
    let mut data: Option<String> = None;
    let mut basic: Option<String> = None;

    let mut i = 0;
    while i < toks.len() {
        let t = toks[i].as_str();
        match t {
            "curl" => {}
            "-X" | "--request" => {
                i += 1;
                method = toks.get(i).cloned();
            }
            "-H" | "--header" => {
                i += 1;
                if let Some(h) = toks.get(i) {
                    if let Some((k, v)) = h.split_once(':') {
                        headers.push((k.trim().to_string(), v.trim().to_string()));
                    }
                }
            }
            "-d" | "--data" | "--data-raw" | "--data-binary" | "--data-ascii" => {
                i += 1;
                data = toks.get(i).cloned();
            }
            "-u" | "--user" => {
                i += 1;
                basic = toks.get(i).cloned();
            }
            "--url" => {
                i += 1;
                url = toks.get(i).cloned();
            }
            other if other.starts_with("http://") || other.starts_with("https://") => {
                url = Some(other.to_string());
            }
            // Skip flags we don't model (and their argument if they take one).
            "-A" | "--user-agent" | "-e" | "--referer" | "-b" | "--cookie" | "-o" | "--output" => {
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }

    let url = url?;
    let method = method
        .map(|m| m.to_lowercase())
        .unwrap_or_else(|| if data.is_some() { "post" } else { "get" }.to_string());
    let auth_mode = if basic.is_some() { "basic" } else { "none" };
    let (mode, body_block) = match &data {
        Some(d) => (
            "text",
            Some(format!("body:text {{\n{}\n}}\n", indent_block(d))),
        ),
        None => ("none", None),
    };

    let name = curl_name(&url);
    let mut s = format!("meta {{\n  name: {name}\n  type: http\n  seq: 1\n}}\n\n");
    s.push_str(&format!(
        "{method} {{\n  url: {url}\n  body: {mode}\n  auth: {auth_mode}\n}}\n"
    ));
    if !headers.is_empty() {
        let lines: Vec<String> = headers.iter().map(|(k, v)| format!("  {k}: {v}")).collect();
        s.push_str(&format!("\nheaders {{\n{}\n}}\n", lines.join("\n")));
    }
    if let Some(u) = basic {
        let (user, pass) = u.split_once(':').unwrap_or((u.as_str(), ""));
        s.push_str(&format!(
            "\nauth:basic {{\n  username: {user}\n  password: {pass}\n}}\n"
        ));
    }
    if let Some(b) = body_block {
        s.push('\n');
        s.push_str(&b);
    }
    Some((name, s))
}

/// A readable request name from the URL's last path segment (or host).
fn curl_name(url: &str) -> String {
    let after = url.split("://").nth(1).unwrap_or(url);
    let path = after.split('?').next().unwrap_or(after);
    let seg = path
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(path)
        .trim();
    if seg.is_empty() || seg.contains('.') && !seg.contains('/') && after.starts_with(seg) {
        // Looks like a bare host; use it.
        after.split('/').next().unwrap_or("curl").to_string()
    } else {
        seg.to_string()
    }
}

/// Split a shell-ish command into tokens, honoring single/double quotes and a
/// trailing-backslash line continuation. Good enough for pasted curl commands.
fn tokenize(cmd: &str) -> Vec<String> {
    let mut toks = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut has = false;
    let mut chars = cmd.chars().peekable();
    while let Some(c) = chars.next() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                } else {
                    cur.push(c);
                }
            }
            None => match c {
                '\'' | '"' => {
                    quote = Some(c);
                    has = true;
                }
                '\\' => {
                    // Line continuation or escaped char: take the next char literally.
                    if let Some(&n) = chars.peek() {
                        if n == '\n' || n == '\r' {
                            chars.next();
                        } else {
                            cur.push(chars.next().unwrap());
                            has = true;
                        }
                    }
                }
                c if c.is_whitespace() => {
                    if has {
                        toks.push(std::mem::take(&mut cur));
                        has = false;
                    }
                }
                c => {
                    cur.push(c);
                    has = true;
                }
            },
        }
    }
    if has {
        toks.push(cur);
    }
    toks
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn tmp(tag: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "bru-import-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn tokenize_handles_quotes_and_continuation() {
        let toks = tokenize("curl -H 'k: v' --data \"a b\" \\\n https://x.test");
        assert_eq!(
            toks,
            vec!["curl", "-H", "k: v", "--data", "a b", "https://x.test"]
        );
    }

    #[test]
    fn curl_to_bru_basic_get_with_header() {
        let (name, bru) = curl_to_bru("curl https://api.test/users -H 'Accept: application/json'")
            .expect("parses");
        assert_eq!(name, "users");
        assert!(bru.contains("get {"));
        assert!(bru.contains("url: https://api.test/users"));
        assert!(bru.contains("Accept: application/json"));
    }

    #[test]
    fn curl_to_bru_post_with_data_and_basic_auth() {
        let (_n, bru) =
            curl_to_bru("curl -X POST https://api.test/login -u admin:secret --data '{\"a\":1}'")
                .unwrap();
        assert!(bru.contains("post {"));
        assert!(bru.contains("auth: basic"));
        assert!(bru.contains("username: admin"));
        assert!(bru.contains("password: secret"));
        assert!(bru.contains("body:text {"));
        assert!(bru.contains("{\"a\":1}"));
    }

    #[test]
    fn curl_to_bru_none_without_url() {
        assert!(curl_to_bru("curl -X GET").is_none());
    }

    #[test]
    fn import_postman_builds_collection_with_folder_and_request() {
        let json = r#"{
          "info": { "name": "My PM", "schema": "v2.1.0" },
          "item": [
            { "name": "Users", "item": [
              { "name": "List", "request": {
                  "method": "GET",
                  "header": [ { "key": "Accept", "value": "application/json" } ],
                  "url": { "raw": "https://api.test/users?page=1",
                           "query": [ { "key": "page", "value": "1" } ] },
                  "auth": { "type": "bearer", "bearer": [ { "key": "token", "value": "T0K" } ] }
              } } ] },
            { "name": "Create", "request": {
                "method": "POST",
                "body": { "mode": "raw", "raw": "{\"a\":1}",
                          "options": { "raw": { "language": "json" } } },
                "url": "https://api.test/create"
            } }
          ]
        }"#;
        let parent = tmp("pm");
        let dir = import_postman(json, &parent).unwrap();
        assert!(dir.join("bruno.json").exists());
        // Folder + nested request.
        let list = dir.join("Users").join("List.bru");
        assert!(list.exists());
        let list_txt = std::fs::read_to_string(&list).unwrap();
        assert!(list_txt.contains("get {"));
        assert!(list_txt.contains("auth: bearer"));
        assert!(list_txt.contains("token: T0K"));
        assert!(list_txt.contains("params:query {"));
        assert!(list_txt.contains("page: 1"));
        // Top-level POST with json body.
        let create = std::fs::read_to_string(dir.join("Create.bru")).unwrap();
        assert!(create.contains("post {"));
        assert!(create.contains("body: json"));
        assert!(create.contains("body:json {"));
        // The whole collection loads + every request round-trips through the parser.
        let tree = bru_lang::load_collection(&dir).unwrap();
        assert_eq!(tree.name, "My PM");
        std::fs::remove_dir_all(&parent).ok();
    }
}

#[cfg(test)]
mod cov_tests {
    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn tmp(tag: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "bru-import-cov-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    // ── tokenize ────────────────────────────────────────────────────────────

    #[test]
    fn tokenize_double_quotes_and_escaped_char() {
        // Escaped space inside an unquoted run stays in the same token.
        let toks = tokenize(r#"curl "a b" c\ d"#);
        assert_eq!(toks, vec!["curl", "a b", "c d"]);
    }

    #[test]
    fn tokenize_empty_quoted_token_is_kept() {
        // An empty quoted string still produces a (blank) token because the
        // quote flips `has` true.
        let toks = tokenize("curl '' x");
        assert_eq!(
            toks,
            vec!["curl".to_string(), String::new(), "x".to_string()]
        );
    }

    #[test]
    fn tokenize_trailing_backslash_at_eof_is_ignored() {
        // A backslash with nothing after it (peek == None) is dropped.
        let toks = tokenize("curl x\\");
        assert_eq!(toks, vec!["curl", "x"]);
    }

    #[test]
    fn tokenize_crlf_continuation() {
        let toks = tokenize("curl \\\r\n https://x.test");
        assert_eq!(toks, vec!["curl", "https://x.test"]);
    }

    #[test]
    fn tokenize_empty_string_is_empty() {
        assert!(tokenize("").is_empty());
    }

    // ── curl_name ───────────────────────────────────────────────────────────

    #[test]
    fn curl_name_uses_last_path_segment() {
        assert_eq!(curl_name("https://api.test/v1/users"), "users");
    }

    #[test]
    fn curl_name_strips_query_string() {
        assert_eq!(curl_name("https://api.test/items?page=2"), "items");
    }

    #[test]
    fn curl_name_trailing_slash_picks_prior_segment() {
        assert_eq!(curl_name("https://api.test/things/"), "things");
    }

    #[test]
    fn curl_name_bare_host_with_dot() {
        // Host-only URL: the dotted host is returned as the name.
        assert_eq!(curl_name("https://api.test"), "api.test");
    }

    #[test]
    fn curl_name_no_scheme() {
        assert_eq!(curl_name("localhost:3000/ping"), "ping");
    }

    // ── curl_to_bru ─────────────────────────────────────────────────────────

    #[test]
    fn curl_to_bru_url_flag_and_long_request() {
        let (name, bru) =
            curl_to_bru("curl --request DELETE --url https://api.test/users/7").unwrap();
        assert_eq!(name, "7");
        assert!(bru.contains("delete {"));
        assert!(bru.contains("url: https://api.test/users/7"));
        assert!(bru.contains("auth: none"));
        assert!(bru.contains("body: none"));
    }

    #[test]
    fn curl_to_bru_data_defaults_to_post() {
        // No -X but data present → method defaults to post.
        let (_n, bru) = curl_to_bru("curl https://api.test/x --data hello").unwrap();
        assert!(bru.contains("post {"));
        assert!(bru.contains("body: text"));
        assert!(bru.contains("body:text {"));
        assert!(bru.contains("  hello"));
    }

    #[test]
    fn curl_to_bru_no_data_defaults_to_get() {
        let (_n, bru) = curl_to_bru("curl https://api.test/x").unwrap();
        assert!(bru.contains("get {"));
        assert!(bru.contains("body: none"));
    }

    #[test]
    fn curl_to_bru_data_raw_and_binary_aliases() {
        let (_n, bru) =
            curl_to_bru("curl https://api.test/x --data-raw RAW --data-binary BIN").unwrap();
        // Last --data wins.
        assert!(bru.contains("  BIN"));
    }

    #[test]
    fn curl_to_bru_basic_auth_no_colon() {
        // -u with no colon → empty password.
        let (_n, bru) = curl_to_bru("curl https://api.test/x -u onlyuser").unwrap();
        assert!(bru.contains("auth: basic"));
        assert!(bru.contains("username: onlyuser"));
        assert!(bru.contains("password: \n") || bru.contains("password: "));
    }

    #[test]
    fn curl_to_bru_header_without_colon_is_skipped() {
        // "-H NoColon" has no ':' so split_once fails and it's dropped; no
        // headers block emitted.
        let (_n, bru) = curl_to_bru("curl https://api.test/x -H NoColon").unwrap();
        assert!(!bru.contains("headers {"));
    }

    #[test]
    fn curl_to_bru_skips_unmodeled_flags_with_args() {
        // -A/-b/-o consume their argument and are not mistaken for a URL.
        let (name, bru) =
            curl_to_bru("curl -A Mozilla -b session=1 -o out.json https://api.test/keep").unwrap();
        assert_eq!(name, "keep");
        assert!(bru.contains("url: https://api.test/keep"));
    }

    #[test]
    fn curl_to_bru_http_scheme_url_positional() {
        let (_n, bru) = curl_to_bru("curl http://plain.test/p").unwrap();
        assert!(bru.contains("url: http://plain.test/p"));
    }

    #[test]
    fn curl_to_bru_method_lowercased() {
        let (_n, bru) = curl_to_bru("curl -X PaTcH https://api.test/x").unwrap();
        assert!(bru.contains("patch {"));
    }

    #[test]
    fn curl_to_bru_none_when_no_url_present() {
        assert!(curl_to_bru("curl -H 'a: b' --data foo").is_none());
    }

    #[test]
    fn curl_to_bru_multiple_headers_emitted() {
        let (_n, bru) = curl_to_bru("curl https://api.test/x -H 'A: 1' -H 'B: 2'").unwrap();
        assert!(bru.contains("headers {"));
        assert!(bru.contains("A: 1"));
        assert!(bru.contains("B: 2"));
    }

    // ── postman_url ─────────────────────────────────────────────────────────

    #[test]
    fn postman_url_string_form() {
        let v = json!("https://api.test/raw");
        assert_eq!(postman_url(Some(&v)), "https://api.test/raw");
    }

    #[test]
    fn postman_url_object_raw() {
        let v = json!({ "raw": "https://api.test/obj" });
        assert_eq!(postman_url(Some(&v)), "https://api.test/obj");
    }

    #[test]
    fn postman_url_object_without_raw_is_empty() {
        let v = json!({ "host": ["api", "test"] });
        assert_eq!(postman_url(Some(&v)), "");
    }

    #[test]
    fn postman_url_none_and_nonstring() {
        assert_eq!(postman_url(None), "");
        let n = json!(42);
        assert_eq!(postman_url(Some(&n)), "");
    }

    // ── postman_kv_block ────────────────────────────────────────────────────

    #[test]
    fn postman_kv_block_skips_empty_keys_and_marks_disabled() {
        let arr = json!([
            { "key": "", "value": "ignored" },
            { "key": "On", "value": "1" },
            { "key": "Off", "value": "0", "disabled": true }
        ]);
        let block = postman_kv_block("headers", Some(&arr)).unwrap();
        assert!(block.starts_with("headers {\n"));
        assert!(block.contains("  On: 1"));
        assert!(block.contains("  ~Off: 0"));
        assert!(!block.contains("ignored"));
    }

    #[test]
    fn postman_kv_block_none_when_all_empty() {
        let arr = json!([{ "key": "", "value": "x" }]);
        assert!(postman_kv_block("headers", Some(&arr)).is_none());
    }

    #[test]
    fn postman_kv_block_none_when_not_array_or_missing() {
        assert!(postman_kv_block("headers", None).is_none());
        let obj = json!({ "not": "array" });
        assert!(postman_kv_block("headers", Some(&obj)).is_none());
    }

    // ── postman_body ────────────────────────────────────────────────────────

    #[test]
    fn postman_body_none_when_missing() {
        let (mode, block) = postman_body(None);
        assert_eq!(mode, "none");
        assert!(block.is_none());
    }

    #[test]
    fn postman_body_raw_json() {
        let b = json!({
            "mode": "raw",
            "raw": "{\"a\":1}",
            "options": { "raw": { "language": "json" } }
        });
        let (mode, block) = postman_body(Some(&b));
        assert_eq!(mode, "json");
        let block = block.unwrap();
        assert!(block.starts_with("body:json {\n"));
        assert!(block.contains("  {\"a\":1}"));
    }

    #[test]
    fn postman_body_raw_xml() {
        let b = json!({
            "mode": "raw",
            "raw": "<a/>",
            "options": { "raw": { "language": "xml" } }
        });
        let (mode, block) = postman_body(Some(&b));
        assert_eq!(mode, "xml");
        assert!(block.unwrap().contains("body:xml {"));
    }

    #[test]
    fn postman_body_raw_text_default_language() {
        // No options → language defaults to "text".
        let b = json!({ "mode": "raw", "raw": "plain" });
        let (mode, block) = postman_body(Some(&b));
        assert_eq!(mode, "text");
        assert!(block.unwrap().contains("body:text {"));
    }

    #[test]
    fn postman_body_urlencoded() {
        let b = json!({
            "mode": "urlencoded",
            "urlencoded": [{ "key": "a", "value": "1" }]
        });
        let (mode, block) = postman_body(Some(&b));
        assert_eq!(mode, "form-urlencoded");
        assert!(block.unwrap().contains("body:form-urlencoded {"));
    }

    #[test]
    fn postman_body_formdata() {
        let b = json!({
            "mode": "formdata",
            "formdata": [{ "key": "f", "value": "v" }]
        });
        let (mode, block) = postman_body(Some(&b));
        assert_eq!(mode, "multipart-form");
        assert!(block.unwrap().contains("body:multipart-form {"));
    }

    #[test]
    fn postman_body_graphql() {
        let b = json!({
            "mode": "graphql",
            "graphql": { "query": "{ me }" }
        });
        let (mode, block) = postman_body(Some(&b));
        assert_eq!(mode, "graphql");
        let block = block.unwrap();
        assert!(block.starts_with("body:graphql {\n"));
        assert!(block.contains("  { me }"));
    }

    #[test]
    fn postman_body_unknown_mode_is_none() {
        let b = json!({ "mode": "file" });
        let (mode, block) = postman_body(Some(&b));
        assert_eq!(mode, "none");
        assert!(block.is_none());
    }

    // ── indent_block ────────────────────────────────────────────────────────

    #[test]
    fn indent_block_indents_each_line() {
        assert_eq!(indent_block("a\nb"), "  a\n  b");
    }

    #[test]
    fn indent_block_empty_string() {
        // "".lines() yields nothing, so the joined result is empty.
        assert_eq!(indent_block(""), "");
    }

    // ── postman_auth_mode ───────────────────────────────────────────────────

    #[test]
    fn postman_auth_mode_variants() {
        let bearer = json!({ "type": "bearer" });
        let basic = json!({ "type": "basic" });
        let apikey = json!({ "type": "apikey" });
        let oauth = json!({ "type": "oauth2" });
        assert_eq!(postman_auth_mode(Some(&bearer)), "bearer");
        assert_eq!(postman_auth_mode(Some(&basic)), "basic");
        assert_eq!(postman_auth_mode(Some(&apikey)), "apikey");
        assert_eq!(postman_auth_mode(Some(&oauth)), "none");
        assert_eq!(postman_auth_mode(None), "none");
    }

    // ── pm_auth_param ───────────────────────────────────────────────────────

    #[test]
    fn pm_auth_param_found_and_missing() {
        let auth = json!({
            "type": "basic",
            "basic": [
                { "key": "username", "value": "u" },
                { "key": "password", "value": "p" }
            ]
        });
        assert_eq!(pm_auth_param(&auth, "basic", "username"), "u");
        assert_eq!(pm_auth_param(&auth, "basic", "password"), "p");
        // Missing key → empty string.
        assert_eq!(pm_auth_param(&auth, "basic", "nope"), "");
        // Missing kind → empty string.
        assert_eq!(pm_auth_param(&auth, "bearer", "token"), "");
    }

    // ── postman_auth_block ──────────────────────────────────────────────────

    #[test]
    fn postman_auth_block_none_inputs() {
        assert!(postman_auth_block(None).is_none());
        let no_type = json!({ "foo": "bar" });
        assert!(postman_auth_block(Some(&no_type)).is_none());
        let unknown = json!({ "type": "oauth2" });
        assert!(postman_auth_block(Some(&unknown)).is_none());
    }

    #[test]
    fn postman_auth_block_bearer() {
        let auth = json!({
            "type": "bearer",
            "bearer": [{ "key": "token", "value": "TK" }]
        });
        let block = postman_auth_block(Some(&auth)).unwrap();
        assert!(block.contains("auth:bearer {"));
        assert!(block.contains("token: TK"));
    }

    #[test]
    fn postman_auth_block_basic() {
        let auth = json!({
            "type": "basic",
            "basic": [
                { "key": "username", "value": "u" },
                { "key": "password", "value": "p" }
            ]
        });
        let block = postman_auth_block(Some(&auth)).unwrap();
        assert!(block.contains("auth:basic {"));
        assert!(block.contains("username: u"));
        assert!(block.contains("password: p"));
    }

    #[test]
    fn postman_auth_block_apikey_header_default() {
        let auth = json!({
            "type": "apikey",
            "apikey": [
                { "key": "key", "value": "X-Key" },
                { "key": "value", "value": "secret" }
            ]
        });
        let block = postman_auth_block(Some(&auth)).unwrap();
        assert!(block.contains("auth:apikey {"));
        assert!(block.contains("key: X-Key"));
        assert!(block.contains("value: secret"));
        // No "in" specified → header placement.
        assert!(block.contains("placement: header"));
    }

    #[test]
    fn postman_auth_block_apikey_query_placement() {
        let auth = json!({
            "type": "apikey",
            "apikey": [
                { "key": "key", "value": "k" },
                { "key": "value", "value": "v" },
                { "key": "in", "value": "query" }
            ]
        });
        let block = postman_auth_block(Some(&auth)).unwrap();
        assert!(block.contains("placement: query"));
    }

    // ── postman_request_bru ─────────────────────────────────────────────────

    #[test]
    fn postman_request_bru_defaults_method_get_and_empty_url() {
        // No method, no url → method get, empty url, body none, auth none.
        let req = json!({});
        let bru = postman_request_bru("Thing", &req, 3);
        assert!(bru.contains("meta {"));
        assert!(bru.contains("name: Thing"));
        assert!(bru.contains("seq: 3"));
        assert!(bru.contains("get {"));
        assert!(bru.contains("url: \n") || bru.contains("url: "));
        assert!(bru.contains("body: none"));
        assert!(bru.contains("auth: none"));
    }

    #[test]
    fn postman_request_bru_emits_query_headers_auth_body() {
        let req = json!({
            "method": "POST",
            "url": {
                "raw": "https://api.test/x?q=1",
                "query": [{ "key": "q", "value": "1" }]
            },
            "header": [{ "key": "Accept", "value": "application/json" }],
            "auth": { "type": "bearer", "bearer": [{ "key": "token", "value": "T" }] },
            "body": { "mode": "raw", "raw": "hi" }
        });
        let bru = postman_request_bru("X", &req, 1);
        assert!(bru.contains("post {"));
        assert!(bru.contains("params:query {"));
        assert!(bru.contains("q: 1"));
        assert!(bru.contains("headers {"));
        assert!(bru.contains("Accept: application/json"));
        assert!(bru.contains("auth:bearer {"));
        assert!(bru.contains("token: T"));
        assert!(bru.contains("body:text {"));
    }

    // ── create_collection ───────────────────────────────────────────────────

    #[test]
    fn create_collection_empty_name_falls_back() {
        let parent = tmp("cc-empty");
        let dir = create_collection(&parent, "   ").unwrap();
        assert!(dir.ends_with("Imported Collection"));
        assert!(dir.join("bruno.json").exists());
        assert!(dir.join("environments").exists());
        std::fs::remove_dir_all(&parent).ok();
    }

    #[test]
    fn create_collection_escapes_quotes_in_name() {
        let parent = tmp("cc-esc");
        // Quote in name is sanitized in the dir but escaped in JSON.
        let dir = create_collection(&parent, "He said \"hi\"").unwrap();
        let json = std::fs::read_to_string(dir.join("bruno.json")).unwrap();
        assert!(json.contains("\\\"hi\\\""));
        std::fs::remove_dir_all(&parent).ok();
    }

    #[test]
    fn create_collection_existing_dir_errors() {
        let parent = tmp("cc-dup");
        let _first = create_collection(&parent, "Dup").unwrap();
        let err = create_collection(&parent, "Dup").unwrap_err();
        assert!(err.contains("already exists"));
        std::fs::remove_dir_all(&parent).ok();
    }

    // ── unique_path ─────────────────────────────────────────────────────────

    #[test]
    fn unique_path_empty_stem_becomes_request() {
        let dir = tmp("up-empty");
        let p = unique_path(&dir, "");
        assert!(p.ends_with("request.bru"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unique_path_appends_counter_on_collision() {
        let dir = tmp("up-coll");
        let first = unique_path(&dir, "item");
        std::fs::write(&first, "x").unwrap();
        let second = unique_path(&dir, "item");
        assert!(second.ends_with("item 2.bru"));
        std::fs::write(&second, "x").unwrap();
        let third = unique_path(&dir, "item");
        assert!(third.ends_with("item 3.bru"));
        std::fs::remove_dir_all(&dir).ok();
    }

    // ── import_postman (end to end) ─────────────────────────────────────────

    #[test]
    fn import_postman_invalid_json_errors() {
        let parent = tmp("pm-bad");
        let err = import_postman("{ not json", &parent).unwrap_err();
        assert!(err.contains("invalid JSON"));
        std::fs::remove_dir_all(&parent).ok();
    }

    #[test]
    fn import_postman_missing_info_name_uses_fallback() {
        // No info.name → "Imported Collection"; no item array → empty.
        let parent = tmp("pm-noinfo");
        let dir = import_postman("{}", &parent).unwrap();
        assert!(dir.ends_with("Imported Collection"));
        assert!(dir.join("bruno.json").exists());
        std::fs::remove_dir_all(&parent).ok();
    }

    #[test]
    fn import_postman_blank_info_name_uses_fallback() {
        let parent = tmp("pm-blank");
        let json = r#"{ "info": { "name": "   " }, "item": [] }"#;
        let dir = import_postman(json, &parent).unwrap();
        assert!(dir.ends_with("Imported Collection"));
        std::fs::remove_dir_all(&parent).ok();
    }

    #[test]
    fn import_postman_name_collision_uses_unique_path() {
        // Two requests with the same name → second gets " 2" suffix.
        let parent = tmp("pm-coll");
        let json = r#"{
          "info": { "name": "Coll" },
          "item": [
            { "name": "Same", "request": { "method": "GET", "url": "https://api.test/a" } },
            { "name": "Same", "request": { "method": "GET", "url": "https://api.test/b" } }
          ]
        }"#;
        let dir = import_postman(json, &parent).unwrap();
        assert!(dir.join("Same.bru").exists());
        assert!(dir.join("Same 2.bru").exists());
        std::fs::remove_dir_all(&parent).ok();
    }

    #[test]
    fn import_postman_unnamed_item_uses_default_name() {
        // Item without a name → "item" stem.
        let parent = tmp("pm-unnamed");
        let json = r#"{
          "info": { "name": "U" },
          "item": [
            { "request": { "method": "GET", "url": "https://api.test/z" } }
          ]
        }"#;
        let dir = import_postman(json, &parent).unwrap();
        assert!(dir.join("item.bru").exists());
        std::fs::remove_dir_all(&parent).ok();
    }

    #[test]
    fn import_postman_skips_items_without_request_or_subitems() {
        // An item that is neither a folder nor a request is ignored.
        let parent = tmp("pm-skip");
        let json = r#"{
          "info": { "name": "S" },
          "item": [
            { "name": "Bogus" },
            { "name": "Real", "request": { "method": "GET", "url": "https://api.test/r" } }
          ]
        }"#;
        let dir = import_postman(json, &parent).unwrap();
        assert!(!dir.join("Bogus.bru").exists());
        assert!(dir.join("Real.bru").exists());
        std::fs::remove_dir_all(&parent).ok();
    }

    #[test]
    fn import_postman_nested_folders_write_folder_bru() {
        let parent = tmp("pm-nested");
        let json = r#"{
          "info": { "name": "N" },
          "item": [
            { "name": "Outer", "item": [
              { "name": "Inner", "item": [
                { "name": "Leaf", "request": { "method": "GET", "url": "https://api.test/leaf" } }
              ] }
            ] }
          ]
        }"#;
        let dir = import_postman(json, &parent).unwrap();
        let outer = dir.join("Outer");
        let inner = outer.join("Inner");
        assert!(outer.join("folder.bru").exists());
        assert!(inner.join("folder.bru").exists());
        assert!(inner.join("Leaf.bru").exists());
        let folder_txt = std::fs::read_to_string(outer.join("folder.bru")).unwrap();
        assert!(folder_txt.contains("type: folder"));
        assert!(folder_txt.contains("name: Outer"));
        std::fs::remove_dir_all(&parent).ok();
    }

    #[test]
    fn import_postman_urlencoded_and_basic_auth_roundtrip() {
        let parent = tmp("pm-urlenc");
        let json = r#"{
          "info": { "name": "E" },
          "item": [
            { "name": "Form", "request": {
                "method": "POST",
                "url": "https://api.test/form",
                "auth": { "type": "basic", "basic": [
                  { "key": "username", "value": "u" },
                  { "key": "password", "value": "p" } ] },
                "body": { "mode": "urlencoded", "urlencoded": [
                  { "key": "a", "value": "1" },
                  { "key": "b", "value": "2", "disabled": true } ] }
            } }
          ]
        }"#;
        let dir = import_postman(json, &parent).unwrap();
        let txt = std::fs::read_to_string(dir.join("Form.bru")).unwrap();
        assert!(txt.contains("body: form-urlencoded"));
        assert!(txt.contains("body:form-urlencoded {"));
        assert!(txt.contains("a: 1"));
        assert!(txt.contains("~b: 2"));
        assert!(txt.contains("auth: basic"));
        assert!(txt.contains("username: u"));
        let tree = bru_lang::load_collection(&dir).unwrap();
        assert_eq!(tree.name, "E");
        std::fs::remove_dir_all(&parent).ok();
    }

    #[test]
    fn import_postman_apikey_query_and_graphql_body() {
        let parent = tmp("pm-gql");
        let json = r#"{
          "info": { "name": "G" },
          "item": [
            { "name": "Gql", "request": {
                "method": "POST",
                "url": "https://api.test/graphql",
                "auth": { "type": "apikey", "apikey": [
                  { "key": "key", "value": "api_key" },
                  { "key": "value", "value": "v" },
                  { "key": "in", "value": "query" } ] },
                "body": { "mode": "graphql", "graphql": { "query": "{ ping }" } }
            } }
          ]
        }"#;
        let dir = import_postman(json, &parent).unwrap();
        let txt = std::fs::read_to_string(dir.join("Gql.bru")).unwrap();
        assert!(txt.contains("auth: apikey"));
        assert!(txt.contains("placement: query"));
        assert!(txt.contains("body: graphql"));
        assert!(txt.contains("body:graphql {"));
        assert!(txt.contains("{ ping }"));
        std::fs::remove_dir_all(&parent).ok();
    }
}
