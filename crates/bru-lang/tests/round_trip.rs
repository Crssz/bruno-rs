//! Golden-file round-trip corpus: `serialize(parse(x)) == x` over real Bruno
//! `.bru` files copied from upstream `usebruno/bruno` (normalized to LF).

use std::fs;
use std::path::PathBuf;

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Upstream fixtures that are deliberately non-canonical (used by Bruno only to
/// exercise the *parser*, e.g. 4-space indent + double-quoted annotation args).
/// They must parse cleanly but are not expected to be byte-stable, since
/// serialize normalizes to canonical 2-space form — exactly as Bruno's own
/// serializer would.
const PARSE_ONLY: &[&str] = &["annotations.bru"];

/// First line index where `a` and `b` differ, for a readable failure message.
fn first_diff(a: &str, b: &str) -> String {
    for (i, (la, lb)) in a.lines().zip(b.lines()).enumerate() {
        if la != lb {
            return format!(
                "line {}:\n  expected: {:?}\n  actual:   {:?}",
                i + 1,
                la,
                lb
            );
        }
    }
    format!(
        "lengths differ: expected {} bytes, actual {} bytes",
        a.len(),
        b.len()
    )
}

#[test]
fn round_trip_corpus_is_byte_stable() {
    let dir = fixtures_dir();
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("fixtures dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "bru"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no .bru fixtures found in {dir:?}");

    let mut failures = Vec::new();
    for path in &files {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let original = fs::read_to_string(path).expect("read fixture");
        if PARSE_ONLY.contains(&name.as_str()) {
            if let Err(e) = bru_lang::parse(&original) {
                failures.push(format!("[PARSE-ERR] {name}: {e}"));
            }
            continue;
        }
        match bru_lang::round_trip(&original) {
            Ok(out) if out == original => {}
            Ok(out) => failures.push(format!(
                "[BYTE-DIFF] {}\n{}",
                path.file_name().unwrap().to_string_lossy(),
                first_diff(&original, &out)
            )),
            Err(e) => failures.push(format!(
                "[PARSE-ERR] {}: {e}",
                path.file_name().unwrap().to_string_lossy()
            )),
        }
    }

    let total = files.len();
    let ok = total - failures.len();
    if !failures.is_empty() {
        panic!(
            "{ok}/{total} fixtures round-trip byte-stable; {} failed:\n\n{}",
            failures.len(),
            failures.join("\n\n")
        );
    }
}
