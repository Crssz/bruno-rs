//! Pure value-to-string formatting helpers (sizes, hex dumps, method labels,
//! run-outcome summaries). No gpui.
pub fn human_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// A classic `offset  hex bytes  ascii` hex dump of a byte buffer.
pub fn hex_dump(bytes: &[u8]) -> String {
    let mut out = String::new();
    for (i, chunk) in bytes.chunks(16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let ascii: String = chunk
            .iter()
            .map(|&b| {
                if (0x20..0x7f).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        out.push_str(&format!(
            "{:08x}  {:<47}  {}\n",
            i * 16,
            hex.join(" "),
            ascii
        ));
    }
    out
}

/// Cycle to the next HTTP method (click-to-change in the URL bar).
pub fn short_method(m: &str) -> String {
    let m = m.to_ascii_uppercase();
    match m.as_str() {
        "DELETE" => "DEL".into(),
        "OPTIONS" => "OPT".into(),
        "" => "?".into(),
        _ => m.chars().take(4).collect(),
    }
}
pub fn format_outcome(o: &bru_engine::RunOutcome) -> String {
    if let Some(e) = &o.error {
        return format!("Error: {e}");
    }
    match &o.response {
        Some(r) => format!(
            "{} {} \u{00B7} {} ms\n\n{}",
            r.status,
            r.status_text,
            r.duration_ms,
            String::from_utf8_lossy(&r.body)
        ),
        None => "(no response)".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_size_units() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(1024 * 1024), "1.00 MB");
        assert_eq!(human_size(3 * 1024 * 1024 / 2), "1.50 MB");
    }

    #[test]
    fn short_method_abbreviations() {
        assert_eq!(short_method("get"), "GET");
        assert_eq!(short_method("post"), "POST");
        assert_eq!(short_method("delete"), "DEL");
        assert_eq!(short_method("options"), "OPT");
        assert_eq!(short_method("patch"), "PATC");
        assert_eq!(short_method(""), "?");
    }

    #[test]
    fn hex_dump_layout() {
        assert_eq!(hex_dump(b""), "");
        let one = hex_dump(b"AB");
        assert!(one.starts_with("00000000"), "{one:?}");
        assert!(one.contains("41 42"), "{one:?}");
        assert!(one.trim_end().ends_with("AB"), "{one:?}");
        // Exact column layout: 8-hex offset, 2 spaces, 47-wide hex gutter, 2 spaces, ascii.
        assert_eq!(one, format!("{:08x}  {:<47}  {}\n", 0, "41 42", "AB"));
        assert!(hex_dump(&[0x41, 0x00]).trim_end().ends_with("A."));
        assert_eq!(hex_dump(&[0u8; 16]).lines().count(), 1);
        let two = hex_dump(&[0u8; 17]);
        assert_eq!(two.lines().count(), 2);
        assert!(two.lines().nth(1).unwrap().starts_with("00000010"));
    }

    #[test]
    fn format_outcome_error_and_empty() {
        let e = bru_engine::RunOutcome::errored("req", "boom");
        assert_eq!(format_outcome(&e), "Error: boom");
        let empty = bru_engine::RunOutcome::default();
        assert_eq!(format_outcome(&empty), "(no response)");
    }
}
