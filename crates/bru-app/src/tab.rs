//! The open-request tab model's load / edit-apply / sub-tab logic.

use crate::*;
use gpui::prelude::*;

impl OpenTab {
    /// Build a tab from a file path (None if unreadable/unparseable).
    pub(crate) fn load(path: PathBuf, cx: &mut Context<BruApp>) -> Option<Self> {
        let file = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| bru_lang::parse(&t).ok())?;
        let req = file.to_request();
        let method = req.as_ref().map(|r| r.method.clone()).unwrap_or_default();
        let url = req.as_ref().map(|r| r.url.clone()).unwrap_or_default();
        let body_editor = cx.new(|cx| CodeEditor::new(cx, ""));
        let body_vars_editor = cx.new(|cx| CodeEditor::new(cx, ""));
        let url_input = cx.new(|cx| CodeEditor::single_line(cx, ""));
        url_input.update(cx, |ed, cx| ed.set_line(&url, cx));
        // Mark the tab dirty on any editor edit (set_text on tab/load doesn't
        // emit Changed, so loading/switching tabs won't false-positive).
        for ed in [&body_editor, &body_vars_editor, &url_input] {
            let p = path.clone();
            cx.subscribe(ed, move |this, _ed, _ev: &editor::Changed, cx| {
                this.dirty.insert(p.clone());
                cx.notify();
            })
            .detach();
            // Hover a `{{var}}` in the URL / body to see its resolved value.
            subscribe_hover(ed, cx);
        }
        let mut tab = Self {
            path,
            method,
            req_tab: ReqTab::Body,
            resp_tab: RespTab::Response,
            file,
            body_editor,
            body_vars_editor,
            url_input,
            edit_kind: EditKind::None,
            kv_rows: Vec::new(),
            var_pre_rows: Vec::new(),
            var_post_rows: Vec::new(),
            auth_rows: Vec::new(),
            path_rows: Vec::new(),
            sending: false,
            script_post: false,
            response: None,
            text: None,
            text_scroll: gpui::ScrollHandle::new(),
            body_scroll: gpui::ScrollHandle::new(),
        };
        tab.load_active_tab(cx);
        Some(tab)
    }

    /// Build a plain-text file tab (a `require`d `.js`/`.json`, or any text file)
    /// editable in-app. Highlight is picked from the extension. `None` if the file
    /// can't be read.
    pub(crate) fn load_text(path: PathBuf, cx: &mut Context<BruApp>) -> Option<Self> {
        let content = std::fs::read_to_string(&path).ok()?;
        let lang = match path.extension().and_then(|e| e.to_str()) {
            Some("js" | "mjs" | "cjs" | "ts") => Lang::JavaScript,
            Some("json") => Lang::Json,
            _ => Lang::Plain,
        };
        let editor = cx.new(|cx| CodeEditor::new(cx, ""));
        editor.update(cx, |ed, cx| ed.set_text(&content, lang, cx));
        let p = path.clone();
        cx.subscribe(&editor, move |this, _ed, _ev: &editor::Changed, cx| {
            this.dirty.insert(p.clone());
            cx.notify();
        })
        .detach();
        // Gives the text editor the right-click menu + Ctrl+click navigation too.
        subscribe_hover(&editor, cx);
        Some(Self {
            path,
            method: String::new(),
            req_tab: ReqTab::Body,
            resp_tab: RespTab::Response,
            file: BruFile::default(),
            body_editor: cx.new(|cx| CodeEditor::new(cx, "")),
            body_vars_editor: cx.new(|cx| CodeEditor::new(cx, "")),
            url_input: cx.new(|cx| CodeEditor::single_line(cx, "")),
            edit_kind: EditKind::None,
            kv_rows: Vec::new(),
            var_pre_rows: Vec::new(),
            var_post_rows: Vec::new(),
            auth_rows: Vec::new(),
            path_rows: Vec::new(),
            sending: false,
            script_post: false,
            response: None,
            text: Some(editor),
            text_scroll: gpui::ScrollHandle::new(),
            body_scroll: gpui::ScrollHandle::new(),
        })
    }

    /// Display title: `meta.name`, else the file stem.
    pub(crate) fn title(&self) -> String {
        self.file
            .request_name()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or_else(|| {
                self.path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| "Untitled".into())
    }

    /// Fold the editor + URL edits into the in-memory file. No-op for plain-text
    /// tabs — their editor *is* the document, written straight to disk on save.
    pub(crate) fn apply_edits(&mut self, cx: &mut Context<BruApp>) {
        if self.text.is_some() {
            return;
        }
        let text = self.body_editor.read(cx).text().to_string();
        if let EditKind::Source = self.edit_kind {
            if let Ok(f) = bru_lang::parse(&text) {
                self.file = f;
            }
        }
        let url = self.url_input.read(cx).text().trim().to_string();
        edit::set_active_url(&mut self.file, &url);
        // Keep params:path in sync with the URL's :tokens, then persist any
        // edited path-param values.
        edit::sync_path_params(&mut self.file, &url);
        if !self.path_rows.is_empty() {
            let vals: Vec<(String, String)> = self
                .path_rows
                .iter()
                .map(|(n, ed)| (n.clone(), ed.read(cx).text().to_string()))
                .collect();
            edit::apply_path_values(&mut self.file, &vals);
        }
        match &self.edit_kind {
            EditKind::Body(block) => {
                if let Some(b) = self.file.blocks.iter_mut().find(|b| &b.name == block) {
                    b.content = BlockContent::Text(text);
                } else if !text.trim().is_empty() {
                    // Create the block on first edit (tests/docs/post-response
                    // script tabs start with no block in the file).
                    self.file.blocks.push(Block {
                        name: block.clone(),
                        content: BlockContent::Text(text),
                    });
                }
            }
            EditKind::Dict(block) => edit::lines_to_dict(&mut self.file, block, &text),
            EditKind::Kv(block) => {
                let rows: Vec<(String, String, bool)> = self
                    .kv_rows
                    .iter()
                    .map(|r| {
                        (
                            r.name.read(cx).text().trim().to_string(),
                            r.value.read(cx).text().to_string(),
                            r.enabled,
                        )
                    })
                    .collect();
                edit::set_kv_block(&mut self.file, block, &rows);
            }
            EditKind::GraphQl => {
                let vars = self.body_vars_editor.read(cx).text().to_string();
                set_text_block(&mut self.file, "body:graphql", text);
                set_text_block(&mut self.file, "body:graphql:vars", vars);
            }
            EditKind::Vars => {
                let pre = collect_var_rows(&self.var_pre_rows, cx);
                let post = collect_var_rows(&self.var_post_rows, cx);
                edit::set_var_block(&mut self.file, "vars:pre-request", &pre);
                edit::set_var_block(&mut self.file, "vars:post-response", &post);
            }
            EditKind::AuthForm(block) => {
                let mut lines = String::new();
                for r in &self.auth_rows {
                    let val = r.editor.read(cx).text();
                    lines.push_str(&format!("{}: {}\n", r.key, val.trim()));
                }
                edit::lines_to_dict(&mut self.file, block, &lines);
            }
            EditKind::None | EditKind::Source => {}
        }
    }

    /// Load the active sub-tab's block into the shared editor.
    pub(crate) fn load_active_tab(&mut self, cx: &mut Context<BruApp>) {
        // Cleared here; the structured-auth / vars branches below repopulate them.
        self.auth_rows = Vec::new();
        self.var_pre_rows = Vec::new();
        self.var_post_rows = Vec::new();
        // The Vars tab is two structured tables (pre-request + post-response) on
        // one page (Bruno's layout), each preserving the `@local` flag.
        if self.req_tab == ReqTab::Vars {
            self.kv_rows = Vec::new();
            self.path_rows = Vec::new();
            let path = self.path.clone();
            self.var_pre_rows = build_var_rows(&self.file, "vars:pre-request", &path, cx);
            self.var_post_rows = build_var_rows(&self.file, "vars:post-response", &path, cx);
            self.edit_kind = EditKind::Vars;
            return;
        }
        // Params/Headers/Assert + form/multipart bodies use the structured grid.
        let kv_block: Option<&str> = match self.req_tab {
            ReqTab::Params => Some("params:query"),
            ReqTab::Headers => Some("headers"),
            ReqTab::Assert => Some("assert"),
            ReqTab::Body => match edit::method_field(&self.file, "body").as_deref() {
                Some("formUrlEncoded") => Some("body:form-urlencoded"),
                Some("multipartForm") => Some("body:multipart-form"),
                _ => None,
            },
            _ => None,
        };
        if let Some(block) = kv_block {
            let path = self.path.clone();
            self.kv_rows = build_kv_rows(&self.file, block, &path, cx);
            // The Params tab also surfaces URL-derived path params.
            self.path_rows = if block == "params:query" {
                edit::kv_block_rows(&self.file, "params:path")
                    .into_iter()
                    .map(|(name, value, _)| {
                        let ed = cx.new(|cx| CodeEditor::single_line(cx, &value));
                        subscribe_grid_editor(&ed, path.clone(), cx);
                        (name, ed)
                    })
                    .collect()
            } else {
                Vec::new()
            };
            self.edit_kind = EditKind::Kv(block.to_string());
            return;
        }
        // GraphQL body: two editors (query + variables).
        if self.req_tab == ReqTab::Body
            && edit::method_field(&self.file, "body").as_deref() == Some("graphql")
        {
            let query = text_block(&self.file, "body:graphql");
            let vars = text_block(&self.file, "body:graphql:vars");
            self.kv_rows = Vec::new();
            self.path_rows = Vec::new();
            self.edit_kind = EditKind::GraphQl;
            self.body_editor
                .update(cx, |ed, cx| ed.set_text(&query, Lang::Plain, cx));
            self.body_vars_editor
                .update(cx, |ed, cx| ed.set_text(&vars, Lang::Json, cx));
            return;
        }
        // Structured Auth form: one labeled field editor per key of the mode.
        if self.req_tab == ReqTab::Auth {
            let mode = edit::method_field(&self.file, "auth").unwrap_or_default();
            if let Some(block) = auth_block_name(&mode) {
                let fields = auth_fields(&mode);
                if !fields.is_empty() {
                    let existing: HashMap<String, String> = edit::kv_block_rows(&self.file, block)
                        .into_iter()
                        .map(|(k, v, _)| (k, v))
                        .collect();
                    self.auth_rows = fields
                        .iter()
                        .map(|(label, key, secret)| {
                            let val = existing.get(*key).cloned().unwrap_or_default();
                            // Secret fields (passwords, tokens) render masked.
                            let editor = if *secret {
                                cx.new(|cx| CodeEditor::masked_line(cx, &val))
                            } else {
                                cx.new(|cx| CodeEditor::single_line(cx, &val))
                            };
                            AuthFieldRow {
                                label: (*label).to_string(),
                                key: (*key).to_string(),
                                editor,
                            }
                        })
                        .collect();
                    self.kv_rows = Vec::new();
                    self.path_rows = Vec::new();
                    self.edit_kind = EditKind::AuthForm(block.to_string());
                    return;
                }
            }
        }
        self.kv_rows = Vec::new();
        self.path_rows = Vec::new();
        let f = &self.file;
        let (text, lang, kind) = match self.req_tab {
            ReqTab::Body => {
                // Mode-driven: the method block's `body:` field picks the block;
                // fall back to any present body:* block for files without it.
                let mode = edit::method_field(f, "body").unwrap_or_default();
                let block = body_block_name(&mode).map(str::to_string).or_else(|| {
                    f.blocks
                        .iter()
                        .find(|b| b.name.starts_with("body:"))
                        .map(|b| b.name.clone())
                });
                match block {
                    Some(b) if b == "body:form-urlencoded" => {
                        (edit::dict_to_lines(f, &b), Lang::Plain, EditKind::Dict(b))
                    }
                    Some(b) => {
                        let lang = if b == "body:json" {
                            Lang::Json
                        } else {
                            Lang::Plain
                        };
                        (text_block(f, &b), lang, EditKind::Body(b))
                    }
                    None => (String::new(), Lang::Plain, EditKind::None),
                }
            }
            ReqTab::Params => (
                edit::dict_to_lines(f, "params:query"),
                Lang::Plain,
                EditKind::Dict("params:query".into()),
            ),
            ReqTab::Headers => (
                edit::dict_to_lines(f, "headers"),
                Lang::Plain,
                EditKind::Dict("headers".into()),
            ),
            // Params/Headers/Assert/Vars are handled by early returns above; these
            // arms only exist to keep the match exhaustive.
            ReqTab::Assert | ReqTab::Vars => (String::new(), Lang::Plain, EditKind::None),
            ReqTab::Auth => {
                let mode = edit::method_field(f, "auth").unwrap_or_default();
                let block = auth_block_name(&mode).map(str::to_string).or_else(|| {
                    f.blocks
                        .iter()
                        .find(|b| b.name.starts_with("auth:"))
                        .map(|b| b.name.clone())
                });
                match block {
                    Some(b) => (edit::dict_to_lines(f, &b), Lang::Plain, EditKind::Dict(b)),
                    None => (String::new(), Lang::Plain, EditKind::None),
                }
            }
            ReqTab::Script => {
                // The single "Script" tab holds both scripts; the inner sub-tab
                // (`script_post`) selects which block is shown/edited.
                let block = if self.script_post {
                    "script:post-response"
                } else {
                    "script:pre-request"
                };
                (
                    text_block(f, block),
                    Lang::JavaScript,
                    EditKind::Body(block.into()),
                )
            }
            ReqTab::Tests => (
                text_block(f, "tests"),
                Lang::JavaScript,
                EditKind::Body("tests".into()),
            ),
            ReqTab::Docs => (
                text_block(f, "docs"),
                Lang::Plain,
                EditKind::Body("docs".into()),
            ),
            ReqTab::Source => (bru_lang::serialize(f), Lang::Plain, EditKind::Source),
        };
        self.edit_kind = kind;
        self.body_editor
            .update(cx, |ed, cx| ed.set_text(&text, lang, cx));
    }

    /// Switch sub-tab: persist the current tab's edits, then load the new one.
    pub(crate) fn switch_tab(&mut self, t: ReqTab, cx: &mut Context<BruApp>) {
        self.apply_edits(cx);
        self.req_tab = t;
        self.load_active_tab(cx);
    }

    /// Switch the inner Script sub-tab (Pre Request ↔ Post Response), persisting
    /// the script currently in the editor before loading the other one.
    pub(crate) fn switch_script_tab(&mut self, post: bool, cx: &mut Context<BruApp>) {
        if self.script_post == post {
            return;
        }
        self.apply_edits(cx);
        self.script_post = post;
        self.load_active_tab(cx);
    }
}
