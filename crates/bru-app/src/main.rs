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

use std::collections::HashMap;
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
    div, prelude::*, px, size, App, Bounds, Context, Div, Entity, Focusable, MouseButton,
    MouseDownEvent, MouseUpEvent, Pixels, Point, StyledText, Window, WindowBounds, WindowOptions,
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
    /// Env-picker dropdown anchored at this point (None = closed).
    env_menu: Option<Point<Pixels>>,
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
    edit_kind: EditKind,
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
        let url_input = cx.new(|cx| CodeEditor::single_line(cx, ""));
        url_input.update(cx, |ed, cx| ed.set_line(&url, cx));
        let mut tab = Self {
            path,
            method,
            req_tab: ReqTab::Body,
            resp_tab: RespTab::Response,
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

/// `~/.bruno-rs/gpui-recent.json` — the recent-collections list.
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

/// Build a `curl` command for the request — method, URL, headers, and body —
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
        let curl_input = cx.new(|cx| CodeEditor::new(cx, ""));
        let timeout_input = cx.new(|cx| CodeEditor::single_line(cx, "30"));
        let search = cx.new(|cx| CodeEditor::single_line(cx, ""));
        // Live-filter the sidebar as the search box changes.
        cx.subscribe(&search, |this, ed, _ev: &editor::Changed, cx| {
            this.search_query = ed.read(cx).text().to_lowercase();
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
            pref_timeout: 30,
            pref_insecure: false,
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
            env_menu: None,
        }
    }

    // ── active environment selector ───────────────────────────────────────────
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

    // ── sidebar context menu ──────────────────────────────────────────────────
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
                .child(item("Run Folder").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_run(cx)),
                ));
        } else {
            card = card.child(item("Open").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_run(cx)),
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
                    .child(
                        solid_btn("Delete").bg(theme::red()).on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.commit_delete(cx)),
                        ),
                    ),
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

    // ── secrets vault ────────────────────────────────────────────────────────
    /// Vault secrets as the lowest-precedence base vars for sends.
    fn vault_vars(&self) -> HashMap<String, String> {
        self.vault.clone().unwrap_or_default()
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
        cx.notify();
    }
    /// Read the timeout input and commit it (ignored if not a number).
    fn apply_prefs(&mut self, cx: &mut Context<Self>) {
        if let Ok(n) = self.timeout_input.read(cx).text().trim().parse::<u64>() {
            self.pref_timeout = n;
        }
        self.prefs_open = false;
        cx.notify();
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

    // ── collection runner ────────────────────────────────────────────────────
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
        let globals = self.vault_vars();
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

    // ── environment manager ──────────────────────────────────────────────────
    fn env_build_rows(&self, name: &str, cx: &mut Context<Self>) -> Vec<EnvRowState> {
        let reveal = self.reveal_secrets;
        envfs::load_env_rows(&self.dir, name)
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
        });
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
        let res = envfs::save_env(&self.dir, &sel, &rows);
        if let Some(ed) = &mut self.env {
            ed.error = res.err();
        }
        cx.notify();
    }

    fn env_new(&mut self, cx: &mut Context<Self>) {
        let existing = envfs::scan_envs(&self.dir);
        let mut name = "New Environment".to_string();
        let mut n = 1;
        while existing.iter().any(|e| e == &name) {
            n += 1;
            name = format!("New Environment {n}");
        }
        match envfs::create_env(&self.dir, &name) {
            Ok(()) => {
                let names = envfs::scan_envs(&self.dir);
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
        let _ = envfs::delete_env(&self.dir, &name);
        let names = envfs::scan_envs(&self.dir);
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
        let _ = envfs::duplicate_env(&self.dir, &name);
        let names = envfs::scan_envs(&self.dir);
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
        match envfs::rename_env(&self.dir, &old, &new) {
            Ok(()) => {
                let names = envfs::scan_envs(&self.dir);
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
        let opts = self.send_options();
        let globals = self.vault_vars();
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
                // Tab may have moved/closed while in flight — re-find by path.
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
        self.status = if ok {
            "Saved".into()
        } else {
            "Save failed".into()
        };
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
            .child(chip("Prefs").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.open_prefs(cx)),
            ))
            .child(
                icon_chip(if theme::is_dark() {
                    "\u{2600}" // ☀ — click for light
                } else {
                    "\u{263E}" // ☾ — click for dark
                })
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|_this, _e: &MouseUpEvent, _w, cx| {
                        theme::toggle();
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
                            .px_1()
                            .text_color(theme::accent())
                            .child("+")
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.new_request(cx)),
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
            let fpath = sub.path.clone();
            let fname = sub.name.clone();
            out.push(folder_row(&sub.name, depth).on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &MouseDownEvent, _win, cx| {
                    this.open_ctx_menu(fpath.clone(), true, fname.clone(), ev.position, cx);
                }),
            ));
            self.push_folder(sub, depth + 1, query, cx, out);
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

    fn response_pane(&self, tab: &OpenTab, window: &mut Window, cx: &mut Context<Self>) -> Div {
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
            strip = strip.child(tab_chip(rt.label(), active).on_mouse_up(
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
                );
        }

        let scroll = |id: &'static str| div().id(id).overflow_y_scroll().flex_1().w_full().p_3();
        let content: gpui::AnyElement = match (&tab.response, tab.resp_tab) {
            (None, _) => div()
                .p_3()
                .text_color(theme::muted())
                .child("No response yet \u{2014} press Send.")
                .into_any_element(),
            (Some(o), RespTab::Response) => {
                let is_json = o
                    .response
                    .as_ref()
                    .map(|r| {
                        r.headers.iter().any(|(k, v)| {
                            k.eq_ignore_ascii_case("content-type") && v.contains("json")
                        })
                    })
                    .unwrap_or(false);
                match (is_json, o.response.as_ref()) {
                    (true, Some(r)) => {
                        let raw = String::from_utf8_lossy(&r.body).to_string();
                        let pretty = serde_json::from_str::<serde_json::Value>(&raw)
                            .ok()
                            .and_then(|v| serde_json::to_string_pretty(&v).ok())
                            .unwrap_or(raw);
                        let mut base = window.text_style();
                        base.font_family = "monospace".into();
                        base.color = theme::text();
                        base.font_size = px(13.).into();
                        let spans = highlight::json(&pretty);
                        scroll("resp-body")
                            .font_family("monospace")
                            .text_size(px(13.))
                            .line_height(px(19.))
                            .child(StyledText::new(pretty).with_default_highlights(&base, spans))
                            .into_any_element()
                    }
                    _ => scroll("resp-body")
                        .font_family("monospace")
                        .text_size(px(13.))
                        .line_height(px(19.))
                        .text_color(theme::subtext())
                        .child(format_outcome(o))
                        .into_any_element(),
                }
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
                let r = o.response.as_ref();
                let txt = format!(
                    "{} {}\n{}\nstatus: {}\ntime: {} ms\nsize: {}",
                    tab.method.to_uppercase(),
                    o.url,
                    o.error.clone().unwrap_or_default(),
                    r.map(|r| r.status).unwrap_or(0),
                    r.map(|r| r.duration_ms).unwrap_or(0),
                    r.map(|r| human_size(r.body.len())).unwrap_or_default(),
                );
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
                .child(self.req_content(tab))
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
        if self.rename.is_some() {
            root = root.child(self.rename_overlay(cx));
        }
        if self.env_menu.is_some() {
            root = root.child(self.env_menu_overlay(cx));
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
