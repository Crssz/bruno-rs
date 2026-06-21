// Phase 3a: real collection data. Loads a Bruno collection (the bundled sample,
// or a path arg), renders a clickable recursive sidebar, and shows the opened
// request's real method/URL/body (JSON bodies tree-sitter-highlighted).
mod actions;
mod chrome;
mod context_menu;
mod cookies;
mod edit;
mod editor;
mod editor_menu;
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
#[derive(Clone, Copy, PartialEq, Debug)]
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

/// Right-click edit menu over a code editor (Cut/Copy/Paste/Select All),
/// anchored at `pos` and acting back on `editor`.
struct EditorMenuState {
    editor: Entity<CodeEditor>,
    pos: Point<Pixels>,
    read_only: bool,
    has_selection: bool,
    formattable: bool,
    /// A `{{var}}` or `require(...)` under the click, for the menu's "Go to" item.
    goto: Option<editor::GotoTarget>,
}

/// One labeled field of the structured Auth form (e.g. "Username" â†’ `username`).
struct AuthFieldRow {
    label: String,
    key: String,
    editor: Entity<CodeEditor>,
}

/// Request sub-tabs (Body is the editable editor; the rest show parsed data).
#[derive(Clone, Copy, PartialEq, Debug)]
enum ReqTab {
    Params,
    Body,
    Headers,
    Auth,
    Assert,
    Vars,
    Script,
    Tests,
    Docs,
    Source,
}

impl ReqTab {
    const ALL: [ReqTab; 10] = [
        ReqTab::Params,
        ReqTab::Body,
        ReqTab::Headers,
        ReqTab::Auth,
        ReqTab::Assert,
        ReqTab::Vars,
        ReqTab::Script,
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
            ReqTab::Tests => "Tests",
            ReqTab::Docs => "Docs",
            ReqTab::Source => "Source",
        }
    }
}
use gpui::{
    actions, div, prelude::*, px, size, App, Bounds, Context, Div, Entity, FocusHandle, Focusable,
    KeyBinding, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, ScrollHandle, Window,
    WindowBounds, WindowOptions,
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
    /// Developer Mode: let scripts `require()` local `.js`/`.json` files. Off
    /// (Safe Mode) by default, matching Bruno's sandboxed-scripts default.
    pref_developer: bool,
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
    /// True while the pointer is over the var popup card. Keeps a scheduled
    /// dismissal (fired when the pointer leaves the `{{var}}`) from closing the
    /// popup before a Copy click lands.
    var_popup_hovered: bool,
    /// Bumped whenever the popup opens or its hover state changes, so an in-flight
    /// delayed dismiss only fires for the latest generation (stale ones no-op).
    var_popup_gen: u64,
    /// Right-click edit menu over a code editor (None = closed).
    editor_menu: Option<EditorMenuState>,
    /// The editor whose in-editor find bar is currently open (None = none), so
    /// Escape can route to it and `close_topmost_overlay` knows to close it first.
    find_editor: Option<Entity<CodeEditor>>,
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
    /// Scroll handle for the response-body viewport, so the find bar can scroll a
    /// match into view.
    resp_scroll: ScrollHandle,
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
#[derive(Clone, Copy, PartialEq, Debug)]
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
    /// Script sub-tab: false = Pre Request, true = Post Response. Both scripts
    /// live under the single "Script" top-level tab (matching Bruno).
    script_post: bool,
    /// The last response (full outcome, for the response sub-tabs).
    response: Option<bru_engine::RunOutcome>,
    /// When `Some`, this is a plain-text file tab (e.g. a `require`d `.js`), not a
    /// `.bru` request: the request UI is bypassed and this single editor fills the
    /// pane. `file` is then an empty placeholder.
    text: Option<Entity<CodeEditor>>,
    /// Scroll position of the text-file editor, so "Go to Implementation" can
    /// scroll a jumped-to symbol into view.
    text_scroll: ScrollHandle,
    /// Scroll position of the shared body/script editor, so local "Go to
    /// Definition" can scroll a jumped-to symbol into view.
    body_scroll: ScrollHandle,
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

/// Track which editor's find bar is open (so Escape routes to it and the parent
/// re-renders when the bar opens/closes or its match set changes — the find bar
/// is a parent-placed, non-scrolling row above the editor's viewport).
fn subscribe_find(ed: &Entity<CodeEditor>, cx: &mut Context<BruApp>) {
    cx.subscribe(ed, |this, ed, ev: &editor::FindChanged, cx| {
        if ev.open {
            this.find_editor = Some(ed.clone());
        } else if this
            .find_editor
            .as_ref()
            .is_some_and(|e| e.entity_id() == ed.entity_id())
        {
            this.find_editor = None;
        }
        cx.notify();
    })
    .detach();
}

/// Subscribe an editor for its `{{var}}` hover popup and its right-click edit
/// menu (both are parent-rendered overlays anchored at the click point).
fn subscribe_hover(ed: &Entity<CodeEditor>, cx: &mut Context<BruApp>) {
    cx.subscribe(ed, |this, _ed, ev: &editor::HoverVar, cx| {
        this.on_hover_var(ev, cx)
    })
    .detach();
    cx.subscribe(ed, |this, _ed, ev: &editor::EditorMenu, cx| {
        this.editor_menu = Some(EditorMenuState {
            editor: ev.editor.clone(),
            pos: ev.pos,
            read_only: ev.read_only,
            has_selection: ev.has_selection,
            formattable: ev.formattable,
            goto: ev.goto.clone(),
        });
        cx.notify();
    })
    .detach();
    cx.subscribe(ed, |this, _ed, ev: &editor::GotoDefinition, cx| {
        this.on_goto_definition(&ev.target, cx)
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
        let (pref_timeout, pref_insecure, light, pref_developer) = load_prefs();
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
        // Response-body viewer: read-only, with a find bar (Ctrl+F) that scrolls
        // matches into view via `resp_scroll`.
        let resp_scroll = ScrollHandle::new();
        let resp_editor = cx.new(|cx| CodeEditor::read_only(cx, ""));
        resp_editor.update(cx, |ed, _| ed.set_find_scroll(resp_scroll.clone()));
        subscribe_find(&resp_editor, cx);
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
            pref_developer,
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
            var_popup_hovered: false,
            var_popup_gen: 0,
            editor_menu: None,
            find_editor: None,
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
            resp_editor,
            resp_scroll,
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
        } else if let Some(i) = self.active.filter(|&i| self.tabs[i].text.is_some()) {
            // A plain-text file tab (e.g. a `require`d `.js`): one full-pane editor,
            // no URL bar / sub-tabs / response pane.
            self.text_pane(&self.tabs[i], cx)
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
        if self.editor_menu.is_some() {
            root = root.child(self.editor_menu_overlay(cx));
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

/// Shared headless-test harness: construct a real `BruApp` on a throwaway copy
/// of the bundled sample collection via `gpui::TestAppContext`, so every module's
/// tests build the app identically. `BruApp::new` is private to this module, so
/// the constructor lives here and is exposed `pub(crate)` for sibling tests.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use gpui::{AppContext, Entity, TestAppContext};
    use std::sync::atomic::{AtomicU32, Ordering};

    /// The bundled sample collection (read-only — never mutate/save against it;
    /// use [`temp_collection`] when a test writes to disk).
    pub(crate) fn sample_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("sample")
    }

    /// A temp directory removed on drop.
    pub(crate) struct TempCollection {
        pub(crate) dir: PathBuf,
    }
    impl Drop for TempCollection {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// A fresh temp copy of the sample collection, for tests that mutate or save.
    pub(crate) fn temp_collection() -> TempCollection {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("bru-app-test-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::fsutil::copy_dir_recursive(&sample_dir(), &dir).unwrap();
        TempCollection { dir }
    }

    /// Construct a BruApp on `dir`, draining the startup git-status spawn so the
    /// app is in a settled state.
    pub(crate) fn build_app(cx: &mut TestAppContext, dir: PathBuf) -> Entity<BruApp> {
        let app = cx.new(|cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        app
    }

    /// Construct a BruApp on a throwaway copy of the sample collection. The
    /// returned `TempCollection` must be kept alive for the test's duration.
    pub(crate) fn app_on_temp(cx: &mut TestAppContext) -> (Entity<BruApp>, TempCollection) {
        let tc = temp_collection();
        let app = build_app(cx, tc.dir.clone());
        (app, tc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::app_on_temp;

    #[gpui::test]
    fn bruapp_constructs_opens_request_and_switches_tabs(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        app.update(cx, |app, _| assert!(app.collection.is_some()));
        let req = tc.dir.join("Repository Info.bru");
        app.update(cx, |app, cx| app.open_request(req.clone(), cx));
        app.update(cx, |app, cx| {
            assert_eq!(app.tabs.len(), 1);
            assert_eq!(app.active, Some(0));
            app.tabs[0].switch_tab(ReqTab::Headers, cx);
            assert_eq!(app.tabs[0].req_tab, ReqTab::Headers);
        });
    }

    /// Drawing the app in a test window exercises the whole `render` view tree
    /// (top bar, sidebar, status bar, request/response panes) headlessly.
    #[gpui::test]
    fn renders_full_app_in_window(cx: &mut gpui::TestAppContext) {
        let tc = crate::test_support::temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_window, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, _window, cx| {
                app.open_request(tc.dir.join("Repository Info.bru"), cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _window, _cx| assert_eq!(app.tabs.len(), 1))
            .unwrap();
    }
}

// Windowed render coverage for the git overlay (statusbar::git_overlay, driven
// by git_ui::open_git). Lives in main.rs because `BruApp::new` is private here.
#[cfg(test)]
mod git_ui_render_cov_tests {
    use super::*;

    // Render the git overlay in a real window: open_git flips git_open so the
    // render tree builds git_overlay. Seed a status with staged / unstaged /
    // untracked entries to drive the per-file color branches and the
    // ahead/behind summary.
    #[gpui::test]
    fn renders_git_overlay_with_status(cx: &mut gpui::TestAppContext) {
        let tc = crate::test_support::temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_window, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, _window, cx| {
                app.git_status = Some(git::Status {
                    branch: "main".into(),
                    ahead: 2,
                    behind: 1,
                    files: vec![
                        git::FileEntry {
                            code: " M".into(),
                            path: "src/a.rs".into(),
                        },
                        git::FileEntry {
                            code: "A ".into(),
                            path: "src/b.rs".into(),
                        },
                        git::FileEntry {
                            code: "??".into(),
                            path: "new.txt".into(),
                        },
                    ],
                });
                app.open_git(cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _window, _cx| assert!(app.git_open))
            .unwrap();
    }

    // Render the git overlay with a clean tree (status present, no files): hits
    // the "Working tree clean." empty-list branch.
    #[gpui::test]
    fn renders_git_overlay_clean_tree(cx: &mut gpui::TestAppContext) {
        let tc = crate::test_support::temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_window, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, _window, cx| {
                app.git_status = Some(git::Status::default());
                app.open_git(cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _window, _cx| assert!(app.git_open))
            .unwrap();
    }

    // Render the git overlay when status couldn't be read (git_status = None)
    // and the discard action is armed: exercises the "Could not read git
    // status" / "Status unavailable" branch plus the "Confirm discard" label
    // and the busy output-color branch.
    #[gpui::test]
    fn renders_git_overlay_without_status_confirm_armed(cx: &mut gpui::TestAppContext) {
        let tc = crate::test_support::temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_window, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, _window, cx| {
                app.git_status = None;
                app.open_git(cx);
            })
            .unwrap();
        cx.run_until_parked();
        // open_git resets git_confirm_discard, so arm it after opening and
        // re-park to rebuild the overlay with the "Confirm discard" label.
        window
            .update(cx, |app, _window, cx| {
                app.git_confirm_discard = true;
                app.git_busy = true;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _window, _cx| {
                assert!(app.git_open);
                assert!(app.git_confirm_discard);
            })
            .unwrap();
    }
}

#[cfg(test)]
mod cov_tests_response {
    use super::*;
    use crate::test_support::temp_collection;

    // A full JSON HttpResponse (content-type json) with headers + body so the
    // pretty-print / JSONPath / hex / raw branches all have something to chew on.
    fn json_http() -> bru_http::HttpResponse {
        bru_http::HttpResponse {
            status: 200,
            status_text: "OK".into(),
            headers: vec![
                ("content-type".into(), "application/json".into()),
                ("x-test".into(), "v".into()),
            ],
            body: br#"{"name":"hi","n":1}"#.to_vec(),
            duration_ms: 12,
        }
    }

    // A non-JSON response so the Response tab takes the plain/raw branch.
    fn text_http() -> bru_http::HttpResponse {
        bru_http::HttpResponse {
            status: 404,
            status_text: "Not Found".into(),
            headers: vec![("content-type".into(), "text/plain".into())],
            body: b"hello world".to_vec(),
            duration_ms: 3,
        }
    }

    // A RunOutcome carrying the given response, plus one passing assertion and
    // one failing test (drives the Tests tab + the header/test count badges).
    fn outcome_with(resp: Option<bru_http::HttpResponse>) -> bru_engine::RunOutcome {
        bru_engine::RunOutcome {
            name: "t".into(),
            method: "GET".into(),
            url: "https://example.test/x".into(),
            response: resp,
            assertions: vec![bru_core::AssertOutcome {
                expr: "res.status".into(),
                operator: "eq".into(),
                expected: "200".into(),
                actual: "200".into(),
                passed: true,
            }],
            tests: vec![bru_engine::TestResult {
                name: "is ok".into(),
                passed: false,
                error: Some("boom".into()),
            }],
            console: Vec::new(),
            vars_set: Vec::new(),
            error: None,
        }
    }

    // Open a window with one request tab; returns the window for further driving.
    fn windowed(
        cx: &mut gpui::TestAppContext,
    ) -> (
        gpui::WindowHandle<BruApp>,
        crate::test_support::TempCollection,
    ) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_window, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, _window, cx| {
                app.open_request(tc.dir.join("Repository Info.bru"), cx);
            })
            .unwrap();
        cx.run_until_parked();
        (window, tc)
    }

    // Set per-tab + per-app response state, then re-park so render() (and thus
    // response_pane) runs against the new state.
    fn drive(
        cx: &mut gpui::TestAppContext,
        window: &gpui::WindowHandle<BruApp>,
        resp_tab: RespTab,
        outcome: bru_engine::RunOutcome,
        raw: bool,
        hex: bool,
        filter: &str,
    ) {
        let filter = filter.to_string();
        window
            .update(cx, |app, _window, cx| {
                app.resp_raw = raw;
                app.resp_hex = hex;
                app.resp_filter_query = filter;
                if let Some(i) = app.active {
                    app.tabs[i].resp_tab = resp_tab;
                    app.tabs[i].response = Some(outcome);
                }
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
    }

    // RespTab::Response, JSON content-type, no filter -> pretty-printed JSON path.
    #[gpui::test]
    fn renders_response_pretty_json(cx: &mut gpui::TestAppContext) {
        let (window, _tc) = windowed(cx);
        drive(
            cx,
            &window,
            RespTab::Response,
            outcome_with(Some(json_http())),
            false,
            false,
            "",
        );
        window
            .update(cx, |app, _window, _cx| {
                assert!(app.response_bytes().is_some());
            })
            .unwrap();
    }

    // JSONPath filter that matches -> the `Some(val)` pretty branch.
    #[gpui::test]
    fn renders_response_jsonpath_match(cx: &mut gpui::TestAppContext) {
        let (window, _tc) = windowed(cx);
        drive(
            cx,
            &window,
            RespTab::Response,
            outcome_with(Some(json_http())),
            false,
            false,
            "$.name",
        );
    }

    // JSONPath filter that matches nothing -> the `(no match)` branch.
    #[gpui::test]
    fn renders_response_jsonpath_no_match(cx: &mut gpui::TestAppContext) {
        let (window, _tc) = windowed(cx);
        drive(
            cx,
            &window,
            RespTab::Response,
            outcome_with(Some(json_http())),
            false,
            false,
            "$.nope",
        );
    }

    // resp_raw == true -> the raw (un-prettified) branch even for JSON bodies.
    #[gpui::test]
    fn renders_response_raw(cx: &mut gpui::TestAppContext) {
        let (window, _tc) = windowed(cx);
        drive(
            cx,
            &window,
            RespTab::Response,
            outcome_with(Some(json_http())),
            true,
            false,
            "",
        );
    }

    // resp_hex == true -> hex_dump branch.
    #[gpui::test]
    fn renders_response_hex(cx: &mut gpui::TestAppContext) {
        let (window, _tc) = windowed(cx);
        drive(
            cx,
            &window,
            RespTab::Response,
            outcome_with(Some(json_http())),
            false,
            true,
            "",
        );
    }

    // A non-JSON content-type -> the plain (raw) branch on the Response tab.
    #[gpui::test]
    fn renders_response_plain_text(cx: &mut gpui::TestAppContext) {
        let (window, _tc) = windowed(cx);
        drive(
            cx,
            &window,
            RespTab::Response,
            outcome_with(Some(text_http())),
            false,
            false,
            "",
        );
    }

    // An outcome with NO HttpResponse -> the `None => format_outcome(o)` branch
    // on the Response tab, and the "(no response)" Headers branch.
    #[gpui::test]
    fn renders_response_format_outcome_when_no_http(cx: &mut gpui::TestAppContext) {
        let (window, _tc) = windowed(cx);
        drive(
            cx,
            &window,
            RespTab::Response,
            outcome_with(None),
            false,
            false,
            "",
        );
        // Now flip to Headers with the same no-response outcome to hit the
        // "(no response)" placeholder branch.
        drive(
            cx,
            &window,
            RespTab::Headers,
            outcome_with(None),
            false,
            false,
            "",
        );
    }

    // Headers tab with a real HttpResponse -> the row-per-header loop (incl. the
    // odd/even striping) and the "Headers (n)" count badge in the strip.
    #[gpui::test]
    fn renders_headers_with_rows(cx: &mut gpui::TestAppContext) {
        let (window, _tc) = windowed(cx);
        drive(
            cx,
            &window,
            RespTab::Headers,
            outcome_with(Some(json_http())),
            false,
            false,
            "",
        );
    }

    // Timeline tab -> request line + request-header lines (via dict_to_lines on
    // the opened request's file) + response status/headers + timing/size.
    #[gpui::test]
    fn renders_timeline(cx: &mut gpui::TestAppContext) {
        let (window, _tc) = windowed(cx);
        drive(
            cx,
            &window,
            RespTab::Timeline,
            outcome_with(Some(json_http())),
            false,
            false,
            "",
        );
    }

    // Tests tab -> one passing assertion (check mark) + one failing test (cross),
    // plus the "Tests passed/total" badge in the strip.
    #[gpui::test]
    fn renders_tests_tab(cx: &mut gpui::TestAppContext) {
        let (window, _tc) = windowed(cx);
        drive(
            cx,
            &window,
            RespTab::Tests,
            outcome_with(Some(json_http())),
            false,
            false,
            "",
        );
    }

    // Tests tab with an outcome that has NO assertions/tests -> the
    // "No assertions or tests." empty branch, and the plain "Tests" label.
    #[gpui::test]
    fn renders_tests_tab_empty(cx: &mut gpui::TestAppContext) {
        let (window, _tc) = windowed(cx);
        let mut o = outcome_with(Some(json_http()));
        o.assertions.clear();
        o.tests.clear();
        drive(cx, &window, RespTab::Tests, o, false, false, "");
    }

    // With a real response set, response_bytes returns the body, copy_response
    // copies it (setting the status), and clear_response then wipes it.
    #[gpui::test]
    fn response_bytes_copy_and_clear_with_data(cx: &mut gpui::TestAppContext) {
        let (window, _tc) = windowed(cx);
        window
            .update(cx, |app, _window, _cx| {
                if let Some(i) = app.active {
                    app.tabs[i].response = Some(outcome_with(Some(json_http())));
                }
            })
            .unwrap();
        window
            .update(cx, |app, _window, _cx| {
                let bytes = app.response_bytes().expect("body present");
                assert!(bytes == br#"{"name":"hi","n":1}"#.to_vec());
            })
            .unwrap();
        window
            .update(cx, |app, _window, cx| {
                app.copy_response(cx);
                assert!(app.status == "Copied response to clipboard");
            })
            .unwrap();
        window
            .update(cx, |app, _window, cx| {
                app.clear_response(cx);
                assert!(app.response_bytes().is_none());
            })
            .unwrap();
    }
}

#[cfg(test)]
mod chrome_cov_tests {
    //! Coverage for `chrome.rs`'s view builders (top_bar / collection_header /
    //! sidebar / push_folder / url_bar). Lives in `main.rs` so it can drive the
    //! private `BruApp`/`OpenTab` fields the chrome branches key off of
    //! (`search_query`, `collapsed`, `home`, `dirty`, the open tab's `method`).
    //! Each test renders in a headless window (template 3) so the builder methods
    //! actually run; a second `run_until_parked` re-draws after the state change.
    use super::*;
    use crate::test_support::temp_collection;

    /// Path of the bundled `Repository` sub-folder inside the loaded tree (so we
    /// drive the real path the sidebar stores in `collapsed`, not a guessed one).
    fn repo_folder_path(app: &BruApp) -> std::path::PathBuf {
        app.collection
            .as_ref()
            .and_then(|c| c.root.folders.iter().find(|f| f.name == "Repository"))
            .map(|f| f.path.clone())
            .expect("sample collection has a Repository folder")
    }

    /// Default render with a loaded collection: exercises `top_bar` (collection
    /// name present), `collection_header` (full toolbar, no env -> muted pill),
    /// and `sidebar`/`push_folder` over the real tree.
    #[gpui::test]
    fn renders_chrome_with_collection(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, _w, _cx| {
                assert!(app.collection.is_some());
                // top_bar reads the collection name for the switcher label.
                let name = app.collection.as_ref().map(|c| c.name.clone()).unwrap();
                assert!(!name.is_empty());
            })
            .unwrap();
    }

    /// `top_bar` / `sidebar` "no collection" branches: with no collection the
    /// switcher shows "No collection" and the sidebar shows the empty placeholder
    /// row. `home_screen` also takes over the content area. Force `collection`
    /// to `None` after construction and re-park so render takes those branches.
    #[gpui::test]
    fn renders_chrome_without_collection(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, _w, cx| {
                app.collection = None;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked(); // render: top_bar/sidebar "no collection" branches
        window
            .update(cx, |app, _w, _cx| {
                assert!(app.collection.is_none());
            })
            .unwrap();
    }

    /// `collection_header` early-return branch: when `home` is set the header
    /// renders an empty `div()` and `home_screen` fills the content.
    #[gpui::test]
    fn collection_header_home_returns_empty(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, _w, cx| {
                app.go_home(cx); // home = true
                assert!(app.home);
            })
            .unwrap();
        cx.run_until_parked(); // re-render -> header early-returns, home_screen runs
        window
            .update(cx, |app, _w, cx| {
                app.go_home(cx); // toggle back off
                assert!(!app.home);
            })
            .unwrap();
        cx.run_until_parked();
    }

    /// `collection_header`'s env pill in its "has environment" state (green dot +
    /// text color), driven via the pub(crate) `select_env`.
    #[gpui::test]
    fn collection_header_with_selected_env(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, _w, cx| {
                app.select_env(Some("New Environment".to_string()), cx);
                assert_eq!(app.selected_env.as_deref(), Some("New Environment"));
            })
            .unwrap();
        cx.run_until_parked(); // env_has = true path in collection_header
        window
            .update(cx, |app, _w, cx| {
                app.select_env(None, cx); // back to "No Environment" / muted
                assert!(app.selected_env.is_none());
            })
            .unwrap();
        cx.run_until_parked();
    }

    /// `sidebar`/`push_folder` with an active search query: forces every branch
    /// open and filters folders + requests by name (`folder_matches` / the
    /// request-name `contains` check).
    #[gpui::test]
    fn sidebar_search_filters_rows(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        // A query that matches the Repository folder + its request, hiding others.
        window
            .update(cx, |app, _w, _cx| {
                app.search_query = "repository".to_string();
            })
            .unwrap();
        cx.run_until_parked();
        // A query that matches nothing: every folder/request row is skipped.
        window
            .update(cx, |app, _w, _cx| {
                app.search_query = "zzz-no-such-request".to_string();
            })
            .unwrap();
        cx.run_until_parked();
        // Clear the query: back to the full tree (collapsed honored again).
        window
            .update(cx, |app, _w, _cx| {
                app.search_query.clear();
                assert!(app.search_query.is_empty());
            })
            .unwrap();
        cx.run_until_parked();
    }

    /// `push_folder`'s collapsed branch: a collapsed folder hides its children
    /// (no recursion) and renders a chevron-right row. Toggling re-expands it.
    #[gpui::test]
    fn sidebar_collapsed_folder(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        let repo = window
            .update(cx, |app, _w, _cx| repo_folder_path(app))
            .unwrap();
        // Collapse via the real sidebar handler.
        window
            .update(cx, |app, _w, cx| {
                app.toggle_folder(repo.clone(), cx);
                assert!(app.collapsed.contains(&repo));
            })
            .unwrap();
        cx.run_until_parked(); // render: collapsed == true branch, no recursion
                               // Expand again (toggle removes it).
        window
            .update(cx, |app, _w, cx| {
                app.toggle_folder(repo.clone(), cx);
                assert!(!app.collapsed.contains(&repo));
            })
            .unwrap();
        cx.run_until_parked();
    }
}

#[cfg(test)]
mod actions_cov_tests {
    //! Coverage for `actions.rs`: the app key-action handlers (save / escape /
    //! palette), `close_topmost_overlay`'s priority ladder, and `palette_overlay`'s
    //! builder. Placed in `main.rs` so the tests can read `BruApp`'s private
    //! overlay-state fields (they aren't `pub(crate)`). The `on_*` handlers take a
    //! real `&mut Window`, so those use the windowed harness (template 3); the
    //! window-free helpers use the entity harness (template 2).
    //!
    //! NOTE: `on_send_action` is intentionally untested — it calls `send()`, which
    //! spawns a raw OS thread that panics the deterministic test scheduler.
    use super::*;
    use crate::test_support::{app_on_temp, temp_collection};

    /// Esc with the command palette open closes it (the first rung of the ladder).
    #[gpui::test]
    fn escape_closes_palette(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, window, cx| {
                app.on_palette_action(&OpenPalette, window, cx);
                assert!(app.palette_open);
                app.on_escape_action(&CloseOverlay, window, cx);
                assert!(!app.palette_open);
            })
            .unwrap();
    }

    /// `on_palette_action` opens the palette and (with a window present) focuses
    /// the palette input without panicking.
    #[gpui::test]
    fn palette_action_opens(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, window, cx| {
                assert!(!app.palette_open);
                app.on_palette_action(&OpenPalette, window, cx);
                assert!(app.palette_open);
            })
            .unwrap();
    }

    /// `on_save_action` saves the active request tab to disk and sets the status.
    #[gpui::test]
    fn save_action_saves_active_tab(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let req = tc.dir.join("Repository Info.bru");
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, window, cx| {
                app.open_request(req.clone(), cx);
                assert_eq!(app.active, Some(0));
                app.on_save_action(&SaveTab, window, cx);
                assert_eq!(app.status, "Saved");
            })
            .unwrap();
        // The serialized request was written back over the temp copy.
        assert!(req.exists());
    }

    /// `on_save_action` is a no-op (no panic, status unchanged) with no active tab.
    #[gpui::test]
    fn save_action_no_active_tab(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, window, cx| {
                assert!(app.active.is_none());
                app.on_save_action(&SaveTab, window, cx);
                // `save` early-returns without an active tab, so status stays empty.
                assert!(app.status.is_empty());
            })
            .unwrap();
    }

    /// Esc with no overlay open is a harmless no-op (the ladder's `else { return }`).
    #[gpui::test]
    fn escape_with_nothing_open_is_noop(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, window, cx| {
                app.on_escape_action(&CloseOverlay, window, cx);
                assert!(!app.palette_open);
                assert!(!app.vault_open);
                assert!(!app.git_open);
            })
            .unwrap();
    }

    /// Esc closes the curl-import overlay via the ladder's `curl_open` rung.
    #[gpui::test]
    fn escape_closes_curl_overlay(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, window, cx| {
                app.open_curl(cx);
                assert!(app.curl_open);
                app.on_escape_action(&CloseOverlay, window, cx);
                assert!(!app.curl_open);
            })
            .unwrap();
    }

    /// Esc closes the secrets-vault overlay.
    #[gpui::test]
    fn escape_closes_vault_overlay(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, window, cx| {
                app.open_vault(cx);
                assert!(app.vault_open);
                app.on_escape_action(&CloseOverlay, window, cx);
                assert!(!app.vault_open);
            })
            .unwrap();
    }

    /// Esc closes the git overlay (and clears the discard-arm flag along with it).
    #[gpui::test]
    fn escape_closes_git_overlay(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, window, cx| {
                app.open_git(cx);
                assert!(app.git_open);
                app.on_escape_action(&CloseOverlay, window, cx);
                assert!(!app.git_open);
                assert!(!app.git_confirm_discard);
            })
            .unwrap();
    }

    /// `close_topmost_overlay` (driven directly, no window) walks its priority
    /// ladder: with both the palette and curl open, palette closes first; a second
    /// call then closes curl.
    #[gpui::test]
    fn close_topmost_overlay_priority_order(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.open_curl(cx);
            app.palette_open = true;
            // Palette outranks curl: first call closes the palette only.
            app.close_topmost_overlay(cx);
            assert!(!app.palette_open);
            assert!(app.curl_open);
            // Second call drops to the curl rung.
            app.close_topmost_overlay(cx);
            assert!(!app.curl_open);
        });
    }

    /// `close_topmost_overlay` closes the lightweight popover group (here the
    /// method-picker dropdown) in a single call.
    #[gpui::test]
    fn close_topmost_overlay_closes_popover(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.method_menu = Some(gpui::point(px(0.), px(0.)));
            app.close_topmost_overlay(cx);
            assert!(app.method_menu.is_none());
        });
    }

    /// `close_topmost_overlay` with nothing open returns without touching state.
    #[gpui::test]
    fn close_topmost_overlay_noop_when_empty(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.close_topmost_overlay(cx);
            assert!(!app.palette_open);
            assert!(!app.curl_open);
            assert!(!app.runner_open);
        });
    }

    /// `palette_overlay` builds its card without a window. With a query that
    /// matches nothing the filtered list is empty; an empty query lists requests
    /// from the loaded collection. Driving both exercises the filter closure and
    /// the per-row builder.
    #[gpui::test]
    fn palette_overlay_builds_filtered_and_unfiltered(cx: &mut gpui::TestAppContext) {
        // Rendered in a window so the palette card (which clones the palette-input
        // editor entity into the view tree) is consumed by the renderer instead of
        // leaking a handle. Re-parking after each query change rebuilds the
        // overlay, exercising the filter closure (empty + no-match) and the
        // per-row builder.
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        // Unfiltered: builds a row per flattened request (collection is loaded).
        window
            .update(cx, |app, window, cx| {
                app.palette_query = String::new();
                app.on_palette_action(&OpenPalette, window, cx);
            })
            .unwrap();
        cx.run_until_parked();
        // Filtered to a string that won't match any request name/path.
        window
            .update(cx, |app, _w, cx| {
                app.palette_query = "zzz-no-such-request-zzz".into();
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _w, _cx| assert!(app.palette_open))
            .unwrap();
    }

    /// `push_folder`'s active-request highlight branch: with a request open, its
    /// sidebar row renders in the active state (`active == true`).
    #[gpui::test]
    fn sidebar_marks_active_request(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        let req = tc.dir.join("Repository Info.bru");
        window
            .update(cx, |app, _w, cx| {
                app.open_request(req.clone(), cx);
            })
            .unwrap();
        cx.run_until_parked(); // sidebar re-render: active row path matches the tab
        window
            .update(cx, |app, _w, _cx| {
                assert_eq!(app.active_tab().map(|t| t.path.clone()), Some(req.clone()));
            })
            .unwrap();
    }

    /// `url_bar` with a GET request: method uppercases, not-dirty (no draft dot),
    /// not sending ("Send" label). Exercises the URL-bar builder via render.
    #[gpui::test]
    fn url_bar_clean_get(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, _w, cx| {
                app.open_request(tc.dir.join("Repository Info.bru"), cx);
            })
            .unwrap();
        cx.run_until_parked(); // url_bar runs in render for the active tab
        window
            .update(cx, |app, _w, _cx| {
                let p = app.active_tab().map(|t| t.path.clone()).unwrap();
                assert!(!app.dirty.contains(&p)); // clean -> no draft dot branch
            })
            .unwrap();
    }

    /// `url_bar`'s dirty + sending + empty-method branches: mark the tab dirty
    /// (draft dot), clear its method (empty -> defaults to "GET"), and flag it
    /// sending ("Sending\u{2026}" label). All three are private `OpenTab`/`BruApp`
    /// fields reachable from this in-module test.
    #[gpui::test]
    fn url_bar_dirty_and_sending(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, _w, cx| {
                app.open_request(tc.dir.join("Repository Info.bru"), cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _w, _cx| {
                let i = app.active.expect("a tab is open");
                let p = app.tabs[i].path.clone();
                app.dirty.insert(p); // draft-dot branch in url_bar
                app.tabs[i].method = String::new(); // empty -> "GET" branch
                app.tabs[i].sending = true; // "Sending" label branch
            })
            .unwrap();
        cx.run_until_parked(); // re-render url_bar with dirty + sending + empty method
        window
            .update(cx, |app, _w, _cx| {
                let i = app.active.unwrap();
                assert!(app.tabs[i].sending);
                assert!(app.tabs[i].method.is_empty());
                assert!(app.dirty.contains(&app.tabs[i].path));
            })
            .unwrap();
    }
}
