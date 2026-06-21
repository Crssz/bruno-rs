//! Pure bru_core mutation helpers for the gpui client: edit the active verb
//! block's URL, and round-trip a Dict block through editable `key: value` lines
//! (`~` prefix = disabled). No gpui — operates only on `BruFile`. A trimmed port
//! of `bru-app/src/edit.rs`.

use bru_core::{Block, BlockContent, BruFile, Entry, Key, Value, HTTP_VERBS};

/// Index of the method block (a standard verb `get`/`post`/… or custom `http`).
fn method_block_index(file: &BruFile) -> Option<usize> {
    file.blocks
        .iter()
        .position(|b| HTTP_VERBS.contains(&b.name.as_str()) || b.name == "http")
}

/// Set the `url` entry of the active method block. No-op if there's no method
/// block. The value is trimmed (the parser stores `Value::Inline` trimmed).
pub fn set_active_url(file: &mut BruFile, url: &str) {
    let Some(i) = method_block_index(file) else {
        return;
    };
    if let BlockContent::Dict(entries) = &mut file.blocks[i].content {
        set_inline_entry(entries, "url", url);
    }
}

/// Change the HTTP method by renaming the verb block (standard verbs only;
/// a custom `http` block is left untouched).
pub fn set_method(file: &mut BruFile, method: &str) {
    if let Some(i) = method_block_index(file) {
        let m = method.to_lowercase();
        if HTTP_VERBS.contains(&m.as_str()) {
            file.blocks[i].name = m;
        }
    }
}

/// Set or insert an inline `key: value` entry, preserving order on update.
fn set_inline_entry(entries: &mut Vec<Entry>, key: &str, value: &str) {
    if let Some(e) = entries.iter_mut().find(|e| e.key.name() == key) {
        e.value = Value::Inline(value.trim().to_string());
    } else {
        entries.push(Entry {
            annotations: Vec::new(),
            disabled: false,
            local: false,
            key: Key::Bare(key.to_string()),
            value: Value::Inline(value.trim().to_string()),
        });
    }
}

/// Reconcile the `params:path` block with the `:name` tokens in `url`: add new
/// tokens (empty value), drop removed ones, keep existing values, and never
/// materialize an empty block. Ported from iced.
pub fn sync_path_params(file: &mut BruFile, url: &str) {
    let tokens = path_param_tokens(url);
    let has_block = file.blocks.iter().any(|b| b.name == "params:path");
    if tokens.is_empty() && !has_block {
        return;
    }
    if !has_block {
        file.blocks.push(Block {
            name: "params:path".to_string(),
            content: BlockContent::Dict(Vec::new()),
        });
    }
    if let Some(b) = file.blocks.iter_mut().find(|b| b.name == "params:path") {
        if let BlockContent::Dict(entries) = &mut b.content {
            entries.retain(|e| tokens.iter().any(|t| t == e.key.name()));
            for t in &tokens {
                if !entries.iter().any(|e| e.key.name() == t) {
                    entries.push(Entry {
                        annotations: Vec::new(),
                        disabled: false,
                        local: false,
                        key: Key::Bare(t.clone()),
                        value: Value::Inline(String::new()),
                    });
                }
            }
        }
    }
    let empty = file
        .block("params:path")
        .map(|b| matches!(&b.content, BlockContent::Dict(e) if e.is_empty()))
        .unwrap_or(false);
    if empty {
        file.blocks.retain(|b| b.name != "params:path");
    }
}

/// Write edited values back to existing `params:path` entries (by name).
pub fn apply_path_values(file: &mut BruFile, values: &[(String, String)]) {
    if let Some(b) = file.blocks.iter_mut().find(|b| b.name == "params:path") {
        if let BlockContent::Dict(entries) = &mut b.content {
            for e in entries.iter_mut() {
                if let Some((_, v)) = values.iter().find(|(n, _)| n == e.key.name()) {
                    e.value = Value::Inline(v.clone());
                }
            }
        }
    }
}

/// Extract `:name` path-parameter tokens from a URL's path (before `?`).
fn path_param_tokens(url: &str) -> Vec<String> {
    let path = url.split('?').next().unwrap_or(url);
    let bytes = path.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len()
                && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_' || bytes[j] == b'-')
            {
                j += 1;
            }
            if j > start {
                let name = &path[start..j];
                // Skip ports (`:8080`) — purely numeric tokens are not params.
                if !name.chars().all(|c| c.is_ascii_digit()) && !out.iter().any(|t| t == name) {
                    out.push(name.to_string());
                }
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

/// Read a Dict block as `(name, value, enabled)` rows for a structured grid.
pub fn kv_block_rows(file: &BruFile, block: &str) -> Vec<(String, String, bool)> {
    match file.block(block).map(|b| &b.content) {
        Some(BlockContent::Dict(entries)) => entries
            .iter()
            .map(|e| {
                (
                    e.key.name().to_string(),
                    e.value.as_inline().to_string(),
                    !e.disabled,
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Rebuild a Dict block from edited grid rows `(name, value, enabled, local)`,
/// **merging onto existing entries by name** so anything the grid doesn't model
/// survives: decorator `@annotations`, a quoted key, and an untouched non-inline
/// value (multiline / list). A fresh inline entry is built only for a new or
/// renamed name; the value is rewritten only when the user actually changed it.
/// Blank names are dropped; removes the block when empty, creates it if absent.
fn merge_dict_block(
    file: &mut BruFile,
    block: &str,
    rows: impl Iterator<Item = (String, String, bool, bool)>,
) {
    let existing: Vec<Entry> = match file.block(block).map(|b| &b.content) {
        Some(BlockContent::Dict(e)) => e.clone(),
        _ => Vec::new(),
    };
    let entries: Vec<Entry> = rows
        .filter(|(k, _, _, _)| !k.trim().is_empty())
        .map(|(k, v, enabled, local)| {
            let name = k.trim();
            match existing.iter().find(|e| e.key.name() == name) {
                // Existing entry: keep its annotations / key form, update flags,
                // and only replace the value if it actually changed (so a
                // multiline/list value the grid flattened to "" is preserved).
                Some(orig) => {
                    let mut e = orig.clone();
                    e.disabled = !enabled;
                    e.local = local;
                    if v != e.value.as_inline() {
                        e.value = Value::Inline(v);
                    }
                    e
                }
                None => Entry {
                    annotations: Vec::new(),
                    disabled: !enabled,
                    local,
                    key: Key::Bare(name.to_string()),
                    value: Value::Inline(v),
                },
            }
        })
        .collect();
    if entries.is_empty() {
        file.blocks.retain(|b| b.name != block);
        return;
    }
    match file.blocks.iter_mut().find(|b| b.name == block) {
        Some(b) => b.content = BlockContent::Dict(entries),
        None => file.blocks.push(Block {
            name: block.to_string(),
            content: BlockContent::Dict(entries),
        }),
    }
}

/// Replace a Dict block from `(name, value, enabled)` grid rows (blank names
/// dropped). Removes the block when no rows remain; creates it if absent.
/// Merges onto existing entries (see [`merge_dict_block`]); params/headers have
/// no `local` flag, so it is always `false` here.
pub fn set_kv_block(file: &mut BruFile, block: &str, rows: &[(String, String, bool)]) {
    merge_dict_block(
        file,
        block,
        rows.iter()
            .map(|(k, v, en)| (k.clone(), v.clone(), *en, false)),
    );
}

/// Read a vars block as `(name, value, enabled, local)` rows. Unlike
/// [`kv_block_rows`], this carries the `local` (`@`) flag so the Vars grid can
/// preserve it on round-trip (params/headers have no such flag).
pub fn var_block_rows(file: &BruFile, block: &str) -> Vec<(String, String, bool, bool)> {
    match file.block(block).map(|b| &b.content) {
        Some(BlockContent::Dict(entries)) => entries
            .iter()
            .map(|e| {
                (
                    e.key.name().to_string(),
                    e.value.as_inline().to_string(),
                    !e.disabled,
                    e.local,
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Replace a vars block from `(name, value, enabled, local)` grid rows, keeping
/// the `local` flag (serialized as the `@` prefix). Merges onto existing entries
/// (see [`merge_dict_block`]) so `@annotations`, quoted keys, and untouched
/// multiline/list values are preserved. Blank names dropped; removes the block
/// when empty, creates it (as Dict) if absent.
pub fn set_var_block(file: &mut BruFile, block: &str, rows: &[(String, String, bool, bool)]) {
    merge_dict_block(file, block, rows.iter().cloned());
}

/// Set a `key: value` field on the active method block (e.g. the `body`/`auth`
/// mode). No-op if there's no method block.
pub fn set_method_field(file: &mut BruFile, key: &str, value: &str) {
    let Some(i) = method_block_index(file) else {
        return;
    };
    if let BlockContent::Dict(entries) = &mut file.blocks[i].content {
        set_inline_entry(entries, key, value);
    }
}

/// Read a `key` field from the active method block (e.g. the current body mode).
pub fn method_field(file: &BruFile, key: &str) -> Option<String> {
    let i = method_block_index(file)?;
    if let BlockContent::Dict(entries) = &file.blocks[i].content {
        entries
            .iter()
            .find(|e| e.key.name() == key)
            .map(|e| e.value.as_inline().to_string())
    } else {
        None
    }
}

/// Set the `name` entry of the `meta` block — used when renaming a request so
/// the tree label (which reads `meta.name`) follows the new file name.
pub fn set_meta_name(file: &mut BruFile, name: &str) {
    if let Some(b) = file.blocks.iter_mut().find(|b| b.name == "meta") {
        if let BlockContent::Dict(entries) = &mut b.content {
            set_inline_entry(entries, "name", name);
        }
    }
}

/// Set the `seq` entry of the `meta` block (used to reorder sibling requests).
pub fn set_meta_seq(file: &mut BruFile, seq: i64) {
    if let Some(b) = file.blocks.iter_mut().find(|b| b.name == "meta") {
        if let BlockContent::Dict(entries) = &mut b.content {
            set_inline_entry(entries, "seq", &seq.to_string());
        }
    }
}

/// Render a Dict block as editable `key: value` lines (`~` prefix for disabled).
/// Empty string for an absent or non-Dict block.
pub fn dict_to_lines(file: &BruFile, block: &str) -> String {
    let Some(BlockContent::Dict(entries)) = file.block(block).map(|b| &b.content) else {
        return String::new();
    };
    entries
        .iter()
        .map(|e| {
            let prefix = if e.disabled { "~" } else { "" };
            format!("{prefix}{}: {}", e.key.name(), e.value.as_inline())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Replace a Dict block's entries from editable `key: value` lines. Removes the
/// block when no non-blank lines remain; creates it (as Dict) if absent.
pub fn lines_to_dict(file: &mut BruFile, block: &str, text: &str) {
    let entries: Vec<Entry> = text.lines().filter_map(parse_line).collect();
    if entries.is_empty() {
        file.blocks.retain(|b| b.name != block);
        return;
    }
    match file.blocks.iter_mut().find(|b| b.name == block) {
        Some(b) => b.content = BlockContent::Dict(entries),
        None => file.blocks.push(Block {
            name: block.to_string(),
            content: BlockContent::Dict(entries),
        }),
    }
}

/// Parse one editable line into an `Entry`, or `None` for a blank line.
fn parse_line(line: &str) -> Option<Entry> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let (disabled, rest) = match line.strip_prefix('~') {
        Some(r) => (true, r.trim_start()),
        None => (false, line),
    };
    let (key, value) = match rest.split_once(':') {
        Some((k, v)) => (k.trim(), v.trim()),
        None => (rest.trim(), ""),
    };
    Some(Entry {
        annotations: Vec::new(),
        disabled,
        local: false,
        key: Key::Bare(key.to_string()),
        value: Value::Inline(value.to_string()),
    })
}

/// Set a `BlockContent::Text` block (create if absent + non-empty).
pub fn set_text_block(file: &mut BruFile, name: &str, content: String) {
    if let Some(b) = file.blocks.iter_mut().find(|b| b.name == name) {
        b.content = BlockContent::Text(content);
    } else if !content.trim().is_empty() {
        file.blocks.push(Block {
            name: name.to_string(),
            content: BlockContent::Text(content),
        });
    }
}

/// The text content of a `BlockContent::Text` block, or empty.
pub fn text_block(f: &BruFile, name: &str) -> String {
    f.block(name)
        .and_then(|b| match &b.content {
            BlockContent::Text(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Body modes the gpui editor can edit (text-based + url-encoded form). The
/// structured types (multipartForm/graphql/file) need dedicated editors and are
/// intentionally omitted from the cycle for now.
pub const BODY_MODES: &[&str] = &[
    "none",
    "json",
    "text",
    "xml",
    "sparql",
    "formUrlEncoded",
    "multipartForm",
    "graphql",
];
pub const AUTH_MODES: &[&str] = &[
    "none", "inherit", "basic", "bearer", "apikey", "oauth2", "digest", "awsv4",
];

/// The `body:<block>` name for a body mode, or None for `none`/unknown.
pub fn body_block_name(mode: &str) -> Option<&'static str> {
    Some(match mode {
        "json" => "body:json",
        "text" => "body:text",
        "xml" => "body:xml",
        "sparql" => "body:sparql",
        "formUrlEncoded" => "body:form-urlencoded",
        "multipartForm" => "body:multipart-form",
        "graphql" => "body:graphql",
        _ => return None,
    })
}

/// The `auth:<block>` name for an auth mode, or None for none/inherit/unknown.
pub fn auth_block_name(mode: &str) -> Option<&'static str> {
    Some(match mode {
        "basic" => "auth:basic",
        "bearer" => "auth:bearer",
        "apikey" => "auth:apikey",
        "oauth2" => "auth:oauth2",
        "digest" => "auth:digest",
        "awsv4" => "auth:awsv4",
        _ => return None,
    })
}

/// The labeled fields of an auth mode's form: `(label, dict key, is secret)`.
/// Keys mirror bru-core's `project_auth` projection. Empty = no structured form.
pub fn auth_fields(mode: &str) -> &'static [(&'static str, &'static str, bool)] {
    match mode {
        "basic" | "digest" => &[
            ("Username", "username", false),
            ("Password", "password", true),
        ],
        "bearer" => &[("Token", "token", true)],
        "apikey" => &[
            ("Key", "key", false),
            ("Value", "value", true),
            ("Placement (header | queryparams)", "placement", false),
        ],
        "oauth2" => &[
            (
                "Grant Type (client_credentials | password)",
                "grant_type",
                false,
            ),
            ("Access Token URL", "access_token_url", false),
            ("Client Id", "client_id", false),
            ("Client Secret", "client_secret", true),
            ("Scope", "scope", false),
            ("Username", "username", false),
            ("Password", "password", true),
        ],
        "awsv4" => &[
            ("Access Key Id", "accessKeyId", true),
            ("Secret Access Key", "secretAccessKey", true),
            ("Session Token", "sessionToken", true),
            ("Service", "service", false),
            ("Region", "region", false),
            ("Profile Name", "profileName", false),
        ],
        _ => &[],
    }
}
/// Whether a variable name is valid (Bruno's `variableNameRegex` = `^[\w-.]*$`):
/// only letters, digits, `_`, `-`, `.`. Empty is treated as valid (no error).
pub fn valid_var_name(name: &str) -> bool {
    name.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn var_block_rows_carries_local_and_enabled() {
        let src = "vars:post-response {\n  @token: res.body.token\n  ~skip: nope\n  plain: x\n}\n";
        let file = bru_lang::parse(src).expect("parse");
        let rows = var_block_rows(&file, "vars:post-response");
        assert_eq!(rows.len(), 3);
        // @token → local, enabled
        assert_eq!(
            rows[0],
            ("token".into(), "res.body.token".into(), true, true)
        );
        // ~skip → disabled, not local
        assert_eq!(rows[1], ("skip".into(), "nope".into(), false, false));
        // plain → enabled, not local
        assert_eq!(rows[2], ("plain".into(), "x".into(), true, false));
    }

    #[test]
    fn set_var_block_preserves_local_through_serialize() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        set_var_block(
            &mut file,
            "vars:post-response",
            &[
                ("token".into(), "res.body.token".into(), true, true),
                ("audit".into(), "x".into(), false, false),
            ],
        );
        let out = bru_lang::serialize(&file);
        // local var keeps its @; disabled var keeps its ~.
        assert!(out.contains("@token: res.body.token"), "got:\n{out}");
        assert!(out.contains("~audit: x"), "got:\n{out}");
    }

    #[test]
    fn set_var_block_empty_removes_block() {
        let mut file = bru_lang::parse("vars:pre-request {\n  a: 1\n}\n").expect("parse");
        set_var_block(&mut file, "vars:pre-request", &[]);
        assert!(file.block("vars:pre-request").is_none());
    }

    #[test]
    fn set_var_block_preserves_annotations_on_unchanged_value() {
        let src = "vars:pre-request {\n  @description(\"hi\")\n  key: value\n}\n";
        let mut file = bru_lang::parse(src).expect("parse");
        // Round-trip the rows unchanged (what happens when the tab is merely viewed).
        let rows = var_block_rows(&file, "vars:pre-request");
        set_var_block(&mut file, "vars:pre-request", &rows);
        let out = bru_lang::serialize(&file);
        // The serializer re-quotes annotation values with single quotes.
        assert!(
            out.contains("@description('hi')"),
            "annotation lost:\n{out}"
        );
        assert!(out.contains("key: value"), "value changed:\n{out}");
    }

    #[test]
    fn set_var_block_keeps_annotation_when_value_edited() {
        let src = "vars:pre-request {\n  @description(\"hi\")\n  key: old\n}\n";
        let mut file = bru_lang::parse(src).expect("parse");
        set_var_block(
            &mut file,
            "vars:pre-request",
            &[("key".into(), "new".into(), true, false)],
        );
        let out = bru_lang::serialize(&file);
        assert!(
            out.contains("@description('hi')"),
            "annotation lost:\n{out}"
        );
        assert!(out.contains("key: new"), "value not updated:\n{out}");
    }

    #[test]
    fn set_var_block_preserves_quoted_key() {
        let mut file = bru_lang::parse("vars:pre-request {\n  \"my var\": 1\n}\n").expect("parse");
        let rows = var_block_rows(&file, "vars:pre-request");
        assert_eq!(rows[0].0, "my var");
        set_var_block(&mut file, "vars:pre-request", &rows);
        let out = bru_lang::serialize(&file);
        assert!(out.contains("\"my var\": 1"), "quotes lost:\n{out}");
    }

    #[test]
    fn set_var_block_preserves_untouched_multiline_value() {
        let mut file =
            bru_lang::parse("vars:pre-request {\n  body: '''line1\nline2'''\n}\n").expect("parse");
        // The grid flattens a multiline value to "" on read; an unchanged
        // round-trip must keep the original form, not overwrite it with empty.
        let rows = var_block_rows(&file, "vars:pre-request");
        set_var_block(&mut file, "vars:pre-request", &rows);
        let out = bru_lang::serialize(&file);
        // The value survives as a multiline (not flattened to empty); the
        // serializer indents continuation lines, so check the markers + content.
        assert!(out.contains("'''line1"), "multiline start lost:\n{out}");
        assert!(out.contains("line2'''"), "multiline end lost:\n{out}");
    }
}

#[cfg(test)]
mod block_tests {
    use super::*;

    #[test]
    fn text_block_round_trip() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        assert_eq!(text_block(&file, "body:json"), "");
        set_text_block(&mut file, "body:json", "{\"a\":1}".to_string());
        assert_eq!(text_block(&file, "body:json"), "{\"a\":1}");
    }

    #[test]
    fn set_text_block_skips_creating_empty() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        set_text_block(&mut file, "body:text", "   ".to_string());
        assert!(file.block("body:text").is_none());
    }

    #[test]
    fn body_and_auth_block_names() {
        assert_eq!(body_block_name("json"), Some("body:json"));
        assert_eq!(body_block_name("graphql"), Some("body:graphql"));
        assert_eq!(body_block_name("none"), None);
        assert_eq!(body_block_name("bogus"), None);
        assert_eq!(auth_block_name("basic"), Some("auth:basic"));
        assert_eq!(auth_block_name("none"), None);
        assert_eq!(auth_block_name("inherit"), None);
    }

    #[test]
    fn auth_fields_shapes() {
        assert!(auth_fields("none").is_empty());
        let bearer = auth_fields("bearer");
        assert_eq!(bearer.len(), 1);
        assert_eq!(bearer[0], ("Token", "token", true));
        let basic = auth_fields("basic");
        assert_eq!(basic.len(), 2);
        assert_eq!(basic[1], ("Password", "password", true));
        assert_eq!(auth_fields("oauth2").len(), 7);
    }

    #[test]
    fn valid_var_name_matches_bruno_regex() {
        for ok in ["", "token", "base_url", "my-var", "a.b.c", "X1_2-3.4"] {
            assert!(valid_var_name(ok), "should be valid: {ok:?}");
        }
        for bad in ["my var", "a b", "tok!", "a/b", "a:b", "a$b"] {
            assert!(!valid_var_name(bad), "should be invalid: {bad:?}");
        }
    }
}
