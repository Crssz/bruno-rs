// Phase 3a: real collection data. Loads a Bruno collection (the bundled sample,
// or a path arg), renders a clickable recursive sidebar, and shows the opened
// request's real method/URL/body (JSON bodies tree-sitter-highlighted).
mod edit;
mod editor;
mod envfs;
mod highlight;
mod import;
mod theme;
mod vault;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use bru_core::{Block, BlockContent, BruFile, CollectionTree, Folder};
use editor::{CodeEditor, Lang};

/// What the single shared editor is currently editing (set per active sub-tab),
/// so its content can be written back to the right place in the BruFile.
enum EditKind {
    /// Nothing editable for this tab.
    None,
    /// A raw-text block (a `body:*` or `script:*` block): set its text verbatim.
    Body(String),
    /// A dictionary block (vars/auth/form-body) edited as `key: value` lines.
    Dict(String),
    /// A dictionary block (params/headers) edited as a structured row grid.
    Kv(String),
    /// GraphQL: body:graphql (query) in the main editor, body:graphql:vars in
    /// the secondary editor.
    GraphQl,
    /// The whole `.bru` source: reparse it on apply.
    Source,
}

/// One row of the structured params/headers grid.
struct KvRow {
    name: Entity<CodeEditor>,
    value: Entity<CodeEditor>,
    enabled: bool,
}

/// Request sub-tabs (Body is the editable editor; the rest show parsed data).
#[derive(Clone, Copy, PartialEq)]
enum ReqTab {
    Params,
    Body,
    Headers,
    Auth,
    Assert,
    Vars,
    PostVars,
    Script,
    PostScript,
    Tests,
    Docs,
    Source,
}

impl ReqTab {
    const ALL: [ReqTab; 12] = [
        ReqTab::Params,
        ReqTab::Body,
        ReqTab::Headers,
        ReqTab::Auth,
        ReqTab::Assert,
        ReqTab::Vars,
        ReqTab::PostVars,
        ReqTab::Script,
        ReqTab::PostScript,
        ReqTab::Tests,
        ReqTab::Docs,
        ReqTab::Source,
    ];
    fn label(self) -> &'static str {
        match self {
            ReqTab::Params => "Params",
            ReqTab::Body => "Body",
            ReqTab::Headers => "Headers",
            ReqTab::Auth => "Auth",
            ReqTab::Assert => "Assert",
            ReqTab::Vars => "Vars",
            ReqTab::PostVars => "Post Vars",
            ReqTab::Script => "Script",
            ReqTab::PostScript => "Post Script",
            ReqTab::Tests => "Tests",
            ReqTab::Docs => "Docs",
            ReqTab::Source => "Source",
        }
    }
}
use gpui::{
    actions, div, prelude::*, px, size, App, Bounds, Context, Div, Entity, FocusHandle, Focusable,
    KeyBinding, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, Window, WindowBounds,
    WindowOptions,
};
use gpui_platform::application;

// App-level keyboard actions (distinct namespace from the editor's actions).
actions!(bru_app, [SaveTab, SendReq, CloseOverlay, OpenPalette]);

/// Bind the app-level shortcuts once at startup (scoped to the BruApp root
/// context so they fire even while a CodeEditor descendant holds focus).
fn bind_app_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("ctrl-s", SaveTab, Some("BruApp")),
        KeyBinding::new("ctrl-enter", SendReq, Some("BruApp")),
        KeyBinding::new("ctrl-k", OpenPalette, Some("BruApp")),
        KeyBinding::new("escape", CloseOverlay, Some("BruApp")),
    ]);
}

/// Flatten every request in a folder tree into `(name, path)` (recursive).
fn flatten_requests(folder: &Folder, out: &mut Vec<(String, PathBuf)>) {
    for sub in &folder.folders {
        flatten_requests(sub, out);
    }
    for req in &folder.requests {
        out.push((req.name.clone(), req.path.clone()));
    }
}

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

/// A sidebar folder row (chevron reflects collapsed state).
fn folder_row(name: &str, depth: usize, collapsed: bool) -> Div {
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
                .child(if collapsed { "\u{25B8}" } else { "\u{25BE}" }),
        )
        .child(
            div()
                .text_size(px(13.))
                .text_color(theme::subtext())
                .child(name.to_string()),
        )
}

/// Whether a folder (or any descendant request/folder name) matches `query`.
fn folder_matches(folder: &Folder, query: &str) -> bool {
    folder.name.to_lowercase().contains(query)
        || folder
            .requests
            .iter()
            .any(|r| r.name.to_lowercase().contains(query))
        || folder.folders.iter().any(|f| folder_matches(f, query))
}

fn status_color(s: u16) -> gpui::Hsla {
    match s {
        200..=299 => theme::green(),
        300..=399 => theme::blue(),
        400..=499 => theme::orange(),
        _ => theme::red(),
    }
}

fn human_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// A classic `offset  hex bytes  ascii` hex dump of a byte buffer.
fn hex_dump(bytes: &[u8]) -> String {
    let mut out = String::new();
    for (i, chunk) in bytes.chunks(16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
        let ascii: String = chunk
            .iter()
            .map(|&b| {
                if (0x20..0x7f).contains(&b) {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        out.push_str(&format!(
            "{:08x}  {:<47}  {}\n",
            i * 16,
            hex.join(" "),
            ascii
        ));
    }
    out
}

/// Cycle to the next HTTP method (click-to-change in the URL bar).
fn next_method(m: &str) -> String {
    const METHODS: [&str; 7] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
    let cur = m.to_uppercase();
    let i = METHODS
        .iter()
        .position(|x| *x == cur)
        .map(|i| (i + 1) % METHODS.len())
        .unwrap_or(0);
    METHODS[i].to_string()
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
/// A clickable checkbox box (gpui has no checkbox primitive).
fn check_box(on: bool) -> Div {
    div()
        .w(px(14.))
        .h(px(14.))
        .rounded_sm()
        .border_1()
        .border_color(theme::border2())
        .flex()
        .items_center()
        .justify_center()
        .when(on, |d| {
            d.bg(theme::accent()).child(
                div()
                    .text_size(px(9.))
                    .text_color(theme::bg())
                    .child("\u{2713}"),
            )
        })
}

fn ghost_btn(label: &str) -> Div {
    div()
        .px_3()
        .py_1()
        .rounded_md()
        .text_size(px(13.))
        .text_color(theme::text())
        .bg(theme::surface0())
        .child(label.to_string())
}

fn solid_btn(label: &str) -> Div {
    div()
        .px_4()
        .py_1()
        .rounded_md()
        .text_size(px(13.))
        .bg(theme::accent())
        .text_color(theme::bg())
        .child(label.to_string())
}

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
    /// The environment-manager overlay, if open.
    env: Option<EnvEditor>,
    /// Collection-runner overlay state.
    runner_open: bool,
    runner_running: bool,
    runner_title: String,
    runner_results: Vec<RunResult>,
    /// Cookies observed from response Set-Cookie headers this session.
    cookies: Vec<CookieEntry>,
    cookies_open: bool,
    /// curl-import overlay (paste a curl command).
    curl_open: bool,
    curl_input: Entity<CodeEditor>,
    /// Preferences: request timeout (s) + TLS-insecure.
    pref_timeout: u64,
    pref_insecure: bool,
    prefs_open: bool,
    timeout_input: Entity<CodeEditor>,
    /// DevTools: console lines + network log.
    console: Vec<String>,
    network: Vec<NetEntry>,
    devtools_open: bool,
    devtools_net: bool,
    /// Secrets vault: Some when unlocked (name->value). Password held to re-save.
    vault: Option<HashMap<String, String>>,
    vault_pw: Option<String>,
    vault_open: bool,
    vault_input: Entity<CodeEditor>,
    vault_rows: Vec<(Entity<CodeEditor>, Entity<CodeEditor>)>,
    vault_error: Option<String>,
    /// Reveal-secrets eye: when true, secret values render in clear (vault + env).
    reveal_secrets: bool,
    /// Recently-opened collections + a Home-screen toggle.
    recent: Vec<String>,
    home: bool,
    /// Sidebar search filter.
    search: Entity<CodeEditor>,
    search_query: String,
    /// Right-click context menu over a sidebar entry (None = closed).
    ctx_menu: Option<CtxMenu>,
    /// Inline rename prompt (None = closed).
    rename: Option<RenameState>,
    /// Delete confirmation: (target, is_dir, display name).
    confirm_delete: Option<(PathBuf, bool, String)>,
    /// The active environment applied to sends/runs (None = no environment).
    selected_env: Option<String>,
    /// The active global (app-level) environment, overlaid under collection vars.
    selected_global_env: Option<String>,
    /// Env-picker dropdown anchored at this point (None = closed).
    env_menu: Option<Point<Pixels>>,
    /// Folder paths whose children are collapsed in the sidebar.
    collapsed: HashSet<PathBuf>,
    /// Sidebar copy/paste clipboard: (source path, is_dir).
    clipboard_item: Option<(PathBuf, bool)>,
    /// Tab paths with unsaved edits (live, via editor Changed events).
    dirty: HashSet<PathBuf>,
    /// Close-confirmation for a dirty tab (the tab index).
    confirm_close: Option<usize>,
    /// JSONPath response-filter input + its current query.
    resp_filter: Entity<CodeEditor>,
    resp_filter_query: String,
    /// Show the response body raw (no pretty-print / filter) when true.
    resp_raw: bool,
    /// Show the response body as a hex dump when true (overrides raw/pretty).
    resp_hex: bool,
    /// Read-only editor for the response body (selectable + copyable).
    resp_editor: Entity<CodeEditor>,
    /// Root focus handle, so app-level key actions dispatch.
    focus_handle: FocusHandle,
    /// Command palette (Ctrl+K jump-to-request): open flag + input + query.
    palette_open: bool,
    palette_input: Entity<CodeEditor>,
    palette_query: String,
}

/// A right-click menu over a sidebar entry, anchored at the click point.
struct CtxMenu {
    target: PathBuf,
    is_dir: bool,
    name: String,
    pos: Point<Pixels>,
}

/// Inline rename prompt for a sidebar entry.
struct RenameState {
    target: PathBuf,
    is_dir: bool,
    input: Entity<CodeEditor>,
}

/// One editable env row: two single-line editors + two flags.
struct EnvRowState {
    name: Entity<CodeEditor>,
    value: Entity<CodeEditor>,
    enabled: bool,
    secret: bool,
}

/// Working state for the environment-manager overlay.
struct EnvEditor {
    names: Vec<String>,
    selected: String,
    rename: Entity<CodeEditor>,
    rows: Vec<EnvRowState>,
    error: Option<String>,
    /// Collection scope (false) vs global/app-level scope (true).
    global: bool,
}

/// Response sub-tabs.
#[derive(Clone, Copy, PartialEq)]
enum RespTab {
    Response,
    Headers,
    Timeline,
    Tests,
}

impl RespTab {
    const ALL: [RespTab; 4] = [
        RespTab::Response,
        RespTab::Headers,
        RespTab::Timeline,
        RespTab::Tests,
    ];
    fn label(self) -> &'static str {
        match self {
            RespTab::Response => "Response",
            RespTab::Headers => "Headers",
            RespTab::Timeline => "Timeline",
            RespTab::Tests => "Tests",
        }
    }
}

/// One open request tab: all per-request state (was inline on BruApp).
struct OpenTab {
    path: PathBuf,
    method: String,
    req_tab: ReqTab,
    resp_tab: RespTab,
    /// The parsed request (source of truth; edits are applied into it).
    file: BruFile,
    /// The shared editor for the active sub-tab's block.
    body_editor: Entity<CodeEditor>,
    /// Single-line editor for the request URL.
    url_input: Entity<CodeEditor>,
    /// Secondary editor (GraphQL variables) shown alongside the body editor.
    body_vars_editor: Entity<CodeEditor>,
    edit_kind: EditKind,
    /// Row editors for the structured params/headers grid (when on those tabs).
    kv_rows: Vec<KvRow>,
    /// URL-derived path params (name, value editor) shown on the Params tab.
    path_rows: Vec<(String, Entity<CodeEditor>)>,
    sending: bool,
    /// The last response (full outcome, for the response sub-tabs).
    response: Option<bru_engine::RunOutcome>,
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
            path_rows: Vec::new(),
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
            EditKind::None | EditKind::Source => {}
        }
    }

    /// Load the active sub-tab's block into the shared editor.
    fn load_active_tab(&mut self, cx: &mut Context<BruApp>) {
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
            self.kv_rows = build_kv_rows(&self.file, block, cx);
            // The Params tab also surfaces URL-derived path params.
            self.path_rows = if block == "params:query" {
                edit::kv_block_rows(&self.file, "params:path")
                    .into_iter()
                    .map(|(name, value, _)| {
                        (name, cx.new(|cx| CodeEditor::single_line(cx, &value)))
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
            // Params/Headers/Assert are handled by the kv-grid early return above;
            // this arm only exists to keep the match exhaustive.
            ReqTab::Assert => (String::new(), Lang::Plain, EditKind::None),
            ReqTab::Vars => (
                edit::dict_to_lines(f, "vars:pre-request"),
                Lang::Plain,
                EditKind::Dict("vars:pre-request".into()),
            ),
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
            ReqTab::PostVars => (
                edit::dict_to_lines(f, "vars:post-response"),
                Lang::Plain,
                EditKind::Dict("vars:post-response".into()),
            ),
            ReqTab::Script => {
                let t = text_block(f, "script:pre-request");
                (t, Lang::Plain, EditKind::Body("script:pre-request".into()))
            }
            ReqTab::PostScript => {
                let t = text_block(f, "script:post-response");
                (
                    t,
                    Lang::Plain,
                    EditKind::Body("script:post-response".into()),
                )
            }
            ReqTab::Tests => (
                text_block(f, "tests"),
                Lang::Plain,
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
    fn switch_tab(&mut self, t: ReqTab, cx: &mut Context<BruApp>) {
        self.apply_edits(cx);
        self.req_tab = t;
        self.load_active_tab(cx);
    }
}

/// Run a request to completion on a fresh tokio runtime (called on a worker
/// thread). Returns the formatted response or an error string.
fn run_blocking(
    file: BruFile,
    dir: PathBuf,
    script_dir: Option<PathBuf>,
    opts: bru_http::SendOptions,
    global_vars: HashMap<String, String>,
    env: Option<String>,
) -> bru_engine::RunOutcome {
    let errout = |e: String| bru_engine::RunOutcome::errored("request".to_string(), e);
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => return errout(e.to_string()),
    };
    rt.block_on(async move {
        let client = match bru_http::HttpClient::new(&opts) {
            Ok(c) => c,
            Err(e) => return errout(e.to_string()),
        };
        // Vault secrets are the base layer; collection + selected-env vars override.
        let mut vars = global_vars;
        for (k, v) in bru_engine::base_vars(&dir, env.as_deref()) {
            vars.insert(k, v);
        }
        let mut ctx = bru_engine::RunContext {
            vars,
            client,
            send_options: opts,
            script_dir,
            ..Default::default()
        };
        bru_engine::run_request(&file, &mut ctx).await
    })
}

/// A step in a JSONPath-ish query.
enum PathStep {
    Key(String),
    Index(usize),
    Wild,
}

/// Tokenize a JSONPath-ish query (`$.a.b[0].c[*]`) into steps. Ported from iced.
fn json_path_tokens(q: &str) -> Vec<PathStep> {
    fn flush(buf: &mut String, steps: &mut Vec<PathStep>) {
        let s = buf.trim();
        if !s.is_empty() {
            steps.push(if s == "*" {
                PathStep::Wild
            } else {
                PathStep::Key(s.to_string())
            });
        }
        buf.clear();
    }
    let mut steps = Vec::new();
    let mut buf = String::new();
    let mut chars = q.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '.' => flush(&mut buf, &mut steps),
            '[' => {
                flush(&mut buf, &mut steps);
                let mut inner = String::new();
                for d in chars.by_ref() {
                    if d == ']' {
                        break;
                    }
                    inner.push(d);
                }
                let inner = inner.trim().trim_matches(|c| c == '"' || c == '\'');
                if inner == "*" {
                    steps.push(PathStep::Wild);
                } else if let Ok(i) = inner.parse::<usize>() {
                    steps.push(PathStep::Index(i));
                } else if !inner.is_empty() {
                    steps.push(PathStep::Key(inner.to_string()));
                }
            }
            _ => buf.push(c),
        }
    }
    flush(&mut buf, &mut steps);
    steps
}

/// Resolve a JSONPath-ish query against a value (supports `.key`, `[i]`, `[*]`).
fn json_path(v: &serde_json::Value, query: &str) -> Option<serde_json::Value> {
    use serde_json::Value as J;
    let q = query.trim();
    let q = q.strip_prefix('$').unwrap_or(q);
    let mut cur: Vec<J> = vec![v.clone()];
    for step in json_path_tokens(q) {
        let mut next = Vec::new();
        for node in &cur {
            match (&step, node) {
                (PathStep::Key(k), J::Object(m)) => {
                    if let Some(child) = m.get(k) {
                        next.push(child.clone());
                    }
                }
                (PathStep::Index(i), J::Array(a)) => {
                    if let Some(child) = a.get(*i) {
                        next.push(child.clone());
                    }
                }
                (PathStep::Wild, J::Array(a)) => next.extend(a.iter().cloned()),
                (PathStep::Wild, J::Object(m)) => next.extend(m.values().cloned()),
                _ => {}
            }
        }
        cur = next;
        if cur.is_empty() {
            return None;
        }
    }
    match cur.len() {
        0 => None,
        1 => cur.into_iter().next(),
        _ => Some(J::Array(cur)),
    }
}

/// Set a `BlockContent::Text` block (create if absent + non-empty).
fn set_text_block(file: &mut BruFile, name: &str, content: String) {
    if let Some(b) = file.blocks.iter_mut().find(|b| b.name == name) {
        b.content = BlockContent::Text(content);
    } else if !content.trim().is_empty() {
        file.blocks.push(Block {
            name: name.to_string(),
            content: BlockContent::Text(content),
        });
    }
}

/// The text content of a `BlockContent::Text` block, or empty.
fn text_block(f: &BruFile, name: &str) -> String {
    f.block(name)
        .and_then(|b| match &b.content {
            BlockContent::Text(s) => Some(s.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// Body modes the gpui editor can edit (text-based + url-encoded form). The
/// structured types (multipartForm/graphql/file) need dedicated editors and are
/// intentionally omitted from the cycle for now.
const BODY_MODES: &[&str] = &[
    "none",
    "json",
    "text",
    "xml",
    "sparql",
    "formUrlEncoded",
    "multipartForm",
    "graphql",
];
const AUTH_MODES: &[&str] = &[
    "none", "inherit", "basic", "bearer", "apikey", "oauth2", "digest", "awsv4",
];

/// The `body:<block>` name for a body mode, or None for `none`/unknown.
fn body_block_name(mode: &str) -> Option<&'static str> {
    Some(match mode {
        "json" => "body:json",
        "text" => "body:text",
        "xml" => "body:xml",
        "sparql" => "body:sparql",
        "formUrlEncoded" => "body:form-urlencoded",
        "multipartForm" => "body:multipart-form",
        "graphql" => "body:graphql",
        _ => return None,
    })
}

/// The `auth:<block>` name for an auth mode, or None for none/inherit/unknown.
fn auth_block_name(mode: &str) -> Option<&'static str> {
    Some(match mode {
        "basic" => "auth:basic",
        "bearer" => "auth:bearer",
        "apikey" => "auth:apikey",
        "oauth2" => "auth:oauth2",
        "digest" => "auth:digest",
        "awsv4" => "auth:awsv4",
        _ => return None,
    })
}

/// Next mode in a cycle list after `current` (wraps to the start).
fn cycle_next(list: &[&str], current: &str) -> String {
    let i = list.iter().position(|m| *m == current).unwrap_or(0);
    list[(i + 1) % list.len()].to_string()
}

/// Build structured grid rows (name/value single-line editors + enabled) from a
/// Dict block, for the params/headers tabs.
fn build_kv_rows(file: &BruFile, block: &str, cx: &mut Context<BruApp>) -> Vec<KvRow> {
    edit::kv_block_rows(file, block)
        .into_iter()
        .map(|(k, v, enabled)| KvRow {
            name: cx.new(|cx| CodeEditor::single_line(cx, &k)),
            value: cx.new(|cx| CodeEditor::single_line(cx, &v)),
            enabled,
        })
        .collect()
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

/// One row in the devtools Network log.
#[derive(Clone)]
struct NetEntry {
    method: String,
    url: String,
    status: u16,
    ms: u128,
    size: usize,
    ok: bool,
}

/// One stored cookie, keyed by (domain, path, name).
#[derive(Clone)]
struct CookieEntry {
    domain: String,
    path: String,
    name: String,
    value: String,
}

/// `~/.bruno-rs/gpui-recent.json` â€” the recent-collections list.
fn recent_path() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let dir = PathBuf::from(home).join(".bruno-rs");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("gpui-recent.json"))
}

fn load_recent() -> Vec<String> {
    recent_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn prefs_path() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let dir = PathBuf::from(home).join(".bruno-rs");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("gpui-prefs.json"))
}

/// Root dir for global (app-level, cross-collection) environments. Holds an
/// `environments/` subdir just like a collection.
fn globals_root() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".bruno-rs").join("globals")
}

/// Load persisted prefs as `(timeout_secs, insecure, light)`.
fn load_prefs() -> (u64, bool, bool) {
    let v = prefs_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    match v {
        Some(v) => (
            v.get("timeout").and_then(|x| x.as_u64()).unwrap_or(30),
            v.get("insecure").and_then(|x| x.as_bool()).unwrap_or(false),
            v.get("light").and_then(|x| x.as_bool()).unwrap_or(false),
        ),
        None => (30, false, false),
    }
}

fn save_prefs(timeout: u64, insecure: bool, light: bool) {
    if let Some(p) = prefs_path() {
        let json = serde_json::json!({ "timeout": timeout, "insecure": insecure, "light": light });
        let _ = std::fs::write(p, json.to_string());
    }
}

fn save_recent(recent: &[String]) {
    if let Some(p) = recent_path() {
        if let Ok(json) = serde_json::to_string(recent) {
            let _ = std::fs::write(p, json);
        }
    }
}

/// Move `s` to the front of the recent list (deduped, capped at 10).
fn bump_recent(recent: &mut Vec<String>, s: String) {
    recent.retain(|r| r != &s);
    recent.insert(0, s);
    recent.truncate(10);
}

/// Host of a URL (no scheme/path/userinfo/port).
fn host_of(u: &str) -> String {
    let s = u.split("://").nth(1).unwrap_or(u);
    let s = s.split('/').next().unwrap_or(s);
    let s = s.rsplit('@').next().unwrap_or(s);
    s.split(':').next().unwrap_or(s).to_string()
}

fn parse_set_cookie(header: &str, host: &str) -> Option<CookieEntry> {
    let mut parts = header.split(';');
    let (name, value) = parts.next()?.trim().split_once('=')?;
    let mut domain = host.to_string();
    let mut path = "/".to_string();
    for attr in parts {
        if let Some((k, v)) = attr.trim().split_once('=') {
            match k.trim().to_ascii_lowercase().as_str() {
                "domain" => domain = v.trim().trim_start_matches('.').to_string(),
                "path" => path = v.trim().to_string(),
                _ => {}
            }
        }
    }
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some(CookieEntry {
        domain,
        path,
        name: name.to_string(),
        value: value.trim().to_string(),
    })
}

fn upsert_cookie(jar: &mut Vec<CookieEntry>, c: CookieEntry) {
    if let Some(e) = jar
        .iter_mut()
        .find(|e| e.domain == c.domain && e.path == c.path && e.name == c.name)
    {
        e.value = c.value;
    } else {
        jar.push(c);
    }
}

/// One request's outcome in a runner batch.
#[derive(Clone)]
struct RunResult {
    name: String,
    passed: bool,
    status: u16,
    ms: u128,
    error: Option<String>,
}

/// Find the sub-folder whose path is `dir`.
fn find_folder<'a>(folder: &'a Folder, dir: &Path) -> Option<&'a Folder> {
    for sub in &folder.folders {
        if sub.path == dir {
            return Some(sub);
        }
        if let Some(f) = find_folder(sub, dir) {
            return Some(f);
        }
    }
    None
}

/// Collect every request path under `folder` (recursive).
fn collect_folder_requests(folder: &Folder, out: &mut Vec<PathBuf>) {
    for sub in &folder.folders {
        collect_folder_requests(sub, out);
    }
    for req in &folder.requests {
        out.push(req.path.clone());
    }
}

/// Recursively copy a directory tree (used by the sidebar "Clone" action).
fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
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
fn set_seq_in_file(path: &Path, seq: i64) {
    if let Ok(text) = std::fs::read_to_string(path) {
        if let Ok(mut f) = bru_lang::parse(&text) {
            edit::set_meta_seq(&mut f, seq);
            let _ = std::fs::write(path, bru_lang::serialize(&f));
        }
    }
}

/// Reveal a path in the OS file manager (Explorer on Windows, Finder on macOS).
fn reveal_in_file_manager(path: &Path) {
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

/// Build a `curl` command for the request â€” method, URL, headers, and body â€”
/// for the "</>" generate-code affordance (copied to the clipboard).
fn to_curl(tab: &OpenTab, cx: &App) -> String {
    let method = if tab.method.is_empty() {
        "GET".to_string()
    } else {
        tab.method.to_uppercase()
    };
    let url = tab.url_input.read(cx).text().trim().to_string();
    let mut out = format!("curl -X {method} '{url}'");
    for line in edit::dict_to_lines(&tab.file, "headers").lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('~') {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            out.push_str(&format!(" \\\n  -H '{}: {}'", k.trim(), v.trim()));
        }
    }
    if let Some(b) = tab.file.blocks.iter().find(|b| b.name.starts_with("body:")) {
        if let BlockContent::Text(body) = &b.content {
            if !body.trim().is_empty() {
                let esc = body.replace('\'', "'\\''");
                out.push_str(&format!(" \\\n  --data '{esc}'"));
            }
        }
    }
    out
}

/// Run request files sequentially through one shared RunContext (Bruno's folder
/// runner) on a fresh tokio runtime. Worker thread.
fn run_folder_blocking(
    files: Vec<PathBuf>,
    vars_base: PathBuf,
    opts: bru_http::SendOptions,
    global_vars: HashMap<String, String>,
    env: Option<String>,
) -> Vec<RunResult> {
    let err_row = |name: &str, e: String| RunResult {
        name: name.to_string(),
        passed: false,
        status: 0,
        ms: 0,
        error: Some(e),
    };
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => return vec![err_row("runtime", e.to_string())],
    };
    rt.block_on(async move {
        let client = match bru_http::HttpClient::new(&opts) {
            Ok(c) => c,
            Err(e) => return vec![err_row("client", e.to_string())],
        };
        let mut vars = global_vars;
        for (k, v) in bru_engine::base_vars(&vars_base, env.as_deref()) {
            vars.insert(k, v);
        }
        let mut ctx = bru_engine::RunContext {
            vars,
            client,
            send_options: opts,
            ..Default::default()
        };
        let mut results = Vec::new();
        for path in files {
            ctx.script_dir = path.parent().map(Path::to_path_buf);
            let fname = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("request")
                .to_string();
            let file = match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|t| bru_lang::parse(&t).map_err(|e| e.to_string()))
            {
                Ok(f) => f,
                Err(e) => {
                    results.push(err_row(&fname, e));
                    continue;
                }
            };
            if file.to_request().is_none() {
                continue; // skip non-HTTP .bru
            }
            let outcome = bru_engine::run_request(&file, &mut ctx).await;
            let status = outcome.response.as_ref().map(|r| r.status).unwrap_or(0);
            let ms = outcome
                .response
                .as_ref()
                .map(|r| r.duration_ms)
                .unwrap_or(0);
            let passed = outcome.error.is_none()
                && outcome.assertions.iter().all(|a| a.passed)
                && outcome.tests.iter().all(|t| t.passed);
            results.push(RunResult {
                name: outcome.name.clone(),
                passed,
                status,
                ms,
                error: outcome.error.clone(),
            });
        }
        results
    })
}

impl BruApp {
    fn new(cx: &mut Context<Self>, dir: PathBuf) -> Self {
        let collection = bru_lang::load_collection(&dir).ok();
        // Apply persisted preferences (timeout / insecure-TLS / theme).
        let (pref_timeout, pref_insecure, light) = load_prefs();
        theme::set_dark(!light);
        let curl_input = cx.new(|cx| CodeEditor::new(cx, ""));
        let timeout_input = cx.new(|cx| CodeEditor::single_line(cx, &pref_timeout.to_string()));
        let search = cx.new(|cx| CodeEditor::single_line(cx, ""));
        // Live-filter the sidebar as the search box changes.
        cx.subscribe(&search, |this, ed, _ev: &editor::Changed, cx| {
            this.search_query = ed.read(cx).text().to_lowercase();
            cx.notify();
        })
        .detach();
        let resp_filter = cx.new(|cx| CodeEditor::single_line(cx, ""));
        // Live JSONPath filter of the response body.
        cx.subscribe(&resp_filter, |this, ed, _ev: &editor::Changed, cx| {
            this.resp_filter_query = ed.read(cx).text().trim().to_string();
            cx.notify();
        })
        .detach();
        let palette_input = cx.new(|cx| CodeEditor::single_line(cx, ""));
        cx.subscribe(&palette_input, |this, ed, _ev: &editor::Changed, cx| {
            this.palette_query = ed.read(cx).text().to_string();
            cx.notify();
        })
        .detach();
        Self {
            dir,
            collection,
            tabs: Vec::new(),
            active: None,
            status: String::new(),
            env: None,
            runner_open: false,
            runner_running: false,
            runner_title: String::new(),
            runner_results: Vec::new(),
            cookies: Vec::new(),
            cookies_open: false,
            curl_open: false,
            curl_input,
            pref_timeout,
            pref_insecure,
            prefs_open: false,
            timeout_input,
            console: Vec::new(),
            network: Vec::new(),
            devtools_open: false,
            devtools_net: false,
            vault: None,
            vault_pw: None,
            vault_open: false,
            vault_input: cx.new(|cx| CodeEditor::single_line(cx, "")),
            vault_rows: Vec::new(),
            vault_error: None,
            reveal_secrets: false,
            recent: load_recent(),
            home: false,
            search,
            search_query: String::new(),
            ctx_menu: None,
            rename: None,
            confirm_delete: None,
            selected_env: None,
            selected_global_env: None,
            env_menu: None,
            collapsed: HashSet::new(),
            clipboard_item: None,
            dirty: HashSet::new(),
            confirm_close: None,
            resp_filter,
            resp_filter_query: String::new(),
            resp_raw: false,
            resp_hex: false,
            resp_editor: cx.new(|cx| CodeEditor::read_only(cx, "")),
            focus_handle: cx.focus_handle(),
            palette_open: false,
            palette_input,
            palette_query: String::new(),
        }
    }

    fn on_save_action(&mut self, _: &SaveTab, _w: &mut Window, cx: &mut Context<Self>) {
        self.save(cx);
        cx.notify();
    }
    fn on_send_action(&mut self, _: &SendReq, _w: &mut Window, cx: &mut Context<Self>) {
        self.send(cx);
        cx.notify();
    }
    fn on_escape_action(&mut self, _: &CloseOverlay, _w: &mut Window, cx: &mut Context<Self>) {
        self.close_topmost_overlay(cx);
    }
    fn on_palette_action(&mut self, _: &OpenPalette, window: &mut Window, cx: &mut Context<Self>) {
        self.palette_open = true;
        let h = self.palette_input.read(cx).focus_handle(cx);
        window.focus(&h, cx);
        cx.notify();
    }

    /// The Ctrl+K jump-to-request command palette.
    fn palette_overlay(&self, cx: &mut Context<Self>) -> Div {
        let q = self.palette_query.to_lowercase();
        let mut items: Vec<(String, PathBuf)> = Vec::new();
        if let Some(tree) = &self.collection {
            flatten_requests(&tree.root, &mut items);
        }
        let filtered: Vec<(String, PathBuf)> = items
            .into_iter()
            .filter(|(n, p)| {
                q.is_empty()
                    || n.to_lowercase().contains(&q)
                    || p.to_string_lossy().to_lowercase().contains(&q)
            })
            .take(60)
            .collect();
        let mut list = div().flex().flex_col().gap_1();
        for (name, path) in filtered {
            let hint = path
                .parent()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let p = path.clone();
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .hover(|s| s.bg(theme::surface0()))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(theme::text())
                            .child(name),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .child(hint),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                            this.open_request(p.clone(), cx);
                            this.palette_open = false;
                            cx.notify();
                        }),
                    ),
            );
        }
        let card = div()
            .w(px(520.))
            .max_h(px(440.))
            .p_3()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .occlude()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .w_full()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(theme::input_bg())
                    .border_1()
                    .border_color(theme::border1())
                    .text_size(px(13.))
                    .child(self.palette_input.clone()),
            )
            .child(
                div()
                    .id("palette-list")
                    .overflow_y_scroll()
                    .flex_1()
                    .child(list),
            );
        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x000000aa))
            .flex()
            .flex_col()
            .items_center()
            .pt(px(80.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseDownEvent, _w, cx| {
                    this.palette_open = false;
                    cx.notify();
                }),
            )
            .child(card)
    }

    /// Esc closes the topmost open overlay/menu (in priority order).
    fn close_topmost_overlay(&mut self, cx: &mut Context<Self>) {
        if self.palette_open {
            self.palette_open = false;
        } else if self.confirm_close.take().is_some()
            || self.confirm_delete.take().is_some()
            || self.rename.take().is_some()
            || self.ctx_menu.take().is_some()
            || self.env_menu.take().is_some()
        {
            // one of the lightweight popovers was closed
        } else if self.curl_open {
            self.curl_open = false;
        } else if self.vault_open {
            self.vault_open = false;
        } else if self.prefs_open {
            self.prefs_open = false;
        } else if self.cookies_open {
            self.cookies_open = false;
        } else if self.devtools_open {
            self.devtools_open = false;
        } else if self.runner_open {
            self.runner_open = false;
        } else if self.env.is_some() {
            self.env = None;
        } else {
            return;
        }
        cx.notify();
    }

    fn toggle_folder(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&path) {
            self.collapsed.insert(path);
        }
        cx.notify();
    }

    /// Create a new sub-folder under `dir` and reload the tree.
    fn new_folder_in(&mut self, dir: &Path, cx: &mut Context<Self>) {
        let mut n = 1;
        let mut path = dir.join("New Folder");
        while path.exists() {
            n += 1;
            path = dir.join(format!("New Folder {n}"));
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("New Folder");
        if std::fs::create_dir_all(&path).is_ok() {
            // Bruno folder metadata lives in folder.bru.
            let meta = format!("meta {{\n  name: {name}\n  seq: 1\n}}\n");
            let _ = std::fs::write(path.join("folder.bru"), meta);
            self.reload_collection(cx);
        }
    }

    /// Scaffold a new Bruno collection under `parent` (bruno.json + an empty
    /// environments/ dir) and open it.
    fn create_collection(&mut self, parent: &Path, cx: &mut Context<Self>) {
        let mut dir = parent.join("New Collection");
        let mut n = 1;
        while dir.exists() {
            n += 1;
            dir = parent.join(format!("New Collection {n}"));
        }
        let name = dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("New Collection");
        if std::fs::create_dir_all(&dir).is_ok() {
            let bruno =
                format!("{{\n  \"version\": \"1\",\n  \"name\": \"{name}\",\n  \"type\": \"collection\"\n}}\n");
            let _ = std::fs::write(dir.join("bruno.json"), bruno);
            let _ = std::fs::create_dir_all(dir.join("environments"));
            self.load_collection(dir, cx);
        }
    }

    // â”€â”€ active environment selector â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    fn open_env_menu(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        self.env_menu = Some(pos);
        cx.notify();
    }
    fn close_env_menu(&mut self, cx: &mut Context<Self>) {
        if self.env_menu.take().is_some() {
            cx.notify();
        }
    }
    fn select_env(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        self.selected_env = name;
        self.env_menu = None;
        cx.notify();
    }
    fn select_global_env(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        self.selected_global_env = name;
        self.env_menu = None;
        cx.notify();
    }

    /// The active-environment dropdown (anchored under the toolbar chip).
    fn env_menu_overlay(&self, cx: &mut Context<Self>) -> Div {
        let Some(pos) = self.env_menu else {
            return div();
        };
        let item = |label: String, active: bool| {
            div()
                .px_3()
                .py_1()
                .text_size(px(12.))
                .text_color(if active {
                    theme::accent()
                } else {
                    theme::text()
                })
                .hover(|s| s.bg(theme::surface0()))
                .child(label)
        };
        let mut card = div()
            .absolute()
            .left(pos.x)
            .top(pos.y)
            .occlude()
            .flex()
            .flex_col()
            .py_1()
            .w(px(200.))
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .child(
                item("No Environment".into(), self.selected_env.is_none()).on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.select_env(None, cx)),
                ),
            );
        for name in envfs::scan_envs(&self.dir) {
            let active = self.selected_env.as_deref() == Some(name.as_str());
            let n = name.clone();
            card = card.child(item(name, active).on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                    this.select_env(Some(n.clone()), cx)
                }),
            ));
        }
        // Global (app-level) environments overlay collection vars beneath them.
        let globals = envfs::scan_envs(&globals_root());
        if !globals.is_empty() {
            card = card.child(
                div()
                    .px_3()
                    .py_1()
                    .text_size(px(10.))
                    .text_color(theme::muted())
                    .child("GLOBAL"),
            );
            card = card.child(
                item("No Global Env".into(), self.selected_global_env.is_none()).on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.select_global_env(None, cx)),
                ),
            );
            for name in globals {
                let active = self.selected_global_env.as_deref() == Some(name.as_str());
                let n = name.clone();
                card = card.child(item(name, active).on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                        this.select_global_env(Some(n.clone()), cx)
                    }),
                ));
            }
        }
        div()
            .absolute()
            .inset_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseDownEvent, _w, cx| this.close_env_menu(cx)),
            )
            .child(card)
    }

    fn go_home(&mut self, cx: &mut Context<Self>) {
        self.home = !self.home;
        cx.notify();
    }

    /// Create a new request file in the collection root and open it.
    fn new_request(&mut self, cx: &mut Context<Self>) {
        let dir = self.dir.clone();
        self.new_request_in(&dir, cx);
    }

    /// Create a new request file in `dir` (a folder) and open it.
    fn new_request_in(&mut self, dir: &Path, cx: &mut Context<Self>) {
        let mut n = 1;
        let mut path = dir.join("New Request.bru");
        while path.exists() {
            n += 1;
            path = dir.join(format!("New Request {n}.bru"));
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("New Request");
        let body = format!(
            "meta {{\n  name: {stem}\n  type: http\n  seq: 1\n}}\n\nget {{\n  url: \n  body: none\n  auth: none\n}}\n"
        );
        if std::fs::write(&path, body).is_ok() {
            self.reload_collection(cx);
            self.open_request(path, cx);
        }
        cx.notify();
    }

    /// Re-read the on-disk collection tree into the sidebar.
    fn reload_collection(&mut self, cx: &mut Context<Self>) {
        if let Ok(tree) = bru_lang::load_collection(&self.dir) {
            self.collection = Some(tree);
        }
        cx.notify();
    }

    // â”€â”€ sidebar context menu â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    fn open_ctx_menu(
        &mut self,
        target: PathBuf,
        is_dir: bool,
        name: String,
        pos: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.ctx_menu = Some(CtxMenu {
            target,
            is_dir,
            name,
            pos,
        });
        cx.notify();
    }

    fn close_ctx_menu(&mut self, cx: &mut Context<Self>) {
        if self.ctx_menu.take().is_some() {
            cx.notify();
        }
    }

    /// Close every open tab whose path is `path` or sits under it (for a folder).
    fn close_tabs_under(&mut self, path: &Path) {
        self.tabs.retain(|t| !t.path.starts_with(path));
        if self.tabs.is_empty() {
            self.active = None;
        } else {
            let i = self.active.unwrap_or(0).min(self.tabs.len() - 1);
            self.active = Some(i);
        }
    }

    /// Duplicate the menu's target (a `.bru` file or a folder) alongside itself.
    fn ctx_duplicate(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.ctx_menu.take() else {
            return;
        };
        let Some(parent) = menu.target.parent() else {
            return;
        };
        if menu.is_dir {
            let mut dest = parent.join(format!("{} copy", menu.name));
            let mut n = 1;
            while dest.exists() {
                n += 1;
                dest = parent.join(format!("{} copy {n}", menu.name));
            }
            let _ = copy_dir_recursive(&menu.target, &dest);
        } else {
            let mut dest = parent.join(format!("{} copy.bru", menu.name));
            let mut n = 1;
            while dest.exists() {
                n += 1;
                dest = parent.join(format!("{} copy {n}.bru", menu.name));
            }
            let _ = std::fs::copy(&menu.target, &dest);
        }
        self.reload_collection(cx);
    }

    /// Run the menu's target: a whole folder, or open + nothing for a request
    /// (the request is opened so the user can Send it).
    fn ctx_run(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.ctx_menu.take() else {
            return;
        };
        if menu.is_dir {
            self.run_folder(menu.target, cx);
        } else {
            self.open_request(menu.target, cx);
        }
        cx.notify();
    }

    /// Run a single request from its row: open it, then send.
    fn ctx_run_request(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.ctx_menu.take() else {
            return;
        };
        self.open_request(menu.target, cx);
        self.send(cx);
        cx.notify();
    }

    /// Copy the menu target onto the sidebar clipboard for a later Paste.
    fn ctx_copy(&mut self, cx: &mut Context<Self>) {
        if let Some(menu) = self.ctx_menu.take() {
            self.clipboard_item = Some((menu.target, menu.is_dir));
            self.status = "Copied to sidebar clipboard".into();
            cx.notify();
        }
    }

    /// Paste the clipboard item into the menu's folder (dedup name).
    fn ctx_paste(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.ctx_menu.take() else {
            return;
        };
        let dest_dir = if menu.is_dir {
            menu.target.clone()
        } else {
            menu.target
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or(menu.target)
        };
        let Some((src, is_dir)) = self.clipboard_item.clone() else {
            return;
        };
        let stem = src
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Item")
            .to_string();
        if is_dir {
            let mut dest = dest_dir.join(&stem);
            let mut n = 1;
            while dest.exists() {
                n += 1;
                dest = dest_dir.join(format!("{stem} {n}"));
            }
            let _ = copy_dir_recursive(&src, &dest);
        } else {
            let mut dest = dest_dir.join(format!("{stem}.bru"));
            let mut n = 1;
            while dest.exists() {
                n += 1;
                dest = dest_dir.join(format!("{stem} {n}.bru"));
            }
            let _ = std::fs::copy(&src, &dest);
        }
        self.reload_collection(cx);
    }

    /// Reveal the menu target in the OS file manager.
    fn ctx_reveal(&mut self, cx: &mut Context<Self>) {
        if let Some(menu) = self.ctx_menu.take() {
            reveal_in_file_manager(&menu.target);
            cx.notify();
        }
    }

    /// Open a folder's `folder.bru` as a tab (folder-level headers/auth/vars/
    /// scripts), creating a minimal one if absent.
    fn ctx_folder_settings(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.ctx_menu.take() else {
            return;
        };
        let bru = menu.target.join("folder.bru");
        if !bru.exists() {
            let name = menu.name;
            let _ = std::fs::write(&bru, format!("meta {{\n  name: {name}\n  seq: 1\n}}\n"));
            self.reload_collection(cx);
        }
        self.open_request(bru, cx);
        cx.notify();
    }

    /// Open the collection's `collection.bru` settings as a tab.
    fn open_collection_settings(&mut self, cx: &mut Context<Self>) {
        let bru = self.dir.join("collection.bru");
        if !bru.exists() {
            let _ = std::fs::write(&bru, "meta {\n  name: Collection\n}\n");
            self.reload_collection(cx);
        }
        self.open_request(bru, cx);
        cx.notify();
    }

    /// Move the menu's request up/down among its siblings (rewrites meta.seq).
    fn ctx_move(&mut self, delta: i64, cx: &mut Context<Self>) {
        let Some(menu) = self.ctx_menu.take() else {
            return;
        };
        let path = menu.target;
        let Some(dir) = path.parent() else { return };
        // Sibling requests in display order, via the loaded tree.
        let mut reqs: Vec<(PathBuf, i64, String)> = {
            let Some(tree) = &self.collection else { return };
            let folder = if dir == self.dir {
                Some(&tree.root)
            } else {
                find_folder(&tree.root, dir)
            };
            let Some(folder) = folder else { return };
            folder
                .requests
                .iter()
                .map(|r| (r.path.clone(), r.seq.unwrap_or(i64::MAX), r.name.clone()))
                .collect()
        };
        reqs.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| a.2.to_lowercase().cmp(&b.2.to_lowercase()))
        });
        let Some(idx) = reqs.iter().position(|(p, _, _)| *p == path) else {
            return;
        };
        let target = (idx as i64 + delta).clamp(0, reqs.len() as i64 - 1) as usize;
        if target == idx {
            return;
        }
        let item = reqs.remove(idx);
        reqs.insert(target, item);
        for (i, (p, _, _)) in reqs.iter().enumerate() {
            set_seq_in_file(p, (i + 1) as i64);
        }
        self.reload_collection(cx);
    }

    fn start_rename(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.ctx_menu.take() else {
            return;
        };
        let input = cx.new(|cx| CodeEditor::single_line(cx, &menu.name));
        self.rename = Some(RenameState {
            target: menu.target,
            is_dir: menu.is_dir,
            input,
        });
        cx.notify();
    }

    fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.rename = None;
        cx.notify();
    }

    fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.rename.take() else {
            return;
        };
        let new_name = state.input.read(cx).text().trim().to_string();
        let Some(parent) = state.target.parent().map(Path::to_path_buf) else {
            return;
        };
        if new_name.is_empty() {
            return;
        }
        if state.is_dir {
            let dest = parent.join(&new_name);
            if dest != state.target && std::fs::rename(&state.target, &dest).is_ok() {
                self.close_tabs_under(&state.target);
            }
        } else {
            // Rewrite meta.name so the tree label follows, then move the file.
            let dest = parent.join(format!("{new_name}.bru"));
            if let Ok(text) = std::fs::read_to_string(&state.target) {
                if let Ok(mut file) = bru_lang::parse(&text) {
                    edit::set_meta_name(&mut file, &new_name);
                    let _ = std::fs::write(&state.target, bru_lang::serialize(&file));
                }
            }
            if dest != state.target && std::fs::rename(&state.target, &dest).is_ok() {
                // Re-point any open tab at the new path.
                for t in &mut self.tabs {
                    if t.path == state.target {
                        t.path = dest.clone();
                    }
                }
            }
        }
        self.reload_collection(cx);
    }

    fn start_delete(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.ctx_menu.take() else {
            return;
        };
        self.confirm_delete = Some((menu.target, menu.is_dir, menu.name));
        cx.notify();
    }

    fn cancel_delete(&mut self, cx: &mut Context<Self>) {
        self.confirm_delete = None;
        cx.notify();
    }

    fn commit_delete(&mut self, cx: &mut Context<Self>) {
        let Some((target, is_dir, _)) = self.confirm_delete.take() else {
            return;
        };
        let ok = if is_dir {
            std::fs::remove_dir_all(&target).is_ok()
        } else {
            std::fs::remove_file(&target).is_ok()
        };
        if ok {
            self.close_tabs_under(&target);
        }
        self.reload_collection(cx);
    }

    /// The anchored right-click menu over a sidebar entry.
    fn ctx_menu_overlay(&self, cx: &mut Context<Self>) -> Div {
        let Some(menu) = &self.ctx_menu else {
            return div();
        };
        let item = |label: &str| {
            div()
                .px_3()
                .py_1()
                .text_size(px(13.))
                .text_color(theme::text())
                .hover(|s| s.bg(theme::surface0()))
                .child(label.to_string())
        };
        let mut card = div()
            .absolute()
            .left(menu.pos.x)
            .top(menu.pos.y)
            .occlude()
            .flex()
            .flex_col()
            .py_1()
            .w(px(180.))
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2());
        if menu.is_dir {
            card = card
                .child(item("New Request").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                        if let Some(m) = this.ctx_menu.take() {
                            this.new_request_in(&m.target, cx);
                        }
                    }),
                ))
                .child(item("New Folder").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                        if let Some(m) = this.ctx_menu.take() {
                            this.new_folder_in(&m.target, cx);
                        }
                    }),
                ))
                .child(item("Run Folder").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_run(cx)),
                ))
                .child(item("Settings").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_folder_settings(cx)),
                ));
            if self.clipboard_item.is_some() {
                card = card.child(item("Paste").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_paste(cx)),
                ));
            }
        } else {
            card = card
                .child(item("Open").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_run(cx)),
                ))
                .child(item("Run").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_run_request(cx)),
                ))
                .child(item("Move Up").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_move(-1, cx)),
                ))
                .child(item("Move Down").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_move(1, cx)),
                ));
        }
        card = card
            .child(item("Rename").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.start_rename(cx)),
            ))
            .child(item("Clone").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_duplicate(cx)),
            ))
            .child(item("Copy").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_copy(cx)),
            ))
            .child(item("Reveal in Explorer").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_reveal(cx)),
            ))
            .child(item("Delete").text_color(theme::red()).on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.start_delete(cx)),
            ));
        // Full-screen transparent catcher: any click outside closes the menu.
        div()
            .absolute()
            .inset_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseDownEvent, _w, cx| this.close_ctx_menu(cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _e: &MouseDownEvent, _w, cx| this.close_ctx_menu(cx)),
            )
            .child(card)
    }

    /// The inline rename prompt (modal).
    fn rename_overlay(&self, cx: &mut Context<Self>) -> Div {
        let Some(state) = &self.rename else {
            return div();
        };
        let title = if state.is_dir {
            "Rename Folder"
        } else {
            "Rename Request"
        };
        let card = div()
            .w(px(420.))
            .p_4()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .occlude()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(px(15.))
                    .text_color(theme::text())
                    .font_weight(gpui::FontWeight::BOLD)
                    .child(title),
            )
            .child(
                div()
                    .w_full()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(theme::input_bg())
                    .border_1()
                    .border_color(theme::border1())
                    .font_family("monospace")
                    .text_size(px(13.))
                    .child(state.input.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(ghost_btn("Cancel").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.cancel_rename(cx)),
                    ))
                    .child(solid_btn("Rename").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.commit_rename(cx)),
                    )),
            );
        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x000000aa))
            .flex()
            .items_center()
            .justify_center()
            .child(card)
    }

    /// The delete-confirmation modal.
    fn delete_overlay(&self, cx: &mut Context<Self>) -> Div {
        let Some((_, is_dir, name)) = &self.confirm_delete else {
            return div();
        };
        let kind = if *is_dir { "folder" } else { "request" };
        let msg = format!("Delete {kind} \u{201c}{name}\u{201d}? This cannot be undone.");
        let card = div()
            .w(px(420.))
            .p_4()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .occlude()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(px(15.))
                    .text_color(theme::text())
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Confirm Delete"),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::subtext())
                    .child(msg),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(ghost_btn("Cancel").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.cancel_delete(cx)),
                    ))
                    .child(solid_btn("Delete").bg(theme::red()).on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.commit_delete(cx)),
                    )),
            );
        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x000000aa))
            .flex()
            .items_center()
            .justify_center()
            .child(card)
    }

    // â”€â”€ secrets vault â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    /// Vault secrets as the lowest-precedence base vars for sends.
    fn vault_vars(&self) -> HashMap<String, String> {
        self.vault.clone().unwrap_or_default()
    }

    /// The send base layer: vault secrets overlaid by the active GLOBAL env's
    /// vars (collection + collection-env vars are layered on top in
    /// run_blocking via base_vars).
    fn send_globals(&self) -> HashMap<String, String> {
        let mut vars = self.vault_vars();
        if let Some(name) = &self.selected_global_env {
            for r in envfs::load_env_rows(&globals_root(), name) {
                if r.enabled {
                    vars.insert(r.name, r.value);
                }
            }
        }
        vars
    }

    fn open_vault(&mut self, cx: &mut Context<Self>) {
        self.vault_open = true;
        self.vault_error = None;
        cx.notify();
    }
    fn close_vault(&mut self, cx: &mut Context<Self>) {
        self.vault_open = false;
        cx.notify();
    }
    fn vault_unlock(&mut self, cx: &mut Context<Self>) {
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
        cx.notify();
    }
    fn vault_lock(&mut self, cx: &mut Context<Self>) {
        self.vault = None;
        self.vault_pw = None;
        self.vault_rows.clear();
        cx.notify();
    }
    fn vault_add_row(&mut self, cx: &mut Context<Self>) {
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
    fn toggle_reveal_secrets(&mut self, cx: &mut Context<Self>) {
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
    fn vault_remove_row(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.vault_rows.len() {
            self.vault_rows.remove(i);
        }
        cx.notify();
    }
    fn vault_save(&mut self, cx: &mut Context<Self>) {
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
        cx.notify();
    }

    fn toggle_devtools(&mut self, cx: &mut Context<Self>) {
        self.devtools_open = !self.devtools_open;
        cx.notify();
    }
    fn clear_devtools(&mut self, cx: &mut Context<Self>) {
        self.console.clear();
        self.network.clear();
        cx.notify();
    }

    fn send_options(&self) -> bru_http::SendOptions {
        bru_http::SendOptions {
            insecure: self.pref_insecure,
            timeout: std::time::Duration::from_secs(self.pref_timeout.max(1)),
            ..Default::default()
        }
    }

    fn open_prefs(&mut self, cx: &mut Context<Self>) {
        self.prefs_open = true;
        cx.notify();
    }
    fn close_prefs(&mut self, cx: &mut Context<Self>) {
        self.prefs_open = false;
        cx.notify();
    }
    fn toggle_insecure(&mut self, cx: &mut Context<Self>) {
        self.pref_insecure = !self.pref_insecure;
        self.persist_prefs();
        cx.notify();
    }
    /// Read the timeout input and commit it (ignored if not a number).
    fn apply_prefs(&mut self, cx: &mut Context<Self>) {
        if let Ok(n) = self.timeout_input.read(cx).text().trim().parse::<u64>() {
            self.pref_timeout = n;
        }
        self.prefs_open = false;
        self.persist_prefs();
        cx.notify();
    }
    /// Write the current prefs (timeout / insecure / theme) to disk.
    fn persist_prefs(&self) {
        save_prefs(self.pref_timeout, self.pref_insecure, !theme::is_dark());
    }

    /// Load (or reload) a collection from `dir`, replacing open tabs.
    fn load_collection(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
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
                self.status = "Loaded collection".into();
            }
            Err(e) => self.status = format!("Failed to load: {e}"),
        }
        cx.notify();
    }

    /// Pick a Postman v2.1 JSON and import it into a new collection.
    fn import_postman(&mut self, cx: &mut Context<Self>) {
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

    fn open_curl(&mut self, cx: &mut Context<Self>) {
        self.curl_open = true;
        cx.notify();
    }
    fn close_curl(&mut self, cx: &mut Context<Self>) {
        self.curl_open = false;
        cx.notify();
    }
    /// Parse the pasted curl command, write it as a request in the collection,
    /// and open it.
    fn import_curl(&mut self, cx: &mut Context<Self>) {
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

    fn open_cookies(&mut self, cx: &mut Context<Self>) {
        self.cookies_open = true;
        cx.notify();
    }
    fn close_cookies(&mut self, cx: &mut Context<Self>) {
        self.cookies_open = false;
        cx.notify();
    }
    fn delete_cookie(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.cookies.len() {
            self.cookies.remove(i);
        }
        cx.notify();
    }
    fn clear_cookies(&mut self, cx: &mut Context<Self>) {
        self.cookies.clear();
        cx.notify();
    }

    // â”€â”€ collection runner â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    fn requests_under(&self, dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let Some(tree) = &self.collection else {
            return out;
        };
        let start = if self.dir == dir {
            Some(&tree.root)
        } else {
            find_folder(&tree.root, dir)
        };
        if let Some(folder) = start {
            collect_folder_requests(folder, &mut out);
        }
        out
    }

    /// Run every request under `dir` sequentially on a worker thread; bridge the
    /// batch back via oneshot + cx.spawn (same pattern as send()).
    fn run_folder(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        if self.runner_running {
            return;
        }
        let files = self.requests_under(&dir);
        self.runner_title = dir
            .file_name()
            .and_then(|s| s.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| "Collection".into());
        self.runner_open = true;
        self.runner_running = true;
        self.runner_results.clear();
        let vars_base = self.dir.clone();
        let opts = self.send_options();
        let globals = self.send_globals();
        let env = self.selected_env.clone();
        let (tx, rx) = futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(run_folder_blocking(files, vars_base, opts, globals, env));
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.runner_running = false;
                if let Ok(results) = result {
                    this.runner_results = results;
                }
                cx.notify();
            });
        })
        .detach();
    }

    // â”€â”€ environment manager â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    /// The dir the env manager currently operates on (collection or globals).
    fn env_dir(&self) -> PathBuf {
        if self.env.as_ref().map(|e| e.global).unwrap_or(false) {
            globals_root()
        } else {
            self.dir.clone()
        }
    }

    fn env_build_rows(&self, name: &str, cx: &mut Context<Self>) -> Vec<EnvRowState> {
        let reveal = self.reveal_secrets;
        let dir = self.env_dir();
        envfs::load_env_rows(&dir, name)
            .into_iter()
            .map(|r| EnvRowState {
                name: cx.new(|cx| CodeEditor::single_line(cx, &r.name)),
                value: cx.new(|cx| {
                    if r.secret && !reveal {
                        CodeEditor::masked_line(cx, &r.value)
                    } else {
                        CodeEditor::single_line(cx, &r.value)
                    }
                }),
                enabled: r.enabled,
                secret: r.secret,
            })
            .collect()
    }

    fn env_collect_rows(&self, ed: &EnvEditor, cx: &App) -> Vec<envfs::EnvRow> {
        ed.rows
            .iter()
            .map(|r| envfs::EnvRow {
                name: r.name.read(cx).text().trim().to_string(),
                value: r.value.read(cx).text().to_string(),
                enabled: r.enabled,
                secret: r.secret,
            })
            .filter(|r| !r.name.is_empty())
            .collect()
    }

    fn env_open(&mut self, cx: &mut Context<Self>) {
        let names = envfs::scan_envs(&self.dir);
        let first = names.first().cloned().unwrap_or_default();
        let rows = self.env_build_rows(&first, cx);
        let rename = cx.new(|cx| CodeEditor::single_line(cx, &first));
        self.env = Some(EnvEditor {
            names,
            selected: first,
            rename,
            rows,
            error: None,
            global: false,
        });
        cx.notify();
    }

    /// Switch the env manager between Collection and Global scope.
    fn env_set_scope(&mut self, global: bool, cx: &mut Context<Self>) {
        if let Some(ed) = &mut self.env {
            ed.global = global;
        }
        let dir = self.env_dir();
        let names = envfs::scan_envs(&dir);
        let first = names.first().cloned().unwrap_or_default();
        let rows = self.env_build_rows(&first, cx);
        if let Some(ed) = &mut self.env {
            ed.names = names;
            ed.selected = first.clone();
            ed.rows = rows;
            ed.error = None;
            ed.rename.update(cx, |li, cx| li.set_line(&first, cx));
        }
        cx.notify();
    }

    fn env_close(&mut self, cx: &mut Context<Self>) {
        self.env = None;
        cx.notify();
    }

    fn env_select(&mut self, name: String, cx: &mut Context<Self>) {
        let rows = self.env_build_rows(&name, cx);
        if let Some(ed) = &mut self.env {
            ed.rename.update(cx, |li, cx| li.set_line(&name, cx));
            ed.selected = name;
            ed.rows = rows;
            ed.error = None;
        }
        cx.notify();
    }

    fn env_add_row(&mut self, cx: &mut Context<Self>) {
        let name = cx.new(|cx| CodeEditor::single_line(cx, ""));
        let value = cx.new(|cx| CodeEditor::single_line(cx, ""));
        if let Some(ed) = &mut self.env {
            ed.rows.push(EnvRowState {
                name,
                value,
                enabled: true,
                secret: false,
            });
        }
        cx.notify();
    }

    fn env_remove_row(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(ed) = &mut self.env {
            if i < ed.rows.len() {
                ed.rows.remove(i);
            }
        }
        cx.notify();
    }

    fn env_toggle_enabled(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(ed) = &mut self.env {
            if let Some(r) = ed.rows.get_mut(i) {
                r.enabled = !r.enabled;
            }
        }
        cx.notify();
    }

    fn env_toggle_secret(&mut self, i: usize, cx: &mut Context<Self>) {
        let reveal = self.reveal_secrets;
        if let Some(ed) = &mut self.env {
            if let Some(r) = ed.rows.get_mut(i) {
                r.secret = !r.secret;
                let mask = r.secret && !reveal;
                r.value.update(cx, |ed, cx| ed.set_masked(mask, cx));
            }
        }
        cx.notify();
    }

    fn env_save(&mut self, cx: &mut Context<Self>) {
        let Some(ed) = self.env.as_ref() else { return };
        if ed.selected.is_empty() {
            if let Some(ed) = &mut self.env {
                ed.error = Some("Select or create an environment first".into());
            }
            cx.notify();
            return;
        }
        let rows = self.env_collect_rows(ed, cx);
        let sel = ed.selected.clone();
        let res = envfs::save_env(&self.env_dir(), &sel, &rows);
        if let Some(ed) = &mut self.env {
            ed.error = res.err();
        }
        cx.notify();
    }

    fn env_new(&mut self, cx: &mut Context<Self>) {
        let dir = self.env_dir();
        let existing = envfs::scan_envs(&dir);
        let mut name = "New Environment".to_string();
        let mut n = 1;
        while existing.iter().any(|e| e == &name) {
            n += 1;
            name = format!("New Environment {n}");
        }
        match envfs::create_env(&dir, &name) {
            Ok(()) => {
                let names = envfs::scan_envs(&dir);
                let rows = self.env_build_rows(&name, cx);
                let rename = cx.new(|cx| CodeEditor::single_line(cx, &name));
                if let Some(ed) = &mut self.env {
                    ed.names = names;
                    ed.rename = rename;
                    ed.selected = name;
                    ed.rows = rows;
                    ed.error = None;
                }
            }
            Err(e) => {
                if let Some(ed) = &mut self.env {
                    ed.error = Some(e);
                }
            }
        }
        cx.notify();
    }

    fn env_delete(&mut self, name: String, cx: &mut Context<Self>) {
        let dir = self.env_dir();
        let _ = envfs::delete_env(&dir, &name);
        let names = envfs::scan_envs(&dir);
        let reselect = self
            .env
            .as_ref()
            .map(|e| e.selected == name)
            .unwrap_or(false);
        let target = if reselect {
            names.first().cloned().unwrap_or_default()
        } else {
            self.env
                .as_ref()
                .map(|e| e.selected.clone())
                .unwrap_or_default()
        };
        let rows = self.env_build_rows(&target, cx);
        let rename = cx.new(|cx| CodeEditor::single_line(cx, &target));
        if let Some(ed) = &mut self.env {
            ed.names = names;
            ed.selected = target;
            ed.rename = rename;
            ed.rows = rows;
        }
        cx.notify();
    }

    fn env_duplicate(&mut self, name: String, cx: &mut Context<Self>) {
        let dir = self.env_dir();
        let _ = envfs::duplicate_env(&dir, &name);
        let names = envfs::scan_envs(&dir);
        if let Some(ed) = &mut self.env {
            ed.names = names;
        }
        cx.notify();
    }

    fn env_rename_apply(&mut self, cx: &mut Context<Self>) {
        let (old, new) = match self.env.as_ref() {
            Some(ed) => (
                ed.selected.clone(),
                ed.rename.read(cx).text().trim().to_string(),
            ),
            None => return,
        };
        if old.is_empty() || new.is_empty() || old == new {
            return;
        }
        match envfs::rename_env(&self.env_dir(), &old, &new) {
            Ok(()) => {
                let names = envfs::scan_envs(&self.env_dir());
                if let Some(ed) = &mut self.env {
                    ed.names = names;
                    ed.selected = new;
                    ed.error = None;
                }
            }
            Err(e) => {
                if let Some(ed) = &mut self.env {
                    ed.error = Some(e);
                }
            }
        }
        cx.notify();
    }

    fn active_tab(&self) -> Option<&OpenTab> {
        self.active.and_then(|i| self.tabs.get(i))
    }

    /// Remove tab `i`, fixing up the active index.
    fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        let p = self.tabs[i].path.clone();
        self.dirty.remove(&p);
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

    /// Close tab `i`, but prompt first if it has unsaved edits (data-loss guard).
    fn request_close_tab(&mut self, i: usize, cx: &mut Context<Self>) {
        let dirty = self
            .tabs
            .get(i)
            .map(|t| self.dirty.contains(&t.path))
            .unwrap_or(false);
        if dirty {
            self.confirm_close = Some(i);
        } else {
            self.close_tab(i);
        }
        cx.notify();
    }

    /// The unsaved-changes confirmation modal for closing a dirty tab.
    fn close_confirm_overlay(&self, cx: &mut Context<Self>) -> Div {
        let Some(i) = self.confirm_close else {
            return div();
        };
        let name = self
            .tabs
            .get(i)
            .map(|t| t.title())
            .unwrap_or_else(|| "this tab".into());
        let card = div()
            .w(px(440.))
            .p_4()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .occlude()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(px(15.))
                    .text_color(theme::text())
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Unsaved Changes"),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::subtext())
                    .child(format!("{name} has unsaved edits.")),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(ghost_btn("Cancel").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                            this.confirm_close = None;
                            cx.notify();
                        }),
                    ))
                    .child(ghost_btn("Discard").text_color(theme::red()).on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                            this.close_tab(i);
                            this.confirm_close = None;
                            cx.notify();
                        }),
                    ))
                    .child(solid_btn("Save & Close").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                            this.active = Some(i);
                            this.save(cx);
                            this.close_tab(i);
                            this.confirm_close = None;
                            cx.notify();
                        }),
                    )),
            );
        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x000000aa))
            .flex()
            .items_center()
            .justify_center()
            .child(card)
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
                // Tab may have moved/closed while in flight â€” re-find by path.
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
    fn save(&mut self, cx: &mut Context<Self>) {
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
    fn set_body_mode(&mut self, mode: &str, cx: &mut Context<Self>) {
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
    fn set_auth_mode(&mut self, mode: &str, cx: &mut Context<Self>) {
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

    // â”€â”€ structured params/headers grid â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    fn kv_add_row(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.active else { return };
        let row = KvRow {
            name: cx.new(|cx| CodeEditor::single_line(cx, "")),
            value: cx.new(|cx| CodeEditor::single_line(cx, "")),
            enabled: true,
        };
        self.tabs[i].kv_rows.push(row);
        self.dirty.insert(self.tabs[i].path.clone());
        cx.notify();
    }
    fn kv_remove_row(&mut self, idx: usize, cx: &mut Context<Self>) {
        let Some(i) = self.active else { return };
        if idx < self.tabs[i].kv_rows.len() {
            self.tabs[i].kv_rows.remove(idx);
            self.dirty.insert(self.tabs[i].path.clone());
            cx.notify();
        }
    }
    fn kv_toggle_row(&mut self, idx: usize, cx: &mut Context<Self>) {
        let Some(i) = self.active else { return };
        if let Some(r) = self.tabs[i].kv_rows.get_mut(idx) {
            r.enabled = !r.enabled;
            self.dirty.insert(self.tabs[i].path.clone());
            cx.notify();
        }
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
            self.home = false;
        }
    }

    fn top_bar(&self, cx: &mut Context<Self>) -> Div {
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
            .child(icon_chip("\u{2302}").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.go_home(cx)),
            ))
            .child(chip("Open Collection").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        this.load_collection(dir, cx);
                    }
                }),
            ))
            .child(chip("New Collection").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                    if let Some(parent) = rfd::FileDialog::new().pick_folder() {
                        this.create_collection(&parent, cx);
                    }
                }),
            ))
            .child(chip("Import").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.import_postman(cx)),
            ))
            .child(chip("curl").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.open_curl(cx)),
            ))
            .child(chip("Run").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                    let dir = this.dir.clone();
                    this.run_folder(dir, cx);
                    cx.notify();
                }),
            ))
            .child(
                div()
                    .text_color(theme::accent())
                    .text_size(px(13.))
                    .child(name),
            )
            .child(
                div()
                    .text_color(theme::muted())
                    .text_size(px(12.))
                    .child("\u{2022} main"),
            )
            .child(div().flex_1())
            .child({
                let label = match &self.selected_env {
                    Some(e) => format!("Env: {e}"),
                    None => "Env: none".to_string(),
                };
                chip(&label)
                    .text_color(if self.selected_env.is_some() {
                        theme::accent()
                    } else {
                        theme::muted()
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, ev: &MouseDownEvent, _w, cx| {
                            this.open_env_menu(ev.position, cx);
                        }),
                    )
            })
            .child(chip("Environments").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.env_open(cx)),
            ))
            .child(chip("Cookies").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.open_cookies(cx)),
            ))
            .child(
                chip("Vault")
                    .text_color(if self.vault.is_some() {
                        theme::green()
                    } else {
                        theme::text()
                    })
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.open_vault(cx)),
                    ),
            )
            .child(chip("Dev Tools").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.toggle_devtools(cx)),
            ))
            .child(chip("Settings").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.open_collection_settings(cx)),
            ))
            .child(chip("Prefs").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.open_prefs(cx)),
            ))
            .child(
                icon_chip(if theme::is_dark() {
                    "\u{2600}" // â˜€ â€” click for light
                } else {
                    "\u{263E}" // â˜¾ â€” click for dark
                })
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                        theme::toggle();
                        this.persist_prefs();
                        cx.notify();
                    }),
                ),
            )
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        let q = self.search_query.clone();
        let mut rows: Vec<Div> = Vec::new();
        if let Some(tree) = &self.collection {
            self.push_folder(&tree.root, 0, &q, cx, &mut rows);
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
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_1()
                    .child(
                        div().text_color(theme::muted()).text_size(px(12.)).child(
                            self.collection
                                .as_ref()
                                .map(|c| c.name.to_uppercase())
                                .unwrap_or_default(),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .items_center()
                            .child(
                                div()
                                    .px_1()
                                    .text_size(px(13.))
                                    .text_color(theme::accent())
                                    .child("\u{1F4C1}+")
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                                            let dir = this.dir.clone();
                                            this.new_folder_in(&dir, cx);
                                        }),
                                    ),
                            )
                            .child(
                                div()
                                    .px_1()
                                    .text_color(theme::accent())
                                    .child("+")
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                                            this.new_request(cx)
                                        }),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(theme::input_bg())
                    .border_1()
                    .border_color(theme::border1())
                    .text_size(px(12.))
                    .child(self.search.clone()),
            )
            .child(
                div()
                    .id("sidebar-rows")
                    .overflow_y_scroll()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(rows),
            )
    }

    fn push_folder(
        &self,
        folder: &Folder,
        depth: usize,
        query: &str,
        cx: &mut Context<Self>,
        out: &mut Vec<Div>,
    ) {
        let mut subs: Vec<&Folder> = folder.folders.iter().collect();
        subs.sort_by_key(|f| f.name.to_lowercase());
        for sub in subs {
            if !query.is_empty() && !folder_matches(sub, query) {
                continue;
            }
            // A search query forces every branch open so matches are visible.
            let collapsed = query.is_empty() && self.collapsed.contains(&sub.path);
            let fpath = sub.path.clone();
            let fname = sub.name.clone();
            let tpath = sub.path.clone();
            out.push(
                folder_row(&sub.name, depth, collapsed)
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseUpEvent, _win, cx| {
                            this.toggle_folder(tpath.clone(), cx);
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, ev: &MouseDownEvent, _win, cx| {
                            this.open_ctx_menu(fpath.clone(), true, fname.clone(), ev.position, cx);
                        }),
                    ),
            );
            if !collapsed {
                self.push_folder(sub, depth + 1, query, cx, out);
            }
        }
        let mut reqs: Vec<&bru_core::RequestItem> = folder.requests.iter().collect();
        reqs.sort_by(|a, b| {
            a.seq
                .unwrap_or(i64::MAX)
                .cmp(&b.seq.unwrap_or(i64::MAX))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        for req in reqs {
            if !query.is_empty() && !req.name.to_lowercase().contains(query) {
                continue;
            }
            let path = req.path.clone();
            let active = self.active_tab().map(|t| t.path.as_path()) == Some(path.as_path());
            let method = req.method.clone().unwrap_or_default();
            let rpath = path.clone();
            let rname = req.name.clone();
            let row = req_row(&method, &req.name, active, depth)
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, _ev: &MouseUpEvent, _win, cx| {
                        this.open_request(path.clone(), cx);
                        cx.notify();
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, ev: &MouseDownEvent, _win, cx| {
                        this.open_ctx_menu(rpath.clone(), false, rname.clone(), ev.position, cx);
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
                    .child(method)
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                            if let Some(i) = this.active {
                                let next = next_method(&this.tabs[i].method);
                                edit::set_method(&mut this.tabs[i].file, &next);
                                this.tabs[i].method = next;
                                cx.notify();
                            }
                        }),
                    ),
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
            .child(icon_chip("</>").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                    let curl = this.active_tab().map(|tab| to_curl(tab, cx));
                    if let Some(curl) = curl {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(curl));
                        this.status = "Copied curl to clipboard".into();
                        cx.notify();
                    }
                }),
            ))
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

    /// Raw bytes of the active tab's last response, if any.
    fn response_bytes(&self) -> Option<Vec<u8>> {
        self.active_tab()
            .and_then(|t| t.response.as_ref())
            .and_then(|o| o.response.as_ref())
            .map(|r| r.body.clone())
    }

    fn copy_response(&mut self, cx: &mut Context<Self>) {
        if let Some(bytes) = self.response_bytes() {
            let s = String::from_utf8_lossy(&bytes).to_string();
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(s));
            self.status = "Copied response to clipboard".into();
            cx.notify();
        }
    }

    fn save_response(&mut self, cx: &mut Context<Self>) {
        let Some(bytes) = self.response_bytes() else {
            return;
        };
        if let Some(path) = rfd::FileDialog::new().save_file() {
            self.status = match std::fs::write(&path, &bytes) {
                Ok(()) => "Saved response".into(),
                Err(e) => format!("Save failed: {e}"),
            };
        }
        cx.notify();
    }

    fn clear_response(&mut self, cx: &mut Context<Self>) {
        if let Some(i) = self.active {
            self.tabs[i].response = None;
            self.status.clear();
            cx.notify();
        }
    }

    fn response_pane(&self, tab: &OpenTab, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        // Sub-tab strip + status/time/size summary.
        let mut strip = div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .px_2()
            .bg(theme::surface0())
            .border_b_1()
            .border_color(theme::border2());
        for rt in RespTab::ALL {
            let active = tab.resp_tab == rt;
            // Headers tab shows a count badge; Tests tab shows passed/total.
            let label = match rt {
                RespTab::Headers => {
                    let n = tab
                        .response
                        .as_ref()
                        .and_then(|o| o.response.as_ref())
                        .map(|r| r.headers.len())
                        .unwrap_or(0);
                    if n > 0 {
                        format!("Headers ({n})")
                    } else {
                        "Headers".to_string()
                    }
                }
                RespTab::Tests => match &tab.response {
                    Some(o) if !o.assertions.is_empty() || !o.tests.is_empty() => {
                        let total = o.assertions.len() + o.tests.len();
                        let passed = o.assertions.iter().filter(|a| a.passed).count()
                            + o.tests.iter().filter(|t| t.passed).count();
                        format!("Tests {passed}/{total}")
                    }
                    _ => "Tests".to_string(),
                },
                _ => rt.label().to_string(),
            };
            strip = strip.child(tab_chip(&label, active).on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                    if let Some(i) = this.active {
                        this.tabs[i].resp_tab = rt;
                    }
                    cx.notify();
                }),
            ));
        }
        if let Some(r) = tab.response.as_ref().and_then(|o| o.response.as_ref()) {
            strip = strip
                .child(div().flex_1())
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(status_color(r.status))
                        .child(format!("{} {}", r.status, r.status_text)),
                )
                .child(
                    div()
                        .px_2()
                        .text_size(px(12.))
                        .text_color(theme::subtext())
                        .child(format!("{} ms", r.duration_ms)),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::subtext())
                        .child(human_size(r.body.len())),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme::muted())
                                .font_family("monospace")
                                .child("$"),
                        )
                        .child(
                            div()
                                .w(px(170.))
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .bg(theme::input_bg())
                                .border_1()
                                .border_color(theme::border1())
                                .font_family("monospace")
                                .text_size(px(12.))
                                .child(self.resp_filter.clone()),
                        ),
                )
                .child(
                    ghost_btn(if self.resp_raw { "Pretty" } else { "Raw" }).on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                            this.resp_raw = !this.resp_raw;
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    ghost_btn("Hex")
                        .when(self.resp_hex, |d| d.text_color(theme::accent()))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                                this.resp_hex = !this.resp_hex;
                                cx.notify();
                            }),
                        ),
                )
                .child(ghost_btn("Copy").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.copy_response(cx)),
                ))
                .child(ghost_btn("Save").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.save_response(cx)),
                ))
                .child(ghost_btn("Clear").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.clear_response(cx)),
                ));
        }

        let scroll = |id: &'static str| div().id(id).overflow_y_scroll().flex_1().w_full().p_3();
        let content: gpui::AnyElement = match (&tab.response, tab.resp_tab) {
            (None, _) => div()
                .p_3()
                .text_color(theme::muted())
                .child("No response yet \u{2014} press Send.")
                .into_any_element(),
            (Some(o), RespTab::Response) => {
                // Compute the displayed body (pretty JSON + JSONPath filter, or
                // raw), then push it into the read-only selectable editor only
                // when it changes (so a live text selection survives re-renders).
                let (displayed, lang) = match o.response.as_ref() {
                    Some(r) if self.resp_hex => (hex_dump(&r.body), Lang::Plain),
                    Some(r) => {
                        let raw = String::from_utf8_lossy(&r.body).to_string();
                        let is_json = !self.resp_raw
                            && r.headers.iter().any(|(k, v)| {
                                k.eq_ignore_ascii_case("content-type") && v.contains("json")
                            });
                        if is_json {
                            match serde_json::from_str::<serde_json::Value>(&raw) {
                                Ok(v) => {
                                    let shown = if self.resp_filter_query.is_empty() {
                                        Some(v)
                                    } else {
                                        json_path(&v, &self.resp_filter_query)
                                    };
                                    match shown {
                                        Some(val) => (
                                            serde_json::to_string_pretty(&val).unwrap_or(raw),
                                            Lang::Json,
                                        ),
                                        None => ("(no match)".to_string(), Lang::Plain),
                                    }
                                }
                                Err(_) => (raw, Lang::Plain),
                            }
                        } else {
                            (raw, Lang::Plain)
                        }
                    }
                    None => (format_outcome(o), Lang::Plain),
                };
                if self.resp_editor.read(cx).text() != displayed {
                    self.resp_editor
                        .update(cx, |ed, cx| ed.set_text(&displayed, lang, cx));
                }
                scroll("resp-body")
                    .font_family("monospace")
                    .text_size(px(13.))
                    .line_height(px(19.))
                    .child(self.resp_editor.clone())
                    .into_any_element()
            }
            (Some(o), RespTab::Headers) => {
                let mut col = div().flex().flex_col().gap_1();
                match &o.response {
                    Some(r) => {
                        for (k, v) in &r.headers {
                            col = col.child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_2()
                                    .child(
                                        div()
                                            .w(px(200.))
                                            .font_family("monospace")
                                            .text_size(px(12.))
                                            .text_color(theme::accent())
                                            .child(k.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .font_family("monospace")
                                            .text_size(px(12.))
                                            .text_color(theme::text())
                                            .child(v.clone()),
                                    ),
                            );
                        }
                    }
                    None => {
                        col = col.child(div().text_color(theme::muted()).child("(no response)"))
                    }
                }
                scroll("resp-headers").child(col).into_any_element()
            }
            (Some(o), RespTab::Timeline) => {
                // curl-style trace: request line + request headers, then the
                // response status + every response header, then timing/size.
                let mut txt = format!("> {} {}\n", tab.method.to_uppercase(), o.url);
                for line in edit::dict_to_lines(&tab.file, "headers").lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('~') {
                        continue;
                    }
                    if let Some((k, v)) = line.split_once(':') {
                        txt.push_str(&format!("> {}: {}\n", k.trim(), v.trim()));
                    }
                }
                if let Some(e) = &o.error {
                    if !e.is_empty() {
                        txt.push_str(&format!("! {e}\n"));
                    }
                }
                if let Some(r) = o.response.as_ref() {
                    txt.push_str(&format!("\n< {} {}\n", r.status, r.status_text));
                    for (k, v) in &r.headers {
                        txt.push_str(&format!("< {k}: {v}\n"));
                    }
                    txt.push_str(&format!(
                        "\ntime: {} ms\nsize: {}",
                        r.duration_ms,
                        human_size(r.body.len())
                    ));
                }
                scroll("resp-timeline")
                    .font_family("monospace")
                    .text_size(px(12.))
                    .text_color(theme::subtext())
                    .child(txt)
                    .into_any_element()
            }
            (Some(o), RespTab::Tests) => {
                let mut col = div().flex().flex_col().gap_1();
                if o.assertions.is_empty() && o.tests.is_empty() {
                    col = col.child(
                        div()
                            .text_color(theme::muted())
                            .text_size(px(12.))
                            .child("No assertions or tests."),
                    );
                }
                for a in &o.assertions {
                    let (m, c) = if a.passed {
                        ("\u{2713}", theme::green())
                    } else {
                        ("\u{2717}", theme::red())
                    };
                    col = col.child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(div().text_color(c).child(m))
                            .child(
                                div()
                                    .font_family("monospace")
                                    .text_size(px(12.))
                                    .text_color(theme::text())
                                    .child(format!("{} {} {}", a.expr, a.operator, a.expected)),
                            ),
                    );
                }
                for t in &o.tests {
                    let (m, c) = if t.passed {
                        ("\u{2713}", theme::green())
                    } else {
                        ("\u{2717}", theme::red())
                    };
                    col = col.child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(div().text_color(c).child(m))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme::text())
                                    .child(format!("test: {}", t.name)),
                            ),
                    );
                }
                scroll("resp-tests").child(col).into_any_element()
            }
        };

        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .bg(theme::bg())
            .border_t_1()
            .border_color(theme::border2())
            .child(strip)
            .child(content)
    }

    /// The clickable request sub-tab strip.
    fn req_subtabs(&self, tab: &OpenTab, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let mut strip = div()
            .id("req-subtabs")
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .overflow_x_scroll()
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
        // A mode-cycle chip pinned right when the Body/Auth tab is active.
        if matches!(tab.req_tab, ReqTab::Body | ReqTab::Auth) {
            let is_body = tab.req_tab == ReqTab::Body;
            let (field, list, prefix) = if is_body {
                ("body", BODY_MODES, "Body")
            } else {
                ("auth", AUTH_MODES, "Auth")
            };
            let cur = edit::method_field(&tab.file, field).unwrap_or_else(|| "none".into());
            strip =
                strip
                    .child(div().flex_1())
                    .child(chip(&format!("{prefix}: {cur}")).on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                            let Some(i) = this.active else { return };
                            let cur =
                                edit::method_field(&this.tabs[i].file, field).unwrap_or_default();
                            let next = cycle_next(list, &cur);
                            if is_body {
                                this.set_body_mode(&next, cx);
                            } else {
                                this.set_auth_mode(&next, cx);
                            }
                        }),
                    ));
        }
        strip
    }

    /// The content for the active request sub-tab (the shared editor).
    fn req_content(&self, tab: &OpenTab, cx: &mut Context<Self>) -> Div {
        if matches!(tab.edit_kind, EditKind::Kv(_)) {
            return self.kv_grid(tab, cx);
        }
        if matches!(tab.edit_kind, EditKind::GraphQl) {
            let pane = |label: &str, id: &'static str, ed: Entity<CodeEditor>| {
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .child(
                        div()
                            .px_3()
                            .pt_2()
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .child(label.to_string()),
                    )
                    .child(
                        div()
                            .id(id)
                            .overflow_y_scroll()
                            .flex_1()
                            .w_full()
                            .p_3()
                            .font_family("monospace")
                            .text_size(px(13.))
                            .line_height(px(19.))
                            .child(ed),
                    )
            };
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .w_full()
                .bg(theme::bg())
                .child(pane("QUERY", "gql-query", tab.body_editor.clone()))
                .child(pane("VARIABLES", "gql-vars", tab.body_vars_editor.clone()));
        }
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

    /// The structured params/headers grid (enable toggle + name + value + âœ•).
    fn kv_grid(&self, tab: &OpenTab, cx: &mut Context<Self>) -> Div {
        let cell = |child: Entity<CodeEditor>, w: Option<Pixels>| {
            let d = div()
                .px_2()
                .py_1()
                .rounded_md()
                .bg(theme::input_bg())
                .border_1()
                .border_color(theme::border1())
                .font_family("monospace")
                .text_size(px(12.))
                .child(child);
            match w {
                Some(w) => d.w(w),
                None => d.flex_1(),
            }
        };
        let block = match &tab.edit_kind {
            EditKind::Kv(b) => b.as_str(),
            _ => "",
        };
        let (col1, col2) = if block == "assert" {
            (
                "Expression  (e.g. res.status)",
                "Operator + Value  (e.g. eq 200)",
            )
        } else {
            ("Name", "Value")
        };
        let mut table = div().flex().flex_col().gap_1().child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .text_size(px(10.))
                .text_color(theme::muted())
                .child(div().w(px(234.)).child(col1))
                .child(div().flex_1().child(col2)),
        );
        for (idx, row) in tab.kv_rows.iter().enumerate() {
            table = table.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(check_box(row.enabled).on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                            this.kv_toggle_row(idx, cx)
                        }),
                    ))
                    .child(cell(row.name.clone(), Some(px(220.))))
                    .child(cell(row.value.clone(), None))
                    .child(
                        div()
                            .px_1()
                            .text_color(theme::red())
                            .child("\u{2715}")
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                    this.kv_remove_row(idx, cx)
                                }),
                            ),
                    ),
            );
        }
        table = table.child(
            div()
                .pt_1()
                .text_size(px(12.))
                .text_color(theme::accent())
                .child("+ Add")
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.kv_add_row(cx)),
                ),
        );
        let mut inner = div().flex().flex_col().gap_3().child(table);
        // URL-derived path params (Params tab only): read-only name + value.
        if !tab.path_rows.is_empty() {
            let mut pt = div().flex().flex_col().gap_1().child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::muted())
                    .child("PATH PARAMS"),
            );
            for (name, ed) in &tab.path_rows {
                pt = pt.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .w(px(220.))
                                .font_family("monospace")
                                .text_size(px(12.))
                                .text_color(theme::accent())
                                .child(format!(":{name}")),
                        )
                        .child(cell(ed.clone(), None)),
                );
            }
            inner = inner.child(pt);
        }
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .bg(theme::bg())
            .child(
                div()
                    .id("kv-grid")
                    .overflow_y_scroll()
                    .flex_1()
                    .w_full()
                    .p_3()
                    .child(inner),
            )
    }

    /// The secrets-vault overlay (unlock or manage).
    fn vault_overlay(&self, cx: &mut Context<Self>) -> Div {
        let unlocked = self.vault.is_some();
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .child(
                div()
                    .flex_1()
                    .text_size(px(15.))
                    .text_color(theme::text())
                    .child("Secrets Vault"),
            )
            .when(unlocked, |d| {
                let eye = if self.reveal_secrets {
                    "\u{1F441} Hide"
                } else {
                    "\u{1F441} Reveal"
                };
                d.child(ghost_btn(eye).on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.toggle_reveal_secrets(cx)),
                ))
                .child(ghost_btn("Lock").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.vault_lock(cx)),
                ))
            })
            .child(ghost_btn("Close").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.close_vault(cx)),
            ));
        let body: Div =
            if !unlocked {
                let prompt = if vault::exists() {
                    "Enter your master password to unlock."
                } else {
                    "No vault yet \u{2014} set a master password to create one."
                };
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::subtext())
                            .child(prompt),
                    )
                    .child(
                        div()
                            .w_full()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(theme::input_bg())
                            .border_1()
                            .border_color(theme::border1())
                            .font_family("monospace")
                            .text_size(px(12.))
                            .child(self.vault_input.clone()),
                    )
                    .child(solid_btn("Unlock").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.vault_unlock(cx)),
                    ))
            } else {
                let cell = |child: Entity<CodeEditor>| {
                    div()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(theme::input_bg())
                        .border_1()
                        .border_color(theme::border1())
                        .text_size(px(12.))
                        .font_family("monospace")
                        .child(child)
                };
                let mut table = div().flex().flex_col().gap_1();
                for (i, (k, v)) in self.vault_rows.iter().enumerate() {
                    table = table.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(cell(k.clone()).w(px(200.)))
                            .child(cell(v.clone()).flex_1())
                            .child(
                                div()
                                    .px_1()
                                    .text_color(theme::red())
                                    .child("\u{2715}")
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                            this.vault_remove_row(i, cx)
                                        }),
                                    ),
                            ),
                    );
                }
                table = table.child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::accent())
                        .child("+ Add Secret")
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.vault_add_row(cx)),
                        ),
                );
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .flex_1()
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .child("Secrets resolve into {{name}} at send (lowest precedence)."),
                    )
                    .child(
                        div()
                            .id("vault-table")
                            .overflow_y_scroll()
                            .flex_1()
                            .child(table),
                    )
                    .child(div().flex().flex_row().justify_end().child(
                        solid_btn("Save").on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.vault_save(cx)),
                        ),
                    ))
            };
        let card = div()
            .flex()
            .flex_col()
            .gap_3()
            .w(px(620.))
            .h(if unlocked { px(440.) } else { px(220.) })
            .p_4()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .occlude()
            .child(header)
            .child(body)
            .children(self.vault_error.as_ref().map(|e| {
                div()
                    .text_size(px(12.))
                    .text_color(theme::red())
                    .child(e.clone())
            }));
        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x00000099))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.close_vault(cx)),
            )
            .child(card)
    }

    /// The devtools dock (Console / Network), pinned to the bottom.
    fn devtools_overlay(&self, cx: &mut Context<Self>) -> Div {
        let tab_btn = |label: &'static str, net: bool, active: bool, cx: &mut Context<Self>| {
            tab_chip(label, active).on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                    this.devtools_net = net;
                    cx.notify();
                }),
            )
        };
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .child(tab_btn("Console", false, !self.devtools_net, cx))
            .child(tab_btn("Network", true, self.devtools_net, cx))
            .child(div().flex_1())
            .child(ghost_btn("Clear").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.clear_devtools(cx)),
            ))
            .child(ghost_btn("Close").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.toggle_devtools(cx)),
            ));
        let body: Div = if self.devtools_net {
            let mut col = div().flex().flex_col().gap_1();
            if self.network.is_empty() {
                col = col.child(
                    div()
                        .text_color(theme::muted())
                        .text_size(px(12.))
                        .child("No requests yet."),
                );
            }
            for e in &self.network {
                let sc = if e.ok {
                    status_color(e.status)
                } else {
                    theme::red()
                };
                col =
                    col.child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(50.))
                                    .font_family("monospace")
                                    .text_size(px(11.))
                                    .text_color(theme::method_color(&e.method))
                                    .child(short_method(&e.method)),
                            )
                            .child(div().w(px(40.)).text_size(px(11.)).text_color(sc).child(
                                if e.ok {
                                    e.status.to_string()
                                } else {
                                    "ERR".to_string()
                                },
                            ))
                            .child(
                                div()
                                    .w(px(64.))
                                    .text_size(px(11.))
                                    .text_color(theme::subtext())
                                    .child(format!("{} ms", e.ms)),
                            )
                            .child(
                                div()
                                    .w(px(80.))
                                    .text_size(px(11.))
                                    .text_color(theme::subtext())
                                    .child(human_size(e.size)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .font_family("monospace")
                                    .text_size(px(11.))
                                    .text_color(theme::text())
                                    .child(e.url.clone()),
                            ),
                    );
            }
            col
        } else {
            let mut col = div().flex().flex_col().gap_1();
            if self.console.is_empty() {
                col = col.child(
                    div()
                        .text_color(theme::muted())
                        .text_size(px(12.))
                        .child("Console is empty."),
                );
            }
            for line in &self.console {
                col = col.child(
                    div()
                        .font_family("monospace")
                        .text_size(px(12.))
                        .text_color(theme::subtext())
                        .child(line.clone()),
                );
            }
            col
        };
        div()
            .absolute()
            .left(px(0.))
            .right(px(0.))
            .bottom(px(0.))
            .h(px(220.))
            .bg(theme::mantle())
            .border_t_1()
            .border_color(theme::border1())
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .occlude()
            .child(header)
            .child(
                div()
                    .id("devtools-body")
                    .overflow_y_scroll()
                    .flex_1()
                    .w_full()
                    .child(body),
            )
    }

    /// The preferences overlay (timeout + TLS-insecure).
    fn prefs_overlay(&self, cx: &mut Context<Self>) -> Div {
        let card = div()
            .flex()
            .flex_col()
            .gap_3()
            .w(px(440.))
            .p_4()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .occlude()
            .child(
                div()
                    .text_size(px(15.))
                    .text_color(theme::text())
                    .child("Preferences"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(150.))
                            .text_size(px(12.))
                            .text_color(theme::subtext())
                            .child("Timeout (seconds)"),
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
                            .font_family("monospace")
                            .text_size(px(12.))
                            .child(self.timeout_input.clone()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(check_box(self.pref_insecure).on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.toggle_insecure(cx)),
                    ))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::subtext())
                            .child("Disable TLS verification (insecure)"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(ghost_btn("Close").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.close_prefs(cx)),
                    ))
                    .child(solid_btn("Apply").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.apply_prefs(cx)),
                    )),
            );
        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x00000099))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.close_prefs(cx)),
            )
            .child(card)
    }

    /// The curl-import overlay (paste a curl command).
    fn curl_overlay(&self, cx: &mut Context<Self>) -> Div {
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .child(
                div()
                    .flex_1()
                    .text_size(px(15.))
                    .text_color(theme::text())
                    .child("Import curl"),
            )
            .child(solid_btn("Import").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.import_curl(cx)),
            ))
            .child(ghost_btn("Close").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.close_curl(cx)),
            ));
        let editor = div()
            .id("curl-input")
            .overflow_y_scroll()
            .flex_1()
            .w_full()
            .p_2()
            .rounded_md()
            .bg(theme::input_bg())
            .border_1()
            .border_color(theme::border1())
            .font_family("monospace")
            .text_size(px(12.))
            .child(self.curl_input.clone());
        let card = div()
            .flex()
            .flex_col()
            .gap_2()
            .w(px(680.))
            .h(px(340.))
            .p_4()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .occlude()
            .child(header)
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::muted())
                    .child("Paste a curl command:"),
            )
            .child(editor);
        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x00000099))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.close_curl(cx)),
            )
            .child(card)
    }

    /// The cookies overlay (captured from response Set-Cookie headers).
    fn cookies_overlay(&self, cx: &mut Context<Self>) -> Div {
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .w_full()
            .child(
                div()
                    .flex_1()
                    .text_size(px(15.))
                    .text_color(theme::text())
                    .child("Cookies"),
            )
            .child(ghost_btn("Clear All").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.clear_cookies(cx)),
            ))
            .child(ghost_btn("Close").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.close_cookies(cx)),
            ));
        let mut list = div()
            .id("cookies-list")
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1()
            .flex_1()
            .w_full();
        if self.cookies.is_empty() {
            list = list.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child("No cookies yet \u{2014} send a request that returns Set-Cookie."),
            );
        }
        for (i, c) in self.cookies.iter().enumerate() {
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(180.))
                            .font_family("monospace")
                            .text_size(px(12.))
                            .text_color(theme::subtext())
                            .child(c.domain.clone()),
                    )
                    .child(
                        div()
                            .w(px(160.))
                            .text_size(px(12.))
                            .text_color(theme::accent())
                            .child(c.name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .font_family("monospace")
                            .text_size(px(12.))
                            .text_color(theme::text())
                            .child(c.value.clone()),
                    )
                    .child(
                        div()
                            .px_1()
                            .text_color(theme::red())
                            .child("\u{2715}")
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                    this.delete_cookie(i, cx)
                                }),
                            ),
                    ),
            );
        }
        let card = div()
            .flex()
            .flex_col()
            .gap_3()
            .w(px(720.))
            .h(px(440.))
            .p_4()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .occlude()
            .child(header)
            .child(list);
        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x00000099))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.close_cookies(cx)),
            )
            .child(card)
    }

    /// The collection-runner overlay (scrim + results card).
    fn runner_overlay(&self, cx: &mut Context<Self>) -> Div {
        let passed = self.runner_results.iter().filter(|x| x.passed).count();
        let total = self.runner_results.len();
        let status_text = if self.runner_running {
            "running\u{2026}".to_string()
        } else {
            format!("{passed}/{total} passed")
        };
        let status_color = if self.runner_running {
            theme::accent()
        } else if passed == total {
            theme::green()
        } else {
            theme::red()
        };
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .w_full()
            .child(
                div()
                    .text_size(px(15.))
                    .text_color(theme::text())
                    .child(format!("Run: {}", self.runner_title)),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(status_color)
                    .child(status_text),
            )
            .child(ghost_btn("Close").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                    this.runner_open = false;
                    cx.notify();
                }),
            ));
        let mut list = div()
            .id("runner-list")
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1()
            .flex_1()
            .w_full();
        if self.runner_running && self.runner_results.is_empty() {
            list = list.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child("Running requests\u{2026}"),
            );
        }
        for res in &self.runner_results {
            let (mark, c) = if res.passed {
                ("\u{2713}", theme::green())
            } else {
                ("\u{2717}", theme::red())
            };
            let detail = match &res.error {
                Some(e) => e.clone(),
                None => format!("{} \u{00B7} {} ms", res.status, res.ms),
            };
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .w(px(14.))
                            .text_size(px(12.))
                            .text_color(c)
                            .child(mark),
                    )
                    .child(
                        div()
                            .w(px(220.))
                            .text_size(px(12.))
                            .text_color(theme::text())
                            .child(res.name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .font_family("monospace")
                            .text_size(px(12.))
                            .text_color(theme::subtext())
                            .child(detail),
                    ),
            );
        }
        let card = div()
            .flex()
            .flex_col()
            .gap_3()
            .w(px(620.))
            .h(px(460.))
            .p_4()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .occlude()
            .child(header)
            .child(list);
        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x00000099))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                    this.runner_open = false;
                    cx.notify();
                }),
            )
            .child(card)
    }

    /// The environment-manager overlay (scrim + card).
    fn env_overlay(&self, cx: &mut Context<Self>) -> Div {
        let ed = self.env.as_ref().expect("env overlay with env=None");

        // Left: env list with New / per-env duplicate + delete.
        let mut list = div().flex().flex_col().gap_1().w(px(220.)).child(
            div()
                .px_2()
                .py_1()
                .rounded_md()
                .text_size(px(12.))
                .text_color(theme::accent())
                .child("+ New Environment")
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.env_new(cx)),
                ),
        );
        for name in &ed.names {
            let active = ed.selected == *name;
            let (n_sel, n_dup, n_del) = (name.clone(), name.clone(), name.clone());
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_size(px(12.))
                            .when(active, |d| d.bg(theme::surface0()))
                            .text_color(if active {
                                theme::text()
                            } else {
                                theme::subtext()
                            })
                            .child(name.clone())
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                    this.env_select(n_sel.clone(), cx)
                                }),
                            ),
                    )
                    .child(
                        div()
                            .px_1()
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .child("\u{29C9}")
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                    this.env_duplicate(n_dup.clone(), cx)
                                }),
                            ),
                    )
                    .child(
                        div()
                            .px_1()
                            .text_size(px(11.))
                            .text_color(theme::red())
                            .child("\u{2715}")
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                    this.env_delete(n_del.clone(), cx)
                                }),
                            ),
                    ),
            );
        }
        let left = div()
            .id("env-list")
            .overflow_y_scroll()
            .w(px(220.))
            .h_full()
            .child(list);

        // Right: rename + variables table + error + Save.
        let right: Div =
            if ed.selected.is_empty() {
                div().flex_1().flex().items_center().justify_center().child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child("Select or create an environment."),
                )
            } else {
                let rename_row = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(240.))
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(theme::input_bg())
                            .border_1()
                            .border_color(theme::border1())
                            .text_size(px(12.))
                            .font_family("monospace")
                            .child(ed.rename.clone()),
                    )
                    .child(ghost_btn("Rename").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.env_rename_apply(cx)),
                    ));
                let cell = |child: Entity<CodeEditor>| {
                    div()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(theme::input_bg())
                        .border_1()
                        .border_color(theme::border1())
                        .text_size(px(12.))
                        .font_family("monospace")
                        .child(child)
                };
                let mut table = div().flex().flex_col().gap_1();
                for (i, r) in ed.rows.iter().enumerate() {
                    table = table.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(check_box(r.enabled).on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                    this.env_toggle_enabled(i, cx)
                                }),
                            ))
                            .child(cell(r.name.clone()).w(px(180.)))
                            .child(cell(r.value.clone()).flex_1())
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .child(check_box(r.secret).on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                            this.env_toggle_secret(i, cx)
                                        }),
                                    ))
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .text_color(theme::muted())
                                            .child("secret"),
                                    ),
                            )
                            .child(
                                div()
                                    .px_1()
                                    .text_color(theme::red())
                                    .child("\u{2715}")
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                            this.env_remove_row(i, cx)
                                        }),
                                    ),
                            ),
                    );
                }
                table = table.child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::accent())
                        .child("+ Add Variable")
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.env_add_row(cx)),
                        ),
                );
                let mut col = div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .gap_2()
                    .child(rename_row)
                    .child(
                        div()
                            .id("env-table")
                            .overflow_y_scroll()
                            .flex_1()
                            .child(table),
                    );
                if let Some(err) = &ed.error {
                    col = col.child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::red())
                            .child(err.clone()),
                    );
                }
                col.child(div().flex().flex_row().justify_end().child(
                    solid_btn("Save").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.env_save(cx)),
                    ),
                ))
            };

        let card = div()
            .w(px(800.))
            .h(px(480.))
            .p_4()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(15.))
                            .text_color(theme::text())
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("Environments"),
                    )
                    .child({
                        let global = self.env.as_ref().map(|e| e.global).unwrap_or(false);
                        div()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .mr_2()
                            .child(
                                ghost_btn("Collection")
                                    .when(!global, |d| d.text_color(theme::accent()))
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                                            this.env_set_scope(false, cx)
                                        }),
                                    ),
                            )
                            .child(
                                ghost_btn("Global")
                                    .when(global, |d| d.text_color(theme::accent()))
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                                            this.env_set_scope(true, cx)
                                        }),
                                    ),
                            )
                    })
                    .child({
                        let eye = if self.reveal_secrets {
                            "\u{1F441} Hide"
                        } else {
                            "\u{1F441} Reveal"
                        };
                        ghost_btn(eye).on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                                this.toggle_reveal_secrets(cx)
                            }),
                        )
                    })
                    .child(ghost_btn("Close").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.env_close(cx)),
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .flex_1()
                    .child(left)
                    .child(right),
            );

        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x000000aa))
            .flex()
            .items_center()
            .justify_center()
            .child(card)
    }

    /// The Home / welcome screen (open / import + recent collections).
    fn home_screen(&self, cx: &mut Context<Self>) -> Div {
        let mut col = div()
            .flex()
            .flex_col()
            .gap_3()
            .items_center()
            .child(
                div()
                    .text_size(px(28.))
                    .text_color(theme::accent())
                    .child("bruno-rs"),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::subtext())
                    .child("Open or import a collection to begin."),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(solid_btn("Open Collection").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                this.load_collection(dir, cx);
                            }
                        }),
                    ))
                    .child(ghost_btn("Import Postman").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.import_postman(cx)),
                    ))
                    .child(ghost_btn("Import curl").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.open_curl(cx)),
                    )),
            );
        if !self.recent.is_empty() {
            col = col.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child("Recent"),
            );
            let mut list = div().flex().flex_col().gap_1().w(px(460.));
            for p in &self.recent {
                let path = PathBuf::from(p);
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.clone());
                let pc = path.clone();
                list = list.child(
                    div()
                        .flex()
                        .flex_col()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(theme::surface0())
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(theme::text())
                                .child(name),
                        )
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(theme::muted())
                                .child(p.clone()),
                        )
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                this.load_collection(pc.clone(), cx)
                            }),
                        ),
                );
            }
            col = col.child(
                div()
                    .id("home-recent")
                    .overflow_y_scroll()
                    .h(px(240.))
                    .child(list),
            );
        }
        div()
            .flex()
            .flex_1()
            .w_full()
            .items_center()
            .justify_center()
            .child(col)
    }

    /// The strip of open request tabs (click to focus, Ã— to close).
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
            let dirty = self.dirty.contains(&t.path);
            let title = if dirty {
                format!("\u{25CF} {}", t.title())
            } else {
                t.title()
            };
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
                            .text_color(if dirty {
                                theme::accent()
                            } else if active {
                                theme::text()
                            } else {
                                theme::muted()
                            })
                            .child(title)
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
                                    this.request_close_tab(i, cx);
                                }),
                            ),
                    ),
            );
        }
        strip
    }

    fn status_bar(&self, cx: &mut Context<Self>) -> Div {
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
            .child(icon_chip("Search").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, window, cx| {
                    let h = this.search.read(cx).focus_handle(cx);
                    window.focus(&h, cx);
                }),
            ))
            .child(icon_chip("Cookies").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.open_cookies(cx)),
            ))
            .child(icon_chip("Dev Tools").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.toggle_devtools(cx)),
            ))
            .child(
                div()
                    .text_color(theme::muted())
                    .text_size(px(11.))
                    .child("v0.0.0"),
            )
    }
}

impl Render for BruApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let strip = self.tab_strip(cx);
        let content = if self.collection.is_none() || self.home {
            self.home_screen(cx)
        } else if let Some(i) = self.active {
            let tab = &self.tabs[i];
            div()
                .flex()
                .flex_col()
                .flex_1()
                .h_full()
                .child(self.url_bar(tab, cx))
                .child(self.req_subtabs(tab, cx))
                .child(self.req_content(tab, cx))
                .child(self.response_pane(tab, window, cx))
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

        let mut root = div()
            .key_context("BruApp")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_save_action))
            .on_action(cx.listener(Self::on_send_action))
            .on_action(cx.listener(Self::on_escape_action))
            .on_action(cx.listener(Self::on_palette_action))
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::bg())
            .text_color(theme::text())
            .child(self.top_bar(cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .w_full()
                    .child(self.sidebar(cx))
                    .child(center),
            )
            .child(self.status_bar(cx));
        if self.env.is_some() {
            root = root.child(self.env_overlay(cx));
        }
        if self.runner_open {
            root = root.child(self.runner_overlay(cx));
        }
        if self.cookies_open {
            root = root.child(self.cookies_overlay(cx));
        }
        if self.curl_open {
            root = root.child(self.curl_overlay(cx));
        }
        if self.prefs_open {
            root = root.child(self.prefs_overlay(cx));
        }
        if self.devtools_open {
            root = root.child(self.devtools_overlay(cx));
        }
        if self.vault_open {
            root = root.child(self.vault_overlay(cx));
        }
        if self.confirm_delete.is_some() {
            root = root.child(self.delete_overlay(cx));
        }
        if self.confirm_close.is_some() {
            root = root.child(self.close_confirm_overlay(cx));
        }
        if self.rename.is_some() {
            root = root.child(self.rename_overlay(cx));
        }
        if self.env_menu.is_some() {
            root = root.child(self.env_menu_overlay(cx));
        }
        if self.palette_open {
            root = root.child(self.palette_overlay(cx));
        }
        // The context menu sits on top so it overlays everything else.
        if self.ctx_menu.is_some() {
            root = root.child(self.ctx_menu_overlay(cx));
        }
        root
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
        bind_app_keys(cx);
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some("bruno-rs".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| BruApp::new(cx, dir.clone())),
        )
        .unwrap();
        cx.activate(true);
    });
}
