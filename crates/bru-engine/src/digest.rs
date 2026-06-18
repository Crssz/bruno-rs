//! HTTP Digest access authentication (RFC 7616 / RFC 2617).
//!
//! Digest is challenge/response: the client sends an unauthenticated request,
//! the server replies `401` with a `WWW-Authenticate: Digest ...` header
//! carrying a `nonce`, and the client recomputes an `Authorization: Digest ...`
//! header from its credentials and resends. The engine drives the round-trip;
//! this module is the pure computation (challenge parse + response digest).

use md5::{Digest, Md5};

/// A parsed `WWW-Authenticate: Digest ...` challenge.
#[derive(Debug, Clone, Default)]
pub struct Challenge {
    pub realm: String,
    pub nonce: String,
    pub qop: Option<String>,
    pub opaque: Option<String>,
    pub algorithm: Option<String>,
}

/// Parse a `WWW-Authenticate` header value. Returns `None` if it is not a
/// `Digest` challenge. Accepts the comma-separated `key=value` form with
/// optionally quoted values (RFC 7616 §3.3 auth-param list).
pub fn parse_challenge(header: &str) -> Option<Challenge> {
    let rest = header.trim();
    // Scheme is case-insensitive; strip the leading `Digest`.
    let rest = rest
        .strip_prefix("Digest")
        .or_else(|| rest.strip_prefix("digest"))?
        .trim_start();

    let mut ch = Challenge::default();
    for (key, val) in parse_params(rest) {
        match key.to_ascii_lowercase().as_str() {
            "realm" => ch.realm = val,
            "nonce" => ch.nonce = val,
            "qop" => ch.qop = Some(val),
            "opaque" => ch.opaque = Some(val),
            "algorithm" => ch.algorithm = Some(val),
            _ => {}
        }
    }
    if ch.nonce.is_empty() {
        return None;
    }
    Some(ch)
}

/// Split a `key=value, key="value"` auth-param list, unquoting quoted values.
fn parse_params(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip separators/whitespace.
        while i < bytes.len() && (bytes[i] == b',' || bytes[i].is_ascii_whitespace()) {
            i += 1;
        }
        // Read key up to '='.
        let key_start = i;
        while i < bytes.len() && bytes[i] != b'=' {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let key = s[key_start..i].trim().to_string();
        i += 1; // skip '='
                // Read value: quoted or bare.
        let value = if i < bytes.len() && bytes[i] == b'"' {
            i += 1; // skip opening quote
            let val_start = i;
            while i < bytes.len() && bytes[i] != b'"' {
                i += 1;
            }
            let v = s[val_start..i].to_string();
            if i < bytes.len() {
                i += 1; // skip closing quote
            }
            v
        } else {
            let val_start = i;
            while i < bytes.len() && bytes[i] != b',' {
                i += 1;
            }
            s[val_start..i].trim().to_string()
        };
        if !key.is_empty() {
            out.push((key, value));
        }
    }
    out
}

fn md5_hex(input: &str) -> String {
    let mut hasher = Md5::new();
    hasher.update(input.as_bytes());
    hex::encode(hasher.finalize())
}

/// The result of computing a Digest response: the full `Authorization` header
/// value to send on the retry.
pub fn authorization_header(
    challenge: &Challenge,
    username: &str,
    password: &str,
    method: &str,
    uri: &str,
    cnonce: &str,
) -> String {
    let base_ha1 = md5_hex(&format!("{}:{}:{}", username, challenge.realm, password));
    // `algorithm=MD5-sess` derives HA1 from a per-session hash.
    let sess = challenge
        .algorithm
        .as_deref()
        .map(|a| a.eq_ignore_ascii_case("MD5-sess"))
        .unwrap_or(false);
    let ha1 = if sess {
        md5_hex(&format!("{base_ha1}:{}:{cnonce}", challenge.nonce))
    } else {
        base_ha1
    };
    let ha2 = md5_hex(&format!("{}:{}", method, uri));

    // RFC 7616: qop may be a comma-separated list; we support `auth`.
    let qop_auth = challenge
        .qop
        .as_deref()
        .map(|q| q.split(',').any(|t| t.trim() == "auth"))
        .unwrap_or(false);

    let nc = "00000001";
    let response = if qop_auth {
        md5_hex(&format!(
            "{ha1}:{}:{nc}:{cnonce}:auth:{ha2}",
            challenge.nonce
        ))
    } else {
        // Legacy RFC 2069 no-qop form.
        md5_hex(&format!("{ha1}:{}:{ha2}", challenge.nonce))
    };

    // Quoted-string values (some echoed from the server's challenge) must be
    // escaped so a hostile/buggy challenge can't break or inject into the header.
    let mut parts = vec![
        format!("username=\"{}\"", quote(username)),
        format!("realm=\"{}\"", quote(&challenge.realm)),
        format!("nonce=\"{}\"", quote(&challenge.nonce)),
        format!("uri=\"{}\"", quote(uri)),
        format!("response=\"{response}\""),
    ];
    if let Some(algorithm) = &challenge.algorithm {
        parts.push(format!("algorithm={}", quote(algorithm)));
    }
    if qop_auth {
        parts.push("qop=auth".to_string());
        parts.push(format!("nc={nc}"));
        parts.push(format!("cnonce=\"{}\"", quote(cnonce)));
    }
    if let Some(opaque) = &challenge.opaque {
        parts.push(format!("opaque=\"{}\"", quote(opaque)));
    }
    format!("Digest {}", parts.join(", "))
}

/// Escape a value for an RFC 7616 quoted-string: backslash-escape `\` and `"`,
/// and drop CR/LF so it can't terminate or inject into the header line.
fn quote(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\r', '\n'], "")
}

/// Derive a client nonce from the system clock (no `rand` dependency): the
/// nanosecond timestamp, MD5-hashed for a fixed-width hex string. Uniqueness
/// per request is all that's needed for `qop=auth`.
pub fn derive_cnonce() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    md5_hex(&format!("bru-cnonce-{nanos}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_challenge_with_qop() {
        let ch = parse_challenge(
            r#"Digest realm="x", nonce="abc", qop="auth", opaque="op", algorithm=MD5"#,
        )
        .unwrap();
        assert_eq!(ch.realm, "x");
        assert_eq!(ch.nonce, "abc");
        assert_eq!(ch.qop.as_deref(), Some("auth"));
        assert_eq!(ch.opaque.as_deref(), Some("op"));
        assert_eq!(ch.algorithm.as_deref(), Some("MD5"));
    }

    #[test]
    fn rejects_non_digest() {
        assert!(parse_challenge("Basic realm=\"x\"").is_none());
    }

    #[test]
    fn rfc2617_known_answer() {
        // RFC 2617 §3.5 worked example.
        let ch = Challenge {
            realm: "testrealm@host.com".to_string(),
            nonce: "dcd98b7102dd2f0e8b11d0f600bfb0c093".to_string(),
            qop: Some("auth".to_string()),
            opaque: Some("5ccc069c403ebaf9f0171e9517f40e41".to_string()),
            algorithm: None,
        };
        let header = authorization_header(
            &ch,
            "Mufasa",
            "Circle Of Life",
            "GET",
            "/dir/index.html",
            "0a4f113b",
        );
        assert!(
            header.contains("response=\"6629fae49393a05397450978507c4ef1\""),
            "header was: {header}"
        );
    }

    #[test]
    fn parses_lowercase_scheme_and_bare_values() {
        // Lowercase `digest` prefix + a bare (unquoted) value for `qop`.
        let ch = parse_challenge("digest realm=\"r\", nonce=abc123, qop=auth").unwrap();
        assert_eq!(ch.realm, "r");
        assert_eq!(ch.nonce, "abc123");
        assert_eq!(ch.qop.as_deref(), Some("auth"));
        assert!(ch.opaque.is_none());
        assert!(ch.algorithm.is_none());
    }

    #[test]
    fn empty_nonce_rejected() {
        // A Digest challenge with no usable nonce is not a valid challenge.
        assert!(parse_challenge("Digest realm=\"r\"").is_none());
    }

    #[test]
    fn ignores_unknown_params() {
        // Unknown keys are dropped; known ones still parse.
        let ch = parse_challenge("Digest nonce=\"n\", stale=true, domain=\"/d\"").unwrap();
        assert_eq!(ch.nonce, "n");
    }

    #[test]
    fn authorization_no_qop_uses_legacy_form() {
        // No qop → the RFC 2069 response form, and no qop/nc/cnonce in the header.
        let ch = Challenge {
            realm: "r".to_string(),
            nonce: "n".to_string(),
            qop: None,
            opaque: None,
            algorithm: None,
        };
        let header = authorization_header(&ch, "u", "p", "GET", "/x", "cn");
        assert!(!header.contains("qop="), "should not emit qop: {header}");
        assert!(!header.contains("cnonce="), "no cnonce in legacy: {header}");
        // Legacy response = MD5(HA1:nonce:HA2).
        let ha1 = md5_hex("u:r:p");
        let ha2 = md5_hex("GET:/x");
        let expect = md5_hex(&format!("{ha1}:n:{ha2}"));
        assert!(
            header.contains(&format!("response=\"{expect}\"")),
            "{header}"
        );
    }

    #[test]
    fn authorization_md5_sess_derives_session_ha1() {
        // algorithm=MD5-sess changes HA1 derivation and emits algorithm + opaque.
        let ch = Challenge {
            realm: "r".to_string(),
            nonce: "n".to_string(),
            qop: Some("auth".to_string()),
            opaque: Some("op".to_string()),
            algorithm: Some("MD5-sess".to_string()),
        };
        let header = authorization_header(&ch, "u", "p", "POST", "/y", "cn");
        let base_ha1 = md5_hex("u:r:p");
        let ha1 = md5_hex(&format!("{base_ha1}:n:cn"));
        let ha2 = md5_hex("POST:/y");
        let expect = md5_hex(&format!("{ha1}:n:00000001:cn:auth:{ha2}"));
        assert!(
            header.contains(&format!("response=\"{expect}\"")),
            "{header}"
        );
        assert!(header.contains("algorithm=MD5-sess"), "{header}");
        assert!(header.contains("opaque=\"op\""), "{header}");
        assert!(header.contains("qop=auth"), "{header}");
        assert!(header.contains("nc=00000001"), "{header}");
    }

    #[test]
    fn authorization_qop_list_selects_auth() {
        // A comma-separated qop list containing `auth` activates qop=auth.
        let ch = Challenge {
            realm: "r".to_string(),
            nonce: "n".to_string(),
            qop: Some("auth-int, auth".to_string()),
            opaque: None,
            algorithm: None,
        };
        let header = authorization_header(&ch, "u", "p", "GET", "/z", "cn");
        assert!(header.contains("qop=auth"), "{header}");
    }

    #[test]
    fn authorization_qop_without_auth_token_falls_back_to_legacy() {
        // A qop value that does not contain `auth` → legacy no-qop response.
        let ch = Challenge {
            realm: "r".to_string(),
            nonce: "n".to_string(),
            qop: Some("auth-int".to_string()),
            opaque: None,
            algorithm: None,
        };
        let header = authorization_header(&ch, "u", "p", "GET", "/z", "cn");
        assert!(!header.contains("qop="), "{header}");
    }

    #[test]
    fn authorization_escapes_quoted_string_values() {
        // A hostile realm/opaque carrying quotes/backslashes/newlines is escaped
        // so it cannot break out of or inject into the header line.
        let ch = Challenge {
            realm: "a\"b\\c".to_string(),
            nonce: "n".to_string(),
            qop: None,
            opaque: Some("o\r\np".to_string()),
            algorithm: None,
        };
        let header = authorization_header(&ch, "us\"er", "p", "GET", "/x", "cn");
        assert!(header.contains("realm=\"a\\\"b\\\\c\""), "{header}");
        assert!(header.contains("username=\"us\\\"er\""), "{header}");
        // CR/LF stripped from opaque.
        assert!(header.contains("opaque=\"op\""), "{header}");
        assert!(!header.contains('\r') && !header.contains('\n'), "{header}");
    }

    #[test]
    fn derive_cnonce_is_fixed_width_hex() {
        let c = derive_cnonce();
        // MD5 hex is 32 chars, all lowercase hex.
        assert_eq!(c.len(), 32);
        assert!(c.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn parse_params_handles_trailing_unterminated_quote() {
        // A value opened with `"` but never closed reads to end-of-input.
        let ch = parse_challenge("Digest nonce=\"unterminated").unwrap();
        assert_eq!(ch.nonce, "unterminated");
    }
}
