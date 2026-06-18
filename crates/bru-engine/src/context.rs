//! Build a run's base variable map from a collection on disk: collection-level
//! `vars:pre-request` (low precedence) overlaid by an environment's vars. Shared
//! by the CLI and the GUI so variable resolution behaves identically.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bru_core::{BlockContent, BruFile};

/// Resolve the base variables for running something at `path`, optionally
/// loading environment `env`. Returns an empty map if `path` is not inside a
/// collection (no `bruno.json` ancestor).
pub fn base_vars(path: &Path, env: Option<&str>) -> HashMap<String, String> {
    let mut vars = HashMap::new();
    let Some(root) = find_collection_root(path) else {
        return vars;
    };

    if let Ok(text) = std::fs::read_to_string(root.join("collection.bru")) {
        if let Ok(file) = bru_lang::parse(&text) {
            for (k, v) in dict_vars(&file, "vars:pre-request") {
                vars.insert(k, v);
            }
        }
    }

    if let Some(env) = env {
        if let Ok(env_vars) = bru_lang::load_env(&root, env) {
            for v in env_vars.into_iter().filter(|v| v.enabled && !v.secret) {
                vars.insert(v.name, v.value);
            }
        }
    }
    vars
}

/// Walk up from `path` to the collection root — the directory containing a
/// `bruno.json`, or (for irregular collections the GUI still opens) a
/// `collection.bru` or an `environments/` dir, so collection + env vars resolve
/// for any folder the app can load.
pub fn find_collection_root(path: &Path) -> Option<PathBuf> {
    let mut dir = if path.is_dir() {
        Some(path)
    } else {
        path.parent()
    };
    while let Some(d) = dir {
        if d.join("bruno.json").is_file()
            || d.join("collection.bru").is_file()
            || d.join("environments").is_dir()
        {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

fn dict_vars(file: &BruFile, block: &str) -> Vec<(String, String)> {
    match file.block(block).map(|b| &b.content) {
        Some(BlockContent::Dict(entries)) => entries
            .iter()
            .filter(|e| !e.disabled)
            .map(|e| (e.key.name().to_string(), e.value.as_inline().to_string()))
            .collect(),
        _ => Vec::new(),
    }
}
