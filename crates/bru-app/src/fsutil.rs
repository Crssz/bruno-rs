//! Filesystem and OS helpers: recursive copy, in-place meta.seq rewrite, and
//! reveal-in-file-manager.

use crate::edit;
use std::path::Path;
/// Recursively copy a directory tree (used by the sidebar "Clone" action).
pub fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

/// Rewrite a request file's `meta.seq` in place (for sibling reordering).
pub fn set_seq_in_file(path: &Path, seq: i64) {
    if let Ok(text) = std::fs::read_to_string(path) {
        if let Ok(mut f) = bru_lang::parse(&text) {
            edit::set_meta_seq(&mut f, seq);
            let _ = std::fs::write(path, bru_lang::serialize(&f));
        }
    }
}

/// Reveal a path in the OS file manager (Explorer on Windows, Finder on macOS).
pub fn reveal_in_file_manager(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = path;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn copy_dir_recursive_copies_nested_tree() {
        let base = std::env::temp_dir().join(format!("bru_fsutil_{}", std::process::id()));
        let src = base.join("src");
        let dst = base.join("dst");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(src.join("nested")).unwrap();
        fs::write(src.join("a.txt"), "alpha").unwrap();
        fs::write(src.join("nested").join("b.txt"), "beta").unwrap();

        copy_dir_recursive(&src, &dst).unwrap();

        assert_eq!(fs::read_to_string(dst.join("a.txt")).unwrap(), "alpha");
        assert_eq!(
            fs::read_to_string(dst.join("nested").join("b.txt")).unwrap(),
            "beta"
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn set_seq_in_file_rewrites_meta_seq() {
        let dir = std::env::temp_dir().join(format!("bru_seq_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("r.bru");
        fs::write(
            &p,
            "meta {\n  name: R\n  seq: 1\n}\n\nget {\n  url: https://x\n}\n",
        )
        .unwrap();
        set_seq_in_file(&p, 7);
        let out = fs::read_to_string(&p).unwrap();
        assert!(out.contains("seq: 7"), "new seq not written:\n{out}");
        assert!(!out.contains("seq: 1"), "old seq remained:\n{out}");
        let _ = fs::remove_dir_all(&dir);
    }
}
