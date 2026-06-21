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
            v.get("developer")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
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

#[cfg(test)]
mod cov_tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    /// Serializes every test that mutates the process-global `USERPROFILE` /
    /// `HOME` env vars so they don't race each other.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// A unique temp dir to point the home env at, removed on drop. We never
    /// touch the real `~/.bruno-rs` because we override the home env first.
    struct TempHome {
        dir: PathBuf,
    }
    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
    fn temp_home() -> TempHome {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("bru-prefs-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        TempHome { dir }
    }

    /// Snapshot of the two home env vars, restored on drop so other tests see
    /// the original environment.
    struct EnvGuard {
        userprofile: Option<std::ffi::OsString>,
        home: Option<std::ffi::OsString>,
    }
    impl EnvGuard {
        fn capture() -> Self {
            EnvGuard {
                userprofile: std::env::var_os("USERPROFILE"),
                home: std::env::var_os("HOME"),
            }
        }
        /// Point the home lookup at `dir` and clear the `HOME` fallback so the
        /// `USERPROFILE` branch is exercised deterministically.
        fn point_home_at(dir: &std::path::Path) {
            std::env::set_var("USERPROFILE", dir);
            std::env::remove_var("HOME");
        }
        fn clear_home() {
            std::env::remove_var("USERPROFILE");
            std::env::remove_var("HOME");
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.userprofile {
                Some(v) => std::env::set_var("USERPROFILE", v),
                None => std::env::remove_var("USERPROFILE"),
            }
            match &self.home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[test]
    fn recent_and_prefs_paths_live_under_bruno_rs() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::capture();
        let home = temp_home();
        EnvGuard::point_home_at(&home.dir);

        let rp = recent_path().expect("recent_path under a set home");
        assert_eq!(rp, home.dir.join(".bruno-rs").join("gpui-recent.json"));
        // recent_path creates the `.bruno-rs` dir as a side effect.
        assert!(home.dir.join(".bruno-rs").is_dir());

        let pp = prefs_path().expect("prefs_path under a set home");
        assert_eq!(pp, home.dir.join(".bruno-rs").join("gpui-prefs.json"));
    }

    #[test]
    fn globals_root_is_bruno_rs_globals() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::capture();
        let home = temp_home();
        EnvGuard::point_home_at(&home.dir);

        assert_eq!(globals_root(), home.dir.join(".bruno-rs").join("globals"));
    }

    #[test]
    fn globals_root_falls_back_to_relative_when_home_unset() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::capture();
        EnvGuard::clear_home();
        // unwrap_or_default() yields an empty PathBuf, so the result is the
        // bare relative `.bruno-rs/globals`.
        assert_eq!(globals_root(), PathBuf::from(".bruno-rs").join("globals"));
    }

    #[test]
    fn paths_are_none_when_home_unset() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::capture();
        EnvGuard::clear_home();
        assert!(recent_path().is_none());
        assert!(prefs_path().is_none());
    }

    #[test]
    fn load_recent_round_trips_written_list() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::capture();
        let home = temp_home();
        EnvGuard::point_home_at(&home.dir);

        // Missing file -> default (empty) list.
        assert!(load_recent().is_empty());

        // Write a known list straight to the path the loader reads.
        let p = recent_path().unwrap();
        std::fs::write(&p, r#"["a","b","c"]"#).unwrap();
        assert_eq!(load_recent(), vec!["a", "b", "c"]);

        // Corrupt JSON -> falls back to default (empty), never panics.
        std::fs::write(&p, "not json").unwrap();
        assert!(load_recent().is_empty());
    }

    #[test]
    fn load_prefs_defaults_and_parses_fields() {
        let _lock = ENV_LOCK.lock().unwrap();
        let _guard = EnvGuard::capture();
        let home = temp_home();
        EnvGuard::point_home_at(&home.dir);

        // No file present -> the documented defaults.
        assert_eq!(load_prefs(), (30, false, false, false));

        let p = prefs_path().unwrap();
        std::fs::write(
            &p,
            r#"{"timeout":12,"insecure":true,"light":true,"developer":true}"#,
        )
        .unwrap();
        assert_eq!(load_prefs(), (12, true, true, true));

        // Partial object -> present field parsed, missing fields defaulted.
        std::fs::write(&p, r#"{"timeout":99}"#).unwrap();
        assert_eq!(load_prefs(), (99, false, false, false));

        // Corrupt JSON -> all defaults (the from_str returns None branch).
        std::fs::write(&p, "}{").unwrap();
        assert_eq!(load_prefs(), (30, false, false, false));
    }

    #[test]
    fn bump_recent_dedupes_when_value_already_at_front() {
        // Re-bumping the current front leaves the order unchanged.
        let mut r = vec!["a".to_string(), "b".to_string()];
        bump_recent(&mut r, "a".to_string());
        assert_eq!(r, vec!["a", "b"]);

        // Bumping into an empty list seeds a single entry.
        let mut empty: Vec<String> = Vec::new();
        bump_recent(&mut empty, "only".to_string());
        assert_eq!(empty, vec!["only"]);
    }
}
