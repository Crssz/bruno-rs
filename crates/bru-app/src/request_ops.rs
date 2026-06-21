//! Send/save plus body/auth/params/vars row mutations.

use crate::*;
use gpui::prelude::*;

impl BruApp {
    /// Send the selected request: run it on a worker thread (its own tokio
    /// runtime) and deliver the result back to the UI via a oneshot + cx.spawn.
    pub(crate) fn send(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.active else { return };
        if self.tabs[i].text.is_some() {
            self.status = "Not a request \u{2014} nothing to send".into();
            cx.notify();
            return;
        }
        self.tabs[i].apply_edits(cx);
        let path = self.tabs[i].path.clone();
        let file = self.tabs[i].file.clone();
        let dir = self.dir.clone();
        let script_dir = path.parent().map(Path::to_path_buf);
        let opts = self.send_options();
        let globals = self.send_globals();
        let env = self.selected_env.clone();
        let developer = self.pref_developer;
        self.tabs[i].sending = true;
        self.status = "Sending\u{2026}".into();
        let (tx, rx) = futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(run_blocking(
                file, dir, script_dir, opts, globals, env, developer,
            ));
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                // Tab may have moved/closed while in flight ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Â re-find by path.
                let status = match &result {
                    Ok(o) if o.error.is_none() => "Response received",
                    Ok(_) => "Request error",
                    Err(_) => "Send cancelled",
                };
                let outcome = result.ok();
                // Capture Set-Cookie headers into the session jar.
                if let Some(r) = outcome.as_ref().and_then(|o| o.response.as_ref()) {
                    let host = host_of(&outcome.as_ref().unwrap().url);
                    for (k, v) in &r.headers {
                        if k.eq_ignore_ascii_case("set-cookie") {
                            if let Some(c) = parse_set_cookie(v, &host) {
                                upsert_cookie(&mut this.cookies, c);
                            }
                        }
                    }
                }
                // DevTools: mirror console + a network-log row.
                if let Some(o) = outcome.as_ref() {
                    for line in &o.console {
                        this.console.push(line.clone());
                    }
                    if this.console.len() > 500 {
                        let d = this.console.len() - 500;
                        this.console.drain(0..d);
                    }
                    this.network.push(NetEntry {
                        method: o.method.clone(),
                        url: o.url.clone(),
                        status: o.response.as_ref().map(|r| r.status).unwrap_or(0),
                        ms: o.response.as_ref().map(|r| r.duration_ms).unwrap_or(0),
                        size: o.response.as_ref().map(|r| r.body.len()).unwrap_or(0),
                        ok: o.error.is_none(),
                    });
                    if this.network.len() > 200 {
                        let d = this.network.len() - 200;
                        this.network.drain(0..d);
                    }
                }
                if let Some(tab) = this.tabs.iter_mut().find(|t| t.path == path) {
                    tab.sending = false;
                    if let Some(o) = outcome {
                        tab.resp_tab = RespTab::Response;
                        tab.response = Some(o);
                    }
                }
                this.status = status.into();
                cx.notify();
            });
        })
        .detach();
    }

    /// Apply the active tab's edits, then serialize it to disk.
    pub(crate) fn save(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.active else { return };
        self.tabs[i].apply_edits(cx);
        let path = self.tabs[i].path.clone();
        // A plain-text tab writes its editor verbatim; a request serializes its file.
        let ok = if let Some(ed) = &self.tabs[i].text {
            std::fs::write(&path, ed.read(cx).text()).is_ok()
        } else {
            std::fs::write(&path, bru_lang::serialize(&self.tabs[i].file)).is_ok()
        };
        if ok {
            self.dirty.remove(&path);
        }
        self.status = if ok {
            "Saved".into()
        } else {
            "Save failed".into()
        };
    }

    /// Change the active request's body mode: set the method `body:` field,
    /// create the content block if absent, and reload the editor.
    pub(crate) fn set_body_mode(&mut self, mode: &str, cx: &mut Context<Self>) {
        let Some(i) = self.active else { return };
        self.tabs[i].apply_edits(cx);
        edit::set_method_field(&mut self.tabs[i].file, "body", mode);
        if let Some(block) = body_block_name(mode) {
            if !self.tabs[i].file.blocks.iter().any(|b| b.name == block) {
                let content = if mode == "formUrlEncoded" || mode == "multipartForm" {
                    BlockContent::Dict(Vec::new())
                } else {
                    BlockContent::Text(String::new())
                };
                self.tabs[i].file.blocks.push(Block {
                    name: block.into(),
                    content,
                });
            }
        }
        self.tabs[i].load_active_tab(cx);
        self.dirty.insert(self.tabs[i].path.clone());
        cx.notify();
    }

    /// Change the active request's auth mode (mirrors `set_body_mode`).
    pub(crate) fn set_auth_mode(&mut self, mode: &str, cx: &mut Context<Self>) {
        let Some(i) = self.active else { return };
        self.tabs[i].apply_edits(cx);
        edit::set_method_field(&mut self.tabs[i].file, "auth", mode);
        if let Some(block) = auth_block_name(mode) {
            if !self.tabs[i].file.blocks.iter().any(|b| b.name == block) {
                self.tabs[i].file.blocks.push(Block {
                    name: block.into(),
                    content: BlockContent::Dict(Vec::new()),
                });
            }
        }
        self.tabs[i].load_active_tab(cx);
        self.dirty.insert(self.tabs[i].path.clone());
        cx.notify();
    }

    // ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ structured params/headers grid ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬
    pub(crate) fn kv_add_row(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.active else { return };
        let path = self.tabs[i].path.clone();
        let name = cx.new(|cx| CodeEditor::single_line(cx, ""));
        let value = cx.new(|cx| CodeEditor::single_line(cx, ""));
        subscribe_grid_editor(&name, path.clone(), cx);
        subscribe_grid_editor(&value, path.clone(), cx);
        self.tabs[i].kv_rows.push(KvRow {
            name,
            value,
            enabled: true,
        });
        self.dirty.insert(path);
        cx.notify();
    }
    pub(crate) fn kv_remove_row(&mut self, idx: usize, cx: &mut Context<Self>) {
        let Some(i) = self.active else { return };
        if idx < self.tabs[i].kv_rows.len() {
            self.tabs[i].kv_rows.remove(idx);
            self.dirty.insert(self.tabs[i].path.clone());
            cx.notify();
        }
    }
    pub(crate) fn kv_toggle_row(&mut self, idx: usize, cx: &mut Context<Self>) {
        let Some(i) = self.active else { return };
        if let Some(r) = self.tabs[i].kv_rows.get_mut(idx) {
            r.enabled = !r.enabled;
            self.dirty.insert(self.tabs[i].path.clone());
            cx.notify();
        }
    }
    /// Move a params/headers row up (`up == true`) or down by swapping neighbors.
    pub(crate) fn kv_move_row(&mut self, idx: usize, up: bool, cx: &mut Context<Self>) {
        let Some(i) = self.active else { return };
        let rows = &mut self.tabs[i].kv_rows;
        let j = match (up, idx) {
            (true, 0) => return,
            (true, _) => idx - 1,
            (false, _) if idx + 1 >= rows.len() => return,
            (false, _) => idx + 1,
        };
        if idx >= rows.len() {
            return;
        }
        rows.swap(idx, j);
        self.dirty.insert(self.tabs[i].path.clone());
        cx.notify();
    }

    // â”€â”€ Vars tables (pre-request + post-response) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    /// Mutable handle to one of the active tab's vars-row vectors.
    pub(crate) fn var_rows_mut(&mut self, i: usize, post: bool) -> &mut Vec<VarRow> {
        if post {
            &mut self.tabs[i].var_post_rows
        } else {
            &mut self.tabs[i].var_pre_rows
        }
    }
    pub(crate) fn var_add_row(&mut self, post: bool, cx: &mut Context<Self>) {
        let Some(i) = self.active else { return };
        let path = self.tabs[i].path.clone();
        let name = cx.new(|cx| CodeEditor::single_line(cx, ""));
        let value = cx.new(|cx| CodeEditor::single_line(cx, ""));
        subscribe_grid_editor(&name, path.clone(), cx);
        subscribe_grid_editor(&value, path.clone(), cx);
        let row = VarRow {
            name,
            value,
            enabled: true,
            local: false,
        };
        self.var_rows_mut(i, post).push(row);
        self.dirty.insert(path);
        cx.notify();
    }
    /// Move a vars row up (`up == true`) or down by swapping with its neighbor.
    pub(crate) fn var_move_row(
        &mut self,
        post: bool,
        idx: usize,
        up: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(i) = self.active else { return };
        let path = self.tabs[i].path.clone();
        let rows = self.var_rows_mut(i, post);
        let j = match (up, idx) {
            (true, 0) => return,
            (true, _) => idx - 1,
            (false, _) if idx + 1 >= rows.len() => return,
            (false, _) => idx + 1,
        };
        if idx >= rows.len() {
            return;
        }
        rows.swap(idx, j);
        self.dirty.insert(path);
        cx.notify();
    }
    pub(crate) fn var_remove_row(&mut self, post: bool, idx: usize, cx: &mut Context<Self>) {
        let Some(i) = self.active else { return };
        let path = self.tabs[i].path.clone();
        let rows = self.var_rows_mut(i, post);
        if idx >= rows.len() {
            return;
        }
        rows.remove(idx);
        self.dirty.insert(path);
        cx.notify();
    }
    pub(crate) fn var_toggle_row(&mut self, post: bool, idx: usize, cx: &mut Context<Self>) {
        let Some(i) = self.active else { return };
        let path = self.tabs[i].path.clone();
        let rows = self.var_rows_mut(i, post);
        let Some(r) = rows.get_mut(idx) else { return };
        r.enabled = !r.enabled;
        self.dirty.insert(path);
        cx.notify();
    }

    /// Open a request as a tab, or focus it if already open.
    pub(crate) fn open_request(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if let Some(i) = self.tabs.iter().position(|t| t.path == path) {
            self.active = Some(i);
            return;
        }
        if let Some(tab) = OpenTab::load(path, cx) {
            self.tabs.push(tab);
            self.active = Some(self.tabs.len() - 1);
            self.status.clear();
            self.home = false;
        }
    }

    /// Open a plain-text file (e.g. a `require`d `.js`) in its own editable tab,
    /// activating it if already open. Used by Ctrl+click "go to" on a module path.
    pub(crate) fn open_text_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if let Some(i) = self.tabs.iter().position(|t| t.path == path) {
            self.active = Some(i);
            cx.notify();
            return;
        }
        match OpenTab::load_text(path, cx) {
            Some(tab) => {
                self.tabs.push(tab);
                self.active = Some(self.tabs.len() - 1);
                self.home = false;
                self.status.clear();
            }
            None => self.status = "Could not open file".into(),
        }
        cx.notify();
    }
}

#[cfg(test)]
mod cov_tests {
    use super::*;
    use crate::test_support::app_on_temp;

    // ── open_request ────────────────────────────────────────────────────

    #[gpui::test]
    fn open_request_opens_new_tab_and_leaves_home(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let req = tc.dir.join("Repository Info.bru");
        app.update(cx, |app, cx| app.open_request(req.clone(), cx));
        app.update(cx, |app, _| {
            assert_eq!(app.tabs.len(), 1);
            assert_eq!(app.active, Some(0));
            assert!(!app.home);
            assert!(app.status.is_empty());
            assert_eq!(app.tabs[0].path, req);
        });
    }

    #[gpui::test]
    fn open_request_focuses_existing_tab(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let a = tc.dir.join("Repository Info.bru");
        let b = tc.dir.join("New Request.bru");
        app.update(cx, |app, cx| {
            app.open_request(a.clone(), cx);
            app.open_request(b.clone(), cx);
        });
        // Re-opening the first should focus it, not push a duplicate.
        app.update(cx, |app, cx| app.open_request(a.clone(), cx));
        app.update(cx, |app, _| {
            assert_eq!(app.tabs.len(), 2);
            assert_eq!(app.active, Some(0));
        });
    }

    // ── open_text_file ──────────────────────────────────────────────────

    #[gpui::test]
    fn open_text_file_opens_json_tab(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let json = tc.dir.join("bruno.json");
        app.update(cx, |app, cx| app.open_text_file(json.clone(), cx));
        app.update(cx, |app, _| {
            assert_eq!(app.tabs.len(), 1);
            assert_eq!(app.active, Some(0));
            assert!(!app.home);
            assert!(app.status.is_empty());
            // A text tab carries an editor in `text`.
            assert!(app.tabs[0].text.is_some());
        });
    }

    #[gpui::test]
    fn open_text_file_focuses_existing(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let json = tc.dir.join("bruno.json");
        app.update(cx, |app, cx| {
            app.open_text_file(json.clone(), cx);
            // Switch focus away so we can prove re-opening re-focuses it.
            app.open_request(tc.dir.join("Repository Info.bru"), cx);
        });
        app.update(cx, |app, _| assert_eq!(app.active, Some(1)));
        app.update(cx, |app, cx| app.open_text_file(json.clone(), cx));
        app.update(cx, |app, _| {
            assert_eq!(app.tabs.len(), 2);
            assert_eq!(app.active, Some(0));
        });
    }

    #[gpui::test]
    fn open_text_file_missing_sets_status(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let missing = tc.dir.join("does-not-exist.js");
        app.update(cx, |app, cx| app.open_text_file(missing, cx));
        app.update(cx, |app, _| {
            assert_eq!(app.tabs.len(), 0);
            assert_eq!(app.status, "Could not open file");
        });
    }

    // ── set_body_mode / set_auth_mode ───────────────────────────────────

    #[gpui::test]
    fn set_body_mode_json_creates_text_block(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let req = tc.dir.join("Repository Info.bru");
        app.update(cx, |app, cx| app.open_request(req.clone(), cx));
        app.update(cx, |app, cx| app.set_body_mode("json", cx));
        app.update(cx, |app, _| {
            let f = &app.tabs[0].file;
            assert_eq!(edit::method_field(f, "body").as_deref(), Some("json"));
            let block = f.blocks.iter().find(|b| b.name == "body:json");
            assert!(block.is_some());
            assert!(matches!(block.unwrap().content, BlockContent::Text(_)));
            assert!(app.dirty.contains(&req));
        });
    }

    #[gpui::test]
    fn set_body_mode_form_creates_dict_block(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let req = tc.dir.join("Repository Info.bru");
        app.update(cx, |app, cx| app.open_request(req.clone(), cx));
        app.update(cx, |app, cx| app.set_body_mode("formUrlEncoded", cx));
        app.update(cx, |app, _| {
            let f = &app.tabs[0].file;
            assert_eq!(
                edit::method_field(f, "body").as_deref(),
                Some("formUrlEncoded")
            );
            let block = f
                .blocks
                .iter()
                .find(|b| b.name == "body:form-urlencoded")
                .expect("form block created");
            assert!(matches!(block.content, BlockContent::Dict(_)));
        });
    }

    #[gpui::test]
    fn set_auth_mode_bearer_creates_block(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let req = tc.dir.join("Repository Info.bru");
        app.update(cx, |app, cx| app.open_request(req.clone(), cx));
        app.update(cx, |app, cx| app.set_auth_mode("bearer", cx));
        app.update(cx, |app, _| {
            let f = &app.tabs[0].file;
            assert_eq!(edit::method_field(f, "auth").as_deref(), Some("bearer"));
            assert!(f.blocks.iter().any(|b| b.name == "auth:bearer"));
            assert!(app.dirty.contains(&req));
        });
    }

    #[gpui::test]
    fn set_auth_mode_none_sets_field_without_block(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let req = tc.dir.join("Repository Info.bru");
        app.update(cx, |app, cx| app.open_request(req.clone(), cx));
        app.update(cx, |app, cx| app.set_auth_mode("none", cx));
        app.update(cx, |app, _| {
            let f = &app.tabs[0].file;
            assert_eq!(edit::method_field(f, "auth").as_deref(), Some("none"));
            // `none` has no auth: block.
            assert!(!f.blocks.iter().any(|b| b.name.starts_with("auth:")));
        });
    }

    // ── kv grid rows (Headers tab) ──────────────────────────────────────

    fn open_on_headers(
        cx: &mut gpui::TestAppContext,
    ) -> (gpui::Entity<BruApp>, crate::test_support::TempCollection) {
        let (app, tc) = app_on_temp(cx);
        let req = tc.dir.join("Repository Info.bru");
        app.update(cx, |app, cx| {
            app.open_request(req, cx);
            app.tabs[0].switch_tab(ReqTab::Headers, cx);
        });
        (app, tc)
    }

    #[gpui::test]
    fn kv_add_row_pushes_editor_row(cx: &mut gpui::TestAppContext) {
        let (app, tc) = open_on_headers(cx);
        let _ = &tc;
        app.update(cx, |app, cx| {
            app.kv_add_row(cx);
            app.kv_add_row(cx);
        });
        app.update(cx, |app, _| {
            assert_eq!(app.tabs[0].kv_rows.len(), 2);
            assert!(app.tabs[0].kv_rows[0].enabled);
            assert!(app.dirty.contains(&app.tabs[0].path));
        });
    }

    #[gpui::test]
    fn kv_toggle_and_remove_row(cx: &mut gpui::TestAppContext) {
        let (app, tc) = open_on_headers(cx);
        let _ = &tc;
        app.update(cx, |app, cx| {
            app.kv_add_row(cx);
            app.kv_add_row(cx);
            app.kv_toggle_row(0, cx);
        });
        app.update(cx, |app, _| assert!(!app.tabs[0].kv_rows[0].enabled));
        app.update(cx, |app, cx| {
            // Out-of-range toggle/remove are silent no-ops.
            app.kv_toggle_row(99, cx);
            app.kv_remove_row(99, cx);
            app.kv_remove_row(0, cx);
        });
        app.update(cx, |app, _| assert_eq!(app.tabs[0].kv_rows.len(), 1));
    }

    #[gpui::test]
    fn kv_move_row_swaps_and_respects_bounds(cx: &mut gpui::TestAppContext) {
        let (app, tc) = open_on_headers(cx);
        let _ = &tc;
        app.update(cx, |app, cx| {
            app.kv_add_row(cx);
            app.kv_add_row(cx);
            app.kv_add_row(cx);
        });
        // Tag row 0 so we can follow it across swaps.
        app.update(cx, |app, cx| {
            app.tabs[0].kv_rows[0]
                .name
                .update(cx, |ed, cx| ed.set_line("first", cx));
        });
        app.update(cx, |app, cx| {
            // Moving up at idx 0 is a no-op.
            app.kv_move_row(0, true, cx);
            // Moving down past the end is a no-op.
            app.kv_move_row(2, false, cx);
            // Move row 0 down -> it lands at index 1.
            app.kv_move_row(0, false, cx);
        });
        app.update(cx, |app, cx| {
            let moved = app.tabs[0].kv_rows[1].name.read(cx).text().to_string();
            assert_eq!(moved, "first");
        });
    }

    // ── vars rows (Vars tab) ────────────────────────────────────────────

    fn open_on_vars(
        cx: &mut gpui::TestAppContext,
    ) -> (gpui::Entity<BruApp>, crate::test_support::TempCollection) {
        let (app, tc) = app_on_temp(cx);
        let req = tc.dir.join("Repository Info.bru");
        app.update(cx, |app, cx| {
            app.open_request(req, cx);
            app.tabs[0].switch_tab(ReqTab::Vars, cx);
        });
        (app, tc)
    }

    #[gpui::test]
    fn var_add_row_targets_correct_table(cx: &mut gpui::TestAppContext) {
        let (app, tc) = open_on_vars(cx);
        let _ = &tc;
        app.update(cx, |app, cx| {
            app.var_add_row(false, cx); // pre-request
            app.var_add_row(true, cx); // post-response
            app.var_add_row(true, cx);
        });
        app.update(cx, |app, _| {
            assert_eq!(app.var_rows_mut(0, false).len(), 1);
            assert_eq!(app.var_rows_mut(0, true).len(), 2);
            assert!(app.dirty.contains(&app.tabs[0].path));
        });
    }

    #[gpui::test]
    fn var_toggle_and_remove_row(cx: &mut gpui::TestAppContext) {
        let (app, tc) = open_on_vars(cx);
        let _ = &tc;
        app.update(cx, |app, cx| {
            app.var_add_row(false, cx);
            app.var_add_row(false, cx);
            app.var_toggle_row(false, 0, cx);
        });
        app.update(cx, |app, _| assert!(!app.var_rows_mut(0, false)[0].enabled));
        app.update(cx, |app, cx| {
            // Out-of-range are no-ops.
            app.var_toggle_row(false, 50, cx);
            app.var_remove_row(false, 50, cx);
            app.var_remove_row(false, 0, cx);
        });
        app.update(cx, |app, _| assert_eq!(app.var_rows_mut(0, false).len(), 1));
    }

    #[gpui::test]
    fn var_move_row_swaps_and_respects_bounds(cx: &mut gpui::TestAppContext) {
        let (app, tc) = open_on_vars(cx);
        let _ = &tc;
        app.update(cx, |app, cx| {
            app.var_add_row(false, cx);
            app.var_add_row(false, cx);
        });
        app.update(cx, |app, cx| {
            app.var_rows_mut(0, false)[0]
                .name
                .update(cx, |ed, cx| ed.set_line("v0", cx));
        });
        app.update(cx, |app, cx| {
            app.var_move_row(false, 0, true, cx); // up at 0 -> no-op
            app.var_move_row(false, 1, false, cx); // down past end -> no-op
            app.var_move_row(false, 0, false, cx); // 0 -> 1
        });
        app.update(cx, |app, cx| {
            let moved = app.var_rows_mut(0, false)[1]
                .name
                .read(cx)
                .text()
                .to_string();
            assert_eq!(moved, "v0");
        });
    }

    // ── save (text tab) ─────────────────────────────────────────────────

    #[gpui::test]
    fn save_text_tab_writes_to_disk(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let json = tc.dir.join("bruno.json");
        app.update(cx, |app, cx| app.open_text_file(json.clone(), cx));
        // Edit the text editor, mark dirty, then save verbatim to disk.
        app.update(cx, |app, cx| {
            if let Some(ed) = app.tabs[0].text.clone() {
                ed.update(cx, |ed, cx| ed.set_text("{\"x\":1}", Lang::Json, cx));
            }
            app.dirty.insert(json.clone());
            app.save(cx);
        });
        app.update(cx, |app, _| {
            assert_eq!(app.status, "Saved");
            assert!(!app.dirty.contains(&json));
        });
        let on_disk = std::fs::read_to_string(&json).unwrap();
        assert_eq!(on_disk, "{\"x\":1}");
    }
}
