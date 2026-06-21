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

#[cfg(test)]
mod cov_tests {
    use super::*;

    // ---- url / method on the active verb block ------------------------------

    #[test]
    fn set_active_url_updates_existing_entry() {
        let mut file = bru_lang::parse("get {\n  url: https://old\n}\n").expect("parse");
        set_active_url(&mut file, "  https://new  ");
        // The value is trimmed.
        assert_eq!(file.dict_value("get", "url"), Some("https://new"));
    }

    #[test]
    fn set_active_url_inserts_when_absent() {
        let mut file = bru_lang::parse("get {\n  body: none\n}\n").expect("parse");
        set_active_url(&mut file, "https://x");
        assert_eq!(file.dict_value("get", "url"), Some("https://x"));
    }

    #[test]
    fn set_active_url_noop_without_method_block() {
        let mut file = bru_lang::parse("meta {\n  name: x\n}\n").expect("parse");
        set_active_url(&mut file, "https://x");
        assert!(file.block("params:path").is_none());
        // No method block means no url entry materialized anywhere.
        assert!(file.block("get").is_none());
    }

    #[test]
    fn set_method_renames_verb_block() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        set_method(&mut file, "POST");
        assert!(file.block("post").is_some());
        assert!(file.block("get").is_none());
    }

    #[test]
    fn set_method_ignores_non_standard_verb() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        set_method(&mut file, "frobnicate");
        // Unknown verb: block name is unchanged.
        assert!(file.block("get").is_some());
    }

    #[test]
    fn set_method_noop_without_method_block() {
        let mut file = bru_lang::parse("meta {\n  name: x\n}\n").expect("parse");
        set_method(&mut file, "post");
        assert!(file.block("post").is_none());
        assert!(file.block("get").is_none());
    }

    // ---- method field round trip --------------------------------------------

    #[test]
    fn set_and_read_method_field() {
        let mut file = bru_lang::parse("post {\n  url: https://x\n}\n").expect("parse");
        assert_eq!(method_field(&file, "body"), None);
        set_method_field(&mut file, "body", "json");
        assert_eq!(method_field(&file, "body"), Some("json".to_string()));
        // Updating an existing entry preserves it (set_inline_entry update branch).
        set_method_field(&mut file, "body", "text");
        assert_eq!(method_field(&file, "body"), Some("text".to_string()));
    }

    #[test]
    fn set_method_field_noop_without_method_block() {
        let mut file = bru_lang::parse("meta {\n  name: x\n}\n").expect("parse");
        set_method_field(&mut file, "body", "json");
        assert_eq!(method_field(&file, "body"), None);
    }

    #[test]
    fn method_field_none_when_missing() {
        let file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        assert_eq!(method_field(&file, "auth"), None);
    }

    // ---- path params ---------------------------------------------------------

    #[test]
    fn sync_path_params_adds_tokens_and_skips_ports() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        // :8080 is a port (numeric) and must be skipped; :id and :userId are params.
        sync_path_params(&mut file, "https://h:8080/u/:userId/p/:id");
        let rows = kv_block_rows(&file, "params:path");
        let names: Vec<&str> = rows.iter().map(|(n, _, _)| n.as_str()).collect();
        assert_eq!(names, vec!["userId", "id"]);
        // New tokens get empty values.
        assert!(rows.iter().all(|(_, v, _)| v.is_empty()));
    }

    #[test]
    fn sync_path_params_keeps_existing_value_and_drops_removed() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        sync_path_params(&mut file, "https://h/:a/:b");
        apply_path_values(
            &mut file,
            &[
                ("a".to_string(), "av".to_string()),
                ("b".to_string(), "bv".to_string()),
            ],
        );
        // Now drop :b, keep :a, add :c.
        sync_path_params(&mut file, "https://h/:a/:c");
        let rows = kv_block_rows(&file, "params:path");
        let names: Vec<&str> = rows.iter().map(|(n, _, _)| n.as_str()).collect();
        assert_eq!(names, vec!["a", "c"]);
        // Existing value for :a survives reconciliation.
        let a = rows.iter().find(|(n, _, _)| n == "a").expect("a row");
        assert_eq!(a.1, "av");
    }

    #[test]
    fn sync_path_params_removes_block_when_no_tokens() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        sync_path_params(&mut file, "https://h/:a");
        assert!(file.block("params:path").is_some());
        // URL with no tokens -> the now-empty block is removed.
        sync_path_params(&mut file, "https://h/plain");
        assert!(file.block("params:path").is_none());
    }

    #[test]
    fn sync_path_params_noop_when_no_tokens_and_no_block() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        sync_path_params(&mut file, "https://h/plain");
        assert!(file.block("params:path").is_none());
    }

    #[test]
    fn sync_path_params_ignores_query_segment() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        // Tokens after `?` (the query) must not be treated as path params.
        sync_path_params(&mut file, "https://h/:a?x=:b");
        let rows = kv_block_rows(&file, "params:path");
        let names: Vec<&str> = rows.iter().map(|(n, _, _)| n.as_str()).collect();
        assert_eq!(names, vec!["a"]);
    }

    #[test]
    fn sync_path_params_dedupes_repeated_token() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        sync_path_params(&mut file, "https://h/:a/:a");
        let rows = kv_block_rows(&file, "params:path");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "a");
    }

    #[test]
    fn apply_path_values_noop_without_block() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        // No params:path block: nothing to write to, must not panic or create.
        apply_path_values(&mut file, &[("a".to_string(), "v".to_string())]);
        assert!(file.block("params:path").is_none());
    }

    #[test]
    fn apply_path_values_ignores_unknown_names() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        sync_path_params(&mut file, "https://h/:a");
        // "zzz" doesn't match an entry: the existing "a" entry is left empty.
        apply_path_values(&mut file, &[("zzz".to_string(), "v".to_string())]);
        let rows = kv_block_rows(&file, "params:path");
        assert_eq!(rows[0].0, "a");
        assert_eq!(rows[0].1, "");
    }

    // ---- kv block (headers/params: no local flag) ---------------------------

    #[test]
    fn kv_block_rows_reads_enabled_flag() {
        let src = "headers {\n  Accept: application/json\n  ~X-Skip: 1\n}\n";
        let file = bru_lang::parse(src).expect("parse");
        let rows = kv_block_rows(&file, "headers");
        assert_eq!(rows[0], ("Accept".into(), "application/json".into(), true));
        assert_eq!(rows[1], ("X-Skip".into(), "1".into(), false));
    }

    #[test]
    fn kv_block_rows_empty_for_absent_or_non_dict() {
        let file = bru_lang::parse("body:json {\n  {}\n}\n").expect("parse");
        // Absent block.
        assert!(kv_block_rows(&file, "headers").is_empty());
        // Present but Text content, not Dict.
        assert!(kv_block_rows(&file, "body:json").is_empty());
    }

    #[test]
    fn set_kv_block_creates_and_drops_blank_names() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        set_kv_block(
            &mut file,
            "headers",
            &[
                ("Accept".into(), "text/plain".into(), true),
                ("   ".into(), "dropme".into(), true),
            ],
        );
        let rows = kv_block_rows(&file, "headers");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "Accept");
    }

    #[test]
    fn set_kv_block_empty_removes_block() {
        let mut file = bru_lang::parse("headers {\n  Accept: x\n}\n").expect("parse");
        set_kv_block(&mut file, "headers", &[]);
        assert!(file.block("headers").is_none());
    }

    #[test]
    fn set_kv_block_merges_onto_existing_disabled_state() {
        let mut file = bru_lang::parse("headers {\n  Accept: json\n}\n").expect("parse");
        // Flip Accept to disabled and change its value (merge update branch).
        set_kv_block(
            &mut file,
            "headers",
            &[("Accept".into(), "xml".into(), false)],
        );
        let rows = kv_block_rows(&file, "headers");
        assert_eq!(rows[0], ("Accept".into(), "xml".into(), false));
    }

    #[test]
    fn set_kv_block_replaces_existing_block_content() {
        let mut file = bru_lang::parse("headers {\n  Old: 1\n}\n").expect("parse");
        // New name entirely -> None branch in merge builds a fresh entry; the
        // block already exists so the Some(b) replace branch runs.
        set_kv_block(&mut file, "headers", &[("New".into(), "2".into(), true)]);
        let rows = kv_block_rows(&file, "headers");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "New");
    }

    // ---- vars block: local flag is always false through set_kv_block --------

    #[test]
    fn set_kv_block_never_sets_local() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        set_kv_block(&mut file, "params:query", &[("q".into(), "1".into(), true)]);
        let rows = var_block_rows(&file, "params:query");
        // local flag (4th element) must be false.
        assert!(!rows[0].3);
    }

    // ---- meta name / seq -----------------------------------------------------

    #[test]
    fn set_meta_name_updates_existing_meta() {
        let mut file = bru_lang::parse("meta {\n  name: Old\n  seq: 1\n}\n").expect("parse");
        set_meta_name(&mut file, "New");
        assert_eq!(file.dict_value("meta", "name"), Some("New"));
    }

    #[test]
    fn set_meta_name_inserts_when_absent() {
        let mut file = bru_lang::parse("meta {\n  seq: 1\n}\n").expect("parse");
        set_meta_name(&mut file, "Added");
        assert_eq!(file.dict_value("meta", "name"), Some("Added"));
    }

    #[test]
    fn set_meta_name_noop_without_meta_block() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        set_meta_name(&mut file, "x");
        assert!(file.block("meta").is_none());
    }

    #[test]
    fn set_meta_seq_writes_number_as_string() {
        let mut file = bru_lang::parse("meta {\n  name: X\n  seq: 1\n}\n").expect("parse");
        set_meta_seq(&mut file, 42);
        assert_eq!(file.dict_value("meta", "seq"), Some("42"));
        assert_eq!(file.seq(), Some(42));
    }

    #[test]
    fn set_meta_seq_noop_without_meta_block() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        set_meta_seq(&mut file, 7);
        assert!(file.block("meta").is_none());
    }

    // ---- dict <-> editable lines round trip ---------------------------------

    #[test]
    fn dict_to_lines_renders_disabled_prefix() {
        let src = "headers {\n  Accept: json\n  ~X-Skip: 1\n}\n";
        let file = bru_lang::parse(src).expect("parse");
        let lines = dict_to_lines(&file, "headers");
        assert_eq!(lines, "Accept: json\n~X-Skip: 1");
    }

    #[test]
    fn dict_to_lines_empty_for_absent_or_non_dict() {
        let file = bru_lang::parse("body:json {\n  raw\n}\n").expect("parse");
        assert_eq!(dict_to_lines(&file, "headers"), "");
        assert_eq!(dict_to_lines(&file, "body:json"), "");
    }

    #[test]
    fn lines_to_dict_parses_enabled_disabled_and_missing_colon() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        // Three lines: a normal pair, a disabled pair, a key with no colon
        // (value becomes empty), plus a blank line that is filtered out.
        lines_to_dict(&mut file, "headers", "A: 1\n~B: 2\n\nC");
        let rows = kv_block_rows(&file, "headers");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], ("A".into(), "1".into(), true));
        assert_eq!(rows[1], ("B".into(), "2".into(), false));
        // "C" with no colon -> empty value, enabled.
        assert_eq!(rows[2], ("C".into(), "".into(), true));
    }

    #[test]
    fn lines_to_dict_empty_removes_block() {
        let mut file = bru_lang::parse("headers {\n  A: 1\n}\n").expect("parse");
        // Only blank/whitespace lines -> block removed.
        lines_to_dict(&mut file, "headers", "   \n\n  ");
        assert!(file.block("headers").is_none());
    }

    #[test]
    fn lines_to_dict_creates_block_when_absent() {
        let mut file = bru_lang::parse("get {\n  url: https://x\n}\n").expect("parse");
        lines_to_dict(&mut file, "params:query", "q: 1");
        let rows = kv_block_rows(&file, "params:query");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "q");
    }

    #[test]
    fn lines_to_dict_replaces_existing_block() {
        let mut file = bru_lang::parse("headers {\n  Old: 1\n}\n").expect("parse");
        lines_to_dict(&mut file, "headers", "New: 2");
        let rows = kv_block_rows(&file, "headers");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], ("New".into(), "2".into(), true));
    }

    // ---- BODY_MODES / AUTH_MODES constants ----------------------------------

    #[test]
    fn mode_constants_have_expected_membership() {
        assert!(BODY_MODES.contains(&"none"));
        assert!(BODY_MODES.contains(&"formUrlEncoded"));
        assert!(AUTH_MODES.contains(&"inherit"));
        assert!(AUTH_MODES.contains(&"awsv4"));
        // Every concrete body mode (except none) has a block name.
        for m in BODY_MODES.iter().filter(|m| **m != "none") {
            assert!(body_block_name(m).is_some(), "no block name for {m:?}");
        }
        // Every concrete auth mode (except none/inherit) has a block name.
        for m in AUTH_MODES
            .iter()
            .filter(|m| **m != "none" && **m != "inherit")
        {
            assert!(auth_block_name(m).is_some(), "no block name for {m:?}");
        }
    }

    #[test]
    fn auth_fields_apikey_and_awsv4_and_digest_shapes() {
        // digest shares the basic shape.
        let digest = auth_fields("digest");
        assert_eq!(digest.len(), 2);
        assert_eq!(digest[0].1, "username");
        let apikey = auth_fields("apikey");
        assert_eq!(apikey.len(), 3);
        assert_eq!(apikey[2].1, "placement");
        let aws = auth_fields("awsv4");
        assert_eq!(aws.len(), 6);
        assert_eq!(aws[0].1, "accessKeyId");
        // unknown mode -> empty
        assert!(auth_fields("inherit").is_empty());
    }
}
