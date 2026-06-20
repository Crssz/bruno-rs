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
    /// Read the timeout input and commit it (ignored if not a number).
    pub(crate) fn apply_prefs(&mut self, cx: &mut Context<Self>) {
        if let Ok(n) = self.timeout_input.read(cx).text().trim().parse::<u64>() {
            self.pref_timeout = n;
        }
        self.prefs_open = false;
        self.persist_prefs();
        cx.notify();
    }
    /// Write the current prefs (timeout / insecure / theme) to disk.
    pub(crate) fn persist_prefs(&self) {
        save_prefs(self.pref_timeout, self.pref_insecure, !theme::is_dark());
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
