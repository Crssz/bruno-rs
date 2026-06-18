//! Walk a Bruno collection folder into a [`CollectionTree`].
//!
//! A collection is a directory marked by `bruno.json`; every `.bru` file is a
//! request, except the inherited-config files `collection.bru` / `folder.bru`.
//! Sub-directories are folders (the `environments` dir is skipped — env files
//! get their own panel later).

use bru_core::{CollectionTree, Folder, RequestItem};
use std::fs;
use std::io;
use std::path::Path;

/// Maximum folder nesting depth walked, to bound recursion on a pathological
/// (or malicious) directory tree.
const MAX_DEPTH: usize = 64;

/// Load the collection rooted at `dir`.
pub fn load_collection(dir: &Path) -> io::Result<CollectionTree> {
    let name = collection_name(dir).unwrap_or_else(|| dir_name(dir));
    let root = load_folder(dir, name.clone(), 0)?;
    Ok(CollectionTree { name, root })
}

/// Read the collection's display name from `bruno.json` (`"name"` field).
fn collection_name(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join("bruno.json")).ok()?;
    // Tiny dependency-free extraction of the JSON "name" string; the full
    // bruno.json model lands with the import work.
    let key = text.find("\"name\"")?;
    let colon = text[key..].find(':')? + key;
    let rest = text[colon + 1..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn dir_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.display().to_string())
}

fn load_folder(dir: &Path, name: String, depth: usize) -> io::Result<Folder> {
    let mut folders = Vec::new();
    let mut requests = Vec::new();

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            let dname = dir_name(&path);
            if dname == "environments" || dname.starts_with('.') || dname == "node_modules" {
                continue;
            }
            if depth >= MAX_DEPTH {
                eprintln!(
                    "warning: skipping {} (max folder depth {MAX_DEPTH} reached)",
                    path.display()
                );
                continue;
            }
            let sub_name = folder_name(&path).unwrap_or(dname);
            folders.push(load_folder(&path, sub_name, depth + 1)?);
        } else if is_request_file(&path) {
            requests.push(load_request(&path));
        }
    }

    // Order by seq (missing seq sorts last), then by name — mirrors Bruno's
    // sidebar ordering closely enough for display.
    folders.sort_by_key(|f| f.name.to_lowercase());
    requests.sort_by(|a, b| {
        a.seq
            .unwrap_or(i64::MAX)
            .cmp(&b.seq.unwrap_or(i64::MAX))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(Folder {
        name,
        path: dir.to_path_buf(),
        folders,
        requests,
    })
}

fn is_request_file(path: &Path) -> bool {
    if path.extension().and_then(|e| e.to_str()) != Some("bru") {
        return false;
    }
    !matches!(
        path.file_name().and_then(|n| n.to_str()),
        Some("collection.bru") | Some("folder.bru")
    )
}

/// A folder's display name from its `folder.bru` (`meta.name`), if present.
fn folder_name(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join("folder.bru")).ok()?;
    let file = crate::parse(&text).ok()?;
    file.request_name().map(str::to_string)
}

fn load_request(path: &Path) -> RequestItem {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    // A request that fails to parse still appears in the tree under its filename,
    // so a single bad file never hides the rest of the collection.
    let parsed = fs::read_to_string(path)
        .ok()
        .and_then(|t| crate::parse(&t).ok());
    match parsed {
        Some(file) => RequestItem {
            name: file.request_name().map(str::to_string).unwrap_or(stem),
            path: path.to_path_buf(),
            method: file.request_method(),
            seq: file.seq(),
        },
        None => RequestItem {
            name: stem,
            path: path.to_path_buf(),
            method: None,
            seq: None,
        },
    }
}
