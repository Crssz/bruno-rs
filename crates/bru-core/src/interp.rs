//! `{{variable}}` interpolation, plus a handful of Bruno dynamic variables.
//!
//! M1 uses a single flattened variable map (callers merge the scopes —
//! env / collection / folder / request / runtime — in precedence order before
//! calling). Unresolved `{{name}}` placeholders are left verbatim so they are
//! visible rather than silently blanked.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Replace every `{{ name }}` in `template` using `vars` (and dynamic `$` vars).
pub fn interpolate(template: &str, vars: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find("}}") {
            Some(end) => {
                let raw = &after[..end];
                match resolve(raw.trim(), vars) {
                    Some(v) => out.push_str(&v),
                    None => {
                        // Keep the original placeholder verbatim when unresolved.
                        out.push_str("{{");
                        out.push_str(raw);
                        out.push_str("}}");
                    }
                }
                rest = &after[end + 2..];
            }
            None => {
                out.push_str("{{");
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn resolve(name: &str, vars: &HashMap<String, String>) -> Option<String> {
    if let Some(dynamic) = name.strip_prefix('$') {
        return dynamic_var(dynamic);
    }
    vars.get(name).cloned()
}

fn dynamic_var(name: &str) -> Option<String> {
    match name {
        "timestamp" => Some(unix_secs().to_string()),
        "isoTimestamp" => Some(iso_timestamp(unix_secs())),
        "randomInt" => Some((next_rand() % 1000).to_string()),
        "guid" | "randomUUID" => Some(uuid_v4()),
        _ => None,
    }
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A non-cryptographic random u64 seeded from the high-resolution clock. Used
/// only for `$randomInt`/`$guid`; never for anything security-sensitive.
fn next_rand() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    // xorshift64* on the clock seed.
    let mut x = nanos ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    x.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

fn uuid_v4() -> String {
    let a = next_rand();
    let b = next_rand();
    let mut bytes = [0u8; 16];
    bytes[..8].copy_from_slice(&a.to_le_bytes());
    bytes[8..].copy_from_slice(&b.to_le_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
    bytes[8] = (bytes[8] & 0x3f) | 0x80; // variant
    let h = |r: std::ops::Range<usize>| {
        bytes[r]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    format!(
        "{}-{}-{}-{}-{}",
        h(0..4),
        h(4..6),
        h(6..8),
        h(8..10),
        h(10..16)
    )
}

/// Minimal RFC 3339 UTC timestamp (`YYYY-MM-DDTHH:MM:SSZ`) without a date crate.
fn iso_timestamp(secs: u64) -> String {
    let days = secs / 86_400;
    let tod = secs % 86_400;
    let (h, mi, s) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Days since 1970-01-01 → (year, month, day). Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
