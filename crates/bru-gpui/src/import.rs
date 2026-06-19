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
