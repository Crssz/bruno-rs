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
        })
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
            _ => Body::None,
        }
    }

    fn project_auth(&self, mode: &str) -> Auth {
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
