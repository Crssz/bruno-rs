//! AWS Signature Version 4 request signing (pure computation).
//!
//! Unlike Digest, SigV4 needs no server round-trip: given the request and the
//! credentials, the `x-amz-date` and `Authorization` headers are computed and
//! attached before the request is sent. This module implements the full
//! algorithm — canonical request → string-to-sign → derived signing key →
//! signature — per the AWS "Signature Version 4 signing process" spec.

use bru_core::{Auth, Body, KeyVal, Request};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use url::Url;

type HmacSha256 = Hmac<Sha256>;

const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// The headers SigV4 produces for a request.
pub struct SignedHeaders {
    pub amz_date: String,
    pub authorization: String,
    /// Present only when a session token was supplied.
    pub security_token: Option<String>,
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// Compute the SigV4 signed headers.
///
/// - `amz_date` is `YYYYMMDDTHHMMSSZ` (and the date stamp `YYYYMMDD` is derived
///   from its first 8 chars).
/// - `host` is the signed `host` header; `x-amz-date` is added automatically, as
///   is `x-amz-security-token` when `session_token` is set.
/// - `payload` is the raw request body bytes (empty slice for no body).
#[allow(clippy::too_many_arguments)]
pub fn sign(
    method: &str,
    canonical_uri: &str,
    canonical_query: &str,
    host: &str,
    payload: &[u8],
    amz_date: &str,
    access_key_id: &str,
    secret_access_key: &str,
    session_token: &str,
    service: &str,
    region: &str,
) -> SignedHeaders {
    let date_stamp = &amz_date[..8];

    // 1. Canonical headers: host + x-amz-date, sorted by name; add
    //    x-amz-security-token when a session token is present.
    let mut canon: Vec<(String, String)> = vec![("host".to_string(), host.trim().to_string())];
    canon.push(("x-amz-date".to_string(), amz_date.to_string()));
    if !session_token.is_empty() {
        canon.push((
            "x-amz-security-token".to_string(),
            session_token.to_string(),
        ));
    }
    canon.sort_by(|a, b| a.0.cmp(&b.0));
    canon.dedup_by(|a, b| a.0 == b.0);

    let canonical_headers: String = canon.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed_headers: String = canon
        .iter()
        .map(|(k, _)| k.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let payload_hash = sha256_hex(payload);

    // 2. Canonical request.
    let canonical_request = format!(
        "{method}\n{canonical_uri}\n{canonical_query}\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
    );

    // 3. String to sign.
    let credential_scope = format!("{date_stamp}/{region}/{service}/aws4_request");
    let string_to_sign = format!(
        "{ALGORITHM}\n{amz_date}\n{credential_scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );

    // 4. Derive the signing key.
    let k_date = hmac_sha256(
        format!("AWS4{secret_access_key}").as_bytes(),
        date_stamp.as_bytes(),
    );
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, service.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");

    // 5. Signature.
    let signature = hex::encode(hmac_sha256(&k_signing, string_to_sign.as_bytes()));

    let authorization = format!(
        "{ALGORITHM} Credential={access_key_id}/{credential_scope}, SignedHeaders={signed_headers}, Signature={signature}"
    );

    SignedHeaders {
        amz_date: amz_date.to_string(),
        authorization,
        security_token: if session_token.is_empty() {
            None
        } else {
            Some(session_token.to_string())
        },
    }
}

/// Format `now` as an `x-amz-date` timestamp (`YYYYMMDDTHHMMSSZ`) in UTC,
/// derived from a Unix-epoch second count (no `chrono` dependency).
pub fn amz_date_from_unix(secs: u64) -> String {
    // Civil-from-days algorithm (Howard Hinnant), days since 1970-01-01.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (hour, min, sec) = (rem / 3600, (rem % 3600) / 60, rem % 60);

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}{m:02}{d:02}T{hour:02}{min:02}{sec:02}Z")
}

/// Serialize the request body to the bytes that will be signed/sent.
fn payload_bytes(body: &Body) -> Vec<u8> {
    match body {
        Body::None => Vec::new(),
        Body::Json(s) | Body::Text(s) | Body::Xml(s) | Body::Sparql(s) => s.clone().into_bytes(),
        Body::FormUrlEncoded(fields) => {
            // Encode with the same serializer bru-http uses (serde_urlencoded), so
            // the signed payload is byte-identical to what is sent on the wire.
            let pairs: Vec<(&str, &str)> = fields
                .iter()
                .filter(|f| f.enabled)
                .map(|f| (f.name.as_str(), f.value.as_str()))
                .collect();
            serde_urlencoded::to_string(&pairs)
                .unwrap_or_default()
                .into_bytes()
        }
        // Must match the bytes bru-http actually sends for a GraphQL body.
        Body::GraphQl { query, variables } => {
            let vars: serde_json::Value = if variables.trim().is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(variables).unwrap_or_else(|_| serde_json::json!({}))
            };
            let payload = serde_json::json!({ "query": query, "variables": vars });
            serde_json::to_string(&payload)
                .unwrap_or_default()
                .into_bytes()
        }
        // SigV4 over multipart is unsupported: the boundary is generated at send
        // time, so the exact signed bytes aren't known here. Signs empty (rare).
        Body::MultipartForm(_) => Vec::new(),
        // For a file body, sign the exact bytes bru-http will send (the selected
        // file), so the signature matches; a read failure signs empty and the
        // send surfaces the error.
        Body::File(items) => items
            .iter()
            .find(|i| i.selected)
            .or_else(|| items.first())
            .filter(|i| !i.path.trim().is_empty())
            .and_then(|i| std::fs::read(&i.path).ok())
            .unwrap_or_default(),
    }
}

/// Resolve [`Auth::AwsV4`] on a request: compute the SigV4 headers and push them
/// onto the request, then clear the auth (it is now baked into the headers). The
/// `region` defaults to `us-east-1` and `service` to `execute-api` when blank.
///
/// Returns `Err` if the URL cannot be parsed for the `Host` header.
pub fn resolve(req: &mut Request, now_unix: u64) -> Result<(), String> {
    let Auth::AwsV4 {
        access_key_id,
        secret_access_key,
        session_token,
        service,
        region,
        ..
    } = req.auth.clone()
    else {
        return Ok(());
    };

    // The multipart boundary is generated at send time, so the exact signed bytes
    // aren't knowable here. Rather than emit a guaranteed-to-be-rejected signature
    // over an empty payload, fail clearly.
    if matches!(req.body, Body::MultipartForm(_)) {
        return Err("awsv4: signing multipart bodies is not supported".to_string());
    }

    let url = url_parse(&req.url)?;
    let host = url.host.clone();
    let canonical_uri = if url.path.is_empty() {
        "/".to_string()
    } else {
        url.path.clone()
    };

    // Canonical query: include the request's enabled query params plus any in
    // the URL, encoded and sorted by key then value (RFC 3986 / SigV4 rules).
    let mut q: Vec<(String, String)> = url.query_pairs.clone();
    for p in req.query.iter().filter(|p| p.enabled) {
        q.push((p.name.clone(), p.value.clone()));
    }
    let mut encoded: Vec<(String, String)> = q
        .iter()
        .map(|(k, v)| (uri_encode(k), uri_encode(v)))
        .collect();
    encoded.sort();
    let canonical_query = encoded
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");

    let service = if service.is_empty() {
        "execute-api".to_string()
    } else {
        service
    };
    let region = if region.is_empty() {
        "us-east-1".to_string()
    } else {
        region
    };

    let payload = payload_bytes(&req.body);
    let amz_date = amz_date_from_unix(now_unix);

    let signed = sign(
        &req.method.to_uppercase(),
        &canonical_uri,
        &canonical_query,
        &host,
        &payload,
        &amz_date,
        &access_key_id,
        &secret_access_key,
        &session_token,
        &service,
        &region,
    );

    req.headers
        .push(KeyVal::new("x-amz-date", &signed.amz_date));
    req.headers
        .push(KeyVal::new("Authorization", &signed.authorization));
    if let Some(token) = signed.security_token {
        req.headers
            .push(KeyVal::new("x-amz-security-token", &token));
    }
    req.auth = Auth::None;
    Ok(())
}

/// A split URL: host (for the `Host` header), the already-percent-encoded path
/// (matching what reqwest puts on the wire), and the *decoded* query pairs (so
/// SigV4 re-encodes each exactly once).
struct ParsedUrl {
    host: String,
    path: String,
    query_pairs: Vec<(String, String)>,
}

fn url_parse(raw: &str) -> Result<ParsedUrl, String> {
    // Parse with the same crate reqwest uses so path encoding, query decoding,
    // and host/port handling match the bytes actually sent on the wire.
    let u = Url::parse(raw).map_err(|e| format!("awsv4: cannot parse URL `{raw}`: {e}"))?;
    let host_str = u
        .host_str()
        .ok_or_else(|| format!("awsv4: URL has no host: `{raw}`"))?;
    // The Host header omits the port when it is the scheme default (matching
    // reqwest/hyper); a non-default explicit port is kept.
    let default_port = match u.scheme() {
        "https" => Some(443),
        "http" => Some(80),
        _ => None,
    };
    let host = match u.port() {
        Some(p) if Some(p) != default_port => format!("{host_str}:{p}"),
        _ => host_str.to_string(),
    };
    let query_pairs = u
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    Ok(ParsedUrl {
        host,
        path: u.path().to_string(),
        query_pairs,
    })
}

/// RFC 3986 unreserved-set URI encoding used by SigV4 for query keys/values
/// (`/` is encoded; the path arrives already percent-encoded from `url::Url`).
fn uri_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AWS SigV4 test-suite `get-vanilla` known-answer vector.
    /// Credentials: AKIDEXAMPLE / wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY,
    /// region us-east-1, service `service`, date 20150830T123600Z, host
    /// example.amazonaws.com, GET /, no query, empty body.
    #[test]
    fn get_vanilla_known_answer() {
        let signed = sign(
            "GET",
            "/",
            "",
            "example.amazonaws.com",
            b"",
            "20150830T123600Z",
            "AKIDEXAMPLE",
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "",
            "service",
            "us-east-1",
        );
        assert!(
            signed.authorization.ends_with(
                "Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
            ),
            "authorization was: {}",
            signed.authorization
        );
        assert!(signed.security_token.is_none());
    }

    #[test]
    fn amz_date_formats_known_instant() {
        // 2015-08-30T12:36:00Z == 1440938160 seconds since epoch.
        assert_eq!(amz_date_from_unix(1_440_938_160), "20150830T123600Z");
    }

    #[test]
    fn uri_encode_rules() {
        assert_eq!(uri_encode("a b~c"), "a%20b~c");
        assert_eq!(uri_encode("/a b/c"), "%2Fa%20b%2Fc");
    }

    #[test]
    fn url_parse_decodes_query_keeps_encoded_path_strips_default_port() {
        let p = url_parse("https://h.example.com:443/a%20b?x=%20y&z=1").unwrap();
        assert_eq!(p.host, "h.example.com"); // default 443 dropped, like hyper
        assert_eq!(p.path, "/a%20b"); // encoded path == wire bytes
        assert_eq!(
            p.query_pairs,
            vec![
                ("x".to_string(), " y".to_string()), // query decoded (single-encode later)
                ("z".to_string(), "1".to_string()),
            ]
        );
    }

    #[test]
    fn url_parse_keeps_non_default_port() {
        assert_eq!(url_parse("http://h:8080/").unwrap().host, "h:8080");
    }

    #[test]
    fn sign_with_session_token_includes_security_token_and_signed_header() {
        let signed = sign(
            "GET",
            "/",
            "",
            "example.amazonaws.com",
            b"",
            "20150830T123600Z",
            "AKIDEXAMPLE",
            "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            "SESSIONTOKEN",
            "service",
            "us-east-1",
        );
        assert_eq!(signed.security_token.as_deref(), Some("SESSIONTOKEN"));
        // x-amz-security-token participates in the signed-headers list.
        assert!(
            signed.authorization.contains("x-amz-security-token"),
            "auth: {}",
            signed.authorization
        );
    }

    #[test]
    fn amz_date_handles_epoch_and_pre_epoch_fallback() {
        // The Unix epoch itself.
        assert_eq!(amz_date_from_unix(0), "19700101T000000Z");
        // A leap-year boundary (2000-03-01) exercises the civil-from-days branch
        // where month/day roll into March (mp >= 10 path) and Feb-29 handling.
        // 2000-02-29T00:00:00Z == 951782400.
        assert_eq!(amz_date_from_unix(951_782_400), "20000229T000000Z");
        // A time with non-zero hour/min/sec.
        assert_eq!(amz_date_from_unix(1), "19700101T000001Z");
    }

    #[test]
    fn uri_encode_passes_unreserved_through() {
        // All unreserved characters survive verbatim.
        assert_eq!(uri_encode("Az0-._~"), "Az0-._~");
        // A reserved byte (e.g. `+`) is percent-encoded.
        assert_eq!(uri_encode("a+b"), "a%2Bb");
    }

    // ── payload_bytes coverage for every Body variant ───────────────────────

    #[test]
    fn payload_bytes_none_is_empty() {
        assert!(payload_bytes(&Body::None).is_empty());
    }

    #[test]
    fn payload_bytes_text_like_variants() {
        assert_eq!(payload_bytes(&Body::Json("{}".into())), b"{}");
        assert_eq!(payload_bytes(&Body::Text("hi".into())), b"hi");
        assert_eq!(payload_bytes(&Body::Xml("<a/>".into())), b"<a/>");
        assert_eq!(payload_bytes(&Body::Sparql("SELECT".into())), b"SELECT");
    }

    #[test]
    fn payload_bytes_form_urlencoded_filters_disabled() {
        let body = Body::FormUrlEncoded(vec![
            KeyVal::new("a", "1"),
            KeyVal {
                name: "skip".into(),
                value: "x".into(),
                enabled: false,
            },
            KeyVal::new("b", "2"),
        ]);
        let bytes = payload_bytes(&body);
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.contains("a=1") && s.contains("b=2"), "got: {s}");
        assert!(!s.contains("skip"), "disabled field leaked: {s}");
    }

    #[test]
    fn payload_bytes_graphql_empty_and_present_variables() {
        // Empty variables → `{}`.
        let empty = payload_bytes(&Body::GraphQl {
            query: "Q".into(),
            variables: "   ".into(),
        });
        let v: serde_json::Value = serde_json::from_slice(&empty).unwrap();
        assert_eq!(v["variables"], serde_json::json!({}));
        assert_eq!(v["query"], "Q");

        // Valid JSON variables are embedded.
        let with = payload_bytes(&Body::GraphQl {
            query: "Q".into(),
            variables: r#"{"id":5}"#.into(),
        });
        let v2: serde_json::Value = serde_json::from_slice(&with).unwrap();
        assert_eq!(v2["variables"]["id"], 5);

        // Invalid JSON variables fall back to `{}` rather than failing.
        let bad = payload_bytes(&Body::GraphQl {
            query: "Q".into(),
            variables: "not json".into(),
        });
        let v3: serde_json::Value = serde_json::from_slice(&bad).unwrap();
        assert_eq!(v3["variables"], serde_json::json!({}));
    }

    #[test]
    fn payload_bytes_multipart_is_empty() {
        assert!(payload_bytes(&Body::MultipartForm(vec![])).is_empty());
    }

    #[test]
    fn payload_bytes_file_reads_selected_then_first() {
        use bru_core::FileBodyItem;
        // No usable path → empty.
        assert!(payload_bytes(&Body::File(vec![])).is_empty());
        assert!(payload_bytes(&Body::File(vec![FileBodyItem {
            path: "   ".into(),
            content_type: None,
            selected: true,
        }]))
        .is_empty());

        // A missing file path reads as empty (read fails → unwrap_or_default).
        let bytes = payload_bytes(&Body::File(vec![FileBodyItem {
            path: "C:/definitely/not/here-xyzzy.bin".into(),
            content_type: None,
            selected: true,
        }]));
        assert!(bytes.is_empty());

        // A real file: write to temp, prefer the selected one.
        let dir = std::env::temp_dir();
        let p_sel = dir.join(format!("bru-sigv4-sel-{}.bin", std::process::id()));
        let p_first = dir.join(format!("bru-sigv4-first-{}.bin", std::process::id()));
        std::fs::write(&p_sel, b"selected-bytes").unwrap();
        std::fs::write(&p_first, b"first-bytes").unwrap();
        let body = Body::File(vec![
            FileBodyItem {
                path: p_first.to_string_lossy().to_string(),
                content_type: None,
                selected: false,
            },
            FileBodyItem {
                path: p_sel.to_string_lossy().to_string(),
                content_type: None,
                selected: true,
            },
        ]);
        assert_eq!(payload_bytes(&body), b"selected-bytes");

        // None selected → falls back to first.
        let body2 = Body::File(vec![FileBodyItem {
            path: p_first.to_string_lossy().to_string(),
            content_type: None,
            selected: false,
        }]);
        assert_eq!(payload_bytes(&body2), b"first-bytes");

        let _ = std::fs::remove_file(&p_sel);
        let _ = std::fs::remove_file(&p_first);
    }

    // ── resolve() coverage ──────────────────────────────────────────────────

    fn awsv4_req(url: &str) -> Request {
        Request {
            method: "GET".to_string(),
            url: url.to_string(),
            auth: Auth::AwsV4 {
                access_key_id: "AKIDEXAMPLE".to_string(),
                secret_access_key: "secret".to_string(),
                session_token: String::new(),
                service: String::new(),
                region: String::new(),
                profile_name: String::new(),
            },
            ..Default::default()
        }
    }

    #[test]
    fn resolve_noop_for_non_awsv4_auth() {
        let mut req = Request {
            url: "https://x.example/".to_string(),
            auth: Auth::None,
            ..Default::default()
        };
        assert!(resolve(&mut req, 0).is_ok());
        // Untouched: no headers pushed, auth unchanged.
        assert!(req.headers.is_empty());
        assert!(matches!(req.auth, Auth::None));
    }

    #[test]
    fn resolve_defaults_service_and_region_when_blank() {
        let mut req = awsv4_req("https://api.example.com/path");
        resolve(&mut req, 1_440_938_160).unwrap();
        // Defaults: execute-api / us-east-1.
        let auth = req
            .headers
            .iter()
            .find(|h| h.name == "Authorization")
            .unwrap();
        assert!(
            auth.value.contains("/us-east-1/execute-api/aws4_request"),
            "credential scope: {}",
            auth.value
        );
        // x-amz-date pushed; auth cleared.
        assert!(req.headers.iter().any(|h| h.name == "x-amz-date"));
        assert!(matches!(req.auth, Auth::None));
        // No session token → no security-token header.
        assert!(!req.headers.iter().any(|h| h.name == "x-amz-security-token"));
    }

    #[test]
    fn resolve_signs_canonical_query_from_url_and_request() {
        let mut req = awsv4_req("https://api.example.com/p?b=2&a=1");
        req.query.push(KeyVal::new("c", "3"));
        resolve(&mut req, 1_440_938_160).unwrap();
        // It produced a signature without error; root path empty → canonical "/".
        assert!(req
            .headers
            .iter()
            .any(|h| h.name == "Authorization" && h.value.contains("Signature=")));
    }

    #[test]
    fn resolve_empty_path_becomes_root() {
        // No path component → canonical_uri defaults to "/".
        let mut req = awsv4_req("https://api.example.com");
        assert!(resolve(&mut req, 0).is_ok());
        assert!(req.headers.iter().any(|h| h.name == "Authorization"));
    }

    #[test]
    fn resolve_multipart_body_is_rejected() {
        let mut req = awsv4_req("https://api.example.com/");
        req.body = Body::MultipartForm(vec![]);
        let err = resolve(&mut req, 0).unwrap_err();
        assert_eq!(err, "awsv4: signing multipart bodies is not supported");
    }

    #[test]
    fn resolve_bad_url_errors() {
        let mut req = awsv4_req("http://");
        assert!(resolve(&mut req, 0).is_err());
    }

    #[test]
    fn url_parse_rejects_missing_host() {
        // A non-http scheme with no authority → no host_str.
        assert!(url_parse("mailto:nobody@example.com").is_err());
        assert!(url_parse("not a url at all").is_err());
    }
}
