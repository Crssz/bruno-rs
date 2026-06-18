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
    let verb = if verb.is_empty() { "get".to_string() } else { verb };
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
    std::fs::write(dir.join("folder.bru"), folder_text(name.trim(), next_seq(parent)))
        .map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Rename a request file or folder, updating its `meta.name` too. Returns the
/// new path.
pub fn rename(path: &Path, is_folder: bool, new_name: &str, at_root: bool) -> Result<PathBuf, String> {
    validate(new_name, at_root)?;
    let parent = path.parent().ok_or("item has no parent directory")?;
    let stem = sanitize(new_name);

    if is_folder {
        let new_dir = parent.join(&stem);
        // A case-only change is the same item on case-insensitive filesystems
        // (Windows/macOS); don't treat it as a pre-existing collision.
        let same = new_dir == path || new_dir.as_os_str().eq_ignore_ascii_case(path.as_os_str());
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
        let same = new_path == path || new_path.as_os_str().eq_ignore_ascii_case(path.as_os_str());
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
        let d = dest_dir.canonicalize().unwrap_or_else(|_| dest_dir.to_path_buf());
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
        if s.is_empty() { fallback.to_string() } else { s }
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
    let same = np == op || np.as_os_str().eq_ignore_ascii_case(op.as_os_str());
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
        if from.is_dir() {
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
            EnvRow { name: "BASE".into(), value: "http://x".into(), enabled: true, secret: false },
            EnvRow { name: "TOKEN".into(), value: String::new(), enabled: true, secret: true },
            EnvRow { name: "OTHER".into(), value: String::new(), enabled: true, secret: true },
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
            parsed.iter().find(|v| v.name == "BASE").map(|v| v.value.as_str()),
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
}
