//! Filesystem operations for collection management: create / rename / clone /
//! delete request `.bru` files and folders. The GUI has no other way to mutate
//! the on-disk tree — every "New Request", "Rename", "Clone", "Delete" action
//! routes through here. All functions return the affected path (or unit) or a
//! human-readable error suitable for a modal's inline message.

use std::path::{Path, PathBuf};

use crate::edit;

/// Strip filesystem-illegal characters from a display name to form a file/dir
/// stem (port of Bruno's `sanitizeName`). Collapses runs of illegal chars and
/// trims; never returns a path separator.
pub fn sanitize(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.trim().chars() {
        match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => out.push('-'),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    let trimmed = out.trim_matches([' ', '.']).to_string();
    trimmed.chars().take(255).collect()
}

/// Validate a display name for a request/folder. `at_root` rejects names that
/// would collide with Bruno's reserved entries.
pub fn validate(name: &str, at_root: bool) -> Result<(), String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("Name cannot be empty".to_string());
    }
    if sanitize(n).is_empty() {
        return Err("Name has no usable characters".to_string());
    }
    if n.len() > 255 {
        return Err("Name is too long".to_string());
    }
    let lower = n.to_ascii_lowercase();
    if matches!(lower.as_str(), "collection" | "folder") {
        return Err(format!("\"{n}\" is a reserved name"));
    }
    if at_root && lower == "environments" {
        return Err("\"environments\" is reserved at the collection root".to_string());
    }
    Ok(())
}

/// `meta.name` of a `.bru` (or a folder via its `folder.bru`), falling back to
/// the file/dir stem.
pub fn display_name(path: &Path) -> String {
    let meta = if path.is_dir() {
        path.join("folder.bru")
    } else {
        path.to_path_buf()
    };
    std::fs::read_to_string(&meta)
        .ok()
        .and_then(|t| bru_lang::parse(&t).ok())
        .and_then(|f| f.request_name().map(str::to_string))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("item")
                .to_string()
        })
}

/// Next `seq` value for a new item in `dir` (count of existing `.bru` files + 1).
fn next_seq(dir: &Path) -> i64 {
    let n = std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    e.path().extension().and_then(|x| x.to_str()) == Some("bru")
                        && e.file_name() != "folder.bru"
                })
                .count()
        })
        .unwrap_or(0);
    n as i64 + 1
}

fn request_text(name: &str, method: &str, url: &str, seq: i64) -> String {
    let verb = method.trim().to_lowercase();
    let verb = if verb.is_empty() {
        "get".to_string()
    } else {
        verb
    };
    format!(
        "meta {{\n  name: {name}\n  type: http\n  seq: {seq}\n}}\n\n{verb} {{\n  url: {url}\n  body: none\n  auth: none\n}}\n"
    )
}

fn folder_text(name: &str, seq: i64) -> String {
    format!("meta {{\n  name: {name}\n  type: folder\n  seq: {seq}\n}}\n")
}

/// Set `meta.name` inside a parsed `.bru`, returning the re-serialized text.
fn with_meta_name(text: &str, name: &str) -> Result<String, String> {
    let mut file = bru_lang::parse(text).map_err(|e| e.to_string())?;
    let entries = edit::dict_block_mut(&mut file, "meta");
    edit::set_entry(entries, "name", name);
    Ok(bru_lang::serialize(&file))
}

/// Create `dir/<sanitized>.bru` for a new HTTP request. Errors if it exists.
pub fn new_request(dir: &Path, name: &str, method: &str, url: &str) -> Result<PathBuf, String> {
    validate(name, false)?;
    let path = dir.join(format!("{}.bru", sanitize(name)));
    if path.exists() {
        return Err(format!("\"{}\" already exists", path.display()));
    }
    std::fs::write(&path, request_text(name.trim(), method, url, next_seq(dir)))
        .map_err(|e| e.to_string())?;
    Ok(path)
}

/// Create `dir/<sanitized>/` with a `folder.bru`. Errors if it exists.
pub fn new_folder(parent: &Path, name: &str, at_root: bool) -> Result<PathBuf, String> {
    validate(name, at_root)?;
    let dir = parent.join(sanitize(name));
    if dir.exists() {
        return Err(format!("\"{}\" already exists", dir.display()));
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join("folder.bru"),
        folder_text(name.trim(), next_seq(parent)),
    )
    .map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Scaffold a brand-new empty collection at `parent/<sanitized name>`: writes a
/// `bruno.json` (the marker the loader recognises) and an `environments/` dir.
/// Returns the new collection directory. Errors if the target already exists.
pub fn create_collection(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("Name cannot be empty".to_string());
    }
    if sanitize(n).is_empty() {
        return Err("Name has no usable characters".to_string());
    }
    let dir = parent.join(sanitize(n));
    if dir.exists() {
        return Err(format!("\"{}\" already exists", dir.display()));
    }
    std::fs::create_dir_all(dir.join("environments")).map_err(|e| e.to_string())?;
    let esc = n.replace('\\', "\\\\").replace('"', "\\\"");
    let json = format!(
        "{{\n  \"version\": \"1\",\n  \"name\": \"{esc}\",\n  \"type\": \"collection\",\n  \"ignore\": [\n    \"node_modules\",\n    \".git\"\n  ]\n}}\n"
    );
    std::fs::write(dir.join("bruno.json"), json).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Rename a request file or folder, updating its `meta.name` too. Returns the
/// new path.
/// Whether `a` and `b` are the same on-disk object. A case-only rename
/// (`Get` → `get`) targets the same file on a case-insensitive filesystem
/// (Windows/macOS) but two *distinct* files on a case-sensitive one (Linux).
/// Comparing canonicalized paths gets this right; the old `eq_ignore_ascii_case`
/// heuristic wrongly treated the case-sensitive collision as "same" and let
/// `fs::rename` silently destroy the other file.
fn same_object(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (a.canonicalize(), b.canonicalize()),
            (Ok(ca), Ok(cb)) if ca == cb
        )
}

pub fn rename(
    path: &Path,
    is_folder: bool,
    new_name: &str,
    at_root: bool,
) -> Result<PathBuf, String> {
    validate(new_name, at_root)?;
    let parent = path.parent().ok_or("item has no parent directory")?;
    let stem = sanitize(new_name);

    if is_folder {
        let new_dir = parent.join(&stem);
        // A case-only change is the same item on case-insensitive filesystems
        // (Windows/macOS); don't treat it as a pre-existing collision.
        let same = same_object(&new_dir, path);
        if !same && new_dir.exists() {
            return Err(format!("\"{}\" already exists", new_dir.display()));
        }
        if new_dir != path {
            std::fs::rename(path, &new_dir).map_err(|e| e.to_string())?;
        }
        let meta = new_dir.join("folder.bru");
        if let Ok(text) = std::fs::read_to_string(&meta) {
            if let Ok(updated) = with_meta_name(&text, new_name.trim()) {
                let _ = std::fs::write(&meta, updated);
            }
        }
        Ok(new_dir)
    } else {
        let new_path = parent.join(format!("{stem}.bru"));
        let same = same_object(&new_path, path);
        if !same && new_path.exists() {
            return Err(format!("\"{}\" already exists", new_path.display()));
        }
        // Rename on disk first (this also performs a case-only change on a
        // case-insensitive filesystem), then rewrite meta.name in place. Doing
        // write-new + remove-old would delete the just-written file when the two
        // paths are the same inode (case-only rename on Windows/macOS).
        if new_path != path {
            std::fs::rename(path, &new_path).map_err(|e| e.to_string())?;
        }
        let text = std::fs::read_to_string(&new_path).map_err(|e| e.to_string())?;
        let updated = with_meta_name(&text, new_name.trim())?;
        std::fs::write(&new_path, updated).map_err(|e| e.to_string())?;
        Ok(new_path)
    }
}

/// Default name suggested when cloning: "<current> copy".
pub fn clone_suggested_name(path: &Path) -> String {
    format!("{} copy", display_name(path))
}

/// Clone a request file or folder under a new name in the same parent.
pub fn clone(path: &Path, is_folder: bool, new_name: &str) -> Result<PathBuf, String> {
    validate(new_name, false)?;
    let parent = path.parent().ok_or("item has no parent directory")?;
    let stem = sanitize(new_name);

    if is_folder {
        let dest = parent.join(&stem);
        if dest.exists() {
            return Err(format!("\"{}\" already exists", dest.display()));
        }
        copy_dir_recursive(path, &dest)?;
        let meta = dest.join("folder.bru");
        if let Ok(text) = std::fs::read_to_string(&meta) {
            if let Ok(updated) = with_meta_name(&text, new_name.trim()) {
                let _ = std::fs::write(&meta, updated);
            }
        }
        Ok(dest)
    } else {
        let dest = parent.join(format!("{stem}.bru"));
        if dest.exists() {
            return Err(format!("\"{}\" already exists", dest.display()));
        }
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let updated = with_meta_name(&text, new_name.trim()).unwrap_or(text);
        std::fs::write(&dest, updated).map_err(|e| e.to_string())?;
        Ok(dest)
    }
}

/// Paste: clone `src` into a (possibly different) directory, auto-deduping the
/// name if it collides (`name`, `name copy`, `name copy 2`, ...).
pub fn clone_to(src: &Path, dest_dir: &Path, is_folder: bool) -> Result<PathBuf, String> {
    // Never copy a folder into itself or a descendant — that recurses forever.
    if is_folder {
        let s = src.canonicalize().unwrap_or_else(|_| src.to_path_buf());
        let d = dest_dir
            .canonicalize()
            .unwrap_or_else(|_| dest_dir.to_path_buf());
        if d == s || d.starts_with(&s) {
            return Err("Cannot paste a folder into itself or a subfolder".to_string());
        }
    }
    let base = display_name(src);
    let fallback = if is_folder { "folder" } else { "request" };
    let mut candidate = base.clone();
    let mut n = 1;
    loop {
        let mut stem = sanitize(&candidate);
        if stem.is_empty() {
            stem = fallback.to_string();
        }
        let probe = if is_folder {
            dest_dir.join(&stem)
        } else {
            dest_dir.join(format!("{stem}.bru"))
        };
        if !probe.exists() {
            break;
        }
        n += 1;
        candidate = if n == 2 {
            format!("{base} copy")
        } else {
            format!("{base} copy {n}")
        };
    }
    let stem = {
        let s = sanitize(&candidate);
        if s.is_empty() {
            fallback.to_string()
        } else {
            s
        }
    };
    if is_folder {
        let dest = dest_dir.join(&stem);
        copy_dir_recursive(src, &dest)?;
        let meta = dest.join("folder.bru");
        if let Ok(text) = std::fs::read_to_string(&meta) {
            if let Ok(updated) = with_meta_name(&text, &candidate) {
                let _ = std::fs::write(&meta, updated);
            }
        }
        Ok(dest)
    } else {
        let dest = dest_dir.join(format!("{stem}.bru"));
        let text = std::fs::read_to_string(src).map_err(|e| e.to_string())?;
        let updated = with_meta_name(&text, &candidate).unwrap_or(text);
        std::fs::write(&dest, updated).map_err(|e| e.to_string())?;
        Ok(dest)
    }
}

/// Write `meta.seq` into a request `.bru` (used to reorder sidebar siblings).
pub fn set_seq(path: &Path, seq: i64) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut file = bru_lang::parse(&text).map_err(|e| e.to_string())?;
    let entries = crate::edit::dict_block_mut(&mut file, "meta");
    crate::edit::set_entry(entries, "seq", &seq.to_string());
    std::fs::write(path, bru_lang::serialize(&file)).map_err(|e| e.to_string())
}

/// Delete a request file or a folder (recursively).
pub fn delete(path: &Path, is_folder: bool) -> Result<(), String> {
    if is_folder {
        std::fs::remove_dir_all(path).map_err(|e| e.to_string())
    } else {
        std::fs::remove_file(path).map_err(|e| e.to_string())
    }
}

// ── environments ────────────────────────────────────────────────────────────

/// One editable environment variable row.
#[derive(Debug, Clone, Default)]
pub struct EnvRow {
    pub name: String,
    pub value: String,
    pub enabled: bool,
    pub secret: bool,
}

fn env_path(collection_dir: &Path, name: &str) -> PathBuf {
    collection_dir
        .join("environments")
        .join(format!("{}.bru", sanitize(name)))
}

/// Serialize env rows to Bruno's env `.bru` form (`vars { }` + `vars:secret [ ]`).
/// Secret values are never written to disk — only the names.
pub fn serialize_env(rows: &[EnvRow]) -> String {
    let mut out = String::from("vars {\n");
    for r in rows.iter().filter(|r| !r.secret) {
        let dis = if r.enabled { "" } else { "~" };
        out.push_str(&format!("  {dis}{}: {}\n", r.name, r.value));
    }
    out.push_str("}\n");
    let secrets: Vec<&EnvRow> = rows.iter().filter(|r| r.secret).collect();
    if !secrets.is_empty() {
        out.push_str("\nvars:secret [\n");
        let items: Vec<String> = secrets
            .iter()
            .map(|r| format!("  {}{}", if r.enabled { "" } else { "~" }, r.name))
            .collect();
        out.push_str(&items.join(",\n"));
        out.push_str("\n]\n");
    }
    out
}

/// Write an environment file (creating the `environments/` dir if needed),
/// preserving any leading `color:` metadata line the original file carried (the
/// var model doesn't track it, and regenerating from scratch would erase it).
pub fn save_env(collection_dir: &Path, name: &str, rows: &[EnvRow]) -> Result<(), String> {
    let dir = collection_dir.join("environments");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = env_path(collection_dir, name);
    let color = std::fs::read_to_string(&path).ok().and_then(|t| {
        t.lines()
            .find(|l| l.trim_start().starts_with("color:"))
            .map(|l| l.to_string())
    });
    let mut out = String::new();
    if let Some(c) = color {
        out.push_str(&c);
        out.push_str("\n\n");
    }
    out.push_str(&serialize_env(rows));
    std::fs::write(path, out).map_err(|e| e.to_string())
}

pub fn create_env(collection_dir: &Path, name: &str) -> Result<(), String> {
    validate(name, false)?;
    let p = env_path(collection_dir, name);
    if p.exists() {
        return Err(format!("Environment \"{name}\" already exists"));
    }
    save_env(collection_dir, name, &[])
}

pub fn delete_env(collection_dir: &Path, name: &str) -> Result<(), String> {
    std::fs::remove_file(env_path(collection_dir, name)).map_err(|e| e.to_string())
}

pub fn rename_env(collection_dir: &Path, old: &str, new: &str) -> Result<(), String> {
    validate(new, false)?;
    let op = env_path(collection_dir, old);
    let np = env_path(collection_dir, new);
    // A case-only change maps to the same file on case-insensitive filesystems.
    let same = same_object(&np, &op);
    if !same && np.exists() {
        return Err(format!("Environment \"{new}\" already exists"));
    }
    if np != op {
        std::fs::rename(&op, &np).map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn duplicate_env(collection_dir: &Path, name: &str) -> Result<(), String> {
    let src = env_path(collection_dir, name);
    let text = std::fs::read_to_string(&src).map_err(|e| e.to_string())?;
    let mut candidate = format!("{name} copy");
    let mut n = 1;
    while env_path(collection_dir, &candidate).exists() {
        n += 1;
        candidate = format!("{name} copy {n}");
    }
    std::fs::write(env_path(collection_dir, &candidate), text).map_err(|e| e.to_string())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        // Classify with the entry's own type (does NOT follow symlinks). Skip
        // symlinks: following one could copy files from outside the collection
        // tree (e.g. a planted link to ~/.ssh) into the clone/paste target.
        let ft = entry.file_type().map_err(|e| e.to_string())?;
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(tag: &str) -> Self {
            use std::sync::atomic::{AtomicU32, Ordering};
            static N: AtomicU32 = AtomicU32::new(0);
            let p = std::env::temp_dir().join(format!(
                "bru-fsops-{tag}-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn sanitize_strips_illegal() {
        assert_eq!(sanitize("a/b:c?"), "a-b-c-");
        assert_eq!(sanitize("  spaced  "), "spaced");
    }

    #[test]
    fn validate_rejects_reserved_and_empty() {
        assert!(validate("", false).is_err());
        assert!(validate("collection", false).is_err());
        assert!(validate("environments", true).is_err());
        assert!(validate("environments", false).is_ok());
        assert!(validate("Get Users", false).is_ok());
    }

    #[test]
    fn create_request_roundtrips() {
        let d = TempDir::new("req");
        let p = new_request(&d.0, "Get User", "GET", "https://api.test/u").unwrap();
        assert!(p.exists());
        let f = bru_lang::parse(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(f.request_name(), Some("Get User"));
        let r = f.to_request().unwrap();
        assert_eq!(r.method, "GET");
        assert_eq!(r.url, "https://api.test/u");
        // Duplicate create fails.
        assert!(new_request(&d.0, "Get User", "GET", "x").is_err());
    }

    #[test]
    fn rename_updates_file_and_meta() {
        let d = TempDir::new("ren");
        let p = new_request(&d.0, "Old", "GET", "x").unwrap();
        let np = rename(&p, false, "New Name", false).unwrap();
        assert!(!p.exists());
        assert!(np.exists());
        let f = bru_lang::parse(&std::fs::read_to_string(&np).unwrap()).unwrap();
        assert_eq!(f.request_name(), Some("New Name"));
    }

    #[test]
    fn clone_and_delete() {
        let d = TempDir::new("clone");
        let p = new_request(&d.0, "Orig", "GET", "x").unwrap();
        let c = clone(&p, false, "Orig copy").unwrap();
        assert!(c.exists() && p.exists());
        delete(&c, false).unwrap();
        assert!(!c.exists());
    }

    #[test]
    fn paste_folder_into_itself_is_rejected() {
        let d = TempDir::new("paste-self");
        let f = new_folder(&d.0, "A", false).unwrap();
        // Pasting A into A (or a descendant) must error, not recurse forever.
        assert!(clone_to(&f, &f, true).is_err());
        let sub = new_folder(&f, "Sub", false).unwrap();
        assert!(clone_to(&f, &sub, true).is_err());
    }

    #[test]
    fn case_only_rename_preserves_file() {
        let d = TempDir::new("case");
        let p = new_request(&d.0, "Get", "GET", "x").unwrap();
        let np = rename(&p, false, "get", false).unwrap();
        // The file must still exist and carry the new meta.name — not be deleted
        // by a write-then-remove on a case-insensitive filesystem.
        assert!(np.exists(), "renamed file should still exist");
        let f = bru_lang::parse(&std::fs::read_to_string(&np).unwrap()).unwrap();
        assert_eq!(f.request_name(), Some("get"));
    }

    #[test]
    fn env_secret_names_roundtrip() {
        let rows = vec![
            EnvRow {
                name: "BASE".into(),
                value: "http://x".into(),
                enabled: true,
                secret: false,
            },
            EnvRow {
                name: "TOKEN".into(),
                value: String::new(),
                enabled: true,
                secret: true,
            },
            EnvRow {
                name: "OTHER".into(),
                value: String::new(),
                enabled: true,
                secret: true,
            },
        ];
        let parsed = bru_lang::parse_env(&serialize_env(&rows));
        let secrets: Vec<&str> = parsed
            .iter()
            .filter(|v| v.secret)
            .map(|v| v.name.as_str())
            .collect();
        // No stray trailing commas baked into names.
        assert_eq!(secrets, vec!["TOKEN", "OTHER"]);
        assert_eq!(
            parsed
                .iter()
                .find(|v| v.name == "BASE")
                .map(|v| v.value.as_str()),
            Some("http://x")
        );
    }

    #[test]
    fn env_case_only_rename() {
        let d = TempDir::new("envcase");
        create_env(&d.0, "dev").unwrap();
        rename_env(&d.0, "dev", "Dev").unwrap();
        assert!(env_path(&d.0, "Dev").exists());
    }

    #[test]
    fn folder_create_clone_delete() {
        let d = TempDir::new("fold");
        let f = new_folder(&d.0, "Stuff", false).unwrap();
        assert!(f.join("folder.bru").exists());
        new_request(&f, "Inner", "GET", "x").unwrap();
        let c = clone(&f, true, "Stuff copy").unwrap();
        assert!(c.join("Inner.bru").exists());
        delete(&f, true).unwrap();
        assert!(!f.exists());
    }

    #[test]
    fn create_collection_scaffolds_marker_and_envs() {
        let d = TempDir::new("newcoll");
        let c = create_collection(&d.0, "My API").unwrap();
        assert!(c.join("bruno.json").exists());
        assert!(c.join("environments").is_dir());
        let json = std::fs::read_to_string(c.join("bruno.json")).unwrap();
        assert!(json.contains("\"name\": \"My API\""));
        // The loader recognises the scaffolded collection and reads its name.
        let tree = bru_lang::load_collection(&c).unwrap();
        assert_eq!(tree.name, "My API");
        // A second create at the same name errors instead of clobbering.
        assert!(create_collection(&d.0, "My API").is_err());
        // Empty / unusable names are rejected.
        assert!(create_collection(&d.0, "   ").is_err());
    }

    // ── sanitize edge cases ─────────────────────────────────────────────────
    #[test]
    fn sanitize_drops_control_chars() {
        // Control chars (< 0x20) are dropped entirely.
        let s = sanitize("a\u{0001}b\u{0009}c");
        assert_eq!(s, "abc");
    }

    #[test]
    fn sanitize_all_illegal_is_empty() {
        // After replacing illegal with '-' and trimming '.'/' ', all-illegal that
        // collapses to only trimmable chars yields empty.
        assert_eq!(sanitize("..."), "");
        assert_eq!(sanitize("   "), "");
        assert_eq!(sanitize("\u{0001}\u{0002}"), "");
    }

    #[test]
    fn sanitize_truncates_to_255() {
        let long = "a".repeat(300);
        assert_eq!(sanitize(&long).chars().count(), 255);
    }

    // ── validate: too-long name ────────────────────────────────────────────
    #[test]
    fn validate_rejects_too_long() {
        let long = "a".repeat(256);
        assert!(validate(&long, false).is_err());
        // Exactly 255 is fine.
        assert!(validate(&"a".repeat(255), false).is_ok());
    }

    #[test]
    fn validate_rejects_no_usable_chars() {
        // Non-empty trimmed, but sanitizes to empty.
        assert!(validate("...", false).is_err());
        assert!(validate("folder", false).is_err());
    }

    // ── display_name variants ──────────────────────────────────────────────
    #[test]
    fn display_name_request_meta() {
        let d = TempDir::new("dn-req");
        let p = new_request(&d.0, "Pretty Name", "GET", "x").unwrap();
        assert_eq!(display_name(&p), "Pretty Name");
    }

    #[test]
    fn display_name_folder_via_folder_bru() {
        let d = TempDir::new("dn-fold");
        let f = new_folder(&d.0, "My Folder", false).unwrap();
        assert_eq!(display_name(&f), "My Folder");
    }

    #[test]
    fn display_name_falls_back_to_stem() {
        let d = TempDir::new("dn-fb");
        // A .bru with no parseable meta.name -> falls back to file stem.
        let p = d.0.join("Lonely.bru");
        std::fs::write(&p, "not valid bru at all <<<").unwrap();
        let name = display_name(&p);
        // Either parse fails (-> stem) or name empty (-> stem).
        assert_eq!(name, "Lonely");
    }

    #[test]
    fn display_name_unreadable_uses_stem() {
        // A path that doesn't exist -> read fails -> stem fallback.
        let name = display_name(Path::new("/nonexistent/Ghost.bru"));
        assert_eq!(name, "Ghost");
    }

    // ── clone_to auto-dedup ────────────────────────────────────────────────
    #[test]
    fn clone_to_dedups_copy_and_copy_2() {
        let d = TempDir::new("cto");
        let src = new_request(&d.0, "Item", "GET", "x").unwrap();
        // First paste into same dir -> "Item copy".
        let c1 = clone_to(&src, &d.0, false).unwrap();
        assert!(c1
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .contains("Item copy"));
        // Second paste -> "Item copy 2".
        let c2 = clone_to(&src, &d.0, false).unwrap();
        let stem2 = c2.file_stem().unwrap().to_str().unwrap();
        assert!(stem2.contains("copy 2") || stem2.contains("copy 3"));
        assert!(c1.exists() && c2.exists());
    }

    #[test]
    fn clone_to_folder_recursive() {
        let d = TempDir::new("cto-fold");
        let f = new_folder(&d.0, "Box", false).unwrap();
        new_request(&f, "Inner", "GET", "x").unwrap();
        let dest_dir = TempDir::new("cto-dest");
        let pasted = clone_to(&f, &dest_dir.0, true).unwrap();
        assert!(pasted.join("Inner.bru").exists());
        assert!(pasted.join("folder.bru").exists());
    }

    // ── save_env preserves leading color line ──────────────────────────────
    #[test]
    fn save_env_preserves_color_line() {
        let d = TempDir::new("env-color");
        let dir = d.0.join("environments");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("dev.bru");
        std::fs::write(&path, "color: #ff0000\n\nvars {\n}\n").unwrap();
        let rows = vec![EnvRow {
            name: "A".into(),
            value: "1".into(),
            enabled: true,
            secret: false,
        }];
        save_env(&d.0, "dev", &rows).unwrap();
        let out = std::fs::read_to_string(&path).unwrap();
        assert!(out.contains("color: #ff0000"));
        assert!(out.contains("A: 1"));
    }

    #[test]
    fn save_env_disabled_and_secret_rows() {
        let d = TempDir::new("env-ser");
        let rows = vec![
            EnvRow {
                name: "ON".into(),
                value: "1".into(),
                enabled: true,
                secret: false,
            },
            EnvRow {
                name: "OFF".into(),
                value: "2".into(),
                enabled: false,
                secret: false,
            },
            EnvRow {
                name: "SEC".into(),
                value: "x".into(),
                enabled: true,
                secret: true,
            },
        ];
        save_env(&d.0, "prod", &rows).unwrap();
        let path = d.0.join("environments").join("prod.bru");
        let out = std::fs::read_to_string(&path).unwrap();
        assert!(out.contains("ON: 1"));
        assert!(out.contains("~OFF: 2")); // disabled prefix
        assert!(out.contains("vars:secret"));
        assert!(out.contains("SEC"));
        assert!(!out.contains("SEC: x")); // secret value never written
    }

    // ── create_env / delete_env ────────────────────────────────────────────
    #[test]
    fn create_env_and_duplicate_error() {
        let d = TempDir::new("env-create");
        create_env(&d.0, "stage").unwrap();
        assert!(env_path(&d.0, "stage").exists());
        // Duplicate -> error.
        assert!(create_env(&d.0, "stage").is_err());
        // Invalid name -> error.
        assert!(create_env(&d.0, "").is_err());
    }

    #[test]
    fn delete_env_removes_file() {
        let d = TempDir::new("env-del");
        create_env(&d.0, "temp").unwrap();
        assert!(env_path(&d.0, "temp").exists());
        delete_env(&d.0, "temp").unwrap();
        assert!(!env_path(&d.0, "temp").exists());
        // Deleting a missing env -> error.
        assert!(delete_env(&d.0, "ghost").is_err());
    }

    // ── rename_env ─────────────────────────────────────────────────────────
    #[test]
    fn rename_env_collision_errors() {
        let d = TempDir::new("env-ren");
        create_env(&d.0, "a").unwrap();
        create_env(&d.0, "b").unwrap();
        // Rename a -> b collides.
        assert!(rename_env(&d.0, "a", "b").is_err());
        // Rename a -> c works.
        rename_env(&d.0, "a", "c").unwrap();
        assert!(env_path(&d.0, "c").exists());
        assert!(!env_path(&d.0, "a").exists());
    }

    // ── duplicate_env ──────────────────────────────────────────────────────
    #[test]
    fn duplicate_env_dedups() {
        let d = TempDir::new("env-dup");
        create_env(&d.0, "src").unwrap();
        duplicate_env(&d.0, "src").unwrap();
        assert!(env_path(&d.0, "src copy").exists());
        // Second duplicate -> "src copy 2".
        duplicate_env(&d.0, "src").unwrap();
        assert!(env_path(&d.0, "src copy 2").exists());
        // Duplicating a missing env -> error.
        assert!(duplicate_env(&d.0, "ghost").is_err());
    }

    // ── set_seq ────────────────────────────────────────────────────────────
    #[test]
    fn set_seq_writes_meta() {
        let d = TempDir::new("seq");
        let p = new_request(&d.0, "Req", "GET", "x").unwrap();
        set_seq(&p, 42).unwrap();
        let f = bru_lang::parse(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(f.dict_value("meta", "seq"), Some("42"));
        // Missing file -> error.
        assert!(set_seq(Path::new("/nonexistent/x.bru"), 1).is_err());
    }

    // ── new_folder reserved-at-root ────────────────────────────────────────
    #[test]
    fn new_folder_reserved_at_root_errors() {
        let d = TempDir::new("fold-res");
        // "environments" is reserved at the collection root.
        assert!(new_folder(&d.0, "environments", true).is_err());
        // But allowed when not at root.
        assert!(new_folder(&d.0, "environments", false).is_ok());
        // Duplicate folder -> error.
        new_folder(&d.0, "Dup", false).unwrap();
        assert!(new_folder(&d.0, "Dup", false).is_err());
    }

    // ── clone request name dedup error path ────────────────────────────────
    #[test]
    fn clone_request_duplicate_errors() {
        let d = TempDir::new("clone-dup");
        let p = new_request(&d.0, "Orig", "GET", "x").unwrap();
        clone(&p, false, "Copy A").unwrap();
        // Cloning to an existing name -> error.
        assert!(clone(&p, false, "Copy A").is_err());
    }

    // ── clone_suggested_name ───────────────────────────────────────────────
    #[test]
    fn clone_suggested_name_appends_copy() {
        let d = TempDir::new("sugg");
        let p = new_request(&d.0, "Thing", "GET", "x").unwrap();
        assert_eq!(clone_suggested_name(&p), "Thing copy");
    }

    // ── clone folder onto existing dest -> error ───────────────────────────
    #[test]
    fn clone_folder_existing_dest_errors() {
        let d = TempDir::new("clone-fold-dup");
        let f = new_folder(&d.0, "Src", false).unwrap();
        new_folder(&d.0, "Dest", false).unwrap();
        // Cloning Src to the name "Dest" (already exists) -> error.
        assert!(clone(&f, true, "Dest").is_err());
    }

    // ── rename missing source -> IO error ──────────────────────────────────
    #[test]
    fn rename_missing_source_errors() {
        let d = TempDir::new("ren-missing");
        let ghost = d.0.join("ghost.bru");
        // The on-disk rename fails because the source doesn't exist.
        assert!(rename(&ghost, false, "New", false).is_err());
    }

    // ── clone request from a missing source -> read error ──────────────────
    #[test]
    fn clone_missing_source_errors() {
        let d = TempDir::new("clone-missing");
        let ghost = d.0.join("ghost.bru");
        assert!(clone(&ghost, false, "Copy").is_err());
    }
}
