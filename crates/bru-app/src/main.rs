// Phase 3a: real collection data. Loads a Bruno collection (the bundled sample,
// or a path arg), renders a clickable recursive sidebar, and shows the opened
// request's real method/URL/body (JSON bodies tree-sitter-highlighted).
mod actions;
mod chrome;
mod context_menu;
mod cookies;
mod edit;
mod editor;
mod env_ui;
mod envfs;
mod format;
mod fsutil;
mod git;
mod git_ui;
mod highlight;
mod icons;
mod import;
mod jsonpath;
mod menus;
mod overlays;
mod prefs;
mod request_ops;
mod request_panes;
mod response;
mod runner;
mod settings;
mod statusbar;
mod tab;
mod tabs;
mod theme;
mod tree;
mod vars;
mod vault;
mod vault_ui;
mod widgets;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use bru_core::{Block, BlockContent, BruFile, CollectionTree, Folder};
use cookies::{host_of, parse_set_cookie, upsert_cookie};
use edit::{
    auth_block_name, auth_fields, body_block_name, set_text_block, text_block, valid_var_name,
    AUTH_MODES, BODY_MODES,
};
use editor::{CodeEditor, Lang};
use format::{format_outcome, hex_dump, human_size, short_method};
use fsutil::{copy_dir_recursive, reveal_in_file_manager, set_seq_in_file};
use jsonpath::json_path;
use prefs::{bump_recent, globals_root, load_prefs, load_recent, save_prefs, save_recent};
use runner::{run_blocking, run_folder_blocking};
use tree::{collect_folder_requests, find_folder, flatten_requests, folder_matches};
use widgets::{
    check_box, chip, folder_row, ghost_btn, icon_chip, req_row, solid_btn, status_color, svg_chip,
    tab_chip,
};

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
    /// An `auth:<mode>` block edited as a labeled per-field form.
    AuthForm(String),
    /// GraphQL: body:graphql (query) in the main editor, body:graphql:vars in
    /// the secondary editor.
    GraphQl,
    /// The Vars tab: two structured tables (`vars:pre-request` +
    /// `vars:post-response`), edited via `var_pre_rows` / `var_post_rows`.
    Vars,
    /// The whole `.bru` source: reparse it on apply.
    Source,
}

/// One row of the structured params/headers grid.
struct KvRow {
    name: Entity<CodeEditor>,
    value: Entity<CodeEditor>,
    enabled: bool,
}

/// One row of a Vars table (pre-request or post-response). Like [`KvRow`] but
/// carries the `local` (`@`) flag so it round-trips (preserved, not editable â€”
/// matching Bruno, which keeps the flag without a dedicated column).
struct VarRow {
    name: Entity<CodeEditor>,
    value: Entity<CodeEditor>,
    enabled: bool,
    local: bool,
}

/// Which scope provides a template variable's effective value (precedence high→
/// low: Request > Env > Collection > Global > Vault).
#[derive(Clone, Copy, PartialEq)]
enum VarScope {
    Request,
    Env,
    Collection,
    Global,
    Vault,
    /// A `{{$dynamic}}` var (e.g. `$guid`, `$timestamp`) generated per request.
    Dynamic,
}

impl VarScope {
    fn label(self) -> &'static str {
        match self {
            VarScope::Request => "Request",
            VarScope::Env => "Env",
            VarScope::Collection => "Collection",
            VarScope::Global => "Global",
            VarScope::Vault => "Vault",
            VarScope::Dynamic => "Dynamic",
        }
    }
    fn color(self) -> gpui::Hsla {
        match self {
            VarScope::Request => theme::teal(),
            VarScope::Env => theme::green(),
            VarScope::Collection => theme::blue(),
            VarScope::Global => theme::purple(),
            VarScope::Vault => theme::accent(),
            VarScope::Dynamic => theme::cyan(),
        }
    }
}

/// Bruno dynamic variables (`{{$name}}`) the engine generates per request — shown
/// in the popup as Dynamic rather than "unset". Mirrors `bru_core` `dynamic_var`.
const DYNAMIC_VARS: &[&str] = &[
    "$guid",
    "$randomUUID",
    "$timestamp",
    "$isoTimestamp",
    "$randomInt",
];

/// The hover popup for a `{{var}}`: its resolved value + originating scope.
struct VarPopup {
    name: String,
    /// Resolved value (empty when `scope` is None = unset).
    value: String,
    scope: Option<VarScope>,
    /// Secret (vault / secret env var): masked unless reveal-secrets is on.
    secret: bool,
    pos: Point<Pixels>,
}

/// One labeled field of the structured Auth form (e.g. "Username" â†’ `username`).
struct AuthFieldRow {
    label: String,
    key: String,
    editor: Entity<CodeEditor>,
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
    Script,
    PostScript,
    Tests,
    Docs,
    Source,
}

impl ReqTab {
    const ALL: [ReqTab; 11] = [
        ReqTab::Params,
        ReqTab::Body,
        ReqTab::Headers,
        ReqTab::Auth,
        ReqTab::Assert,
        ReqTab::Vars,
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
    /// Right-click menu over an open tab: (tab index, anchor). None = closed.
    tab_menu: Option<(usize, Point<Pixels>)>,
    /// Hover popup for a `{{var}}` (None = closed).
    var_popup: Option<VarPopup>,
    /// Cached non-request variable scopes (vault/global/collection/env), keyed by
    /// name. Rebuilt by `refresh_vars` on collection/env/vault changes; request
    /// vars are resolved live from the active tab.
    var_scopes: HashMap<String, (VarScope, String, bool)>,
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
    /// "Close All" confirmation when one or more tabs have unsaved edits.
    confirm_close_all: bool,
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
    /// Request-pane width (px) for the request|response split; None = even 50/50.
    split_req_w: Option<f32>,
    /// True while the split divider is being dragged.
    split_dragging: bool,
    /// Sidebar width (px) and whether its divider is being dragged.
    sidebar_w: f32,
    sidebar_dragging: bool,
    /// Method-picker dropdown anchored at this point (None = closed).
    method_menu: Option<Point<Pixels>>,
    /// Body/auth mode dropdown: (anchor, is_body). None = closed.
    mode_menu: Option<(Point<Pixels>, bool)>,
    /// Whether the collection dir is a git work tree (git present + inside repo).
    /// Drives the status-bar chip independently of `git_status`, so a transient
    /// `git status` failure doesn't hide the only entry point to the git overlay.
    git_repo: bool,
    /// Parsed git status (None = not a repo, or status couldn't be read yet).
    git_status: Option<git::Status>,
    /// Whether the git overlay (commit/push/pull panel) is open.
    git_open: bool,
    /// True while a git operation is running on a worker thread.
    git_busy: bool,
    /// The last git operation's result or error message.
    git_output: String,
    /// Commit-message input for the git overlay.
    git_msg: Entity<CodeEditor>,
    /// Arms the destructive "Discard all" button (two-click confirm).
    git_confirm_discard: bool,
}

/// A right-click menu over a sidebar entry, anchored at the click point.
struct CtxMenu {
    target: PathBuf,
    is_dir: bool,
    /// The collection root (top of the sidebar): a reduced item set, no
    /// rename/clone/delete.
    root: bool,
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
    /// Vars-tab row editors: pre-request and post-response tables.
    var_pre_rows: Vec<VarRow>,
    var_post_rows: Vec<VarRow>,
    /// Field editors for the structured Auth form (when on the Auth tab).
    auth_rows: Vec<AuthFieldRow>,
    /// URL-derived path params (name, value editor) shown on the Params tab.
    path_rows: Vec<(String, Entity<CodeEditor>)>,
    sending: bool,
    /// The last response (full outcome, for the response sub-tabs).
    response: Option<bru_engine::RunOutcome>,
}

/// Build structured grid rows (name/value single-line editors + enabled) from a
/// Dict block, for the params/headers tabs. Each editor is subscribed so edits
/// mark the tab dirty (and re-render) rather than being silently lost on close.
fn build_kv_rows(file: &BruFile, block: &str, path: &Path, cx: &mut Context<BruApp>) -> Vec<KvRow> {
    edit::kv_block_rows(file, block)
        .into_iter()
        .map(|(k, v, enabled)| {
            let name = cx.new(|cx| CodeEditor::single_line(cx, &k));
            let value = cx.new(|cx| CodeEditor::single_line(cx, &v));
            subscribe_grid_editor(&name, path.to_path_buf(), cx);
            subscribe_grid_editor(&value, path.to_path_buf(), cx);
            KvRow {
                name,
                value,
                enabled,
            }
        })
        .collect()
}

/// Subscribe a structured-grid cell editor (Vars / Params / Headers / â€¦) so a
/// keystroke marks the tab dirty and re-renders the parent â€” otherwise cell
/// edits are silently lost on close, and live name validation can't update.
fn subscribe_grid_editor(ed: &Entity<CodeEditor>, path: PathBuf, cx: &mut Context<BruApp>) {
    subscribe_hover(ed, cx);
    cx.subscribe(ed, move |this, _ed, _ev: &editor::Changed, cx| {
        this.dirty.insert(path.clone());
        cx.notify();
    })
    .detach();
}

/// Subscribe an editor so hovering a `{{var}}` in it shows the value popup.
fn subscribe_hover(ed: &Entity<CodeEditor>, cx: &mut Context<BruApp>) {
    cx.subscribe(ed, |this, _ed, ev: &editor::HoverVar, cx| {
        this.on_hover_var(ev, cx)
    })
    .detach();
}

/// Collection-level `vars:pre-request` (enabled), read fresh from `collection.bru`.
fn collection_vars(dir: &Path) -> Vec<(String, String)> {
    let Ok(text) = std::fs::read_to_string(dir.join("collection.bru")) else {
        return Vec::new();
    };
    let Ok(file) = bru_lang::parse(&text) else {
        return Vec::new();
    };
    edit::var_block_rows(&file, "vars:pre-request")
        .into_iter()
        .filter(|(_, _, enabled, _)| *enabled)
        .map(|(k, v, _, _)| (k, v))
        .collect()
}

/// Build Vars-table rows from a vars Dict block, carrying the `local` flag. Each
/// editor is subscribed so edits mark the tab dirty + drive live validation.
fn build_var_rows(
    file: &BruFile,
    block: &str,
    path: &Path,
    cx: &mut Context<BruApp>,
) -> Vec<VarRow> {
    edit::var_block_rows(file, block)
        .into_iter()
        .map(|(k, v, enabled, local)| {
            let name = cx.new(|cx| CodeEditor::single_line(cx, &k));
            let value = cx.new(|cx| CodeEditor::single_line(cx, &v));
            subscribe_grid_editor(&name, path.to_path_buf(), cx);
            subscribe_grid_editor(&value, path.to_path_buf(), cx);
            VarRow {
                name,
                value,
                enabled,
                local,
            }
        })
        .collect()
}

/// Read Vars-table row editors back into `(name, value, enabled, local)` tuples.
fn collect_var_rows(rows: &[VarRow], cx: &Context<BruApp>) -> Vec<(String, String, bool, bool)> {
    rows.iter()
        .map(|r| {
            (
                r.name.read(cx).text().trim().to_string(),
                r.value.read(cx).text().to_string(),
                r.enabled,
                r.local,
            )
        })
        .collect()
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

/// One request's outcome in a runner batch.
#[derive(Clone)]
struct RunResult {
    name: String,
    passed: bool,
    status: u16,
    ms: u128,
    error: Option<String>,
}

/// Build a `curl` command for the request ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Â method, URL, headers, and body ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Â
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
        let git_msg = cx.new(|cx| CodeEditor::single_line(cx, ""));
        let mut app = Self {
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
            tab_menu: None,
            var_popup: None,
            var_scopes: HashMap::new(),
            rename: None,
            confirm_delete: None,
            selected_env: None,
            selected_global_env: None,
            env_menu: None,
            collapsed: HashSet::new(),
            clipboard_item: None,
            dirty: HashSet::new(),
            confirm_close: None,
            confirm_close_all: false,
            resp_filter,
            resp_filter_query: String::new(),
            resp_raw: false,
            resp_hex: false,
            resp_editor: cx.new(|cx| CodeEditor::read_only(cx, "")),
            focus_handle: cx.focus_handle(),
            palette_open: false,
            palette_input,
            palette_query: String::new(),
            split_req_w: None,
            split_dragging: false,
            sidebar_w: 280.0,
            sidebar_dragging: false,
            method_menu: None,
            mode_menu: None,
            git_repo: false,
            git_status: None,
            git_open: false,
            git_busy: false,
            git_output: String::new(),
            git_msg,
            git_confirm_discard: false,
        };
        app.refresh_git_status(cx);
        app.refresh_vars();
        app
    }
}

impl Render for BruApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let strip = self.tab_strip(cx);
        let content = if self.collection.is_none() || self.home {
            self.home_screen(cx)
        } else if let Some(i) = self.active {
            let tab = &self.tabs[i];
            // The URL bar spans the full width; below it the request pane (left)
            // and response pane (right) share a horizontal split â€” Bruno's layout.
            // Request pane: fixed width once the split has been dragged, else
            // an even half. The divider between the panes is draggable.
            let req_pane = {
                let p = div()
                    .flex()
                    .flex_col()
                    .min_w_0()
                    .min_h_0()
                    .child(self.req_subtabs(tab, cx))
                    .child(self.req_content(tab, cx));
                match self.split_req_w {
                    Some(w) => p.w(px(w)),
                    None => p.flex_1(),
                }
            };
            let divider = div()
                .w(px(6.))
                .h_full()
                .bg(if self.split_dragging {
                    theme::accent()
                } else {
                    theme::border1()
                })
                .hover(|s| s.bg(theme::accent()))
                .cursor(gpui::CursorStyle::ResizeLeftRight)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseDownEvent, _w, cx| {
                        this.split_dragging = true;
                        cx.notify();
                    }),
                );
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(self.url_bar(tab, cx))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .child(req_pane)
                        .child(divider)
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .flex_1()
                                .min_w_0()
                                .min_h_0()
                                .child(self.response_pane(tab, window, cx)),
                        ),
                )
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
            .child(self.collection_header(cx))
            .child(strip)
            .child(content);

        let mut root = div()
            .key_context("BruApp")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::on_save_action))
            .on_action(cx.listener(Self::on_send_action))
            .on_action(cx.listener(Self::on_escape_action))
            .on_action(cx.listener(Self::on_palette_action))
            .on_mouse_move(cx.listener(|this, ev: &gpui::MouseMoveEvent, window, cx| {
                let x = f32::from(ev.position.x);
                if this.sidebar_dragging {
                    this.sidebar_w = x.clamp(180.0, 480.0);
                    cx.notify();
                } else if this.split_dragging {
                    let total = f32::from(window.viewport_size().width);
                    // The split row begins after the sidebar + its 6px divider.
                    let left = this.sidebar_w + 6.0;
                    let max = (total - left - 260.0).max(240.0);
                    this.split_req_w = Some((x - left).clamp(240.0, max));
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                    if this.split_dragging || this.sidebar_dragging {
                        this.split_dragging = false;
                        this.sidebar_dragging = false;
                        cx.notify();
                    }
                    // Any click anywhere dismisses the var popup. The popup's own
                    // Copy button (bubbles here too) runs its handler first, so a
                    // copy still completes before this clear.
                    if this.var_popup.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme::bg())
            .text_color(theme::text())
            .font_family("Inter")
            .text_size(px(13.))
            .child(self.top_bar(cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(self.sidebar(cx))
                    .child(
                        div()
                            .w(px(6.))
                            .h_full()
                            .bg(if self.sidebar_dragging {
                                theme::accent()
                            } else {
                                theme::border1()
                            })
                            .hover(|s| s.bg(theme::accent()))
                            .cursor(gpui::CursorStyle::ResizeLeftRight)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _e: &MouseDownEvent, _w, cx| {
                                    this.sidebar_dragging = true;
                                    cx.notify();
                                }),
                            ),
                    )
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
        if self.git_open {
            root = root.child(self.git_overlay(cx));
        }
        if self.confirm_delete.is_some() {
            root = root.child(self.delete_overlay(cx));
        }
        if self.confirm_close.is_some() {
            root = root.child(self.close_confirm_overlay(cx));
        }
        if self.confirm_close_all {
            root = root.child(self.close_all_overlay(cx));
        }
        if self.rename.is_some() {
            root = root.child(self.rename_overlay(cx));
        }
        if self.env_menu.is_some() {
            root = root.child(self.env_menu_overlay(cx));
        }
        if self.method_menu.is_some() {
            root = root.child(self.method_menu_overlay(cx));
        }
        if self.mode_menu.is_some() {
            root = root.child(self.mode_menu_overlay(cx));
        }
        if self.palette_open {
            root = root.child(self.palette_overlay(cx));
        }
        // The context menus sit on top so they overlay everything else.
        if self.ctx_menu.is_some() {
            root = root.child(self.ctx_menu_overlay(cx));
        }
        if self.tab_menu.is_some() {
            root = root.child(self.tab_menu_overlay(cx));
        }
        if self.var_popup.is_some() {
            root = root.child(self.var_popup_overlay(window, cx));
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

    application()
        .with_assets(icons::Assets)
        .run(move |cx: &mut App| {
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
