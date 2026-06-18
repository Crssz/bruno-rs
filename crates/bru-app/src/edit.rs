//! In-place mutation of a [`BruFile`] from structured edits. The GUI keeps one
//! `BruFile` per open request as the source of truth; these helpers translate a
//! field change into a block/entry mutation, after which the file is re-projected
//! (`to_request`) for display and serialized (`bru_lang::serialize`) on save.
//!
//! Text bodies (json/xml/scripts/…) are stored 2-space indented on disk (Bruno's
//! `indentString`); the editors hold the *outdented* payload, so writes re-indent.

use bru_core::{Block, BlockContent, BruFile, Entry, Key, Value, HTTP_VERBS};

/// The method block: a standard verb (`get`/`post`/…) or a custom `http { method }`.
fn method_block_index(file: &BruFile) -> Option<usize> {
    file.blocks
        .iter()
        .position(|b| HTTP_VERBS.contains(&b.name.as_str()) || b.name == "http")
}

/// Change the HTTP method, preserving the method block's url/body/auth dict.
pub fn set_method(file: &mut BruFile, method: &str) {
    let target = method.to_lowercase();
    let Some(i) = method_block_index(file) else {
        return;
    };
    if HTTP_VERBS.contains(&target.as_str()) {
        file.blocks[i].name = target;
        if let BlockContent::Dict(entries) = &mut file.blocks[i].content {
            entries.retain(|e| e.key.name() != "method");
        }
    } else {
        file.blocks[i].name = "http".to_string();
        if let BlockContent::Dict(entries) = &mut file.blocks[i].content {
            set_entry(entries, "method", &target);
        }
    }
}

/// Set a key inside the method block's dict (`url`, `body`, `auth`).
pub fn set_method_field(file: &mut BruFile, key: &str, value: &str) {
    let Some(i) = method_block_index(file) else {
        return;
    };
    if let BlockContent::Dict(entries) = &mut file.blocks[i].content {
        set_entry(entries, key, value);
    }
}

/// Set the request URL.
pub fn set_url(file: &mut BruFile, url: &str) {
    set_method_field(file, "url", url);
}

/// Set or insert an inline `key: value` entry in a dict, preserving order.
/// The value is trimmed because the parser stores `Value::Inline` trimmed, so an
/// untrimmed value would silently change on the next reload.
pub fn set_entry(entries: &mut Vec<Entry>, key: &str, value: &str) {
    if let Some(e) = entries.iter_mut().find(|e| e.key.name() == key) {
        e.value = Value::Inline(value.trim().to_string());
    } else {
        entries.push(new_entry(key, value));
    }
}

pub fn new_entry(key: &str, value: &str) -> Entry {
    Entry {
        annotations: Vec::new(),
        disabled: false,
        local: false,
        key: Key::Bare(key.to_string()),
        value: Value::Inline(value.trim().to_string()),
    }
}

/// Borrow (creating if absent) the entries of a dictionary block.
pub fn dict_block_mut<'a>(file: &'a mut BruFile, name: &str) -> &'a mut Vec<Entry> {
    let idx = match file.blocks.iter().position(|b| b.name == name) {
        Some(i) => i,
        None => {
            file.blocks.push(Block {
                name: name.to_string(),
                content: BlockContent::Dict(Vec::new()),
            });
            file.blocks.len() - 1
        }
    };
    // The block may have been parsed as Text/List; coerce to Dict for editing.
    if !matches!(file.blocks[idx].content, BlockContent::Dict(_)) {
        file.blocks[idx].content = BlockContent::Dict(Vec::new());
    }
    match &mut file.blocks[idx].content {
        BlockContent::Dict(entries) => entries,
        _ => unreachable!(),
    }
}

/// Replace the verbatim text of a text block, re-indenting the payload 2 spaces
/// per line (the inverse of `request::outdent`). An empty payload empties the
/// block. Creates the block if absent.
pub fn set_text_block(file: &mut BruFile, name: &str, payload: &str) {
    let stored = indent2(payload);
    match file.blocks.iter_mut().find(|b| b.name == name) {
        Some(b) => b.content = BlockContent::Text(stored),
        None => file.blocks.push(Block {
            name: name.to_string(),
            content: BlockContent::Text(stored),
        }),
    }
}

/// Append a verbatim text block (used to add a new `example`). Unlike
/// [`set_text_block`] this never replaces an existing block and does not
/// re-indent — the caller supplies the exact stored text.
pub fn push_text_block(file: &mut BruFile, name: &str, text: String) {
    file.blocks.push(Block {
        name: name.to_string(),
        content: BlockContent::Text(text),
    });
}

/// Read `meta.tags` (a list value), if present.
pub fn meta_tags(file: &BruFile) -> Vec<String> {
    match file.block("meta").map(|b| &b.content) {
        Some(BlockContent::Dict(entries)) => entries
            .iter()
            .find(|e| e.key.name() == "tags")
            .map(|e| match &e.value {
                Value::List(items) => items.clone(),
                _ => Vec::new(),
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Write `meta.tags` as a list value (removing the entry when empty).
pub fn set_meta_tags(file: &mut BruFile, tags: Vec<String>) {
    let entries = dict_block_mut(file, "meta");
    if tags.is_empty() {
        entries.retain(|e| e.key.name() != "tags");
    } else if let Some(e) = entries.iter_mut().find(|e| e.key.name() == "tags") {
        e.value = Value::List(tags);
    } else {
        let mut e = new_entry("tags", "");
        e.value = Value::List(tags);
        entries.push(e);
    }
}

/// Toggle the `disabled` flag of the entry at `idx` in a dict block.
pub fn toggle_entry(file: &mut BruFile, block: &str, idx: usize, enabled: bool) {
    let entries = dict_block_mut(file, block);
    if let Some(e) = entries.get_mut(idx) {
        e.disabled = !enabled;
    }
}

/// Set the `@`-local flag of the entry at `idx` (meaningful in `vars:*` blocks).
pub fn set_entry_local(file: &mut BruFile, block: &str, idx: usize, local: bool) {
    let entries = dict_block_mut(file, block);
    if let Some(e) = entries.get_mut(idx) {
        e.local = local;
    }
}

/// Set the key (name) of the entry at `idx`, preserving its quoting style.
pub fn set_entry_key(file: &mut BruFile, block: &str, idx: usize, name: &str) {
    let entries = dict_block_mut(file, block);
    if let Some(e) = entries.get_mut(idx) {
        e.key = match &e.key {
            Key::Quoted(_) => Key::Quoted(name.to_string()),
            Key::Bare(_) => Key::Bare(name.to_string()),
        };
    }
}

/// Set the inline value of the entry at `idx`. A `'''multiline'''` or list value
/// is left untouched (overwriting it from a single-line cell would silently drop
/// the original form and any `@contentType`); only Inline values are editable here.
pub fn set_entry_value(file: &mut BruFile, block: &str, idx: usize, value: &str) {
    let entries = dict_block_mut(file, block);
    if let Some(e) = entries.get_mut(idx) {
        if matches!(e.value, Value::Inline(_)) {
            e.value = Value::Inline(value.trim().to_string());
        }
    }
}

/// Replace a dict block's entries with `(name, value, enabled)` rows (used by
/// bulk edit). To avoid clobbering data the bulk text can't represent, a row
/// whose name matches an existing entry reuses that entry's annotations, key
/// quoting, `@`-local flag, and — when the bulk value is left empty — its
/// original `'''multiline'''`/list value. Removes the block if `rows` is empty.
pub fn replace_block_entries(
    file: &mut BruFile,
    block: &str,
    rows: Vec<(String, String, bool, bool)>,
) {
    if rows.is_empty() {
        file.blocks.retain(|b| b.name != block);
        return;
    }
    // Snapshot old entries into a per-name FIFO so duplicate keys (e.g. two
    // same-named headers, one disabled) each keep their own annotations/quoting/
    // value instead of collapsing onto a single last-write-wins entry.
    let mut old: std::collections::HashMap<String, std::collections::VecDeque<Entry>> =
        std::collections::HashMap::new();
    if let Some(BlockContent::Dict(entries)) = file.block(block).map(|b| &b.content) {
        for e in entries {
            old.entry(e.key.name().to_string())
                .or_default()
                .push_back(e.clone());
        }
    }
    let entries = dict_block_mut(file, block);
    entries.clear();
    for (name, value, enabled, local) in rows {
        let mut e = match old.get_mut(&name).and_then(|q| q.pop_front()) {
            Some(prev) => {
                let mut e = prev.clone();
                // Keep a non-Inline value only when the user left the cell empty.
                if !value.is_empty() || matches!(prev.value, Value::Inline(_)) {
                    e.value = Value::Inline(value);
                }
                e
            }
            None => new_entry(&name, &value),
        };
        e.disabled = !enabled;
        // `@`-local is explicit in the bulk text, so it survives a rename.
        e.local = local;
        entries.push(e);
    }
}

/// Append a blank `key: value` row to a dict block.
pub fn add_row(file: &mut BruFile, block: &str) {
    dict_block_mut(file, block).push(new_entry("", ""));
}

/// Update the *selected* `body:file` entry's path + content-type in place,
/// preserving any other candidate file entries. Creates a single selected entry
/// when the block is empty/absent.
pub fn set_file_body(file: &mut BruFile, path: &str, content_type: Option<&str>) {
    let ct = content_type
        .filter(|c| !c.is_empty())
        .map(|c| format!(" @contentType({c})"))
        .unwrap_or_default();
    let value = format!("@file({path}){ct}");
    let entries = dict_block_mut(file, "body:file");
    let idx = entries
        .iter()
        .position(|e| !e.disabled)
        .or(if entries.is_empty() { None } else { Some(0) });
    match idx {
        Some(i) => entries[i].value = Value::Inline(value),
        None => entries.push(new_entry("file", &value)),
    }
}

/// Remove the row at `idx` from a dict block.
pub fn remove_row(file: &mut BruFile, block: &str, idx: usize) {
    let entries = dict_block_mut(file, block);
    if idx < entries.len() {
        entries.remove(idx);
    }
}

/// Reconcile the `params:path` block from `:name` tokens in the URL (Bruno keeps
/// path params URL-derived): adds entries for new tokens, drops entries whose
/// token is gone, preserves values for surviving ones. Purely-numeric tokens
/// (e.g. a `:8080` port) are ignored.
pub fn sync_path_params(file: &mut BruFile, url: &str) {
    let tokens = path_param_tokens(url);
    let has_block = file.blocks.iter().any(|b| b.name == "params:path");
    // Don't materialize an empty `params:path {}` block for a param-less URL.
    if tokens.is_empty() && !has_block {
        return;
    }
    let entries = dict_block_mut(file, "params:path");
    entries.retain(|e| tokens.iter().any(|t| t == e.key.name()));
    for t in &tokens {
        if !entries.iter().any(|e| e.key.name() == t) {
            entries.push(new_entry(t, ""));
        }
    }
    // When the last path param is removed, drop the now-empty block (canonical).
    if entries.is_empty() {
        file.blocks.retain(|b| b.name != "params:path");
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

/// Prefix every line with two spaces (Bruno's body indentation). Empty in → empty.
fn indent2(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    s.split('\n')
        .map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            format!("  {line}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use bru_core::Body;

    fn parse(s: &str) -> BruFile {
        bru_lang::parse(s).unwrap()
    }

    const SRC: &str = "meta {\n  name: X\n  type: http\n}\n\nget {\n  url: https://a.test\n  body: none\n  auth: none\n}\n";

    #[test]
    fn change_method_keeps_url() {
        let mut f = parse(SRC);
        set_method(&mut f, "POST");
        let r = f.to_request().unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.url, "https://a.test");
    }

    #[test]
    fn set_url_roundtrips() {
        let mut f = parse(SRC);
        set_url(&mut f, "https://b.test/x");
        assert_eq!(f.to_request().unwrap().url, "https://b.test/x");
    }

    #[test]
    fn add_and_edit_header() {
        let mut f = parse(SRC);
        add_row(&mut f, "headers");
        set_entry_key(&mut f, "headers", 0, "X-Test");
        set_entry_value(&mut f, "headers", 0, "1");
        let r = f.to_request().unwrap();
        assert_eq!(r.headers.len(), 1);
        assert_eq!(r.headers[0].name, "X-Test");
        assert_eq!(r.headers[0].value, "1");
    }

    #[test]
    fn json_body_is_reindented() {
        let mut f = parse(SRC);
        set_method_field(&mut f, "body", "json");
        set_text_block(&mut f, "body:json", "{\n  \"a\": 1\n}");
        match f.to_request().unwrap().body {
            Body::Json(s) => assert_eq!(s, "{\n  \"a\": 1\n}"),
            other => panic!("expected json body, got {other:?}"),
        }
    }

    #[test]
    fn bulk_replace_preserves_annotations_and_local() {
        let src = "vars:pre-request {\n  @description(\"keep\")\n  key: value\n  @tok: secret\n}\n";
        let mut f = parse(src);
        // Rows mirror the bulk projection (name, value, enabled) with no edits.
        replace_block_entries(
            &mut f,
            "vars:pre-request",
            vec![
                ("key".into(), "value".into(), true, false),
                ("tok".into(), "secret".into(), true, true),
            ],
        );
        let out = bru_lang::serialize(&f);
        assert!(out.contains("@description"), "annotation dropped:\n{out}");
        assert!(out.contains("@tok"), "local-var @ flag dropped:\n{out}");
    }

    #[test]
    fn bulk_replace_keeps_local_on_rename() {
        let src = "vars:pre-request {\n  @tok: secret\n}\n";
        let mut f = parse(src);
        // Rename @tok -> @token via bulk rows (local flag carried explicitly).
        replace_block_entries(
            &mut f,
            "vars:pre-request",
            vec![("token".into(), "secret".into(), true, true)],
        );
        let out = bru_lang::serialize(&f);
        assert!(out.contains("@token"), "local @ lost on rename:\n{out}");
    }

    #[test]
    fn toggle_disables_entry() {
        let mut f = parse(SRC);
        add_row(&mut f, "headers");
        set_entry_key(&mut f, "headers", 0, "k");
        toggle_entry(&mut f, "headers", 0, false);
        assert!(!f.to_request().unwrap().headers[0].enabled);
    }

    // ── set_method ─────────────────────────────────────────────────────────
    #[test]
    fn set_method_no_method_block_is_noop() {
        // A file with only meta (no method block) -> early return.
        let mut f = parse("meta {\n  name: X\n  type: http\n}\n");
        set_method(&mut f, "POST"); // must not panic
        assert!(f.to_request().is_none());
    }

    #[test]
    fn set_method_standard_verb_drops_method_key() {
        // Start from a custom http verb, switch to a standard verb: the `method`
        // entry must be removed from the dict.
        let src = "meta {\n  name: X\n  type: http\n}\n\nhttp {\n  url: https://a.test\n  body: none\n  auth: none\n  method: purge\n}\n";
        let mut f = parse(src);
        set_method(&mut f, "GET");
        let block = f.block("get").unwrap();
        if let BlockContent::Dict(entries) = &block.content {
            assert!(entries.iter().all(|e| e.key.name() != "method"));
        } else {
            panic!("expected dict");
        }
        assert_eq!(f.to_request().unwrap().method, "GET");
    }

    #[test]
    fn set_method_custom_verb_uses_http_block() {
        let mut f = parse(SRC);
        set_method(&mut f, "PURGE");
        // Method block renamed to `http` with a method entry.
        assert!(f.block("http").is_some());
        assert_eq!(f.to_request().unwrap().method, "PURGE");
    }

    // ── set_file_body ──────────────────────────────────────────────────────
    #[test]
    fn set_file_body_creates_with_content_type() {
        let mut f = parse(SRC);
        set_file_body(&mut f, "/tmp/x.bin", Some("application/octet-stream"));
        let block = f.block("body:file").unwrap();
        if let BlockContent::Dict(entries) = &block.content {
            assert_eq!(entries.len(), 1);
            if let Value::Inline(v) = &entries[0].value {
                assert!(v.contains("@file(/tmp/x.bin)"));
                assert!(v.contains("@contentType(application/octet-stream)"));
            } else {
                panic!("expected inline");
            }
        }
    }

    #[test]
    fn set_file_body_no_content_type() {
        let mut f = parse(SRC);
        set_file_body(&mut f, "/tmp/y.bin", None);
        let block = f.block("body:file").unwrap();
        if let BlockContent::Dict(entries) = &block.content {
            if let Value::Inline(v) = &entries[0].value {
                assert!(v.contains("@file(/tmp/y.bin)"));
                assert!(!v.contains("@contentType"));
            }
        }
        // Empty content_type also yields no @contentType.
        let mut f2 = parse(SRC);
        set_file_body(&mut f2, "/tmp/z.bin", Some(""));
        if let Some(BlockContent::Dict(entries)) = f2.block("body:file").map(|b| &b.content) {
            if let Value::Inline(v) = &entries[0].value {
                assert!(!v.contains("@contentType"));
            }
        }
    }

    #[test]
    fn set_file_body_updates_selected_entry() {
        // First enabled (non-disabled) entry is updated, others preserved.
        let src = "meta {\n  name: X\n  type: http\n}\n\nget {\n  url: x\n  body: file\n  auth: none\n}\n\nbody:file {\n  ~old: @file(/old)\n  cur: @file(/cur)\n}\n";
        let mut f = parse(src);
        set_file_body(&mut f, "/new", None);
        if let Some(BlockContent::Dict(entries)) = f.block("body:file").map(|b| &b.content) {
            // The disabled entry is at index 0; first non-disabled (index 1) updated.
            let updated = entries.iter().find(|e| !e.disabled).unwrap();
            if let Value::Inline(v) = &updated.value {
                assert!(v.contains("/new"));
            }
        }
    }

    #[test]
    fn set_file_body_all_disabled_updates_first() {
        // No non-disabled entry, but block non-empty -> updates index 0.
        let src = "meta {\n  name: X\n  type: http\n}\n\nget {\n  url: x\n  body: file\n  auth: none\n}\n\nbody:file {\n  ~a: @file(/a)\n}\n";
        let mut f = parse(src);
        set_file_body(&mut f, "/b", None);
        if let Some(BlockContent::Dict(entries)) = f.block("body:file").map(|b| &b.content) {
            if let Value::Inline(v) = &entries[0].value {
                assert!(v.contains("/b"));
            }
        }
    }

    // ── meta_tags / set_meta_tags ──────────────────────────────────────────
    #[test]
    fn meta_tags_empty_and_present() {
        let f = parse(SRC);
        assert!(meta_tags(&f).is_empty());
        // Build a tags list via set_meta_tags, serialize, reparse, then read back
        // (the list serializes to a multiline `[ ... ]` form the parser accepts).
        let mut writer = parse(SRC);
        set_meta_tags(&mut writer, vec!["a".into(), "b".into()]);
        let f2 = parse(&bru_lang::serialize(&writer));
        assert_eq!(meta_tags(&f2), vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn meta_tags_non_list_value_is_empty() {
        // A `tags` entry that is not a list -> Vec::new().
        let f = parse("meta {\n  name: X\n  type: http\n  tags: scalar\n}\n");
        assert!(meta_tags(&f).is_empty());
    }

    #[test]
    fn set_meta_tags_add_replace_remove() {
        let mut f = parse(SRC);
        // Add.
        set_meta_tags(&mut f, vec!["one".into(), "two".into()]);
        assert_eq!(meta_tags(&f), vec!["one".to_string(), "two".to_string()]);
        // Replace existing.
        set_meta_tags(&mut f, vec!["three".into()]);
        assert_eq!(meta_tags(&f), vec!["three".to_string()]);
        // Remove when empty.
        set_meta_tags(&mut f, vec![]);
        assert!(meta_tags(&f).is_empty());
    }

    // ── sync_path_params ───────────────────────────────────────────────────
    #[test]
    fn sync_path_params_no_tokens_no_block_is_noop() {
        let mut f = parse(SRC);
        sync_path_params(&mut f, "https://a.test/plain");
        assert!(f.block("params:path").is_none());
    }

    #[test]
    fn sync_path_params_adds_drops_and_skips_port() {
        let mut f = parse(SRC);
        // :8080 is a port (numeric) -> skipped; :id and :sub added.
        sync_path_params(&mut f, "https://a.test:8080/u/:id/x/:sub");
        let tags: Vec<String> = block_names(&f, "params:path");
        assert!(tags.contains(&"id".to_string()));
        assert!(tags.contains(&"sub".to_string()));
        assert!(!tags.contains(&"8080".to_string()));

        // Dropping :sub from the URL removes it; :id survives.
        sync_path_params(&mut f, "https://a.test/u/:id");
        let tags2 = block_names(&f, "params:path");
        assert!(tags2.contains(&"id".to_string()));
        assert!(!tags2.contains(&"sub".to_string()));
    }

    #[test]
    fn sync_path_params_drops_now_empty_block() {
        let mut f = parse(SRC);
        sync_path_params(&mut f, "https://a.test/:id");
        assert!(f.block("params:path").is_some());
        // Remove the last param -> the block itself is dropped.
        sync_path_params(&mut f, "https://a.test/plain");
        assert!(f.block("params:path").is_none());
    }

    // Helper: collect the key names of a dict block.
    fn block_names(f: &BruFile, block: &str) -> Vec<String> {
        match f.block(block).map(|b| &b.content) {
            Some(BlockContent::Dict(entries)) => {
                entries.iter().map(|e| e.key.name().to_string()).collect()
            }
            _ => Vec::new(),
        }
    }

    // ── remove_row ─────────────────────────────────────────────────────────
    #[test]
    fn remove_row_in_and_out_of_range() {
        let mut f = parse(SRC);
        add_row(&mut f, "headers");
        add_row(&mut f, "headers");
        set_entry_key(&mut f, "headers", 0, "a");
        set_entry_key(&mut f, "headers", 1, "b");
        // Out of range -> no-op.
        remove_row(&mut f, "headers", 99);
        assert_eq!(block_names(&f, "headers").len(), 2);
        // In range.
        remove_row(&mut f, "headers", 0);
        assert_eq!(block_names(&f, "headers"), vec!["b".to_string()]);
    }

    // ── set_entry_local ────────────────────────────────────────────────────
    #[test]
    fn set_entry_local_toggles_flag() {
        let mut f = parse("vars:pre-request {\n  k: v\n}\n");
        set_entry_local(&mut f, "vars:pre-request", 0, true);
        if let Some(BlockContent::Dict(entries)) = f.block("vars:pre-request").map(|b| &b.content) {
            assert!(entries[0].local);
        }
        // Out of range -> no-op.
        set_entry_local(&mut f, "vars:pre-request", 99, true);
    }

    // ── push_text_block ────────────────────────────────────────────────────
    #[test]
    fn push_text_block_appends_verbatim() {
        let mut f = parse(SRC);
        push_text_block(&mut f, "example", "  name: e\n".to_string());
        let blocks: Vec<&str> = f
            .blocks
            .iter()
            .filter(|b| b.name == "example")
            .map(|b| b.name.as_str())
            .collect();
        assert_eq!(blocks.len(), 1);
        // Pushing again adds a second one (never replaces).
        push_text_block(&mut f, "example", "  name: e2\n".to_string());
        assert_eq!(f.blocks.iter().filter(|b| b.name == "example").count(), 2);
    }

    // ── set_text_block ─────────────────────────────────────────────────────
    #[test]
    fn set_text_block_create_replace_empty() {
        let mut f = parse(SRC);
        // Create absent.
        set_text_block(&mut f, "body:json", "{\n  \"a\": 1\n}");
        if let Some(BlockContent::Text(t)) = f.block("body:json").map(|b| &b.content) {
            // Re-indented 2 spaces per line.
            assert!(t.starts_with("  {"));
        } else {
            panic!("expected text block");
        }
        // Replace existing.
        set_text_block(&mut f, "body:json", "replaced");
        if let Some(BlockContent::Text(t)) = f.block("body:json").map(|b| &b.content) {
            assert_eq!(t, "  replaced");
        }
        // Empty payload empties the block.
        set_text_block(&mut f, "body:json", "");
        if let Some(BlockContent::Text(t)) = f.block("body:json").map(|b| &b.content) {
            assert!(t.is_empty());
        }
    }

    // ── dict_block_mut: coerce Text/List to Dict ───────────────────────────
    #[test]
    fn dict_block_mut_coerces_text_block() {
        // A docs block parses as Text; dict_block_mut must coerce it to Dict.
        let mut f = parse("docs {\n  some text\n}\n");
        let entries = dict_block_mut(&mut f, "docs");
        entries.push(new_entry("k", "v"));
        assert!(matches!(
            f.block("docs").unwrap().content,
            BlockContent::Dict(_)
        ));
    }

    #[test]
    fn dict_block_mut_creates_absent() {
        let mut f = parse(SRC);
        let _ = dict_block_mut(&mut f, "brand-new");
        assert!(matches!(
            f.block("brand-new").unwrap().content,
            BlockContent::Dict(_)
        ));
    }

    // ── replace_block_entries ──────────────────────────────────────────────
    #[test]
    fn replace_block_entries_empty_removes_block() {
        let mut f = parse("headers {\n  A: 1\n}\n");
        replace_block_entries(&mut f, "headers", vec![]);
        assert!(f.block("headers").is_none());
    }

    #[test]
    fn replace_block_entries_duplicate_keys_fifo() {
        // Two same-named headers, one disabled, keep their own identities.
        let src = "headers {\n  Set-Cookie: a\n  ~Set-Cookie: b\n}\n";
        let mut f = parse(src);
        replace_block_entries(
            &mut f,
            "headers",
            vec![
                ("Set-Cookie".into(), "a".into(), true, false),
                ("Set-Cookie".into(), "b".into(), false, false),
            ],
        );
        if let Some(BlockContent::Dict(entries)) = f.block("headers").map(|b| &b.content) {
            assert_eq!(entries.len(), 2);
            assert!(!entries[0].disabled);
            assert!(entries[1].disabled);
        }
    }

    #[test]
    fn replace_block_entries_preserves_non_inline_on_empty_cell() {
        // A multiline value survives when the bulk cell is left empty.
        let src = "vars:pre-request {\n  big: '''\n  line1\n  line2\n  '''\n}\n";
        let mut f = parse(src);
        // Confirm it parsed as a non-Inline value first.
        replace_block_entries(
            &mut f,
            "vars:pre-request",
            vec![("big".into(), String::new(), true, false)],
        );
        let out = bru_lang::serialize(&f);
        // The original multiline content survives (empty cell -> keep prev value).
        assert!(out.contains("line1"), "multiline value dropped:\n{out}");
    }

    #[test]
    fn replace_block_entries_new_key_when_absent() {
        let mut f = parse(SRC);
        replace_block_entries(
            &mut f,
            "headers",
            vec![("X-New".into(), "1".into(), true, false)],
        );
        assert_eq!(f.to_request().unwrap().headers[0].name, "X-New");
    }

    // ── add_row ────────────────────────────────────────────────────────────
    #[test]
    fn add_row_appends_blank() {
        let mut f = parse(SRC);
        add_row(&mut f, "params:query");
        if let Some(BlockContent::Dict(entries)) = f.block("params:query").map(|b| &b.content) {
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].key.name(), "");
        }
    }

    // ── set_entry_value: non-Inline untouched ──────────────────────────────
    #[test]
    fn set_entry_value_leaves_non_inline_untouched() {
        let src = "vars:pre-request {\n  big: '''\n  multiline\n  '''\n}\n";
        let mut f = parse(src);
        set_entry_value(&mut f, "vars:pre-request", 0, "single");
        let out = bru_lang::serialize(&f);
        // Non-Inline value not overwritten by the single-line cell.
        assert!(
            out.contains("multiline"),
            "non-inline value clobbered:\n{out}"
        );
    }

    #[test]
    fn set_entry_value_updates_inline() {
        let mut f = parse(SRC);
        add_row(&mut f, "headers");
        set_entry_key(&mut f, "headers", 0, "k");
        set_entry_value(&mut f, "headers", 0, "  trimmed  ");
        assert_eq!(f.to_request().unwrap().headers[0].value, "trimmed");
        // Out of range -> no-op.
        set_entry_value(&mut f, "headers", 99, "x");
    }

    // ── set_entry_key: preserves quoting ───────────────────────────────────
    #[test]
    fn set_entry_key_preserves_quoting() {
        // Bare key stays bare.
        let mut f = parse("headers {\n  bare: 1\n}\n");
        set_entry_key(&mut f, "headers", 0, "renamed");
        if let Some(BlockContent::Dict(entries)) = f.block("headers").map(|b| &b.content) {
            assert!(matches!(entries[0].key, Key::Bare(_)));
            assert_eq!(entries[0].key.name(), "renamed");
        }
        // Quoted key stays quoted.
        let mut f2 = parse("headers {\n  \"X Spaced\": 1\n}\n");
        set_entry_key(&mut f2, "headers", 0, "Y Spaced");
        if let Some(BlockContent::Dict(entries)) = f2.block("headers").map(|b| &b.content) {
            assert!(matches!(entries[0].key, Key::Quoted(_)));
        }
        // Out of range -> no-op.
        set_entry_key(&mut f2, "headers", 99, "z");
    }
}
