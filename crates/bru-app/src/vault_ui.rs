//! Secrets-vault unlock/lock and row editing.

use crate::*;
use gpui::prelude::*;

impl BruApp {
    pub(crate) fn open_vault(&mut self, cx: &mut Context<Self>) {
        self.vault_open = true;
        self.vault_error = None;
        cx.notify();
    }
    pub(crate) fn close_vault(&mut self, cx: &mut Context<Self>) {
        self.vault_open = false;
        cx.notify();
    }
    pub(crate) fn vault_unlock(&mut self, cx: &mut Context<Self>) {
        let pw = self.vault_input.read(cx).text().to_string();
        let reveal = self.reveal_secrets;
        match vault::load(&pw) {
            Ok(map) => {
                let mut rows: Vec<(String, String)> =
                    map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                rows.sort_by(|a, b| a.0.cmp(&b.0));
                self.vault_rows = rows
                    .into_iter()
                    .map(|(k, v)| {
                        (
                            cx.new(|cx| CodeEditor::single_line(cx, &k)),
                            cx.new(|cx| {
                                if reveal {
                                    CodeEditor::single_line(cx, &v)
                                } else {
                                    CodeEditor::masked_line(cx, &v)
                                }
                            }),
                        )
                    })
                    .collect();
                self.vault = Some(map);
                self.vault_pw = Some(pw);
                self.vault_error = None;
            }
            Err(e) => self.vault_error = Some(e),
        }
        self.refresh_vars();
        cx.notify();
    }
    pub(crate) fn vault_lock(&mut self, cx: &mut Context<Self>) {
        self.vault = None;
        self.vault_pw = None;
        self.vault_rows.clear();
        self.refresh_vars();
        cx.notify();
    }
    pub(crate) fn vault_add_row(&mut self, cx: &mut Context<Self>) {
        let reveal = self.reveal_secrets;
        self.vault_rows.push((
            cx.new(|cx| CodeEditor::single_line(cx, "")),
            cx.new(|cx| {
                if reveal {
                    CodeEditor::single_line(cx, "")
                } else {
                    CodeEditor::masked_line(cx, "")
                }
            }),
        ));
        cx.notify();
    }

    /// Flip the reveal-secrets eye and re-mask/unmask every value editor live.
    pub(crate) fn toggle_reveal_secrets(&mut self, cx: &mut Context<Self>) {
        self.reveal_secrets = !self.reveal_secrets;
        let reveal = self.reveal_secrets;
        for (_, v) in &self.vault_rows {
            v.update(cx, |ed, cx| ed.set_masked(!reveal, cx));
        }
        if let Some(env) = &self.env {
            for row in &env.rows {
                if row.secret {
                    row.value.update(cx, |ed, cx| ed.set_masked(!reveal, cx));
                }
            }
        }
        cx.notify();
    }
    pub(crate) fn vault_remove_row(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.vault_rows.len() {
            self.vault_rows.remove(i);
        }
        cx.notify();
    }
    pub(crate) fn vault_save(&mut self, cx: &mut Context<Self>) {
        let map: HashMap<String, String> = self
            .vault_rows
            .iter()
            .map(|(k, v)| {
                (
                    k.read(cx).text().trim().to_string(),
                    v.read(cx).text().to_string(),
                )
            })
            .filter(|(k, _)| !k.is_empty())
            .collect();
        if let Some(pw) = &self.vault_pw {
            match vault::save(pw, &map) {
                Ok(()) => {
                    self.vault = Some(map);
                    self.vault_error = None;
                }
                Err(e) => self.vault_error = Some(e),
            }
        }
        self.refresh_vars();
        cx.notify();
    }
}

#[cfg(test)]
mod cov_tests {
    //! Covers `vault_unlock` (the load -> build-rows path that the overlays.rs
    //! cov_tests skip by injecting an unlocked vault directly), plus a real
    //! save -> reload round trip and the wrong-password error branch.
    //!
    //! Vault disk IO resolves `~/.bruno-rs` from `USERPROFILE`/`HOME`, so every
    //! test here first redirects those env vars to a throwaway temp dir via
    //! `HomeGuard` and never touches the user's real home. A process-wide
    //! `HOME_LOCK` serializes the env mutation so concurrent test threads can't
    //! observe each other's redirected home.

    use super::*;
    use std::ffi::OsString;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    static HOME_LOCK: Mutex<()> = Mutex::new(());

    /// Restores `USERPROFILE`/`HOME` on drop (even on panic), so redirecting the
    /// vault's home dir into a temp location can never leak into other tests.
    struct HomeGuard {
        userprofile: Option<OsString>,
        home: Option<OsString>,
        dir: PathBuf,
    }
    impl HomeGuard {
        /// Point the vault home at a fresh temp dir for the lifetime of the guard.
        fn redirect() -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let n = N.fetch_add(1, Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("bru-app-vault-test-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let g = HomeGuard {
                userprofile: std::env::var_os("USERPROFILE"),
                home: std::env::var_os("HOME"),
                dir: dir.clone(),
            };
            std::env::set_var("USERPROFILE", &dir);
            std::env::set_var("HOME", &dir);
            g
        }
    }
    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match &self.userprofile {
                Some(v) => std::env::set_var("USERPROFILE", v),
                None => std::env::remove_var("USERPROFILE"),
            }
            match &self.home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// A windowed BruApp on a throwaway sample copy (matches the overlays.rs
    /// harness). The returned `TempCollection` must outlive the test.
    fn windowed(
        cx: &mut gpui::TestAppContext,
    ) -> (
        gpui::WindowHandle<BruApp>,
        crate::test_support::TempCollection,
    ) {
        let tc = crate::test_support::temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        (window, tc)
    }

    /// Unlocking against a *missing* vault file yields an empty map (no rows, no
    /// decrypt, no disk write beyond the temp home dir), and records the password
    /// so a later save can persist.
    #[gpui::test]
    fn vault_unlock_empty_then_lock(cx: &mut gpui::TestAppContext) {
        let _lock = HOME_LOCK.lock().unwrap();
        let _home = HomeGuard::redirect();
        let (window, _tc) = windowed(cx);
        window
            .update(cx, |app, _w, cx| {
                app.open_vault(cx);
                app.vault_input
                    .update(cx, |ed, cx| ed.set_line("pw-empty", cx));
                app.vault_unlock(cx);
                // Missing file -> Ok(empty map): unlocked, no rows, no error.
                assert!(app.vault.is_some());
                assert!(app.vault_rows.is_empty());
                assert!(app.vault_error.is_none());
                assert!(app.vault_pw.as_deref() == Some("pw-empty"));
                // Lock clears the in-memory state.
                app.vault_lock(cx);
                assert!(app.vault.is_none());
                assert!(app.vault_pw.is_none());
                assert!(app.vault_rows.is_empty());
            })
            .unwrap();
        cx.run_until_parked();
    }

    /// Full round trip: unlock -> add a secret -> save (encrypts to the temp
    /// home) -> lock -> unlock again rebuilds the masked row from disk. Then a
    /// wrong-password unlock hits the `Err` branch and sets `vault_error`.
    #[gpui::test]
    fn vault_save_reload_and_wrong_password(cx: &mut gpui::TestAppContext) {
        let _lock = HOME_LOCK.lock().unwrap();
        let _home = HomeGuard::redirect();
        let (window, _tc) = windowed(cx);
        window
            .update(cx, |app, _w, cx| {
                app.open_vault(cx);
                app.vault_input
                    .update(cx, |ed, cx| ed.set_line("master", cx));
                app.vault_unlock(cx);
                assert!(app.vault.is_some());

                // Add one secret row and fill the key/value editors.
                app.vault_add_row(cx);
                assert!(app.vault_rows.len() == 1);
                let (k, v) = &app.vault_rows[0];
                k.update(cx, |ed, cx| ed.set_line("API_KEY", cx));
                v.update(cx, |ed, cx| ed.set_line("s3cr3t", cx));

                // Save: vault_pw is Some, so this encrypts + writes under the temp
                // home (no real ~/.bruno-rs), then mirrors into `vault`.
                app.vault_save(cx);
                assert!(app.vault_error.is_none());
                assert!(
                    app.vault
                        .as_ref()
                        .unwrap()
                        .get("API_KEY")
                        .map(String::as_str)
                        == Some("s3cr3t")
                );

                // Lock then reload from disk: the masked-row build path runs (the
                // value editor is masked because reveal_secrets is false).
                app.vault_lock(cx);
                assert!(app.vault.is_none());
                app.vault_input
                    .update(cx, |ed, cx| ed.set_line("master", cx));
                app.vault_unlock(cx);
                assert!(app.vault.is_some());
                assert!(app.vault_rows.len() == 1);
                assert!(app.vault_error.is_none());
                assert!(app.vault_rows[0].0.read(cx).text() == "API_KEY");
                assert!(app.vault_rows[0].1.read(cx).text() == "s3cr3t");

                // Wrong password against the now-existing file -> Err branch.
                app.vault_lock(cx);
                app.vault_input
                    .update(cx, |ed, cx| ed.set_line("wrong", cx));
                app.vault_unlock(cx);
                assert!(app.vault.is_none());
                assert!(app.vault_error.is_some());
            })
            .unwrap();
        cx.run_until_parked();
    }

    /// Reloading with reveal_secrets on exercises the *unmasked* value-editor
    /// branch of the row build inside `vault_unlock`.
    #[gpui::test]
    fn vault_unlock_revealed_rows(cx: &mut gpui::TestAppContext) {
        let _lock = HOME_LOCK.lock().unwrap();
        let _home = HomeGuard::redirect();
        let (window, _tc) = windowed(cx);
        window
            .update(cx, |app, _w, cx| {
                // Seed a vault file via one unlock + save.
                app.open_vault(cx);
                app.vault_input.update(cx, |ed, cx| ed.set_line("pw", cx));
                app.vault_unlock(cx);
                app.vault_add_row(cx);
                let (k, v) = &app.vault_rows[0];
                k.update(cx, |ed, cx| ed.set_line("token", cx));
                v.update(cx, |ed, cx| ed.set_line("abc", cx));
                app.vault_save(cx);
                app.vault_lock(cx);

                // Reload with reveal on -> single_line (unmasked) value editors.
                app.reveal_secrets = true;
                app.vault_input.update(cx, |ed, cx| ed.set_line("pw", cx));
                app.vault_unlock(cx);
                assert!(app.vault_rows.len() == 1);
                assert!(app.vault_rows[0].1.read(cx).text() == "abc");
            })
            .unwrap();
        cx.run_until_parked();
    }
}
