//! Minimal env-file CRUD for the gpui env manager (port of the env half of
//! `bru-app/src/fsops.rs`; bru-app is a separate workspace + a binary crate, so
//! its `pub` fns are unreachable here). Reading is delegated to `bru_lang`.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default)]
pub struct EnvRow {
    pub name: String,
    pub value: String,
    pub enabled: bool,
    pub secret: bool,
}

/// Strip filesystem-illegal chars to a file stem.
pub fn sanitize(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.trim().chars() {
        match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => out.push('-'),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out.trim_matches([' ', '.']).chars().take(255).collect()
}

/// Validate an env display name.
pub fn validate(name: &str) -> Result<(), String> {
    let n = name.trim();
    if n.is_empty() {
        return Err("Name cannot be empty".into());
    }
    if sanitize(n).is_empty() {
        return Err("Name has no usable characters".into());
    }
    if n.len() > 255 {
        return Err("Name is too long".into());
    }
    Ok(())
}

fn env_path(collection_dir: &Path, name: &str) -> PathBuf {
    collection_dir
        .join("environments")
        .join(format!("{}.bru", sanitize(name)))
}

fn same_object(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(ca), Ok(cb)) if ca == cb)
}

/// Serialize rows to Bruno env `.bru` form. Secret VALUES never hit disk.
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

pub fn save_env(collection_dir: &Path, name: &str, rows: &[EnvRow]) -> Result<(), String> {
    let dir = collection_dir.join("environments");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = env_path(collection_dir, name);
    // Preserve a leading `color:` line if the file already carried one.
    let color = std::fs::read_to_string(&path).ok().and_then(|t| {
        t.lines()
            .find(|l| l.trim_start().starts_with("color:"))
            .map(str::to_string)
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
    validate(name)?;
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
    validate(new)?;
    let op = env_path(collection_dir, old);
    let np = env_path(collection_dir, new);
    if !same_object(&np, &op) && np.exists() {
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

/// List env names under `<dir>/environments`, sorted.
pub fn scan_envs(dir: &Path) -> Vec<String> {
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir.join("environments")) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("bru") {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    v.push(stem.to_string());
                }
            }
        }
    }
    v.sort();
    v
}

/// Load an env's vars into editable rows (read path stays in bru_lang).
pub fn load_env_rows(dir: &Path, name: &str) -> Vec<EnvRow> {
    if name.is_empty() {
        return Vec::new();
    }
    bru_lang::load_env(dir, name)
        .unwrap_or_default()
        .into_iter()
        .map(|v| EnvRow {
            name: v.name,
            value: v.value,
            enabled: v.enabled,
            secret: v.secret,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn tmp() -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!(
            "bru-gpui-env-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn serialize_keeps_secret_names_not_values() {
        let rows = vec![
            EnvRow {
                name: "baseUrl".into(),
                value: "https://x".into(),
                enabled: true,
                secret: false,
            },
            EnvRow {
                name: "token".into(),
                value: "s3cr3t".into(),
                enabled: true,
                secret: true,
            },
        ];
        let s = serialize_env(&rows);
        assert!(s.contains("baseUrl: https://x"));
        assert!(s.contains("vars:secret"));
        assert!(s.contains("token"));
        assert!(!s.contains("s3cr3t")); // secret value never written
    }

    #[test]
    fn crud_roundtrip() {
        let d = tmp();
        create_env(&d, "Prod").unwrap();
        assert!(create_env(&d, "Prod").is_err()); // dup errors
        save_env(
            &d,
            "Prod",
            &[EnvRow {
                name: "k".into(),
                value: "v".into(),
                enabled: true,
                secret: false,
            }],
        )
        .unwrap();
        let rows = load_env_rows(&d, "Prod");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "k");
        rename_env(&d, "Prod", "Production").unwrap();
        assert_eq!(scan_envs(&d), vec!["Production".to_string()]);
        duplicate_env(&d, "Production").unwrap();
        assert_eq!(scan_envs(&d).len(), 2);
        delete_env(&d, "Production").unwrap();
        std::fs::remove_dir_all(&d).ok();
    }
}
