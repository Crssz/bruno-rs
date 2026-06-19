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
