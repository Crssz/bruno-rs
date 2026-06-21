//! Persisted app preferences and the recent-collections list, stored under
//! ~/.bruno-rs/. bump_recent is the pure list-maintenance helper.

use std::path::PathBuf;
/// `~/.bruno-rs/gpui-recent.json` ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Â the recent-collections list.
pub fn recent_path() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let dir = PathBuf::from(home).join(".bruno-rs");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("gpui-recent.json"))
}

pub fn load_recent() -> Vec<String> {
    recent_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn prefs_path() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let dir = PathBuf::from(home).join(".bruno-rs");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("gpui-prefs.json"))
}

/// Root dir for global (app-level, cross-collection) environments. Holds an
/// `environments/` subdir just like a collection.
pub fn globals_root() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".bruno-rs").join("globals")
}

/// Load persisted prefs as `(timeout_secs, insecure, light, developer)`.
pub fn load_prefs() -> (u64, bool, bool, bool) {
    let v = prefs_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    match v {
        Some(v) => (
            v.get("timeout").and_then(|x| x.as_u64()).unwrap_or(30),
            v.get("insecure").and_then(|x| x.as_bool()).unwrap_or(false),
            v.get("light").and_then(|x| x.as_bool()).unwrap_or(false),
            v.get("developer").and_then(|x| x.as_bool()).unwrap_or(false),
        ),
        None => (30, false, false, false),
    }
}

pub fn save_prefs(timeout: u64, insecure: bool, light: bool, developer: bool) {
    if let Some(p) = prefs_path() {
        let json = serde_json::json!({
            "timeout": timeout,
            "insecure": insecure,
            "light": light,
            "developer": developer,
        });
        let _ = std::fs::write(p, json.to_string());
    }
}

pub fn save_recent(recent: &[String]) {
    if let Some(p) = recent_path() {
        if let Ok(json) = serde_json::to_string(recent) {
            let _ = std::fs::write(p, json);
        }
    }
}

/// Move `s` to the front of the recent list (deduped, capped at 10).
pub fn bump_recent(recent: &mut Vec<String>, s: String) {
    recent.retain(|r| r != &s);
    recent.insert(0, s);
    recent.truncate(10);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_recent_moves_to_front_and_dedupes() {
        let mut r = vec!["b".to_string(), "c".to_string()];
        bump_recent(&mut r, "a".to_string());
        assert_eq!(r, vec!["a", "b", "c"]);
        // An existing entry moves to the front without duplicating.
        bump_recent(&mut r, "c".to_string());
        assert_eq!(r, vec!["c", "a", "b"]);
    }

    #[test]
    fn bump_recent_caps_at_ten() {
        let mut r = Vec::new();
        for i in 0..15 {
            bump_recent(&mut r, format!("item{i}"));
        }
        assert_eq!(r.len(), 10);
        assert_eq!(r[0], "item14"); // most recent first
        assert_eq!(r[9], "item5");
    }
}
