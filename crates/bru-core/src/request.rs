//! The semantic request view: a typed projection of a [`BruFile`] (the lossless
//! block model) into the fields an HTTP engine needs. This is a *read view* —
//! never the serialization source of truth.

use crate::model::{BlockContent, BruFile, Value};

/// An enabled-aware key/value row (header, query/path param, form field).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyVal {
    pub name: String,
    pub value: String,
    pub enabled: bool,
}

/// A pre/post-request variable binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Var {
    pub name: String,
    pub value: String,
    pub enabled: bool,
    pub local: bool,
}

/// A declarative assertion (`assert` block row): an expression and an expected
/// operator+value (e.g. `res.status` → `eq 200`, or a bare value meaning `eq`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assertion {
    pub expr: String,
    pub value: String,
    pub enabled: bool,
}

/// One field of a `multipart/form-data` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipartField {
    pub name: String,
    pub value: MultipartValue,
    /// An explicit per-part content-type (`@contentType(...)`), if any.
    pub content_type: Option<String>,
    pub enabled: bool,
}

/// A multipart field is either an inline text value or a file (by path on disk).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MultipartValue {
    Text(String),
    File(String),
}

/// One entry of a binary `body:file` block. Bruno allows several candidate files
/// with one `selected`; the selected entry's bytes are sent as the request body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileBodyItem {
    pub path: String,
    pub content_type: Option<String>,
    /// The active file (Bruno's `~` prefix marks the non-selected ones).
    pub selected: bool,
}

/// The request body, by mode.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Body {
    #[default]
    None,
    Json(String),
    Text(String),
    Xml(String),
    Sparql(String),
    FormUrlEncoded(Vec<KeyVal>),
    GraphQl {
        query: String,
        variables: String,
    },
    MultipartForm(Vec<MultipartField>),
    /// Binary file body (`body:file`). The selected entry is sent.
    File(Vec<FileBodyItem>),
}

/// Where an API-key credential is placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiKeyPlacement {
    Header,
    Query,
}

/// OAuth 2.0 settings (non-interactive grants). Browser grants
/// (authorization_code/implicit) are deferred.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OAuth2 {
    /// `client_credentials` | `password`.
    pub grant_type: String,
    pub access_token_url: String,
    pub client_id: String,
    pub client_secret: String,
    pub scope: String,
    /// `password` grant only.
    pub username: String,
    pub password: String,
    /// `body` (client creds in form body) | `basic_auth_header`.
    pub credentials_placement: String,
    /// Where the obtained token is placed on the request: `header` | `query`.
    pub token_placement: String,
    pub token_header_prefix: String,
    pub token_query_key: String,
}

/// The request's auth, by mode. Schemes beyond these (digest/awsv4/ntlm/…) land
/// in later milestones; an unsupported mode projects to [`Auth::None`].
// OAuth2 carries more fields than the other variants; there is exactly one Auth
// per request, so the size difference is not worth boxing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Auth {
    #[default]
    None,
    /// Inherit from the collection/folder chain.
    Inherit,
    Basic {
        username: String,
        password: String,
    },
    Bearer {
        token: String,
    },
    ApiKey {
        key: String,
        value: String,
        placement: ApiKeyPlacement,
    },
    OAuth2(OAuth2),
    /// HTTP Digest (RFC 7616/2617). Resolved in the engine via a 401 challenge
    /// (the server's `WWW-Authenticate: Digest ...` nonce drives the response).
    Digest {
        username: String,
        password: String,
    },
    /// AWS Signature Version 4. Resolved (pure computation) before send: the
    /// signed `Authorization`/`x-amz-date` headers are pushed and auth cleared.
    AwsV4 {
        access_key_id: String,
        secret_access_key: String,
        session_token: String,
        service: String,
        region: String,
        profile_name: String,
    },
}

/// Per-request transport overrides projected from the `settings` block. `None`
/// means "inherit the run-level default" so the engine only diverges from its
/// shared client when a request explicitly opts in.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequestSettings {
    pub follow_redirects: Option<bool>,
    pub max_redirects: Option<u32>,
    /// Request timeout in milliseconds (Bruno stores `timeout` in ms).
    pub timeout_ms: Option<u64>,
    pub encode_url: Option<bool>,
}

/// A typed HTTP request projected from a `.bru` file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Request {
    pub method: String,
    pub url: String,
    pub query: Vec<KeyVal>,
    pub path_params: Vec<KeyVal>,
    pub headers: Vec<KeyVal>,
    pub body: Body,
    pub auth: Auth,
    pub vars_pre: Vec<Var>,
    pub vars_post: Vec<Var>,
    pub assertions: Vec<Assertion>,
    pub settings: RequestSettings,
}

impl BruFile {
    /// Project this file into a typed [`Request`]. Returns `None` if there is no
    /// method block (i.e. it isn't a request `.bru`).
    pub fn to_request(&self) -> Option<Request> {
        let method = self.request_method()?;
        let method_block = self.method_block()?;
        let url = method_dict_value(method_block, "url").unwrap_or_default();
        let body_mode = method_dict_value(method_block, "body").unwrap_or("none");
        let auth_mode = method_dict_value(method_block, "auth").unwrap_or("none");

        Some(Request {
            method,
            url: url.to_string(),
            query: self.key_vals("params:query"),
            path_params: self.key_vals("params:path"),
            headers: self.key_vals("headers"),
            body: self.project_body(body_mode),
            auth: self.project_auth(auth_mode),
            vars_pre: self.vars("vars:pre-request"),
            vars_post: self.vars("vars:post-response"),
            assertions: self.assertions(),
            settings: self.project_settings(),
        })
    }

    /// Project the `settings` dict block into typed transport overrides. Absent
    /// keys stay `None` (inherit the run default).
    fn project_settings(&self) -> RequestSettings {
        let get = |key: &str| -> Option<String> {
            match self.block("settings").map(|b| &b.content) {
                Some(BlockContent::Dict(entries)) => entries
                    .iter()
                    .find(|e| !e.disabled && e.key.name() == key)
                    .map(|e| e.value.as_inline().trim().to_string()),
                _ => None,
            }
        };
        let bool_of = |s: String| match s.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        };
        RequestSettings {
            follow_redirects: get("followRedirects").and_then(bool_of),
            max_redirects: get("maxRedirects").and_then(|s| s.parse().ok()),
            timeout_ms: get("timeout").and_then(|s| s.parse().ok()),
            encode_url: get("encodeUrl").and_then(bool_of),
        }
    }

    fn method_block(&self) -> Option<&crate::model::Block> {
        self.blocks
            .iter()
            .find(|b| crate::model::HTTP_VERBS.contains(&b.name.as_str()) || b.name == "http")
    }

    fn key_vals(&self, block: &str) -> Vec<KeyVal> {
        match self.block(block).map(|b| &b.content) {
            Some(BlockContent::Dict(entries)) => entries
                .iter()
                .map(|e| KeyVal {
                    name: e.key.name().to_string(),
                    value: e.value.as_inline().to_string(),
                    enabled: !e.disabled,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn vars(&self, block: &str) -> Vec<Var> {
        match self.block(block).map(|b| &b.content) {
            Some(BlockContent::Dict(entries)) => entries
                .iter()
                .map(|e| Var {
                    name: e.key.name().to_string(),
                    value: e.value.as_inline().to_string(),
                    enabled: !e.disabled,
                    local: e.local,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn assertions(&self) -> Vec<Assertion> {
        match self.block("assert").map(|b| &b.content) {
            Some(BlockContent::Dict(entries)) => entries
                .iter()
                .map(|e| Assertion {
                    expr: e.key.name().to_string(),
                    value: e.value.as_inline().to_string(),
                    enabled: !e.disabled,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn project_body(&self, mode: &str) -> Body {
        let text = |name: &str| self.text_block(name).map(outdent).unwrap_or_default();
        match mode {
            "json" => Body::Json(text("body:json")),
            "text" => Body::Text(text("body:text")),
            "xml" => Body::Xml(text("body:xml")),
            "sparql" => Body::Sparql(text("body:sparql")),
            "formUrlEncoded" => Body::FormUrlEncoded(self.key_vals("body:form-urlencoded")),
            "graphql" => Body::GraphQl {
                query: text("body:graphql"),
                variables: text("body:graphql:vars"),
            },
            "multipartForm" => Body::MultipartForm(self.multipart_fields("body:multipart-form")),
            "file" => Body::File(self.file_body_items("body:file")),
            _ => Body::None,
        }
    }

    fn file_body_items(&self, block: &str) -> Vec<FileBodyItem> {
        match self.block(block).map(|b| &b.content) {
            Some(BlockContent::Dict(entries)) => entries.iter().map(parse_file_body_item).collect(),
            _ => Vec::new(),
        }
    }

    fn multipart_fields(&self, block: &str) -> Vec<MultipartField> {
        match self.block(block).map(|b| &b.content) {
            Some(BlockContent::Dict(entries)) => {
                entries.iter().map(parse_multipart_field).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Project the auth for a given `mode` from the per-mode `auth:*` blocks.
    /// Public so callers (e.g. collection/folder settings, whose mode lives in a
    /// top-level `auth { mode }` block rather than the method block) can reuse it.
    pub fn project_auth(&self, mode: &str) -> Auth {
        let v = |block: &str, key: &str| self.dict_value(block, key).unwrap_or("").to_string();
        match mode {
            "inherit" => Auth::Inherit,
            "basic" => Auth::Basic {
                username: v("auth:basic", "username"),
                password: v("auth:basic", "password"),
            },
            "bearer" => Auth::Bearer {
                token: v("auth:bearer", "token"),
            },
            "apikey" => Auth::ApiKey {
                key: v("auth:apikey", "key"),
                value: v("auth:apikey", "value"),
                placement: if self.dict_value("auth:apikey", "placement") == Some("queryparams") {
                    ApiKeyPlacement::Query
                } else {
                    ApiKeyPlacement::Header
                },
            },
            "oauth2" => {
                let with_default = |key: &str, default: &str| {
                    let val = v("auth:oauth2", key);
                    if val.is_empty() {
                        default.to_string()
                    } else {
                        val
                    }
                };
                Auth::OAuth2(OAuth2 {
                    grant_type: v("auth:oauth2", "grant_type"),
                    access_token_url: v("auth:oauth2", "access_token_url"),
                    client_id: v("auth:oauth2", "client_id"),
                    client_secret: v("auth:oauth2", "client_secret"),
                    scope: v("auth:oauth2", "scope"),
                    username: v("auth:oauth2", "username"),
                    password: v("auth:oauth2", "password"),
                    credentials_placement: with_default("credentials_placement", "body"),
                    token_placement: with_default("token_placement", "header"),
                    token_header_prefix: v("auth:oauth2", "token_header_prefix"),
                    token_query_key: with_default("token_query_key", "access_token"),
                })
            }
            "digest" => Auth::Digest {
                username: v("auth:digest", "username"),
                password: v("auth:digest", "password"),
            },
            "awsv4" => Auth::AwsV4 {
                access_key_id: v("auth:awsv4", "accessKeyId"),
                secret_access_key: v("auth:awsv4", "secretAccessKey"),
                session_token: v("auth:awsv4", "sessionToken"),
                service: v("auth:awsv4", "service"),
                region: v("auth:awsv4", "region"),
                profile_name: v("auth:awsv4", "profileName"),
            },
            _ => Auth::None,
        }
    }

    fn text_block(&self, name: &str) -> Option<&str> {
        match &self.block(name)?.content {
            BlockContent::Text(t) => Some(t.as_str()),
            _ => None,
        }
    }

    /// The pre-request script body (`script:pre-request`), outdented to source.
    pub fn script_pre(&self) -> Option<String> {
        self.text_block("script:pre-request").map(outdent)
    }

    /// The post-response script body (`script:post-response`), outdented.
    pub fn script_post(&self) -> Option<String> {
        self.text_block("script:post-response").map(outdent)
    }

    /// The `tests` script body, outdented.
    pub fn tests_script(&self) -> Option<String> {
        self.text_block("tests").map(outdent)
    }
}

/// Read a single value out of a method block's dictionary.
fn method_dict_value<'a>(block: &'a crate::model::Block, key: &str) -> Option<&'a str> {
    match &block.content {
        BlockContent::Dict(entries) => {
            entries
                .iter()
                .find(|e| e.key.name() == key)
                .map(|e| match &e.value {
                    Value::Inline(s) => s.as_str(),
                    _ => "",
                })
        }
        _ => None,
    }
}

/// Parse one `body:multipart-form` dict entry into a [`MultipartField`].
///
/// An inline value of the form `@file(path)` (optionally followed by
/// ` @contentType(ct)`) projects to a [`MultipartValue::File`]; any other value
/// is plain [`MultipartValue::Text`]. The leading `@file(...)` token must be at
/// the start of the value for it to count as a file part.
fn parse_multipart_field(e: &crate::model::Entry) -> MultipartField {
    let name = e.key.name().to_string();
    let enabled = !e.disabled;
    let raw = e.value.as_inline().trim();

    if raw.starts_with("@file(") {
        let (path, content_type) = parse_file_ref(raw);
        return MultipartField {
            name,
            value: MultipartValue::File(path),
            content_type,
            enabled,
        };
    }

    MultipartField {
        name,
        value: MultipartValue::Text(raw.to_string()),
        content_type: None,
        enabled,
    }
}

/// Parse a `body:file` entry (`file: @file(path) @contentType(ct)`, `~` = not
/// selected) into a [`FileBodyItem`].
fn parse_file_body_item(e: &crate::model::Entry) -> FileBodyItem {
    let selected = !e.disabled;
    let raw = e.value.as_inline().trim();
    let (path, content_type) = parse_file_ref(raw);
    FileBodyItem {
        path,
        content_type,
        selected,
    }
}

/// Extract `(path, content_type)` from an `@file(path) [@contentType(ct)]` value.
/// The path may itself contain parentheses (e.g. `file (1).pdf`), so the closing
/// `)` of `@file(...)` is the last one *before* any `@contentType(` marker — not
/// merely the first `)`.
fn parse_file_ref(raw: &str) -> (String, Option<String>) {
    let Some(rest) = raw.strip_prefix("@file(") else {
        return (raw.to_string(), None);
    };
    let (file_part, content_type) = match rest.rfind("@contentType(") {
        Some(m) => {
            let ct = rest[m..]
                .strip_prefix("@contentType(")
                .and_then(|s| s.strip_suffix(')'))
                .map(|c| c.trim().to_string());
            (&rest[..m], ct)
        }
        None => (rest, None),
    };
    // `file_part` ends with the `@file(` closing paren (plus optional whitespace).
    let trimmed = file_part.trim_end();
    let path = trimmed.strip_suffix(')').unwrap_or(trimmed).trim().to_string();
    (path, content_type)
}

/// Strip up to two leading spaces from each line — the inverse of the 2-space
/// indent the serializer applies to verbatim bodies (port of Bruno's
/// `outdentString`). Turns stored block text into the real body payload.
fn outdent(s: &str) -> String {
    s.split('\n')
        .map(|line| {
            // Normalize CRLF: drop a trailing '\r' so bodies don't carry stray
            // carriage returns (common when a .bru is authored on Windows).
            let line = line.strip_suffix('\r').unwrap_or(line);
            line.strip_prefix("  ").unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::parse_file_ref;

    #[test]
    fn file_ref_keeps_parens_in_path() {
        // Parens inside the path must survive (Windows `file (1).pdf`).
        assert_eq!(
            parse_file_ref("@file(C:\\a\\report (final).pdf)"),
            ("C:\\a\\report (final).pdf".to_string(), None)
        );
        assert_eq!(
            parse_file_ref("@file(/tmp/x (1).bin) @contentType(application/octet-stream)"),
            (
                "/tmp/x (1).bin".to_string(),
                Some("application/octet-stream".to_string())
            )
        );
        assert_eq!(
            parse_file_ref("@file(/tmp/plain.txt)"),
            ("/tmp/plain.txt".to_string(), None)
        );
    }
}
