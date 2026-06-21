//! DevTools, preferences, and curl/postman/cookie import dialogs.

use crate::*;
use gpui::prelude::*;

impl BruApp {
    pub(crate) fn toggle_devtools(&mut self, cx: &mut Context<Self>) {
        self.devtools_open = !self.devtools_open;
        cx.notify();
    }
    pub(crate) fn clear_devtools(&mut self, cx: &mut Context<Self>) {
        self.console.clear();
        self.network.clear();
        cx.notify();
    }

    pub(crate) fn send_options(&self) -> bru_http::SendOptions {
        bru_http::SendOptions {
            insecure: self.pref_insecure,
            timeout: std::time::Duration::from_secs(self.pref_timeout.max(1)),
            ..Default::default()
        }
    }

    pub(crate) fn open_prefs(&mut self, cx: &mut Context<Self>) {
        self.prefs_open = true;
        cx.notify();
    }
    pub(crate) fn close_prefs(&mut self, cx: &mut Context<Self>) {
        self.prefs_open = false;
        cx.notify();
    }
    pub(crate) fn toggle_insecure(&mut self, cx: &mut Context<Self>) {
        self.pref_insecure = !self.pref_insecure;
        self.persist_prefs();
        cx.notify();
    }
    /// Toggle Developer Mode: enables `require()` of local files in scripts.
    pub(crate) fn toggle_developer(&mut self, cx: &mut Context<Self>) {
        self.pref_developer = !self.pref_developer;
        self.persist_prefs();
        cx.notify();
    }
    /// Read the timeout input and commit it (ignored if not a number).
    pub(crate) fn apply_prefs(&mut self, cx: &mut Context<Self>) {
        if let Ok(n) = self.timeout_input.read(cx).text().trim().parse::<u64>() {
            self.pref_timeout = n;
        }
        self.prefs_open = false;
        self.persist_prefs();
        cx.notify();
    }
    /// Write the current prefs (timeout / insecure / theme / developer) to disk.
    pub(crate) fn persist_prefs(&self) {
        save_prefs(
            self.pref_timeout,
            self.pref_insecure,
            !theme::is_dark(),
            self.pref_developer,
        );
    }

    /// Load (or reload) a collection from `dir`, replacing open tabs.
    pub(crate) fn load_collection(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        match bru_lang::load_collection(&dir) {
            Ok(tree) => {
                self.collection = Some(tree);
                bump_recent(&mut self.recent, dir.to_string_lossy().into_owned());
                save_recent(&self.recent);
                self.dir = dir;
                self.tabs.clear();
                self.active = None;
                self.env = None;
                self.home = false;
                self.git_open = false;
                self.git_confirm_discard = false;
                self.git_output.clear();
                self.git_repo = false;
                self.git_status = None;
                self.status = "Loaded collection".into();
                self.refresh_git_status(cx);
                self.refresh_vars();
            }
            Err(e) => self.status = format!("Failed to load: {e}"),
        }
        cx.notify();
    }

    /// Pick a Postman v2.1 JSON and import it into a new collection.
    pub(crate) fn import_postman(&mut self, cx: &mut Context<Self>) {
        let Some(file) = rfd::FileDialog::new()
            .add_filter("Postman collection", &["json"])
            .pick_file()
        else {
            return;
        };
        match std::fs::read_to_string(&file) {
            Ok(json) => {
                let parent = file
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| PathBuf::from("."));
                match import::import_postman(&json, &parent) {
                    Ok(dir) => self.load_collection(dir, cx),
                    Err(e) => {
                        self.status = format!("Import failed: {e}");
                        cx.notify();
                    }
                }
            }
            Err(e) => {
                self.status = format!("Read failed: {e}");
                cx.notify();
            }
        }
    }

    pub(crate) fn open_curl(&mut self, cx: &mut Context<Self>) {
        self.curl_open = true;
        cx.notify();
    }
    pub(crate) fn close_curl(&mut self, cx: &mut Context<Self>) {
        self.curl_open = false;
        cx.notify();
    }
    /// Parse the pasted curl command, write it as a request in the collection,
    /// and open it.
    pub(crate) fn import_curl(&mut self, cx: &mut Context<Self>) {
        let text = self.curl_input.read(cx).text().to_string();
        let Some((name, bru)) = import::curl_to_bru(&text) else {
            self.status = "No URL in curl command".into();
            cx.notify();
            return;
        };
        let path = self.dir.join(format!("{}.bru", envfs::sanitize(&name)));
        if std::fs::write(&path, bru).is_ok() {
            self.curl_open = false;
            if let Ok(tree) = bru_lang::load_collection(&self.dir) {
                self.collection = Some(tree);
            }
            self.open_request(path, cx);
        } else {
            self.status = "Could not write request".into();
        }
        cx.notify();
    }

    pub(crate) fn open_cookies(&mut self, cx: &mut Context<Self>) {
        self.cookies_open = true;
        cx.notify();
    }
    pub(crate) fn close_cookies(&mut self, cx: &mut Context<Self>) {
        self.cookies_open = false;
        cx.notify();
    }
    pub(crate) fn delete_cookie(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.cookies.len() {
            self.cookies.remove(i);
        }
        cx.notify();
    }
    pub(crate) fn clear_cookies(&mut self, cx: &mut Context<Self>) {
        self.cookies.clear();
        cx.notify();
    }
}

#[cfg(test)]
mod cov_tests {
    use super::*;
    use crate::test_support::app_on_temp;

    // ---- DevTools ----------------------------------------------------------

    #[gpui::test]
    fn toggle_devtools_flips_flag(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            assert!(!app.devtools_open);
            app.toggle_devtools(cx);
            assert!(app.devtools_open);
            app.toggle_devtools(cx);
            assert!(!app.devtools_open);
        });
    }

    #[gpui::test]
    fn clear_devtools_empties_console_and_network(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.console.push("a line".to_string());
            app.console.push("another".to_string());
            assert!(!app.console.is_empty());
            app.clear_devtools(cx);
            assert!(app.console.is_empty());
            assert!(app.network.is_empty());
        });
    }

    // ---- send_options ------------------------------------------------------

    #[gpui::test]
    fn send_options_reflects_prefs_and_clamps_timeout(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, _cx| {
            // Mutate prefs directly (without persisting to disk).
            app.pref_insecure = true;
            app.pref_timeout = 42;
            let o = app.send_options();
            assert!(o.insecure);
            assert!(o.timeout == std::time::Duration::from_secs(42));
        });
    }

    #[gpui::test]
    fn send_options_zero_timeout_clamps_to_one_second(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, _cx| {
            app.pref_insecure = false;
            app.pref_timeout = 0;
            let o = app.send_options();
            assert!(!o.insecure);
            // `.max(1)` floors the duration at one second.
            assert!(o.timeout == std::time::Duration::from_secs(1));
        });
    }

    // ---- prefs overlay open/close (no persistence) -------------------------

    #[gpui::test]
    fn open_and_close_prefs(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            assert!(!app.prefs_open);
            app.open_prefs(cx);
            assert!(app.prefs_open);
            app.close_prefs(cx);
            assert!(!app.prefs_open);
        });
    }

    // ---- curl overlay + import --------------------------------------------

    #[gpui::test]
    fn open_and_close_curl(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            assert!(!app.curl_open);
            app.open_curl(cx);
            assert!(app.curl_open);
            app.close_curl(cx);
            assert!(!app.curl_open);
        });
    }

    #[gpui::test]
    fn import_curl_with_no_url_sets_status(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.open_curl(cx);
            app.curl_input
                .update(cx, |e, cx| e.set_text("curl -X GET", Lang::Plain, cx));
            app.import_curl(cx);
            // No URL token -> curl_to_bru returns None -> overlay stays open.
            assert!(app.curl_open);
            assert_eq!(app.status, "No URL in curl command");
        });
    }

    #[gpui::test]
    fn import_curl_with_url_writes_and_opens_request(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            let before = app.tabs.len();
            app.open_curl(cx);
            app.curl_input.update(cx, |e, cx| {
                e.set_text("curl https://example.com/widgets", Lang::Plain, cx)
            });
            app.import_curl(cx);
            // A valid URL closes the overlay, writes a .bru, and opens it as a tab.
            assert!(!app.curl_open);
            assert_eq!(app.tabs.len(), before + 1);
        });
        // The request file landed in the temp collection dir.
        let written = tc.dir.join("widgets.bru");
        assert!(written.exists());
    }

    // ---- cookies overlay + jar ops ----------------------------------------

    #[gpui::test]
    fn open_and_close_cookies(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            assert!(!app.cookies_open);
            app.open_cookies(cx);
            assert!(app.cookies_open);
            app.close_cookies(cx);
            assert!(!app.cookies_open);
        });
    }

    #[gpui::test]
    fn delete_cookie_removes_in_range_and_ignores_out_of_range(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.cookies.push(CookieEntry {
                domain: "a.com".to_string(),
                path: "/".to_string(),
                name: "x".to_string(),
                value: "1".to_string(),
            });
            app.cookies.push(CookieEntry {
                domain: "b.com".to_string(),
                path: "/".to_string(),
                name: "y".to_string(),
                value: "2".to_string(),
            });
            // Out-of-range index is a no-op.
            app.delete_cookie(5, cx);
            assert_eq!(app.cookies.len(), 2);
            // In-range removes that entry.
            app.delete_cookie(0, cx);
            assert_eq!(app.cookies.len(), 1);
            assert_eq!(app.cookies[0].name, "y");
        });
    }

    #[gpui::test]
    fn clear_cookies_empties_jar(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.cookies.push(CookieEntry {
                domain: "a.com".to_string(),
                path: "/".to_string(),
                name: "x".to_string(),
                value: "1".to_string(),
            });
            app.clear_cookies(cx);
            assert!(app.cookies.is_empty());
        });
    }
}
