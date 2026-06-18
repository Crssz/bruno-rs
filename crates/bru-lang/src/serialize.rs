//! Model -> `.bru` text. Mirrors Bruno's `jsonToBru`/`utils.js` formatting so that
//! `serialize(parse(x)) == x` for canonical Bruno files.

use bru_core::{Annotation, Block, BlockContent, BruFile, Entry, Key, Value};

/// Serialize a [`BruFile`] to its `.bru` text form.
pub fn serialize(file: &BruFile) -> String {
    let mut out = String::new();
    for block in &file.blocks {
        out.push_str(&serialize_block(block));
        out.push_str("\n\n");
    }
    strip_last_line(out)
}

fn serialize_block(block: &Block) -> String {
    match &block.content {
        BlockContent::Dict(entries) => {
            if entries.is_empty() {
                format!("{} {{\n}}", block.name)
            } else {
                let body: Vec<String> = entries.iter().map(serialize_entry).collect();
                format!("{} {{\n{}\n}}", block.name, indent_string(&body.join("\n")))
            }
        }
        BlockContent::Text(text) => {
            if text.is_empty() {
                format!("{} {{\n}}", block.name)
            } else {
                // Text blocks store the verbatim inner bytes; emit as-is (no re-indent).
                format!("{} {{\n{}\n}}", block.name, text)
            }
        }
        BlockContent::List(items) => {
            // env `vars:secret [ ... ]` — bare names, 2-space indented, comma-joined.
            let body: Vec<String> = items.iter().map(|i| format!("  {i}")).collect();
            format!("{} [\n{}\n]", block.name, body.join(",\n"))
        }
    }
}

fn serialize_entry(entry: &Entry) -> String {
    let ann = serialize_annotations(&entry.annotations);
    let dis = if entry.disabled { "~" } else { "" };
    let loc = if entry.local { "@" } else { "" };
    format!(
        "{ann}{dis}{loc}{}: {}",
        serialize_key(&entry.key),
        serialize_value(&entry.value)
    )
}

fn serialize_key(key: &Key) -> String {
    match key {
        Key::Bare(s) => s.clone(),
        Key::Quoted(s) => format!("\"{}\"", s.replace('"', "\\\"")),
    }
}

fn serialize_value(value: &Value) -> String {
    match value {
        Value::Inline(s) => s.clone(),
        Value::List(items) => {
            let body: Vec<String> = items.iter().map(|i| format!("  {i}")).collect();
            format!("[\n{}\n]", body.join("\n"))
        }
        Value::Multiline { text, content_type } => {
            let ct = content_type
                .as_ref()
                .map(|c| format!(" @contentType({c})"))
                .unwrap_or_default();
            format!("'''{text}'''{ct}")
        }
    }
}

/// Port of `utils.serializeAnnotations`: emits each decorator on its own line and,
/// when non-empty, appends a trailing newline so the key line follows underneath.
fn serialize_annotations(annotations: &[Annotation]) -> String {
    if annotations.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = annotations
        .iter()
        .map(|a| match &a.value {
            None => format!("@{}", a.name),
            Some(v) if v.contains('\n') => {
                format!("@{}('''\n{}\n''')", a.name, indent_string(v))
            }
            Some(v) => {
                let quote = if v.contains('\'') { '"' } else { '\'' };
                format!("@{}({quote}{v}{quote})", a.name)
            }
        })
        .collect();
    format!("{}\n", lines.join("\n"))
}

/// Port of `utils.indentString`: prefix every line with 2 spaces, normalizing
/// line endings to `\n`.
pub(crate) fn indent_string(s: &str) -> String {
    if s.is_empty() {
        return String::new();
    }
    split_lines(s)
        .map(|line| format!("  {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Split on `\r\n | \r | \n`, mirroring Bruno's `/\r\n|\r|\n/` line split.
fn split_lines(s: &str) -> impl Iterator<Item = &str> {
    s.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l))
}

/// Port of `jsonToBru.stripLastLine`: drop a single trailing `\r?\n`.
fn strip_last_line(mut s: String) -> String {
    if s.ends_with('\n') {
        s.pop();
        if s.ends_with('\r') {
            s.pop();
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indent_string_empty_returns_empty() {
        // The early-return branch for an empty input.
        assert_eq!(indent_string(""), "");
    }

    #[test]
    fn indent_string_prefixes_each_line_and_normalizes_crlf() {
        // Multi-line input: split_lines strips `\r`, each line gets 2 spaces.
        assert_eq!(indent_string("a\r\nb\nc"), "  a\n  b\n  c");
        // Single line, no trailing newline.
        assert_eq!(indent_string("solo"), "  solo");
    }

    #[test]
    fn strip_last_line_handles_lf_crlf_and_none() {
        // Plain `\n` → dropped (the inner `\r` check is false).
        assert_eq!(strip_last_line("x\n".to_string()), "x");
        // `\r\n` → both popped (covers the inner `\r` pop).
        assert_eq!(strip_last_line("x\r\n".to_string()), "x");
        // No trailing newline → unchanged.
        assert_eq!(strip_last_line("x".to_string()), "x");
        // Lone `\r` (no `\n`) → unchanged (outer `if` false).
        assert_eq!(strip_last_line("x\r".to_string()), "x\r");
    }
}
