//! Environment `.bru` loading. An env file holds a `vars { ... }` dict and an
//! optional `vars:secret [ ... ]` name list (secret *values* never live on disk).
//! `color:` is a bare top-level line; we strip it before the generic block
//! parser, which expects `name { ... }` / `name [ ... ]` blocks.

use bru_core::BlockContent;
use std::path::Path;

/// One environment variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
    pub enabled: bool,
    pub secret: bool,
}

/// Parse environment `.bru` text into its variables. Malformed input yields an
/// empty list rather than an error (a bad env file shouldn't abort a run).
pub fn parse_env(text: &str) -> Vec<EnvVar> {
    // Drop the bare top-level `color:` line(s); keep indented var lines intact.
    let filtered: String = text
        .lines()
        .filter(|l| !l.starts_with("color:"))
        .collect::<Vec<_>>()
        .join("\n");

    let file = match crate::parse(&filtered) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };

    let mut vars = Vec::new();
    for block in &file.blocks {
        match (block.name.as_str(), &block.content) {
            ("vars", BlockContent::Dict(entries)) => {
                for e in entries {
                    vars.push(EnvVar {
                        name: e.key.name().to_string(),
                        value: e.value.as_inline().to_string(),
                        enabled: !e.disabled,
                        secret: false,
                    });
                }
            }
            ("vars:secret", BlockContent::List(items)) => {
                for item in items {
                    let (name, enabled) = match item.strip_prefix('~') {
                        Some(rest) => (rest.to_string(), false),
                        None => (item.clone(), true),
                    };
                    vars.push(EnvVar {
                        name,
                        value: String::new(),
                        enabled,
                        secret: true,
                    });
                }
            }
            _ => {}
        }
    }
    vars
}

/// Load `environments/<name>.bru` under a collection directory.
pub fn load_env(collection_dir: &Path, name: &str) -> std::io::Result<Vec<EnvVar>> {
    let path = collection_dir
        .join("environments")
        .join(format!("{name}.bru"));
    let text = std::fs::read_to_string(path)?;
    Ok(parse_env(&text))
}
