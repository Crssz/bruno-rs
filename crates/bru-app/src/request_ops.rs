//! Send/save plus body/auth/params/vars row mutations.

use crate::*;
use gpui::prelude::*;

impl BruApp {
    /// Send the selected request: run it on a worker thread (its own tokio
    /// runtime) and deliver the result back to the UI via a oneshot + cx.spawn.
    pub(crate) fn send(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.active else { return };
        self.tabs[i].apply_edits(cx);
        let path = self.tabs[i].path.clone();
        let file = self.tabs[i].file.clone();
        let dir = self.dir.clone();
        let script_dir = path.parent().map(Path::to_path_buf);
        let opts = self.send_options();
        let globals = self.send_globals();
        let env = self.selected_env.clone();
        self.tabs[i].sending = true;
        self.status = "Sending\u{2026}".into();
        let (tx, rx) = futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(run_blocking(file, dir, script_dir, opts, globals, env));
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
        let ok = std::fs::write(&path, bru_lang::serialize(&self.tabs[i].file)).is_ok();
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
}
