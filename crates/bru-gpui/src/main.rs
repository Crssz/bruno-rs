// Phase 3a: real collection data. Loads a Bruno collection (the bundled sample,
// or a path arg), renders a clickable recursive sidebar, and shows the opened
// request's real method/URL/body (JSON bodies tree-sitter-highlighted).
mod edit;
mod editor;
mod highlight;
mod theme;

use std::path::{Path, PathBuf};

use bru_core::{BlockContent, BruFile, CollectionTree, Folder};
use editor::{CodeEditor, Lang};

/// What the single shared editor is currently editing (set per active sub-tab),
/// so its content can be written back to the right place in the BruFile.
enum EditKind {
    /// Nothing editable for this tab.
    None,
    /// A raw-text block (a `body:*` or `script:*` block): set its text verbatim.
    Body(String),
    /// A dictionary block (params/headers/vars/auth) edited as `key: value` lines.
    Dict(String),
    /// The whole `.bru` source: reparse it on apply.
    Source,
}

/// Request sub-tabs (Body is the editable editor; the rest show parsed data).
#[derive(Clone, Copy, PartialEq)]
enum ReqTab {
    Params,
    Body,
    Headers,
    Auth,
    Vars,
    Script,
    Source,
}

impl ReqTab {
    const ALL: [ReqTab; 7] = [
        ReqTab::Params,
        ReqTab::Body,
        ReqTab::Headers,
        ReqTab::Auth,
        ReqTab::Vars,
        ReqTab::Script,
        ReqTab::Source,
    ];
    fn label(self) -> &'static str {
        match self {
            ReqTab::Params => "Params",
            ReqTab::Body => "Body",
            ReqTab::Headers => "Headers",
            ReqTab::Auth => "Auth",
            ReqTab::Vars => "Vars",
            ReqTab::Script => "Script",
            ReqTab::Source => "Source",
        }
    }
}
use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, Div, Entity, MouseButton, MouseUpEvent,
    Window, WindowBounds, WindowOptions,
};
use gpui_platform::application;

/// A pill/button used in the chrome (ghost style).
fn chip(label: &str) -> Div {
    div()
        .px_3()
        .py_1()
        .rounded_md()
        .bg(theme::surface0())
        .text_color(theme::text())
        .text_size(px(13.))
        .child(label.to_string())
}

fn icon_chip(label: &str) -> Div {
    div()
        .px_2()
        .py_1()
        .rounded_md()
        .text_color(theme::subtext())
        .text_size(px(12.))
        .child(label.to_string())
}

/// A sidebar request row: colored method badge + name, indented by depth.
fn req_row(method: &str, name: &str, active: bool, depth: usize) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .pr_2()
        .py_1()
        .pl(px(8. + depth as f32 * 14.))
        .rounded_md()
        .when(active, |d| d.bg(theme::surface0()))
        .child(
            div()
                .w(px(36.))
                .text_size(px(10.))
                .font_family("monospace")
                .text_color(theme::method_color(method))
                .child(short_method(method)),
        )
        .child(
            div()
                .text_size(px(13.))
                .text_color(if active {
                    theme::text()
                } else {
                    theme::subtext()
                })
                .child(name.to_string()),
        )
}

/// A sidebar folder row.
fn folder_row(name: &str, depth: usize) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .pr_2()
        .py_1()
        .pl(px(8. + depth as f32 * 14.))
        .child(
            div()
                .text_size(px(11.))
                .text_color(theme::muted())
                .child("\u{25BE}"),
        )
        .child(
            div()
                .text_size(px(13.))
                .text_color(theme::subtext())
                .child(name.to_string()),
        )
}

fn short_method(m: &str) -> String {
    let m = m.to_ascii_uppercase();
    match m.as_str() {
        "DELETE" => "DEL".into(),
        "OPTIONS" => "OPT".into(),
        "" => "?".into(),
        _ => m.chars().take(4).collect(),
    }
}

/// A tab label (request / response sub-tabs).
fn tab_chip(label: &str, active: bool) -> Div {
    div()
        .px_3()
        .py_1()
        .text_size(px(12.))
        .text_color(if active {
            theme::text()
        } else {
            theme::muted()
        })
        .when(active, |d| d.border_b_1().border_color(theme::accent()))
        .child(label.to_string())
}

struct BruApp {
    dir: PathBuf,
    collection: Option<CollectionTree>,
    /// Open request tabs + the active index.
    tabs: Vec<OpenTab>,
    active: Option<usize>,
    status: String,
}

/// One open request tab: all per-request state (was inline on BruApp).
struct OpenTab {
    path: PathBuf,
    method: String,
    req_tab: ReqTab,
    /// The parsed request (source of truth; edits are applied into it).
    file: BruFile,
    /// The shared editor for the active sub-tab's block.
    body_editor: Entity<CodeEditor>,
    /// Single-line editor for the request URL.
    url_input: Entity<CodeEditor>,
    edit_kind: EditKind,
    sending: bool,
    response: Option<String>,
}

impl OpenTab {
    /// Build a tab from a file path (None if unreadable/unparseable).
    fn load(path: PathBuf, cx: &mut Context<BruApp>) -> Option<Self> {
        let file = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| bru_lang::parse(&t).ok())?;
        let req = file.to_request();
        let method = req.as_ref().map(|r| r.method.clone()).unwrap_or_default();
        let url = req.as_ref().map(|r| r.url.clone()).unwrap_or_default();
        let body_editor = cx.new(|cx| CodeEditor::new(cx, ""));
        let url_input = cx.new(|cx| CodeEditor::single_line(cx, ""));
        url_input.update(cx, |ed, cx| ed.set_line(&url, cx));
        let mut tab = Self {
            path,
            method,
            req_tab: ReqTab::Body,
            file,
            body_editor,
            url_input,
            edit_kind: EditKind::None,
            sending: false,
            response: None,
        };
        tab.load_active_tab(cx);
        Some(tab)
    }

    /// Display title: `meta.name`, else the file stem.
    fn title(&self) -> String {
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

    /// Fold the editor + URL edits into the in-memory file.
    fn apply_edits(&mut self, cx: &mut Context<BruApp>) {
        let text = self.body_editor.read(cx).text().to_string();
        if let EditKind::Source = self.edit_kind {
            if let Ok(f) = bru_lang::parse(&text) {
                self.file = f;
            }
        }
        let url = self.url_input.read(cx).text().trim().to_string();
        edit::set_active_url(&mut self.file, &url);
        match &self.edit_kind {
            EditKind::Body(block) => {
                if let Some(b) = self.file.blocks.iter_mut().find(|b| &b.name == block) {
                    b.content = BlockContent::Text(text);
                }
            }
            EditKind::Dict(block) => edit::lines_to_dict(&mut self.file, block, &text),
            EditKind::None | EditKind::Source => {}
        }
    }

    /// Load the active sub-tab's block into the shared editor.
    fn load_active_tab(&mut self, cx: &mut Context<BruApp>) {
        let f = &self.file;
        let (text, lang, kind) = match self.req_tab {
            ReqTab::Body => match f.blocks.iter().find(|b| b.name.starts_with("body:")) {
                Some(b) => {
                    let t = match &b.content {
                        BlockContent::Text(s) => s.clone(),
                        _ => String::new(),
                    };
                    let lang = if b.name == "body:json" {
                        Lang::Json
                    } else {
                        Lang::Plain
                    };
                    (t, lang, EditKind::Body(b.name.clone()))
                }
                None => (String::new(), Lang::Plain, EditKind::None),
            },
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
            ReqTab::Vars => (
                edit::dict_to_lines(f, "vars:pre-request"),
                Lang::Plain,
                EditKind::Dict("vars:pre-request".into()),
            ),
            ReqTab::Auth => match f.blocks.iter().find(|b| b.name.starts_with("auth:")) {
                Some(b) => (
                    edit::dict_to_lines(f, &b.name),
                    Lang::Plain,
                    EditKind::Dict(b.name.clone()),
                ),
                None => (String::new(), Lang::Plain, EditKind::None),
            },
            ReqTab::Script => {
                let t = f
                    .block("script:pre-request")
                    .and_then(|b| match &b.content {
                        BlockContent::Text(s) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                (t, Lang::Plain, EditKind::Body("script:pre-request".into()))
            }
            ReqTab::Source => (bru_lang::serialize(f), Lang::Plain, EditKind::Source),
        };
        self.edit_kind = kind;
        self.body_editor
            .update(cx, |ed, cx| ed.set_text(&text, lang, cx));
    }

    /// Switch sub-tab: persist the current tab's edits, then load the new one.
    fn switch_tab(&mut self, t: ReqTab, cx: &mut Context<BruApp>) {
        self.apply_edits(cx);
        self.req_tab = t;
        self.load_active_tab(cx);
    }
}

/// Run a request to completion on a fresh tokio runtime (called on a worker
/// thread). Returns the formatted response or an error string.
fn run_blocking(file: BruFile, dir: PathBuf, script_dir: Option<PathBuf>) -> Result<String, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async move {
        let opts = bru_http::SendOptions::default();
        let client = bru_http::HttpClient::new(&opts).map_err(|e| e.to_string())?;
        let mut ctx = bru_engine::RunContext {
            vars: bru_engine::base_vars(&dir, None),
            client,
            send_options: opts,
            script_dir,
            ..Default::default()
        };
        let outcome = bru_engine::run_request(&file, &mut ctx).await;
        Ok(format_outcome(&outcome))
    })
}

/// Rows `(key, value, disabled)` of a dictionary block.
#[allow(dead_code)]
fn dict_rows(b: &bru_core::Block) -> Vec<(String, String, bool)> {
    match &b.content {
        BlockContent::Dict(entries) => entries
            .iter()
            .map(|e| {
                (
                    e.key.name().to_string(),
                    e.value.as_inline().to_string(),
                    e.disabled,
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn format_outcome(o: &bru_engine::RunOutcome) -> String {
    if let Some(e) = &o.error {
        return format!("Error: {e}");
    }
    match &o.response {
        Some(r) => format!(
            "{} {} \u{00B7} {} ms\n\n{}",
            r.status,
            r.status_text,
            r.duration_ms,
            String::from_utf8_lossy(&r.body)
        ),
        None => "(no response)".to_string(),
    }
}

impl BruApp {
    fn new(_cx: &mut Context<Self>, dir: PathBuf) -> Self {
        let collection = bru_lang::load_collection(&dir).ok();
        Self {
            dir,
            collection,
            tabs: Vec::new(),
            active: None,
            status: String::new(),
        }
    }

    fn active_tab(&self) -> Option<&OpenTab> {
        self.active.and_then(|i| self.tabs.get(i))
    }

    /// Remove tab `i`, fixing up the active index.
    fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.tabs.remove(i);
        self.active = if self.tabs.is_empty() {
            None
        } else {
            match self.active {
                Some(a) if a > i => Some(a - 1),
                Some(a) if a == i => Some(i.min(self.tabs.len() - 1)),
                other => other,
            }
        };
    }

    /// Send the selected request: run it on a worker thread (its own tokio
    /// runtime) and deliver the result back to the UI via a oneshot + cx.spawn.
    fn send(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.active else { return };
        self.tabs[i].apply_edits(cx);
        let path = self.tabs[i].path.clone();
        let file = self.tabs[i].file.clone();
        let dir = self.dir.clone();
        let script_dir = path.parent().map(Path::to_path_buf);
        self.tabs[i].sending = true;
        self.status = "Sending\u{2026}".into();
        let (tx, rx) = futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(run_blocking(file, dir, script_dir));
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                // Tab may have moved/closed while in flight — re-find by path.
                let (status, body) = match result {
                    Ok(Ok(b)) => ("Response received", Some(b)),
                    Ok(Err(e)) => ("Send failed", Some(format!("Error: {e}"))),
                    Err(_) => ("Send cancelled", None),
                };
                if let Some(tab) = this.tabs.iter_mut().find(|t| t.path == path) {
                    tab.sending = false;
                    if let Some(b) = body {
                        tab.response = Some(b);
                    }
                }
                this.status = status.into();
                cx.notify();
            });
        })
        .detach();
    }

    /// Apply the active tab's edits, then serialize it to disk.
    fn save(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.active else { return };
        self.tabs[i].apply_edits(cx);
        let path = self.tabs[i].path.clone();
        let ok = std::fs::write(&path, bru_lang::serialize(&self.tabs[i].file)).is_ok();
        self.status = if ok { "Saved".into() } else { "Save failed".into() };
    }

    /// Open a request as a tab, or focus it if already open.
    fn open_request(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if let Some(i) = self.tabs.iter().position(|t| t.path == path) {
            self.active = Some(i);
            return;
        }
        if let Some(tab) = OpenTab::load(path, cx) {
            self.tabs.push(tab);
            self.active = Some(self.tabs.len() - 1);
            self.status.clear();
        }
    }

    fn top_bar(&self) -> Div {
        let name = self
            .collection
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "No collection".into());
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .w_full()
            .px_3()
            .py_2()
            .bg(theme::mantle())
            .border_b_1()
            .border_color(theme::border1())
            .child(icon_chip("\u{2302}"))
            .child(chip("Open Collection"))
            .child(chip("New"))
            .child(div().text_color(theme::accent()).text_size(px(13.)).child(name))
            .child(
                div()
                    .text_color(theme::muted())
                    .text_size(px(12.))
                    .child("\u{2022} main"),
            )
            .child(div().flex_1())
            .child(div().text_color(theme::muted()).text_size(px(12.)).child("Env:"))
            .child(chip("Prod"))
            .child(icon_chip("Vault"))
            .child(icon_chip("Eye"))
            .child(icon_chip("Theme"))
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        let mut rows: Vec<Div> = Vec::new();
        if let Some(tree) = &self.collection {
            self.push_folder(&tree.root, 0, cx, &mut rows);
        } else {
            rows.push(
                div()
                    .p_2()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child("No collection loaded."),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_1()
            .w(px(280.))
            .h_full()
            .bg(theme::bg())
            .border_r_1()
            .border_color(theme::border1())
            .p_2()
            .child(
                div()
                    .px_1()
                    .py_1()
                    .text_color(theme::muted())
                    .text_size(px(12.))
                    .child(
                        self.collection
                            .as_ref()
                            .map(|c| c.name.to_uppercase())
                            .unwrap_or_default(),
                    ),
            )
            .children(rows)
    }

    fn push_folder(
        &self,
        folder: &Folder,
        depth: usize,
        cx: &mut Context<Self>,
        out: &mut Vec<Div>,
    ) {
        let mut subs: Vec<&Folder> = folder.folders.iter().collect();
        subs.sort_by_key(|f| f.name.to_lowercase());
        for sub in subs {
            out.push(folder_row(&sub.name, depth));
            self.push_folder(sub, depth + 1, cx, out);
        }
        let mut reqs: Vec<&bru_core::RequestItem> = folder.requests.iter().collect();
        reqs.sort_by(|a, b| {
            a.seq
                .unwrap_or(i64::MAX)
                .cmp(&b.seq.unwrap_or(i64::MAX))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        for req in reqs {
            let path = req.path.clone();
            let active = self.active_tab().map(|t| t.path.as_path()) == Some(path.as_path());
            let method = req.method.clone().unwrap_or_default();
            let row = req_row(&method, &req.name, active, depth).on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _ev: &MouseUpEvent, _win, cx| {
                    this.open_request(path.clone(), cx);
                    cx.notify();
                }),
            );
            out.push(row);
        }
    }

    fn url_bar(&self, tab: &OpenTab, cx: &mut Context<Self>) -> Div {
        let method = if tab.method.is_empty() {
            "GET".to_string()
        } else {
            tab.method.to_uppercase()
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .px_2()
            .py_2()
            .bg(theme::mantle())
            .border_b_1()
            .border_color(theme::border1())
            .child(
                div()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(theme::surface0())
                    .text_color(theme::method_color(&method))
                    .text_size(px(12.))
                    .font_family("monospace")
                    .child(method),
            )
            .child(
                div()
                    .flex_1()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(theme::input_bg())
                    .border_1()
                    .border_color(theme::border1())
                    .text_color(theme::text())
                    .text_size(px(13.))
                    .font_family("monospace")
                    .child(tab.url_input.clone()),
            )
            .child(icon_chip("</>"))
            .child(chip("Save").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseUpEvent, _w, cx| {
                    this.save(cx);
                    cx.notify();
                }),
            ))
            .child(
                div()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .bg(theme::accent())
                    .text_color(theme::bg())
                    .text_size(px(13.))
                    .child(if tab.sending {
                        "Sending\u{2026}".to_string()
                    } else {
                        "Send \u{2192}".to_string()
                    })
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _ev: &MouseUpEvent, _w, cx| {
                            this.send(cx);
                            cx.notify();
                        }),
                    ),
            )
    }

    fn response_pane(&self, tab: &OpenTab) -> Div {
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .px_2()
            .py_1()
            .bg(theme::surface0())
            .border_b_1()
            .border_color(theme::border2())
            .child(tab_chip("Response", true));
        let body = div()
            .id("resp")
            .overflow_y_scroll()
            .flex_1()
            .w_full()
            .p_3()
            .font_family("monospace")
            .text_size(px(13.))
            .line_height(px(19.))
            .text_color(theme::subtext())
            .child(
                tab.response
                    .clone()
                    .unwrap_or_else(|| "No response yet \u{2014} press Send.".to_string()),
            );
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .bg(theme::bg())
            .border_t_1()
            .border_color(theme::border2())
            .child(header)
            .child(body)
    }

    /// The clickable request sub-tab strip.
    fn req_subtabs(&self, tab: &OpenTab, cx: &mut Context<Self>) -> Div {
        let mut strip = div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .px_2()
            .bg(theme::surface0())
            .border_b_1()
            .border_color(theme::border2());
        for t in ReqTab::ALL {
            let active = tab.req_tab == t;
            strip = strip.child(tab_chip(t.label(), active).on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _ev: &MouseUpEvent, _w, cx| {
                    if let Some(i) = this.active {
                        this.tabs[i].switch_tab(t, cx);
                    }
                    cx.notify();
                }),
            ));
        }
        strip
    }

    /// The content for the active request sub-tab (the shared editor).
    fn req_content(&self, tab: &OpenTab) -> Div {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .bg(theme::bg())
            .child(
                div()
                    .id("body")
                    .overflow_y_scroll()
                    .flex_1()
                    .w_full()
                    .p_3()
                    .font_family("monospace")
                    .text_size(px(13.))
                    .line_height(px(19.))
                    .child(tab.body_editor.clone()),
            )
    }

    /// The strip of open request tabs (click to focus, × to close).
    fn tab_strip(&self, cx: &mut Context<Self>) -> Div {
        let mut strip = div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .px_2()
            .bg(theme::mantle())
            .border_b_1()
            .border_color(theme::border1());
        for (i, t) in self.tabs.iter().enumerate() {
            let active = self.active == Some(i);
            strip = strip.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .when(active, |d| {
                        d.bg(theme::surface0())
                            .border_b_1()
                            .border_color(theme::accent())
                    })
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(if active {
                                theme::text()
                            } else {
                                theme::muted()
                            })
                            .child(t.title())
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _ev: &MouseUpEvent, _w, cx| {
                                    this.active = Some(i);
                                    cx.notify();
                                }),
                            ),
                    )
                    .child(
                        div()
                            .px_1()
                            .text_size(px(12.))
                            .text_color(theme::muted())
                            .child("\u{00D7}")
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _ev: &MouseUpEvent, _w, cx| {
                                    this.close_tab(i);
                                    cx.notify();
                                }),
                            ),
                    ),
            );
        }
        strip
    }

    fn status_bar(&self) -> Div {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .w_full()
            .px_3()
            .py_1()
            .bg(theme::mantle())
            .border_t_1()
            .border_color(theme::border1())
            .child(
                div()
                    .px_2()
                    .text_color(theme::green())
                    .text_size(px(11.))
                    .child(self.status.clone()),
            )
            .child(div().flex_1())
            .child(icon_chip("Search"))
            .child(icon_chip("Cookies"))
            .child(icon_chip("Dev Tools"))
            .child(
                div()
                    .text_color(theme::muted())
                    .text_size(px(11.))
                    .child("v0.0.0"),
            )
    }
}

impl Render for BruApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let strip = self.tab_strip(cx);
        let content = if let Some(i) = self.active {
            let tab = &self.tabs[i];
            div()
                .flex()
                .flex_col()
                .flex_1()
                .h_full()
                .child(self.url_bar(tab, cx))
                .child(self.req_subtabs(tab, cx))
                .child(self.req_content(tab))
                .child(self.response_pane(tab))
        } else {
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .text_color(theme::muted())
                .child("Select a request from the sidebar.")
        };
        let center = div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .child(strip)
            .child(content);

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::bg())
            .text_color(theme::text())
            .child(self.top_bar())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .w_full()
                    .child(self.sidebar(cx))
                    .child(center),
            )
            .child(self.status_bar())
    }
}

fn main() {
    // Load the path arg, else the bundled sample collection.
    let dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("sample"));

    application().run(move |cx: &mut App| {
        editor::bind_keys(cx);
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| BruApp::new(cx, dir.clone())),
        )
        .unwrap();
        cx.activate(true);
    });
}
