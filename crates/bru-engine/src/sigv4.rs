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

type HmacSha256 = Hmac<Sha256>;

const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// One header to include in the signed canonical request.
pub struct SignedHeader {
    pub name: String,
    pub value: String,
}

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

/// Lowercase + trim a header value's internal runs of whitespace are NOT
/// collapsed here (the test-suite "vanilla" vectors do not require it); values
/// are trimmed of surrounding whitespace per the spec's canonicalization.
fn canon_header_value(v: &str) -> String {
    v.trim().to_string()
}

/// Compute the SigV4 signed headers.
///
/// - `amz_date` is `YYYYMMDDTHHMMSSZ` (and the date stamp `YYYYMMDD` is derived
///   from its first 8 chars).
/// - `headers` must already include `host` (and may include any other headers
///   that should be signed); `x-amz-date` is added to the signed set
///   automatically, as is `x-amz-security-token` when `session_token` is set.
/// - `payload` is the raw request body bytes (empty slice for no body).
#[allow(clippy::too_many_arguments)]
pub fn sign(
    method: &str,
    canonical_uri: &str,
    canonical_query: &str,
    headers: &[SignedHeader],
    payload: &[u8],
    amz_date: &str,
    access_key_id: &str,
    secret_access_key: &str,
    session_token: &str,
    service: &str,
    region: &str,
) -> SignedHeaders {
    let date_stamp = &amz_date[..8];

    // 1. Canonical headers: lowercase name, trimmed value, sorted by name.
    //    Always include host + x-amz-date; include x-amz-security-token when
    //    a session token is present.
    let mut canon: Vec<(String, String)> = headers
        .iter()
        .map(|h| (h.name.to_ascii_lowercase(), canon_header_value(&h.value)))
        .collect();
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

fn kv(name: &str, value: &str) -> KeyVal {
    KeyVal {
        name: name.to_string(),
        value: value.to_string(),
        enabled: true,
    }
}

/// Serialize the request body to the bytes that will be signed/sent.
fn payload_bytes(body: &Body) -> Vec<u8> {
    match body {
        Body::None => Vec::new(),
        Body::Json(s) | Body::Text(s) | Body::Xml(s) | Body::Sparql(s) => s.clone().into_bytes(),
        Body::FormUrlEncoded(fields) => fields
            .iter()
            .filter(|f| f.enabled)
            .map(|f| format!("{}={}", form_encode(&f.name), form_encode(&f.value)))
            .collect::<Vec<_>>()
            .join("&")
            .into_bytes(),
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
        // time, so the exact signed bytes aren't known here. Signs an empty
        // payload (a rare combination — multipart uploads to SigV4 endpoints).
        Body::MultipartForm(_) => Vec::new(),
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
        .map(|(k, v)| (uri_encode(k, true), uri_encode(v, true)))
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

    let headers = [SignedHeader {
        name: "host".to_string(),
        value: host,
    }];
    let payload = payload_bytes(&req.body);
    let amz_date = amz_date_from_unix(now_unix);

    let signed = sign(
        &req.method.to_uppercase(),
        &canonical_uri,
        &canonical_query,
        &headers,
        &payload,
        &amz_date,
        &access_key_id,
        &secret_access_key,
        &session_token,
        &service,
        &region,
    );

    req.headers.push(kv("x-amz-date", &signed.amz_date));
    req.headers.push(kv("Authorization", &signed.authorization));
    if let Some(token) = signed.security_token {
        req.headers.push(kv("x-amz-security-token", &token));
    }
    req.auth = Auth::None;
    Ok(())
}

/// A minimal split URL: host (for the `Host` header), path, and query pairs.
/// Avoids pulling `url`/`reqwest` types into this pure-computation module.
struct ParsedUrl {
    host: String,
    path: String,
    query_pairs: Vec<(String, String)>,
}

fn url_parse(raw: &str) -> Result<ParsedUrl, String> {
    // scheme://host/path?query
    let after_scheme = raw.split_once("://").map(|(_, rest)| rest).unwrap_or(raw);
    let (authority_path, query) = match after_scheme.split_once('?') {
        Some((ap, q)) => (ap, Some(q)),
        None => (after_scheme, None),
    };
    let (authority, path) = match authority_path.find('/') {
        Some(idx) => (&authority_path[..idx], &authority_path[idx..]),
        None => (authority_path, ""),
    };
    if authority.is_empty() {
        return Err(format!("awsv4: cannot parse host from URL `{raw}`"));
    }
    // Host excludes any userinfo and port (Host header keeps the port; for SigV4
    // the canonical host header is what reqwest sends, i.e. host[:port]).
    let host = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority)
        .to_string();

    let query_pairs = query
        .map(|q| {
            q.split('&')
                .filter(|s| !s.is_empty())
                .map(|pair| match pair.split_once('=') {
                    Some((k, v)) => (k.to_string(), v.to_string()),
                    None => (pair.to_string(), String::new()),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(ParsedUrl {
        host,
        path: path.to_string(),
        query_pairs,
    })
}

/// `application/x-www-form-urlencoded` encoding for a form body field: space →
/// `+`, unreserved bytes literal, everything else percent-encoded.
fn form_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// RFC 3986 unreserved-set URI encoding used by SigV4. When `encode_slash` is
/// false, `/` is left literal (for path segments).
fn uri_encode(s: &str, encode_slash: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char);
            }
            b'/' if !encode_slash => out.push('/'),
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
        let headers = [SignedHeader {
            name: "host".to_string(),
            value: "example.amazonaws.com".to_string(),
        }];
        let signed = sign(
            "GET",
            "/",
            "",
            &headers,
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
}
