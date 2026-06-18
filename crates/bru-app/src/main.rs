//! bruno-rs - an iced (wgpu) desktop client, laid out and styled to match Bruno.
//!
//! The window mirrors Bruno: a top bar (open collection, environment selector,
//! status), a collapsible collection sidebar with method-coloured rows, a strip of
//! open-request tabs, a method-dropdown + URL + Save/Send bar, the request
//! sub-tabs (Params/Body/Headers/Auth/Vars/Script/Assert/Tests/Docs/Settings, plus
//! a raw Source tab), and a response pane with Response/Headers/Timeline/Tests
//! sub-tabs.
//!
//! Each open request keeps an in-memory [`BruFile`] as the single source of truth.
//! Structured edits mutate that file in place (see [`edit`]); the file is
//! re-projected (`to_request`) for display and serialized on Save. The Source tab
//! shows/edits the serialized `.bru` and live-commits whenever it parses. Sending
//! runs async over `bru-engine` against the in-memory file, so unsaved edits are
//! sent and the network never blocks the UI.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod edit;
mod fsops;
mod theme;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use bru_core::{Auth, Body, BruFile, CollectionTree, Folder, KeyVal, MultipartValue, Request};
use bru_engine::{base_vars, run_request, RunContext, RunOutcome};
use bru_http::{HttpClient, HttpResponse, SendOptions};
use iced::keyboard::{key::Named, Key};
use iced::widget::{
    button, checkbox, column, container, mouse_area, opaque, pick_list, row, scrollable, stack,
    text, text_editor, text_input, tooltip, Column, Space,
};
use iced::{Center, Element, Fill, Length, Padding, Point, Subscription, Task};

use theme::*;

fn main() -> iced::Result {
    iced::application(App::boot, App::update, App::view)
        .title("bruno-rs")
        .theme(app_theme)
        .subscription(App::subscription)
        .run()
}

fn app_theme(_: &App) -> iced::Theme {
    theme::base_theme()
}

// === state ===================================================================

#[derive(Default)]
struct App {
    collection: Option<CollectionTree>,
    collection_dir: Option<PathBuf>,
    collapsed: HashSet<PathBuf>,
    envs: Vec<String>,
    selected_env: Option<String>,
    tabs: Vec<Tab>,
    active: Option<usize>,
    status: String,
    /// Bruno "Developer Mode": lets scripts `require()` local `.js`. Off by default.
    developer_mode: bool,
    /// Monotonic id source for tabs (stable across reorder/close; used to route
    /// async Send results back to the right tab even if indices shift).
    next_id: usize,
    /// Last known cursor position, used to anchor context menus at the click.
    cursor: Point,
    /// The open right-click context menu, if any.
    menu: Option<MenuState>,
    /// The open `{{variable}}` value popover (copy button), if any.
    var_popup: Option<VarPopup>,
    /// The open modal dialog, if any.
    modal: Option<Modal>,
    /// Sidebar filter text.
    search: String,
    /// In-app clipboard for Copy/Paste of tree items: (path, is_folder).
    clipboard_item: Option<(PathBuf, bool)>,
    /// Response pane split orientation (false = vertical stack, true = side-by-side).
    layout_horizontal: bool,
    /// Request-pane fraction of the split (0.2..0.85).
    split: f32,
    /// Active divider drag: (anchor cursor Y, anchor split) while dragging.
    split_drag: Option<(f32, f32)>,
    /// Sidebar drag-and-drop: the request being dragged and the row hovered over.
    dragging: Option<PathBuf>,
    drag_over: Option<PathBuf>,
    /// The environment-manager overlay, if open.
    env_editor: Option<EnvEditor>,
    /// The collection/folder runner overlay, if open.
    runner: Option<Runner>,
    /// Bottom devtools console panel: collected log lines + visibility.
    console: Vec<String>,
    console_open: bool,
    /// Which devtools dock tab is shown (Console / Network).
    devtools_tab: DevTab,
    /// Session network log (one row per request sent).
    network: Vec<NetEntry>,
    /// Persisted preferences (request timeout, TLS verification).
    prefs: Prefs,
    /// Cached collection + selected-environment variables, for the URL variable
    /// hover preview. Recomputed on collection load / env change / env save so
    /// `view()` (which runs on every mouse move) never touches disk.
    vars: HashMap<String, String>,
    /// Current git branch of the open collection (read from `.git/HEAD` on load),
    /// shown as a chip in the top bar. None when the collection isn't a git repo.
    git_branch: Option<String>,
    /// Cookies observed from response `Set-Cookie` headers this session, shown in
    /// the Cookies manager. (A viewer: a fresh client is built per send, so these
    /// aren't auto-replayed — see the Cookies overlay note.)
    cookies: Vec<CookieEntry>,
    cookies_open: bool,
}

/// One stored cookie, keyed by (domain, path, name) for upsert.
#[derive(Debug, Clone)]
struct CookieEntry {
    domain: String,
    path: String,
    name: String,
    value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum DevTab {
    #[default]
    Console,
    Network,
}

/// One row in the devtools Network log.
#[derive(Debug, Clone)]
struct NetEntry {
    method: String,
    url: String,
    status: u16,
    ms: u128,
    size: usize,
    ok: bool,
}

/// Persisted user preferences. Stored as JSON in the home directory.
#[derive(Debug, Clone)]
struct Prefs {
    timeout_secs: u64,
    insecure: bool,
    /// Use the light palette instead of the dark one.
    light: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            timeout_secs: 30,
            insecure: false,
            light: false,
        }
    }
}

impl Prefs {
    fn send_options(&self) -> SendOptions {
        SendOptions {
            insecure: self.insecure,
            timeout: std::time::Duration::from_secs(self.timeout_secs.max(1)),
            ..SendOptions::default()
        }
    }
}

fn prefs_path() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join(".bruno-rs.json"))
}

fn load_prefs() -> Prefs {
    let mut p = Prefs::default();
    if let Some(path) = prefs_path() {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(t) = v.get("timeout_secs").and_then(|x| x.as_u64()) {
                    p.timeout_secs = t;
                }
                if let Some(b) = v.get("insecure").and_then(|x| x.as_bool()) {
                    p.insecure = b;
                }
                if let Some(b) = v.get("light").and_then(|x| x.as_bool()) {
                    p.light = b;
                }
            }
        }
    }
    p
}

fn save_prefs(p: &Prefs) {
    if let Some(path) = prefs_path() {
        let v = serde_json::json!({
            "timeout_secs": p.timeout_secs,
            "insecure": p.insecure,
            "light": p.light,
        });
        let _ = std::fs::write(path, v.to_string());
    }
}

/// The collection/folder runner overlay.
#[derive(Debug, Clone, Default)]
struct Runner {
    title: String,
    running: bool,
    results: Vec<RunResult>,
}

/// One request's outcome in a runner batch.
#[derive(Debug, Clone)]
struct RunResult {
    name: String,
    passed: bool,
    status: u16,
    ms: u128,
    error: Option<String>,
}

/// Working state for the environment manager overlay.
#[derive(Debug, Clone, Default)]
struct EnvEditor {
    /// The environment currently being edited ("" = none selected yet).
    selected: String,
    /// Edit buffer for renaming the selected environment.
    rename_buf: String,
    rows: Vec<fsops::EnvRow>,
    error: Option<String>,
}

/// What an open tab represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum TabKind {
    #[default]
    Request,
    CollectionSettings,
    FolderSettings,
}

/// One open tab (a request, or a collection/folder settings pane).
struct Tab {
    id: usize,
    kind: TabKind,
    /// `None` for an unsaved draft (no file on disk yet).
    path: Option<PathBuf>,
    file: BruFile,
    /// Serialized text at last save/load; compared to detect unsaved edits.
    saved_text: String,
    /// Cached unsaved-edits flag (recomputed on every edit; keeps `view` cheap).
    dirty: bool,
    req_tab: ReqTab,
    resp_tab: RespTab,
    result: Option<RunOutcome>,
    sending: bool,
    /// User dismissed the large-response guard for this tab.
    reveal_large: bool,
    /// How to render the response body (Pretty / Raw / Hex / Tree).
    resp_format: RespFormat,
    /// Read-only, selectable, highlighted buffer for the response body.
    resp_editor: text_editor::Content,
    /// Expanded JSON-tree node paths (for the Tree view).
    resp_expanded: HashSet<String>,
    /// JSONPath filter applied to the response body (empty = show full body).
    resp_filter: String,
    /// Edit buffer for adding a tag in the Settings tab.
    tag_input: String,
    /// The KV section currently in raw "bulk edit" mode, if any.
    bulk: Option<KvSection>,
    bulk_editor: text_editor::Content,
    editors: Editors,
    /// The Source buffer holds uncommitted text that doesn't parse, so `file`
    /// is stale relative to it. While set, re-entering the Source tab keeps the
    /// user's broken edits instead of regenerating from `file`.
    source_invalid: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum RespFormat {
    #[default]
    Pretty,
    Raw,
    Hex,
    Tree,
}

impl Tab {
    fn title(&self) -> String {
        match self.kind {
            TabKind::CollectionSettings => "Collection Settings".to_string(),
            TabKind::FolderSettings => {
                let folder = self
                    .path
                    .as_deref()
                    .and_then(Path::parent)
                    .and_then(|p| p.file_name())
                    .and_then(|s| s.to_str())
                    .unwrap_or("Folder");
                format!("{folder} Settings")
            }
            TabKind::Request => self
                .file
                .request_name()
                .map(str::to_string)
                .filter(|s| !s.is_empty())
                .or_else(|| self.path.as_deref().map(file_stem))
                .unwrap_or_else(|| "Untitled".to_string()),
        }
    }
    fn is_settings(&self) -> bool {
        !matches!(self.kind, TabKind::Request)
    }
    fn recompute_dirty(&mut self) {
        self.dirty = bru_lang::serialize(&self.file) != self.saved_text;
    }
    /// Rebuild the read-only response buffer from the current result + format.
    fn rebuild_resp_editor(&mut self) {
        let text = match &self.result {
            Some(o) if o.error.is_none() => match &o.response {
                Some(r) => match self.resp_format {
                    RespFormat::Hex => hex_dump(&r.body),
                    RespFormat::Raw => r.text(),
                    _ => pretty_body(r),
                },
                None => String::new(),
            },
            _ => String::new(),
        };
        self.resp_editor = text_editor::Content::with_text(&text);
    }
}

/// An open context menu: which item it targets and where to draw it.
#[derive(Debug, Clone)]
struct MenuState {
    target: MenuTarget,
    at: Point,
}

/// A click-opened popover for a `{{variable}}` pill: shows the resolved value
/// and a Copy button. Anchored at the cursor like the context menu.
#[derive(Debug, Clone)]
struct VarPopup {
    name: String,
    value: Option<String>,
}

#[derive(Debug, Clone)]
enum MenuTarget {
    Request(PathBuf),
    Folder(PathBuf),
    Collection,
    Tab(usize),
    /// The response-actions kebab (Copy / Save / Clear / …).
    Response,
}

/// A modal dialog. Text/selection state lives inline so the view is pure.
#[derive(Debug, Clone)]
enum Modal {
    NewRequest {
        dir: PathBuf,
        name: String,
        method: String,
        url: String,
        error: Option<String>,
    },
    NewFolder {
        parent: PathBuf,
        name: String,
        error: Option<String>,
    },
    Rename {
        path: PathBuf,
        is_folder: bool,
        name: String,
        error: Option<String>,
    },
    Clone {
        path: PathBuf,
        is_folder: bool,
        name: String,
        error: Option<String>,
    },
    Delete {
        path: PathBuf,
        is_folder: bool,
        name: String,
    },
    ConfirmClose {
        id: usize,
    },
    Palette {
        query: String,
        selected: usize,
    },
    Code {
        code: String,
    },
    Prefs,
    SaveExample {
        name: String,
    },
}

/// The multiline `text_editor` buffers for the active request. Rebuilt from
/// `file` when a request is opened or the relevant sub-tab/body-mode changes.
#[derive(Default)]
struct Editors {
    source: text_editor::Content,
    body: text_editor::Content,
    /// Which `body:*` block the `body` editor currently maps to.
    body_kind: String,
    gql_query: text_editor::Content,
    gql_vars: text_editor::Content,
    script_pre: text_editor::Content,
    script_post: text_editor::Content,
    tests: text_editor::Content,
    docs: text_editor::Content,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ReqTab {
    #[default]
    Params,
    Body,
    Headers,
    Auth,
    Vars,
    Script,
    Assert,
    Tests,
    Docs,
    Settings,
    Examples,
    Source,
}

impl ReqTab {
    const ALL: [ReqTab; 12] = [
        ReqTab::Params,
        ReqTab::Body,
        ReqTab::Headers,
        ReqTab::Auth,
        ReqTab::Vars,
        ReqTab::Script,
        ReqTab::Assert,
        ReqTab::Tests,
        ReqTab::Docs,
        ReqTab::Settings,
        ReqTab::Examples,
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
            ReqTab::Assert => "Assert",
            ReqTab::Tests => "Tests",
            ReqTab::Docs => "Docs",
            ReqTab::Settings => "Settings",
            ReqTab::Examples => "Examples",
            ReqTab::Source => "Source",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum RespTab {
    #[default]
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

/// A dictionary-backed editable section (params/headers/vars/assert/form/...).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KvSection {
    Query,
    Path,
    Headers,
    Form,
    Multipart,
    Assert,
    VarsPre,
    VarsPost,
}

impl KvSection {
    fn block(self) -> &'static str {
        match self {
            KvSection::Query => "params:query",
            KvSection::Path => "params:path",
            KvSection::Headers => "headers",
            KvSection::Form => "body:form-urlencoded",
            KvSection::Multipart => "body:multipart-form",
            KvSection::Assert => "assert",
            KvSection::VarsPre => "vars:pre-request",
            KvSection::VarsPost => "vars:post-response",
        }
    }
}

/// An editable auth credential field. Maps to a `(block, key)` pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthField {
    BasicUser,
    BasicPass,
    BearerToken,
    ApiKeyKey,
    ApiKeyValue,
    ApiKeyPlacement,
    DigestUser,
    DigestPass,
    AwsAccessKey,
    AwsSecretKey,
    AwsSessionToken,
    AwsService,
    AwsRegion,
    AwsProfile,
    Oauth2GrantType,
    Oauth2TokenUrl,
    Oauth2ClientId,
    Oauth2ClientSecret,
    Oauth2Scope,
    Oauth2Username,
    Oauth2Password,
}

impl AuthField {
    fn target(self) -> (&'static str, &'static str) {
        match self {
            AuthField::BasicUser => ("auth:basic", "username"),
            AuthField::BasicPass => ("auth:basic", "password"),
            AuthField::BearerToken => ("auth:bearer", "token"),
            AuthField::ApiKeyKey => ("auth:apikey", "key"),
            AuthField::ApiKeyValue => ("auth:apikey", "value"),
            AuthField::ApiKeyPlacement => ("auth:apikey", "placement"),
            AuthField::DigestUser => ("auth:digest", "username"),
            AuthField::DigestPass => ("auth:digest", "password"),
            AuthField::AwsAccessKey => ("auth:awsv4", "accessKeyId"),
            AuthField::AwsSecretKey => ("auth:awsv4", "secretAccessKey"),
            AuthField::AwsSessionToken => ("auth:awsv4", "sessionToken"),
            AuthField::AwsService => ("auth:awsv4", "service"),
            AuthField::AwsRegion => ("auth:awsv4", "region"),
            AuthField::AwsProfile => ("auth:awsv4", "profileName"),
            AuthField::Oauth2GrantType => ("auth:oauth2", "grant_type"),
            AuthField::Oauth2TokenUrl => ("auth:oauth2", "access_token_url"),
            AuthField::Oauth2ClientId => ("auth:oauth2", "client_id"),
            AuthField::Oauth2ClientSecret => ("auth:oauth2", "client_secret"),
            AuthField::Oauth2Scope => ("auth:oauth2", "scope"),
            AuthField::Oauth2Username => ("auth:oauth2", "username"),
            AuthField::Oauth2Password => ("auth:oauth2", "password"),
        }
    }
}

/// Which multiline editor a `text_editor` action targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditorField {
    Body,
    GqlQuery,
    GqlVars,
    ScriptPre,
    ScriptPost,
    Tests,
    Docs,
}

#[derive(Debug, Clone)]
enum Message {
    OpenFolder,
    OpenRequest(PathBuf),
    SelectTab(usize),
    CloseTab(usize),
    ToggleFolder(PathBuf),
    ReqTab(ReqTab),
    RespTab(RespTab),
    SelectEnv(Option<String>),
    ToggleDevMode(bool),
    MethodChanged(String),
    UrlChanged(String),
    BodyModeChanged(String),
    AuthModeChanged(String),
    KvName(KvSection, usize, String),
    KvValue(KvSection, usize, String),
    KvToggle(KvSection, usize, bool),
    KvAdd(KvSection),
    KvRemove(KvSection, usize),
    AuthEdit(AuthField, String),
    SettingText(&'static str, String),
    SettingBool(&'static str, bool),
    EditField(EditorField, text_editor::Action),
    SourceEdit(text_editor::Action),
    Save,
    Send,
    Sent(usize, Box<RunOutcome>),

    // ── context menus ──
    CursorMoved(Point),
    OpenMenu(MenuTarget),
    CloseMenu,
    SplitDragStart,
    PointerUp,
    SidebarDragStart(PathBuf),
    SidebarDragOver(PathBuf),
    SidebarDragOut(PathBuf),

    // ── tree item management ──
    Search(String),
    NewDraft,
    NewRequestPrompt(PathBuf),
    NewFolderPrompt(PathBuf),
    RenamePrompt(PathBuf, bool),
    ClonePrompt(PathBuf, bool),
    DeletePrompt(PathBuf, bool),
    CopyItem(PathBuf, bool),
    PasteItem(PathBuf),
    RevealItem(PathBuf),
    RunItem(PathBuf),
    CollapseAll,
    MoveItem(PathBuf, i32),
    OpenSettings(PathBuf, TabKind),
    GenerateCode(PathBuf),
    /// Generate code for the active tab's in-memory request (includes unsaved
    /// edits; works for never-saved draft tabs). Fired by the URL-bar `</>` icon.
    GenerateCodeActive,
    CopyText(String),

    // ── variable value popover ──
    OpenVarPopup(String, Option<String>),
    CloseVarPopup,
    CopyVarValue(String),

    // ── tab management ──
    CloseOthers(usize),
    CloseRight(usize),
    CloseLeft(usize),
    CloseSaved,
    CloseAll,
    RevertTab(usize),
    CopyTabPath(usize),
    CloneTab(usize),

    // ── modals ──
    ModalName(String),
    ModalUrl(String),
    ModalMethod(String),
    ModalSubmit,
    ModalCancel,
    OpenPalette,
    PaletteQuery(String),
    PaletteMove(i32),

    // ── response actions ──
    CopyResponse,
    DownloadResponse,
    ClearResponse,
    ToggleLayout,
    RevealLarge,
    RespFormatChanged(String),
    OpenInBrowser,
    BrowseFileBody,
    FileBodyContentType(String),
    RespEditorAction(text_editor::Action),
    RespFilter(String),
    ToggleJsonNode(String),
    SaveExamplePrompt,
    TagInput(String),
    AddTag,
    RemoveTag(usize),
    BrowseMultipartFile(usize),
    ToggleBulk(KvSection),
    BulkEdit(text_editor::Action),
    KvLocal(KvSection, usize, bool),

    // ── environment manager ──
    OpenEnvEditor,
    EnvSelect(String),
    EnvName(usize, String),
    EnvValue(usize, String),
    EnvToggle(usize, bool),
    EnvSecret(usize, bool),
    EnvAddRow,
    EnvRemoveRow(usize),
    EnvSave,
    EnvClose,
    EnvNew,
    EnvDelete(String),
    EnvDuplicate(String),
    EnvRenameBuf(String),
    EnvRenameApply,

    // ── runner ──
    RunFolder(PathBuf),
    RunnerDone(Vec<RunResult>),
    RunnerClose,

    // ── devtools ──
    ToggleConsole,
    ClearConsole,
    DevtoolsTab(DevTab),

    // ── cookies ──
    OpenCookies,
    CloseCookies,
    DeleteCookie(usize),
    ClearCookies,

    // ── preferences ──
    OpenPrefs,
    PrefTimeout(String),
    PrefInsecure(bool),
    ToggleTheme(bool),

    // ── keyboard ──
    Key(iced::keyboard::Event),
}

// === update ==================================================================

impl App {
    fn boot() -> App {
        let mut app = App {
            prefs: load_prefs(),
            split: 0.6,
            ..App::default()
        };
        theme::set_light(app.prefs.light);
        match std::env::args().nth(1) {
            Some(arg) => app.load(PathBuf::from(arg)),
            None => app.status = "Open a Bruno collection folder to begin.".to_string(),
        }
        app
    }

    fn load(&mut self, dir: PathBuf) {
        match bru_lang::load_collection(&dir) {
            Ok(tree) => {
                self.status = format!("Loaded \"{}\"", tree.name);
                self.collection = Some(tree);
                self.envs = scan_envs(&dir);
                self.git_branch = git_branch(&dir);
                self.collection_dir = Some(dir);
                self.selected_env = None;
                self.collapsed.clear();
                self.tabs.clear();
                self.active = None;
                self.refresh_vars();
            }
            Err(e) => self.status = format!("Failed to open {}: {e}", dir.display()),
        }
    }

    /// Open (or focus) a request tab. Returns `false` (leaving `active`
    /// unchanged) if the file can't be read/parsed, so callers like Run don't
    /// then act on the previously-active tab.
    fn open_request(&mut self, path: PathBuf) -> bool {
        if let Some(i) = self
            .tabs
            .iter()
            .position(|t| t.path.as_deref() == Some(&path))
        {
            self.active = Some(i);
            return true;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                self.status = format!("Failed to read {}: {e}", path.display());
                return false;
            }
        };
        let file = match bru_lang::parse(&text) {
            Ok(f) => f,
            Err(e) => {
                self.status = format!("Parse error in {}: {e}", path.display());
                return false;
            }
        };
        let saved_text = bru_lang::serialize(&file);
        let mut tab = self.blank_tab(Some(path), file, saved_text);
        load_editors_for(&mut tab);
        self.tabs.push(tab);
        self.active = Some(self.tabs.len() - 1);
        self.status.clear();
        true
    }

    /// Construct a fresh tab with a unique id; does not push it.
    fn blank_tab(&mut self, path: Option<PathBuf>, file: BruFile, saved_text: String) -> Tab {
        self.next_id += 1;
        Tab {
            id: self.next_id,
            kind: TabKind::Request,
            path,
            file,
            saved_text,
            dirty: false,
            req_tab: ReqTab::Params,
            resp_tab: RespTab::Response,
            result: None,
            sending: false,
            reveal_large: false,
            resp_format: RespFormat::Pretty,
            resp_editor: text_editor::Content::new(),
            resp_expanded: HashSet::new(),
            resp_filter: String::new(),
            tag_input: String::new(),
            bulk: None,
            bulk_editor: text_editor::Content::new(),
            editors: Editors::default(),
            source_invalid: false,
        }
    }

    /// Open (or focus) a collection/folder settings tab backed by `path`
    /// (a `collection.bru` or `folder.bru`). Missing files start blank.
    fn open_settings(&mut self, path: PathBuf, kind: TabKind) {
        self.menu = None;
        if let Some(i) = self
            .tabs
            .iter()
            .position(|t| t.path.as_deref() == Some(&path))
        {
            self.active = Some(i);
            return;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let file = bru_lang::parse(&text).unwrap_or_default();
        let saved_text = bru_lang::serialize(&file);
        let mut tab = self.blank_tab(Some(path), file, saved_text);
        tab.kind = kind;
        tab.req_tab = ReqTab::Headers;
        load_editors_for(&mut tab);
        self.tabs.push(tab);
        self.active = Some(self.tabs.len() - 1);
        self.status.clear();
    }

    /// Open a new unsaved draft request tab (the "+" button).
    fn new_draft(&mut self) {
        let n = self.tabs.len() + 1;
        let text = format!(
            "meta {{\n  name: Untitled {n}\n  type: http\n  seq: 1\n}}\n\nget {{\n  url: \n  body: none\n  auth: none\n}}\n"
        );
        let file = bru_lang::parse(&text).unwrap_or_default();
        let mut tab = self.blank_tab(None, file, String::new());
        tab.dirty = true;
        load_editors_for(&mut tab);
        self.tabs.push(tab);
        self.active = Some(self.tabs.len() - 1);
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenFolder => {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    self.load(dir);
                }
            }
            Message::OpenRequest(path) => {
                self.menu = None;
                self.cancel_drag();
                self.open_request(path);
            }
            Message::SelectTab(i) => {
                self.cancel_drag();
                if i < self.tabs.len() {
                    self.active = Some(i);
                }
            }
            Message::CloseTab(i) => {
                self.menu = None;
                if i < self.tabs.len() {
                    if self.tabs[i].dirty {
                        self.modal = Some(Modal::ConfirmClose {
                            id: self.tabs[i].id,
                        });
                    } else {
                        self.remove_tab(i);
                    }
                }
            }
            Message::ToggleFolder(path) => {
                if !self.collapsed.remove(&path) {
                    self.collapsed.insert(path);
                }
            }
            Message::ReqTab(t) => {
                if let Some(i) = self.active {
                    let prev = self.tabs[i].req_tab;
                    if prev == ReqTab::Source && t != ReqTab::Source {
                        self.commit_source(i);
                    }
                    self.tabs[i].req_tab = t;
                    load_editors_for(&mut self.tabs[i]);
                }
            }
            Message::RespTab(t) => {
                if let Some(i) = self.active {
                    self.tabs[i].resp_tab = t;
                }
            }
            Message::SelectEnv(env) => {
                self.selected_env = env;
                self.refresh_vars();
            }
            Message::ToggleDevMode(on) => self.developer_mode = on,
            Message::MethodChanged(m) => self.mutate(|f| edit::set_method(f, &m)),
            Message::UrlChanged(u) => self.mutate(|f| {
                edit::set_url(f, &u);
                edit::sync_path_params(f, &u);
            }),
            Message::BodyModeChanged(mode) => {
                self.mutate(|f| {
                    edit::set_method_field(f, "body", &mode);
                });
                if let Some(i) = self.active {
                    load_editors_for(&mut self.tabs[i]);
                }
            }
            Message::AuthModeChanged(mode) => {
                // Settings files keep the mode in a top-level `auth { mode }` block;
                // requests keep it in the method block.
                let settings = self
                    .active
                    .map(|i| self.tabs[i].is_settings())
                    .unwrap_or(false);
                self.mutate(|f| {
                    if settings {
                        let entries = edit::dict_block_mut(f, "auth");
                        edit::set_entry(entries, "mode", &mode);
                    } else {
                        edit::set_method_field(f, "auth", &mode);
                    }
                });
            }
            Message::KvName(s, i, v) => self.mutate(|f| edit::set_entry_key(f, s.block(), i, &v)),
            Message::KvValue(s, i, v) => {
                self.mutate(|f| edit::set_entry_value(f, s.block(), i, &v))
            }
            Message::KvToggle(s, i, on) => self.mutate(|f| edit::toggle_entry(f, s.block(), i, on)),
            Message::KvAdd(s) => self.mutate(|f| edit::add_row(f, s.block())),
            Message::KvRemove(s, i) => self.mutate(|f| edit::remove_row(f, s.block(), i)),
            Message::KvLocal(s, i, b) => self.mutate(|f| edit::set_entry_local(f, s.block(), i, b)),
            Message::AuthEdit(field, v) => {
                let (block, key) = field.target();
                self.mutate(|f| {
                    let entries = edit::dict_block_mut(f, block);
                    edit::set_entry(entries, key, &v);
                });
            }
            Message::SettingText(key, v) => self.mutate(|f| {
                let entries = edit::dict_block_mut(f, "settings");
                edit::set_entry(entries, key, &v);
            }),
            Message::SettingBool(key, b) => self.mutate(|f| {
                let entries = edit::dict_block_mut(f, "settings");
                edit::set_entry(entries, key, if b { "true" } else { "false" });
            }),
            Message::EditField(field, action) => {
                if let Some(i) = self.active {
                    let tab = &mut self.tabs[i];
                    let block: String = match field {
                        EditorField::Body => tab.editors.body_kind.clone(),
                        EditorField::GqlQuery => "body:graphql".into(),
                        EditorField::GqlVars => "body:graphql:vars".into(),
                        EditorField::ScriptPre => "script:pre-request".into(),
                        EditorField::ScriptPost => "script:post-response".into(),
                        EditorField::Tests => "tests".into(),
                        EditorField::Docs => "docs".into(),
                    };
                    let content = match field {
                        EditorField::Body => &mut tab.editors.body,
                        EditorField::GqlQuery => &mut tab.editors.gql_query,
                        EditorField::GqlVars => &mut tab.editors.gql_vars,
                        EditorField::ScriptPre => &mut tab.editors.script_pre,
                        EditorField::ScriptPost => &mut tab.editors.script_post,
                        EditorField::Tests => &mut tab.editors.tests,
                        EditorField::Docs => &mut tab.editors.docs,
                    };
                    content.perform(action);
                    let payload = content.text();
                    if !block.is_empty() {
                        // iced 0.14 text() adds no artifact newline, so any trailing
                        // newline is real user content — store the buffer verbatim.
                        edit::set_text_block(&mut tab.file, &block, &payload);
                    }
                    tab.recompute_dirty();
                }
            }
            Message::SourceEdit(action) => {
                if let Some(i) = self.active {
                    self.tabs[i].editors.source.perform(action);
                    let text = self.tabs[i].editors.source.text();
                    match bru_lang::parse(&text) {
                        Ok(f) => {
                            self.tabs[i].file = f;
                            self.tabs[i].source_invalid = false;
                            self.tabs[i].recompute_dirty();
                            self.status.clear();
                        }
                        Err(e) => {
                            self.tabs[i].source_invalid = true;
                            self.status = format!("Source parse error: {e}");
                        }
                    }
                }
            }
            Message::Save => {
                if let Some(i) = self.active {
                    self.save_tab(i);
                }
            }
            Message::Send => {
                if let Some(i) = self.active {
                    // Settings tabs aren't requests; a tab already in flight must
                    // not start a second concurrent send (Cmd+Enter bypasses the
                    // Send button's own !sending gate).
                    if self.tabs[i].is_settings() || self.tabs[i].sending {
                        return Task::none();
                    }
                    self.tabs[i].sending = true;
                    self.tabs[i].result = None;
                    self.tabs[i].reveal_large = false;
                    self.status = "Sending...".to_string();
                    let id = self.tabs[i].id;
                    let file = self.tabs[i].file.clone();
                    let vars_path = self.tabs[i]
                        .path
                        .clone()
                        .or_else(|| self.collection_dir.clone());
                    let script_dir = self.tabs[i]
                        .path
                        .as_deref()
                        .and_then(Path::parent)
                        .map(Path::to_path_buf)
                        .or_else(|| self.collection_dir.clone());
                    let env = self.selected_env.clone();
                    let dev = self.developer_mode;
                    let opts = self.prefs.send_options();
                    return Task::perform(
                        send_request(file, vars_path, script_dir, env, dev, opts),
                        move |o| Message::Sent(id, o),
                    );
                }
            }
            Message::Sent(id, outcome) => {
                // Only update the shared status bar when the completed send is the
                // tab the user is actually looking at (background sends shouldn't
                // mask the active tab's state).
                if self.active.and_then(|i| self.tabs.get(i)).map(|t| t.id) == Some(id) {
                    self.status = summarize(&outcome);
                }
                // Mirror this run's console + errors into the devtools console.
                let label = self
                    .tabs
                    .iter()
                    .find(|t| t.id == id)
                    .map(|t| t.title())
                    .unwrap_or_default();
                for line in &outcome.console {
                    self.console.push(format!("[{label}] {line}"));
                }
                if let Some(e) = &outcome.error {
                    self.console.push(format!("[{label}] error: {e}"));
                }
                if self.console.len() > 1000 {
                    let drop = self.console.len() - 1000;
                    self.console.drain(0..drop);
                }
                // Network log row.
                self.network.push(NetEntry {
                    method: outcome.method.clone(),
                    url: outcome.url.clone(),
                    status: outcome.response.as_ref().map(|r| r.status).unwrap_or(0),
                    ms: outcome
                        .response
                        .as_ref()
                        .map(|r| r.duration_ms)
                        .unwrap_or(0),
                    size: outcome.response.as_ref().map(|r| r.body.len()).unwrap_or(0),
                    ok: outcome.error.is_none(),
                });
                if self.network.len() > 500 {
                    let drop = self.network.len() - 500;
                    self.network.drain(0..drop);
                }
                // Capture Set-Cookie headers into the cookie viewer.
                if let Some(resp) = outcome.response.as_ref() {
                    let host = host_of(&outcome.url);
                    for (k, v) in &resp.headers {
                        if k.eq_ignore_ascii_case("set-cookie") {
                            if let Some(c) = parse_set_cookie(v, &host) {
                                upsert_cookie(&mut self.cookies, c);
                            }
                        }
                    }
                }
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
                    tab.sending = false;
                    tab.result = Some(*outcome);
                    tab.resp_expanded.clear();
                    tab.rebuild_resp_editor();
                }
            }

            // ── context menus ──
            Message::CursorMoved(p) => {
                self.cursor = p;
                if let Some((anchor_y, anchor_split)) = self.split_drag {
                    // ~500px reference height keeps drag sensitivity reasonable.
                    self.split = (anchor_split + (p.y - anchor_y) / 500.0).clamp(0.2, 0.85);
                }
            }
            Message::SplitDragStart => self.split_drag = Some((self.cursor.y, self.split)),
            Message::PointerUp => {
                self.split_drag = None;
                if let (Some(src), Some(dst)) = (self.dragging.take(), self.drag_over.take()) {
                    self.reorder_to(src, dst);
                }
            }
            Message::SidebarDragStart(path) => self.dragging = Some(path),
            Message::SidebarDragOver(path) => {
                if self.dragging.is_some() && self.dragging.as_deref() != Some(path.as_path()) {
                    self.drag_over = Some(path);
                }
            }
            Message::SidebarDragOut(path) => {
                // Leaving a row clears it as the target, so releasing over empty
                // space (or a non-droppable row) is a no-op, not a stale reorder.
                if self.drag_over.as_deref() == Some(path.as_path()) {
                    self.drag_over = None;
                }
            }
            Message::OpenMenu(target) => {
                self.cancel_drag();
                self.menu = Some(MenuState {
                    target,
                    at: self.cursor,
                });
            }
            Message::CloseMenu => self.menu = None,

            // ── tree item management ──
            Message::Search(q) => self.search = q,
            Message::NewDraft => self.new_draft(),
            Message::NewRequestPrompt(dir) => {
                self.menu = None;
                self.modal = Some(Modal::NewRequest {
                    dir,
                    name: String::new(),
                    method: "GET".to_string(),
                    url: String::new(),
                    error: None,
                });
            }
            Message::NewFolderPrompt(parent) => {
                self.menu = None;
                self.modal = Some(Modal::NewFolder {
                    parent,
                    name: String::new(),
                    error: None,
                });
            }
            Message::RenamePrompt(path, is_folder) => {
                self.menu = None;
                let name = fsops::display_name(&path);
                self.modal = Some(Modal::Rename {
                    path,
                    is_folder,
                    name,
                    error: None,
                });
            }
            Message::ClonePrompt(path, is_folder) => {
                self.menu = None;
                let name = fsops::clone_suggested_name(&path);
                self.modal = Some(Modal::Clone {
                    path,
                    is_folder,
                    name,
                    error: None,
                });
            }
            Message::DeletePrompt(path, is_folder) => {
                self.menu = None;
                let name = fsops::display_name(&path);
                self.modal = Some(Modal::Delete {
                    path,
                    is_folder,
                    name,
                });
            }
            Message::CopyItem(path, is_folder) => {
                self.menu = None;
                self.clipboard_item = Some((path, is_folder));
                self.status = "Copied".to_string();
            }
            Message::PasteItem(dir) => {
                self.menu = None;
                if let Some((src, is_folder)) = self.clipboard_item.clone() {
                    match fsops::clone_to(&src, &dir, is_folder) {
                        Ok(_) => self.reload_tree(),
                        Err(e) => self.status = e,
                    }
                }
            }
            Message::RevealItem(path) => {
                self.menu = None;
                reveal_in_explorer(&path);
            }
            Message::RunItem(path) => {
                self.menu = None;
                // Only send if the request actually opened (keep the parse/read
                // error visible instead of running the previously-active tab).
                if self.open_request(path) {
                    return Task::done(Message::Send);
                }
            }
            Message::CollapseAll => {
                self.menu = None;
                if let Some(tree) = &self.collection {
                    let mut paths = Vec::new();
                    collect_folder_paths(&tree.root, &mut paths);
                    self.collapsed.extend(paths);
                }
            }
            Message::MoveItem(path, delta) => {
                self.menu = None;
                if let Some(dir) = path.parent() {
                    let sibs = self.sibling_requests(dir);
                    if let Some(idx) = sibs.iter().position(|p| p == &path) {
                        let new = (idx as i32 + delta).clamp(0, sibs.len() as i32 - 1) as usize;
                        if new != idx {
                            let mut order = sibs;
                            let item = order.remove(idx);
                            order.insert(new, item);
                            self.apply_order(&order);
                        }
                    }
                }
            }
            Message::OpenSettings(path, kind) => self.open_settings(path, kind),
            Message::GenerateCode(path) => {
                self.menu = None;
                let req = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|t| bru_lang::parse(&t).ok())
                    .and_then(|f| f.to_request());
                match req {
                    Some(r) => self.modal = Some(Modal::Code { code: gen_curl(&r) }),
                    None => self.status = "Not an HTTP request".to_string(),
                }
            }
            Message::GenerateCodeActive => {
                match self
                    .active
                    .and_then(|i| self.tabs.get(i))
                    .and_then(|t| t.file.to_request())
                {
                    Some(r) => self.modal = Some(Modal::Code { code: gen_curl(&r) }),
                    None => self.status = "Not an HTTP request".to_string(),
                }
            }
            Message::CopyText(s) => return iced::clipboard::write(s),
            Message::OpenVarPopup(name, value) => {
                self.menu = None;
                self.var_popup = Some(VarPopup { name, value });
            }
            Message::CloseVarPopup => self.var_popup = None,
            Message::CopyVarValue(v) => {
                self.var_popup = None;
                return iced::clipboard::write(v);
            }

            // ── tab management (bulk close keeps dirty tabs to avoid data loss) ──
            Message::CloseOthers(i) => {
                self.menu = None;
                self.close_where(|j| j != i);
            }
            Message::CloseRight(i) => {
                self.menu = None;
                self.close_where(|j| j > i);
            }
            Message::CloseLeft(i) => {
                self.menu = None;
                self.close_where(|j| j < i);
            }
            Message::CloseSaved => {
                self.menu = None;
                self.close_where(|_| true); // keeps dirty tabs
            }
            Message::CloseAll => {
                self.menu = None;
                self.close_where(|_| true); // drop the clean tabs first
                                            // Then prompt for the first remaining dirty tab (repeat to clear all).
                if let Some(id) = self.tabs.iter().find(|t| t.dirty).map(|t| t.id) {
                    self.modal = Some(Modal::ConfirmClose { id });
                }
            }
            Message::RevertTab(i) => {
                self.menu = None;
                if let Some(tab) = self.tabs.get_mut(i) {
                    if let Ok(f) = bru_lang::parse(&tab.saved_text) {
                        tab.file = f;
                        tab.recompute_dirty();
                        load_editors_for(tab);
                    }
                }
            }
            Message::CopyTabPath(i) => {
                self.menu = None;
                if let Some(p) = self.tabs.get(i).and_then(|t| t.path.clone()) {
                    return iced::clipboard::write(p.to_string_lossy().into_owned());
                }
            }
            Message::CloneTab(i) => {
                self.menu = None;
                if let Some(path) = self.tabs.get(i).and_then(|t| t.path.clone()) {
                    return Task::done(Message::ClonePrompt(path, false));
                }
            }

            // ── modals ──
            Message::ModalName(v) => self.modal_set_name(v),
            Message::ModalUrl(v) => {
                if let Some(Modal::NewRequest { url, .. }) = &mut self.modal {
                    *url = v;
                }
            }
            Message::ModalMethod(v) => {
                if let Some(Modal::NewRequest { method, .. }) = &mut self.modal {
                    *method = v;
                }
            }
            Message::ModalSubmit => return self.submit_modal(),
            Message::ModalCancel => self.modal = None,
            Message::OpenPalette => {
                self.menu = None;
                if self.modal.is_none() && self.env_editor.is_none() && self.runner.is_none() {
                    self.modal = Some(Modal::Palette {
                        query: String::new(),
                        selected: 0,
                    });
                }
            }
            Message::PaletteQuery(q) => {
                if let Some(Modal::Palette { query, selected }) = &mut self.modal {
                    *query = q;
                    *selected = 0;
                }
            }
            Message::PaletteMove(d) => {
                let n = self.palette_results().len();
                if let Some(Modal::Palette { selected, .. }) = &mut self.modal {
                    if n > 0 {
                        let s = *selected as i32 + d;
                        *selected = s.rem_euclid(n as i32) as usize;
                    }
                }
            }

            // ── response actions ──
            Message::CopyResponse => {
                if let Some(text) = self
                    .active
                    .and_then(|i| self.tabs[i].result.as_ref())
                    .and_then(|o| o.response.as_ref())
                    .map(pretty_body)
                {
                    return iced::clipboard::write(text);
                }
            }
            Message::DownloadResponse => {
                if let Some(resp) = self
                    .active
                    .and_then(|i| self.tabs[i].result.as_ref())
                    .and_then(|o| o.response.as_ref())
                {
                    if let Some(path) = rfd::FileDialog::new().set_file_name("response").save_file()
                    {
                        match std::fs::write(&path, &resp.body) {
                            Ok(()) => self.status = format!("Saved {}", path.display()),
                            Err(e) => self.status = format!("Save failed: {e}"),
                        }
                    }
                }
            }
            Message::ClearResponse => {
                if let Some(i) = self.active {
                    self.tabs[i].result = None;
                    self.status.clear();
                }
            }
            Message::ToggleLayout => self.layout_horizontal = !self.layout_horizontal,
            Message::RevealLarge => {
                if let Some(i) = self.active {
                    self.tabs[i].reveal_large = true;
                }
            }
            Message::RespFormatChanged(v) => {
                if let Some(i) = self.active {
                    self.tabs[i].resp_format = match v.as_str() {
                        "raw" => RespFormat::Raw,
                        "hex" => RespFormat::Hex,
                        "tree" => RespFormat::Tree,
                        _ => RespFormat::Pretty,
                    };
                    self.tabs[i].rebuild_resp_editor();
                }
            }
            Message::RespEditorAction(action) => {
                // Read-only: allow selection/scroll, ignore edits.
                if let Some(i) = self.active {
                    if !action.is_edit() {
                        self.tabs[i].resp_editor.perform(action);
                    }
                }
            }
            Message::ToggleJsonNode(path) => {
                if let Some(i) = self.active {
                    if !self.tabs[i].resp_expanded.remove(&path) {
                        self.tabs[i].resp_expanded.insert(path);
                    }
                }
            }
            Message::RespFilter(q) => {
                if let Some(i) = self.active {
                    self.tabs[i].resp_filter = q;
                }
            }
            Message::SaveExamplePrompt => {
                if let Some(i) = self.active {
                    let n = example_count(&self.tabs[i].file) + 1;
                    self.modal = Some(Modal::SaveExample {
                        name: format!("Example {n}"),
                    });
                }
            }
            Message::TagInput(v) => {
                if let Some(i) = self.active {
                    self.tabs[i].tag_input = v;
                }
            }
            Message::AddTag => {
                if let Some(i) = self.active {
                    let tag = self.tabs[i].tag_input.trim().to_string();
                    if !tag.is_empty() {
                        let mut tags = edit::meta_tags(&self.tabs[i].file);
                        if !tags.contains(&tag) {
                            tags.push(tag);
                            edit::set_meta_tags(&mut self.tabs[i].file, tags);
                            self.tabs[i].recompute_dirty();
                        }
                        self.tabs[i].tag_input.clear();
                    }
                }
            }
            Message::RemoveTag(idx) => {
                if let Some(i) = self.active {
                    let mut tags = edit::meta_tags(&self.tabs[i].file);
                    if idx < tags.len() {
                        tags.remove(idx);
                        edit::set_meta_tags(&mut self.tabs[i].file, tags);
                        self.tabs[i].recompute_dirty();
                    }
                }
            }
            Message::BrowseMultipartFile(idx) => {
                if let Some(p) = rfd::FileDialog::new().pick_file() {
                    let val = format!("@file({})", p.to_string_lossy());
                    self.mutate(|f| edit::set_entry_value(f, "body:multipart-form", idx, &val));
                }
            }
            Message::ToggleBulk(section) => {
                if let Some(i) = self.active {
                    if self.tabs[i].bulk == Some(section) {
                        self.tabs[i].bulk = None; // back to table (already committed live)
                    } else {
                        let text = bulk_text(&self.tabs[i].file, section.block());
                        self.tabs[i].bulk_editor = text_editor::Content::with_text(&text);
                        self.tabs[i].bulk = Some(section);
                    }
                }
            }
            Message::BulkEdit(action) => {
                if let Some(i) = self.active {
                    self.tabs[i].bulk_editor.perform(action);
                    if let Some(section) = self.tabs[i].bulk {
                        let text = self.tabs[i].bulk_editor.text();
                        let rows = parse_bulk(&text);
                        edit::replace_block_entries(&mut self.tabs[i].file, section.block(), rows);
                        self.tabs[i].recompute_dirty();
                    }
                }
            }
            Message::OpenInBrowser => {
                if let Some(resp) = self
                    .active
                    .and_then(|i| self.tabs[i].result.as_ref())
                    .and_then(|o| o.response.as_ref())
                {
                    open_response_in_browser(resp);
                }
            }
            Message::BrowseFileBody => {
                if let Some(p) = rfd::FileDialog::new().pick_file() {
                    let path = p.to_string_lossy().into_owned();
                    let ct = self.active_file_body().and_then(|i| i.content_type.clone());
                    self.mutate(|f| edit::set_file_body(f, &path, ct.as_deref()));
                }
            }
            Message::FileBodyContentType(v) => {
                if let Some(item) = self.active_file_body() {
                    let ct = if v.trim().is_empty() { None } else { Some(v) };
                    self.mutate(|f| edit::set_file_body(f, &item.path, ct.as_deref()));
                }
            }

            // ── environment manager ──
            Message::OpenEnvEditor => {
                self.menu = None;
                let ed = match self.envs.first().cloned() {
                    Some(first) => {
                        let rows = self.load_env_rows(&first);
                        EnvEditor {
                            selected: first.clone(),
                            rename_buf: first,
                            rows,
                            error: None,
                        }
                    }
                    None => EnvEditor::default(),
                };
                self.env_editor = Some(ed);
            }
            Message::EnvSelect(name) => {
                let rows = self.load_env_rows(&name);
                if let Some(ed) = &mut self.env_editor {
                    ed.rename_buf = name.clone();
                    ed.selected = name;
                    ed.rows = rows;
                    ed.error = None;
                }
            }
            Message::EnvName(i, v) => {
                if let Some(ed) = &mut self.env_editor {
                    if let Some(r) = ed.rows.get_mut(i) {
                        r.name = v;
                    }
                }
            }
            Message::EnvValue(i, v) => {
                if let Some(ed) = &mut self.env_editor {
                    if let Some(r) = ed.rows.get_mut(i) {
                        r.value = v;
                    }
                }
            }
            Message::EnvToggle(i, on) => {
                if let Some(ed) = &mut self.env_editor {
                    if let Some(r) = ed.rows.get_mut(i) {
                        r.enabled = on;
                    }
                }
            }
            Message::EnvSecret(i, on) => {
                if let Some(ed) = &mut self.env_editor {
                    if let Some(r) = ed.rows.get_mut(i) {
                        r.secret = on;
                    }
                }
            }
            Message::EnvAddRow => {
                if let Some(ed) = &mut self.env_editor {
                    ed.rows.push(fsops::EnvRow {
                        enabled: true,
                        ..Default::default()
                    });
                }
            }
            Message::EnvRemoveRow(i) => {
                if let Some(ed) = &mut self.env_editor {
                    if i < ed.rows.len() {
                        ed.rows.remove(i);
                    }
                }
            }
            Message::EnvSave => {
                if let (Some(dir), Some(ed)) = (self.collection_dir.clone(), &mut self.env_editor) {
                    if ed.selected.is_empty() {
                        ed.error = Some("Select or create an environment first".to_string());
                    } else {
                        match fsops::save_env(&dir, &ed.selected, &ed.rows) {
                            Ok(()) => ed.error = None,
                            Err(e) => ed.error = Some(e),
                        }
                        self.envs = scan_envs(&dir);
                        self.refresh_vars();
                    }
                }
            }
            Message::EnvClose => self.env_editor = None,
            Message::EnvNew => {
                if let Some(dir) = self.collection_dir.clone() {
                    let mut name = "New Environment".to_string();
                    let mut n = 1;
                    while self.envs.iter().any(|e| e == &name) {
                        n += 1;
                        name = format!("New Environment {n}");
                    }
                    match fsops::create_env(&dir, &name) {
                        Ok(()) => {
                            self.envs = scan_envs(&dir);
                            let rows = self.load_env_rows(&name);
                            if let Some(ed) = &mut self.env_editor {
                                ed.rename_buf = name.clone();
                                ed.selected = name;
                                ed.rows = rows;
                                ed.error = None;
                            }
                        }
                        Err(e) => {
                            if let Some(ed) = &mut self.env_editor {
                                ed.error = Some(e);
                            }
                        }
                    }
                }
            }
            Message::EnvDelete(name) => {
                if let Some(dir) = self.collection_dir.clone() {
                    let _ = fsops::delete_env(&dir, &name);
                    self.envs = scan_envs(&dir);
                    if self.selected_env.as_deref() == Some(name.as_str()) {
                        self.selected_env = None;
                        self.refresh_vars();
                    }
                    if let Some(ed) = &mut self.env_editor {
                        if ed.selected == name {
                            ed.selected = self.envs.first().cloned().unwrap_or_default();
                        }
                    }
                    let sel = self
                        .env_editor
                        .as_ref()
                        .map(|e| e.selected.clone())
                        .unwrap_or_default();
                    let rows = self.load_env_rows(&sel);
                    if let Some(ed) = &mut self.env_editor {
                        ed.rows = rows;
                        // Keep the rename buffer pointed at the new selection, so a
                        // later Rename doesn't retarget the just-deleted env's name.
                        ed.rename_buf = ed.selected.clone();
                    }
                }
            }
            Message::EnvDuplicate(name) => {
                if let Some(dir) = self.collection_dir.clone() {
                    let _ = fsops::duplicate_env(&dir, &name);
                    self.envs = scan_envs(&dir);
                }
            }
            Message::EnvRenameBuf(v) => {
                if let Some(ed) = &mut self.env_editor {
                    ed.rename_buf = v;
                }
            }
            Message::EnvRenameApply => {
                if let Some(dir) = self.collection_dir.clone() {
                    let (old, new) = self
                        .env_editor
                        .as_ref()
                        .map(|e| (e.selected.clone(), e.rename_buf.trim().to_string()))
                        .unwrap_or_default();
                    if !old.is_empty() && !new.is_empty() && old != new {
                        match fsops::rename_env(&dir, &old, &new) {
                            Ok(()) => {
                                self.envs = scan_envs(&dir);
                                if self.selected_env.as_deref() == Some(old.as_str()) {
                                    self.selected_env = Some(new.clone());
                                    self.refresh_vars();
                                }
                                let rows = self.load_env_rows(&new);
                                if let Some(ed) = &mut self.env_editor {
                                    ed.selected = new.clone();
                                    ed.rename_buf = new;
                                    ed.rows = rows;
                                    ed.error = None;
                                }
                            }
                            Err(e) => {
                                if let Some(ed) = &mut self.env_editor {
                                    ed.error = Some(e);
                                }
                            }
                        }
                    }
                }
            }

            // ── runner ──
            Message::RunFolder(dir) => {
                self.menu = None;
                let title = dir
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| "Collection".to_string());
                self.runner = Some(Runner {
                    title,
                    running: true,
                    results: Vec::new(),
                });
                let files = self.requests_under(&dir);
                let env = self.selected_env.clone();
                let dev = self.developer_mode;
                let opts = self.prefs.send_options();
                return Task::perform(run_folder(files, dir, env, dev, opts), Message::RunnerDone);
            }
            Message::RunnerDone(results) => {
                if let Some(r) = &mut self.runner {
                    r.running = false;
                    r.results = results;
                }
            }
            Message::RunnerClose => self.runner = None,

            // ── devtools ──
            Message::ToggleConsole => self.console_open = !self.console_open,
            Message::ClearConsole => {
                self.console.clear();
                self.network.clear();
            }
            Message::DevtoolsTab(t) => {
                self.devtools_tab = t;
                self.console_open = true;
            }

            // ── cookies ──
            Message::OpenCookies => {
                self.menu = None;
                self.cookies_open = true;
            }
            Message::CloseCookies => self.cookies_open = false,
            Message::DeleteCookie(i) => {
                if i < self.cookies.len() {
                    self.cookies.remove(i);
                }
            }
            Message::ClearCookies => self.cookies.clear(),

            // ── preferences ──
            Message::OpenPrefs => {
                self.menu = None;
                self.modal = Some(Modal::Prefs);
            }
            Message::PrefTimeout(v) => {
                // Only commit a valid number; an empty/garbage field leaves the
                // setting unchanged (so it can't silently degrade to a 1s timeout).
                if let Ok(n) = v.trim().parse::<u64>() {
                    self.prefs.timeout_secs = n;
                    save_prefs(&self.prefs);
                }
            }
            Message::PrefInsecure(b) => {
                self.prefs.insecure = b;
                save_prefs(&self.prefs);
            }
            Message::ToggleTheme(light) => {
                self.prefs.light = light;
                theme::set_light(light);
                save_prefs(&self.prefs);
            }

            // ── keyboard ──
            Message::Key(event) => return self.handle_key(event),
        }
        Task::none()
    }

    /// Apply a mutation to the active request's file (no-op if none open).
    fn mutate(&mut self, f: impl FnOnce(&mut BruFile)) {
        if let Some(i) = self.active {
            f(&mut self.tabs[i].file);
            self.tabs[i].recompute_dirty();
        }
    }

    /// Re-parse the Source editor into the file if it parses cleanly; otherwise
    /// keep the last good file but warn (so a tab switch isn't silently lossy).
    fn commit_source(&mut self, i: usize) {
        let text = self.tabs[i].editors.source.text();
        match bru_lang::parse(&text) {
            Ok(f) => {
                self.tabs[i].file = f;
                self.tabs[i].source_invalid = false;
                self.tabs[i].recompute_dirty();
            }
            Err(e) => self.status = format!("Source not committed - {e}"),
        }
    }

    fn save_tab(&mut self, i: usize) {
        // In the Source tab, validate the raw buffer into the file first; then,
        // like every other path, persist the *canonical* serialization so the
        // dirty flag (serialize(file) == saved_text) clears after a save.
        let source_tab = self.tabs[i].req_tab == ReqTab::Source;
        if source_tab {
            let raw = self.tabs[i].editors.source.text();
            match bru_lang::parse(&raw) {
                Ok(f) => self.tabs[i].file = f,
                Err(e) => {
                    self.status = format!("Not saved - {e}");
                    return;
                }
            }
        }
        let text = bru_lang::serialize(&self.tabs[i].file);

        // A draft has no file yet: prompt for a location (native save dialog).
        let path = match self.tabs[i].path.clone() {
            Some(p) => p,
            None => {
                let mut dlg = rfd::FileDialog::new().set_file_name("request.bru");
                if let Some(dir) = &self.collection_dir {
                    dlg = dlg.set_directory(dir);
                }
                match dlg.save_file() {
                    Some(p) => {
                        let p = if p.extension().is_some() {
                            p
                        } else {
                            p.with_extension("bru")
                        };
                        self.tabs[i].path = Some(p.clone());
                        p
                    }
                    None => return,
                }
            }
        };

        match std::fs::write(&path, &text) {
            Ok(()) => {
                // Re-render the Source buffer to the canonical text just written,
                // so the visible source matches disk and dirty stays clear.
                if source_tab {
                    self.tabs[i].editors.source = text_editor::Content::with_text(&text);
                    self.tabs[i].source_invalid = false;
                }
                self.tabs[i].saved_text = text;
                self.tabs[i].recompute_dirty();
                self.status = "Saved".to_string();
                self.reload_tree();
            }
            Err(e) => self.status = format!("Not saved - {e}"),
        }
    }

    /// Remove the tab at `i` and fix the active index.
    fn remove_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        self.tabs.remove(i);
        self.active = if self.tabs.is_empty() {
            None
        } else {
            Some(i.min(self.tabs.len() - 1))
        };
    }

    /// Close tabs whose index matches `pred`, but keep any with unsaved edits
    /// (so bulk close never silently drops work).
    fn close_where(&mut self, pred: impl Fn(usize) -> bool) {
        let active_id = self.active.and_then(|i| self.tabs.get(i)).map(|t| t.id);
        let mut idx = 0;
        self.tabs.retain(|t| {
            let keep = !pred(idx) || t.dirty;
            idx += 1;
            keep
        });
        self.active = match active_id.and_then(|id| self.tabs.iter().position(|t| t.id == id)) {
            Some(i) => Some(i),
            None => (!self.tabs.is_empty()).then_some(0),
        };
    }

    /// Request paths under `dir`, in the exact order the sidebar shows them
    /// (the loader's seq+name ordering) — so the runner matches what the user
    /// sees and chains variables correctly.
    fn requests_under(&self, dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let Some(tree) = &self.collection else {
            return out;
        };
        // The collection root vs a specific folder node.
        let start = if self.collection_dir.as_deref() == Some(dir) {
            Some(&tree.root)
        } else {
            find_folder(&tree.root, dir)
        };
        if let Some(folder) = start {
            collect_folder_requests(folder, &mut out);
        }
        out
    }

    /// The active request's selected file-body entry, if its body is a file.
    fn active_file_body(&self) -> Option<bru_core::FileBodyItem> {
        let i = self.active?;
        match self.tabs[i].file.to_request()?.body {
            Body::File(items) => items
                .iter()
                .find(|x| x.selected)
                .or_else(|| items.first())
                .cloned(),
            _ => None,
        }
    }

    /// The request paths directly inside `dir`, in current tree order.
    fn sibling_requests(&self, dir: &Path) -> Vec<PathBuf> {
        self.collection
            .as_ref()
            .and_then(|t| {
                if self.collection_dir.as_deref() == Some(dir) {
                    Some(&t.root)
                } else {
                    find_folder(&t.root, dir)
                }
            })
            .map(|f| f.requests.iter().map(|r| r.path.clone()).collect())
            .unwrap_or_default()
    }

    /// Renumber `meta.seq` across an ordered sibling list to lock in the order,
    /// resyncing any open tab so a later Save can't revert it, then reload.
    fn apply_order(&mut self, order: &[PathBuf]) {
        for (i, p) in order.iter().enumerate() {
            let seq = (i + 1) as i64;
            let _ = fsops::set_seq(p, seq);
            if let Some(t) = self
                .tabs
                .iter_mut()
                .find(|t| t.path.as_deref() == Some(p.as_path()))
            {
                let entries = edit::dict_block_mut(&mut t.file, "meta");
                edit::set_entry(entries, "seq", &seq.to_string());
                if let Ok(disk) = std::fs::read_to_string(p) {
                    t.saved_text = disk;
                }
                t.recompute_dirty();
            }
        }
        self.reload_tree();
    }

    /// Clear any in-progress sidebar drag (defensive: a lost mouse-release must
    /// not leave a stale drag that reorders on the next unrelated click).
    fn cancel_drag(&mut self) {
        self.dragging = None;
        self.drag_over = None;
    }

    /// Drop `src` immediately before `dst` (drag-and-drop). Only within one folder.
    fn reorder_to(&mut self, src: PathBuf, dst: PathBuf) {
        let (Some(sdir), Some(ddir)) = (src.parent(), dst.parent()) else {
            return;
        };
        if sdir != ddir || src == dst {
            return;
        }
        let mut order = self.sibling_requests(sdir);
        let Some(si) = order.iter().position(|p| p == &src) else {
            return;
        };
        let item = order.remove(si);
        let dpos = order.iter().position(|p| p == &dst).unwrap_or(order.len());
        order.insert(dpos, item);
        self.apply_order(&order);
    }

    /// Load an environment's variables into editable rows.
    fn load_env_rows(&self, name: &str) -> Vec<fsops::EnvRow> {
        if name.is_empty() {
            return Vec::new();
        }
        let Some(dir) = &self.collection_dir else {
            return Vec::new();
        };
        bru_lang::load_env(dir, name)
            .unwrap_or_default()
            .into_iter()
            .map(|v| fsops::EnvRow {
                name: v.name,
                value: v.value,
                enabled: v.enabled,
                secret: v.secret,
            })
            .collect()
    }

    /// Reload the collection tree from disk after a filesystem mutation, keeping
    /// open tabs and the current selection.
    fn reload_tree(&mut self) {
        if let Some(dir) = self.collection_dir.clone() {
            if let Ok(tree) = bru_lang::load_collection(&dir) {
                self.collection = Some(tree);
            }
            self.envs = scan_envs(&dir);
        }
        self.refresh_vars();
    }

    /// Recompute the cached collection + selected-environment variable map used
    /// for the URL hover preview. Cheap to call; reads at most two `.bru` files.
    fn refresh_vars(&mut self) {
        self.vars = match &self.collection_dir {
            Some(dir) => base_vars(dir, self.selected_env.as_deref()),
            None => HashMap::new(),
        };
    }

    fn modal_set_name(&mut self, v: String) {
        match &mut self.modal {
            Some(Modal::NewRequest { name, .. })
            | Some(Modal::NewFolder { name, .. })
            | Some(Modal::Rename { name, .. })
            | Some(Modal::Clone { name, .. })
            | Some(Modal::SaveExample { name }) => *name = v,
            _ => {}
        }
    }

    /// Apply the current modal (Create/Rename/Clone/Delete/Confirm-close).
    fn submit_modal(&mut self) -> Task<Message> {
        let Some(modal) = self.modal.take() else {
            return Task::none();
        };
        let at_root = |p: &Path| self.collection_dir.as_deref() == Some(p);
        match modal {
            Modal::NewRequest {
                dir,
                name,
                method,
                url,
                ..
            } => match fsops::new_request(&dir, &name, &method, &url) {
                Ok(p) => {
                    self.reload_tree();
                    self.open_request(p);
                }
                Err(e) => {
                    self.modal = Some(Modal::NewRequest {
                        dir,
                        name,
                        method,
                        url,
                        error: Some(e),
                    });
                }
            },
            Modal::NewFolder { parent, name, .. } => {
                match fsops::new_folder(&parent, &name, at_root(&parent)) {
                    Ok(_) => self.reload_tree(),
                    Err(e) => {
                        self.modal = Some(Modal::NewFolder {
                            parent,
                            name,
                            error: Some(e),
                        })
                    }
                }
            }
            Modal::Rename {
                path,
                is_folder,
                name,
                ..
            } => {
                let root = path.parent().map(at_root).unwrap_or(false);
                match fsops::rename(&path, is_folder, &name, root) {
                    Ok(newp) => {
                        self.repath_tabs(&path, &newp);
                        // A renamed file changes its meta.name on disk; sync any
                        // open tab so its title is right and a later Save doesn't
                        // overwrite the new name with the stale in-memory one.
                        if !is_folder {
                            if let Some(t) = self
                                .tabs
                                .iter_mut()
                                .find(|t| t.path.as_deref() == Some(newp.as_path()))
                            {
                                let entries = edit::dict_block_mut(&mut t.file, "meta");
                                edit::set_entry(entries, "name", name.trim());
                                if let Ok(disk) = std::fs::read_to_string(&newp) {
                                    t.saved_text = disk;
                                }
                                t.recompute_dirty();
                                load_editors_for(t);
                            }
                        }
                        self.reload_tree();
                    }
                    Err(e) => {
                        self.modal = Some(Modal::Rename {
                            path,
                            is_folder,
                            name,
                            error: Some(e),
                        })
                    }
                }
            }
            Modal::Clone {
                path,
                is_folder,
                name,
                ..
            } => match fsops::clone(&path, is_folder, &name) {
                Ok(_) => self.reload_tree(),
                Err(e) => {
                    self.modal = Some(Modal::Clone {
                        path,
                        is_folder,
                        name,
                        error: Some(e),
                    })
                }
            },
            Modal::Delete {
                path, is_folder, ..
            } => match fsops::delete(&path, is_folder) {
                Ok(()) => {
                    let active_id = self.active.and_then(|i| self.tabs.get(i)).map(|t| t.id);
                    self.tabs
                        .retain(|t| !t.path.as_deref().is_some_and(|p| p.starts_with(&path)));
                    self.active = active_id
                        .and_then(|id| self.tabs.iter().position(|t| t.id == id))
                        .or_else(|| (!self.tabs.is_empty()).then_some(0));
                    self.reload_tree();
                }
                Err(e) => self.status = e,
            },
            Modal::ConfirmClose { id } => {
                if let Some(i) = self.tabs.iter().position(|t| t.id == id) {
                    self.remove_tab(i);
                }
            }
            Modal::Palette { .. } => {
                // Enter in the palette opens the selected result.
                if let Some(p) = self
                    .palette_results()
                    .get(self.palette_selected())
                    .map(|r| r.1.clone())
                {
                    self.open_request(p);
                }
            }
            Modal::SaveExample { name } => {
                if let Some(i) = self.active {
                    let req = self.tabs[i].file.to_request();
                    let resp = self.tabs[i]
                        .result
                        .as_ref()
                        .and_then(|o| o.response.clone());
                    if let (Some(req), Some(resp)) = (req, resp) {
                        let text = build_example_text(name.trim(), &req, &resp);
                        edit::push_text_block(&mut self.tabs[i].file, "example", text);
                        self.tabs[i].req_tab = ReqTab::Examples;
                        self.tabs[i].recompute_dirty();
                        self.save_tab(i);
                    }
                }
            }
            Modal::Code { .. } | Modal::Prefs => {}
        }
        Task::none()
    }

    /// After a rename, point any open tab (and the collapsed set) at the new path.
    fn repath_tabs(&mut self, old: &Path, new: &Path) {
        for t in &mut self.tabs {
            if let Some(p) = &t.path {
                if p == old {
                    t.path = Some(new.to_path_buf());
                } else if let Ok(rest) = p.strip_prefix(old) {
                    t.path = Some(new.join(rest));
                }
            }
        }
        let moved: Vec<PathBuf> = self
            .collapsed
            .iter()
            .filter(|p| p.starts_with(old))
            .cloned()
            .collect();
        for p in moved {
            self.collapsed.remove(&p);
            if let Ok(rest) = p.strip_prefix(old) {
                self.collapsed.insert(new.join(rest));
            }
        }
    }

    fn palette_selected(&self) -> usize {
        match &self.modal {
            Some(Modal::Palette { selected, .. }) => *selected,
            _ => 0,
        }
    }

    /// Flattened, filtered request list for the command palette.
    fn palette_results(&self) -> Vec<(String, PathBuf)> {
        let query = match &self.modal {
            Some(Modal::Palette { query, .. }) => query.to_lowercase(),
            _ => String::new(),
        };
        let mut out = Vec::new();
        if let Some(tree) = &self.collection {
            collect_request_index(&tree.root, &mut out);
        }
        out.into_iter()
            .filter(|(name, _)| query.is_empty() || name.to_lowercase().contains(&query))
            .take(50)
            .collect()
    }

    fn handle_key(&mut self, event: iced::keyboard::Event) -> Task<Message> {
        let iced::keyboard::Event::KeyPressed { key, modifiers, .. } = event else {
            return Task::none();
        };
        // Esc closes the topmost overlay (menu > modal > runner > env editor).
        if key == Key::Named(Named::Escape) {
            if self.var_popup.is_some() {
                self.var_popup = None;
                return Task::none();
            }
            if self.menu.is_some() {
                self.menu = None;
                return Task::none();
            }
            if self.modal.is_some() {
                self.modal = None;
                return Task::none();
            }
            if self.runner.is_some() {
                self.runner = None;
                return Task::none();
            }
            if self.env_editor.is_some() {
                self.env_editor = None;
                return Task::none();
            }
        }
        // Enter submits a modal.
        if key == Key::Named(Named::Enter) && self.modal.is_some() {
            return self.submit_modal();
        }
        // Arrow keys move the command-palette selection.
        if matches!(self.modal, Some(Modal::Palette { .. })) {
            match key {
                Key::Named(Named::ArrowDown) => return Task::done(Message::PaletteMove(1)),
                Key::Named(Named::ArrowUp) => return Task::done(Message::PaletteMove(-1)),
                _ => {}
            }
        }
        // While any overlay is open, global shortcuts must not fire against the
        // hidden background tab (they'd save/send/close it silently). Esc + the
        // modal Enter/arrows above already handle overlay-local keys.
        if self.modal.is_some() || self.env_editor.is_some() || self.runner.is_some() {
            return Task::none();
        }
        let cmd = modifiers.command();
        if cmd {
            if let Key::Character(c) = &key {
                match c.as_str() {
                    "s" => {
                        if let Some(i) = self.active {
                            self.save_tab(i);
                        }
                    }
                    "w" => {
                        if let Some(i) = self.active {
                            return Task::done(Message::CloseTab(i));
                        }
                    }
                    "k" => return Task::done(Message::OpenPalette),
                    _ => {}
                }
            }
            if key == Key::Named(Named::Enter) {
                return Task::done(Message::Send);
            }
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            iced::keyboard::listen().map(Message::Key),
            // End a divider drag on any left-button release, even if the cursor
            // is no longer over the thin divider strip (mouse_area's on_release
            // only fires while hovering it).
            iced::event::listen_with(|event, _status, _id| match event {
                iced::event::Event::Mouse(iced::mouse::Event::ButtonReleased(
                    iced::mouse::Button::Left,
                )) => Some(Message::PointerUp),
                _ => None,
            }),
        ])
    }
}

/// Rebuild the multiline editor buffers needed by a tab's current sub-tab.
fn load_editors_for(tab: &mut Tab) {
    let req = tab.file.to_request();
    match tab.req_tab {
        ReqTab::Body => {
            if let Some(r) = &req {
                match &r.body {
                    Body::Json(s) | Body::Text(s) | Body::Xml(s) | Body::Sparql(s) => {
                        tab.editors.body = text_editor::Content::with_text(s);
                        tab.editors.body_kind = body_block_name(&r.body).to_string();
                    }
                    Body::GraphQl { query, variables } => {
                        tab.editors.gql_query = text_editor::Content::with_text(query);
                        tab.editors.gql_vars = text_editor::Content::with_text(variables);
                    }
                    _ => {}
                }
            }
        }
        ReqTab::Script => {
            tab.editors.script_pre =
                text_editor::Content::with_text(&tab.file.script_pre().unwrap_or_default());
            tab.editors.script_post =
                text_editor::Content::with_text(&tab.file.script_post().unwrap_or_default());
        }
        ReqTab::Tests => {
            tab.editors.tests =
                text_editor::Content::with_text(&tab.file.tests_script().unwrap_or_default());
        }
        ReqTab::Docs => {
            tab.editors.docs = text_editor::Content::with_text(&docs_text(&tab.file));
        }
        // Keep uncommitted unparseable edits: regenerating from `file` would
        // silently discard the user's in-progress (broken) source text.
        ReqTab::Source if !tab.source_invalid => {
            tab.editors.source = text_editor::Content::with_text(&bru_lang::serialize(&tab.file));
        }
        _ => {}
    }
}

// === view ====================================================================

impl App {
    fn view(&self) -> Element<'_, Message> {
        let mut center = column![self.request_tabs(), self.main_panel()]
            .width(Fill)
            .height(Fill);
        if self.console_open {
            center = center.push(self.console_panel());
        }
        let body = column![
            self.top_bar(),
            row![self.sidebar(), center].height(Fill),
            self.status_bar(),
        ];
        let base = container(body)
            .style(|_| panel(BG(), None))
            .width(Fill)
            .height(Fill);
        // Track the cursor (to anchor context menus) without blocking children.
        let base = mouse_area(base).on_move(Message::CursorMoved);

        let mut layers = stack![base];
        if let Some(menu) = &self.menu {
            layers = layers.push(self.menu_overlay(menu));
        }
        if let Some(vp) = &self.var_popup {
            layers = layers.push(self.var_popup_overlay(vp));
        }
        if let Some(modal) = &self.modal {
            layers = layers.push(self.modal_overlay(modal));
        }
        if let Some(ed) = &self.env_editor {
            layers = layers.push(self.env_overlay(ed));
        }
        if let Some(r) = &self.runner {
            layers = layers.push(self.runner_overlay(r));
        }
        if self.cookies_open {
            layers = layers.push(self.cookies_overlay());
        }
        layers.into()
    }

    fn runner_overlay<'a>(&'a self, r: &'a Runner) -> Element<'a, Message> {
        let passed = r.results.iter().filter(|x| x.passed).count();
        let total = r.results.len();
        let header = row![
            text(format!("Run: {}", r.title))
                .size(15)
                .color(TEXT())
                .font(BOLD),
            fill_x(),
            text(if r.running {
                "running...".to_string()
            } else {
                format!("{passed}/{total} passed")
            })
            .size(12)
            .color(if r.running {
                ACCENT()
            } else if passed == total {
                GREEN()
            } else {
                RED()
            }),
            button(text("Close").size(13).color(TEXT()))
                .style(|_, s| ghost_button(s))
                .padding(Padding::from([6, 14]))
                .on_press(Message::RunnerClose),
        ]
        .spacing(10)
        .align_y(Center);

        let mut list = Column::new().spacing(2);
        if r.running && r.results.is_empty() {
            list = list.push(text("Running requests...").size(12).color(MUTED()));
        }
        for res in &r.results {
            let (mark, c) = if res.passed {
                ("\u{2713}", GREEN())
            } else {
                ("\u{2717}", RED())
            };
            let detail = match &res.error {
                Some(e) => e.clone(),
                None => format!("{} \u{00B7} {} ms", res.status, res.ms),
            };
            list = list.push(
                row![
                    text(mark).size(12).color(c),
                    text(res.name.clone())
                        .size(12)
                        .color(TEXT())
                        .width(Length::FillPortion(2)),
                    text(detail)
                        .size(12)
                        .color(SUBTEXT())
                        .font(MONO)
                        .width(Length::FillPortion(3)),
                ]
                .spacing(10)
                .align_y(Center),
            );
        }

        let card = container(
            column![
                header,
                container(scrollable(list).height(Fill)).height(Fill)
            ]
            .spacing(12),
        )
        .style(|_| modal_card())
        .width(Length::Fixed(620.0))
        .height(Length::Fixed(460.0))
        .padding(16);

        let backdrop = opaque(
            mouse_area(
                container(Space::new())
                    .width(Fill)
                    .height(Fill)
                    .style(|_| scrim()),
            )
            .on_press(Message::RunnerClose),
        );
        stack![backdrop, container(opaque(card)).center(Fill).padding(40)].into()
    }

    /// The Cookies manager overlay: cookies captured from response `Set-Cookie`
    /// headers this session, with per-row delete and Clear All.
    fn cookies_overlay(&self) -> Element<'_, Message> {
        let header = row![
            text("Cookies").size(15).color(TEXT()).font(BOLD),
            fill_x(),
            button(text("Clear All").size(11).color(SUBTEXT()))
                .style(|_, s| icon_button(s, SUBTEXT()))
                .padding(Padding::from([2, 8]))
                .on_press(Message::ClearCookies),
            button(text("\u{00D7}").size(13).color(MUTED()))
                .style(|_, s| icon_button(s, MUTED()))
                .padding(Padding::from([2, 6]))
                .on_press(Message::CloseCookies),
        ]
        .spacing(8)
        .align_y(Center);

        let mut list = Column::new().spacing(4);
        if self.cookies.is_empty() {
            list = list.push(
                text("No cookies yet \u{2014} send a request that returns Set-Cookie.")
                    .size(12)
                    .color(MUTED()),
            );
        }
        for (i, c) in self.cookies.iter().enumerate() {
            list = list.push(
                row![
                    text(c.domain.clone())
                        .size(12)
                        .color(SUBTEXT())
                        .font(MONO)
                        .width(Length::FillPortion(2)),
                    text(c.name.clone())
                        .size(12)
                        .color(ACCENT())
                        .width(Length::FillPortion(2)),
                    text(c.value.clone())
                        .size(12)
                        .color(TEXT())
                        .font(MONO)
                        .width(Length::FillPortion(3)),
                    button(text("\u{00D7}").size(12).color(MUTED()))
                        .style(|_, s| icon_button(s, MUTED()))
                        .padding(Padding::from([0, 6]))
                        .on_press(Message::DeleteCookie(i)),
                ]
                .spacing(10)
                .align_y(Center),
            );
        }

        let card = container(
            column![
                header,
                container(scrollable(list).height(Fill)).height(Fill)
            ]
            .spacing(12),
        )
        .style(|_| modal_card())
        .width(Length::Fixed(680.0))
        .height(Length::Fixed(440.0))
        .padding(16);
        let backdrop = opaque(
            mouse_area(
                container(Space::new())
                    .width(Fill)
                    .height(Fill)
                    .style(|_| scrim()),
            )
            .on_press(Message::CloseCookies),
        );
        stack![backdrop, container(opaque(card)).center(Fill).padding(40)].into()
    }

    /// The bottom devtools dock with Console / Network sub-tabs.
    fn console_panel(&self) -> Element<'_, Message> {
        let tab_btn = |label: &str, t: DevTab| {
            let active = self.devtools_tab == t;
            button(
                text(label.to_string())
                    .size(12)
                    .color(if active { TEXT() } else { MUTED() }),
            )
            .style(move |_, _| tab_button(active))
            .padding(Padding::from([2, 8]))
            .on_press(Message::DevtoolsTab(t))
        };
        let header = row![
            tab_btn("Console", DevTab::Console),
            tab_btn("Network", DevTab::Network),
            fill_x(),
            button(text("Clear").size(11).color(SUBTEXT()))
                .style(|_, s| icon_button(s, SUBTEXT()))
                .padding(Padding::from([2, 6]))
                .on_press(Message::ClearConsole),
            button(text("\u{00D7}").size(13).color(MUTED()))
                .style(|_, s| icon_button(s, MUTED()))
                .padding(Padding::from([2, 6]))
                .on_press(Message::ToggleConsole),
        ]
        .spacing(6)
        .align_y(Center);

        let body: Element<'_, Message> = match self.devtools_tab {
            DevTab::Console => {
                let mut col = Column::new().spacing(1);
                if self.console.is_empty() {
                    col = col.push(text("Console is empty.").size(12).color(MUTED()));
                }
                for line in &self.console {
                    col = col.push(text(line.clone()).size(12).color(SUBTEXT()).font(MONO));
                }
                scrollable(col).height(Fill).into()
            }
            DevTab::Network => {
                let mut col = Column::new().spacing(1);
                if self.network.is_empty() {
                    col = col.push(text("No requests yet.").size(12).color(MUTED()));
                }
                for e in &self.network {
                    let status = if e.ok {
                        text(e.status.to_string())
                            .size(12)
                            .color(status_color(e.status))
                    } else {
                        text("ERR").size(12).color(RED())
                    };
                    col = col.push(
                        row![
                            text(short_method(&e.method))
                                .size(11)
                                .color(method_color(&e.method))
                                .font(MONO)
                                .width(46),
                            status.width(44),
                            text(format!("{} ms", e.ms))
                                .size(11)
                                .color(SUBTEXT())
                                .width(70),
                            text(human_size(e.size)).size(11).color(SUBTEXT()).width(70),
                            text(e.url.clone()).size(11).color(TEXT()).font(MONO),
                        ]
                        .spacing(8)
                        .align_y(Center),
                    );
                }
                scrollable(col).height(Fill).into()
            }
        };

        container(column![header, container(body).padding(6).height(Fill)].spacing(2))
            .style(|_| panel(MANTLE(), Some(BORDER1())))
            .width(Fill)
            .height(Length::Fixed(180.0))
            .padding(6)
            .into()
    }

    /// A click-anywhere catcher beneath an overlay so an outside click dismisses it.
    fn dismiss_layer(&self, msg: Message) -> Element<'_, Message> {
        opaque(
            mouse_area(container(Space::new()).width(Fill).height(Fill))
                .on_press(msg.clone())
                .on_right_press(msg),
        )
    }

    /// The floating context menu, anchored at the stored cursor position.
    fn menu_overlay<'a>(&'a self, menu: &'a MenuState) -> Element<'a, Message> {
        let items = self.menu_items(&menu.target);
        let mut col = Column::new().spacing(1).padding(4);
        for item in items {
            col = col.push(item);
        }
        let panel = container(col)
            .style(|_| menu_panel())
            .width(Length::Fixed(210.0));
        // Offset toward the click with leading spacers (clamped a little).
        let x = menu.at.x.max(0.0);
        let y = menu.at.y.max(0.0);
        let positioned = column![Space::new().height(y), row![Space::new().width(x), panel],];
        stack![self.dismiss_layer(Message::CloseMenu), positioned,].into()
    }

    /// The `{{variable}}` value popover: the resolved value plus a Copy button,
    /// anchored at the cursor. Click-opened (unlike the hover tooltip) so its
    /// Copy button is actually reachable.
    fn var_popup_overlay<'a>(&'a self, vp: &'a VarPopup) -> Element<'a, Message> {
        let mut label = String::from("{{");
        label.push_str(&vp.name);
        label.push_str("}}");
        let value_line: Element<'a, Message> = match &vp.value {
            Some(v) if v.is_empty() => text("(empty)").size(12).color(MUTED()).into(),
            Some(v) => text(v.clone()).size(12).font(MONO).color(TEXT()).into(),
            None => text("not set").size(12).color(RED()).into(),
        };
        let mut col =
            column![text(label).size(12).font(MONO).color(ACCENT()), value_line,].spacing(8);
        if let Some(v) = vp.value.clone() {
            col = col.push(
                button(text("Copy").size(12).color(TEXT()))
                    .style(|_, s| ghost_button(s))
                    .padding(Padding::from([4, 12]))
                    .on_press(Message::CopyVarValue(v)),
            );
        }
        // No full-screen backdrop: the pills beneath stay hoverable, so moving to
        // another `{{var}}` switches the popover. Leaving the panel closes it.
        let panel = mouse_area(
            container(col)
                .style(|_| menu_panel())
                .padding(10)
                .width(Length::Fixed(340.0)),
        )
        .on_exit(Message::CloseVarPopup);
        // Drop it just below the cursor so moving down enters the panel (rather
        // than the panel opening directly under the pointer).
        let x = self.cursor.x.max(0.0);
        let y = (self.cursor.y + 14.0).max(0.0);
        column![Space::new().height(y), row![Space::new().width(x), panel],].into()
    }

    /// The list of menu rows for a given right-click target.
    fn menu_items<'a>(&self, target: &MenuTarget) -> Vec<Element<'a, Message>> {
        let mut v: Vec<Element<'a, Message>> = Vec::new();
        match target {
            MenuTarget::Request(p) => {
                v.push(menu_row("Open", false, Message::OpenRequest(p.clone())));
                v.push(menu_row("Run", false, Message::RunItem(p.clone())));
                v.push(menu_sep());
                v.push(menu_row(
                    "Clone",
                    false,
                    Message::ClonePrompt(p.clone(), false),
                ));
                v.push(menu_row("Copy", false, Message::CopyItem(p.clone(), false)));
                v.push(menu_row(
                    "Rename",
                    false,
                    Message::RenamePrompt(p.clone(), false),
                ));
                v.push(menu_row(
                    "Generate Code",
                    false,
                    Message::GenerateCode(p.clone()),
                ));
                v.push(menu_sep());
                v.push(menu_row("Move Up", false, Message::MoveItem(p.clone(), -1)));
                v.push(menu_row(
                    "Move Down",
                    false,
                    Message::MoveItem(p.clone(), 1),
                ));
                v.push(menu_row(
                    "Reveal in Explorer",
                    false,
                    Message::RevealItem(p.clone()),
                ));
                v.push(menu_sep());
                v.push(menu_row(
                    "Delete",
                    true,
                    Message::DeletePrompt(p.clone(), false),
                ));
            }
            MenuTarget::Folder(p) => {
                v.push(menu_row(
                    "New Request",
                    false,
                    Message::NewRequestPrompt(p.clone()),
                ));
                v.push(menu_row(
                    "New Folder",
                    false,
                    Message::NewFolderPrompt(p.clone()),
                ));
                v.push(menu_row("Run", false, Message::RunFolder(p.clone())));
                v.push(menu_sep());
                v.push(menu_row(
                    "Clone",
                    false,
                    Message::ClonePrompt(p.clone(), true),
                ));
                v.push(menu_row("Copy", false, Message::CopyItem(p.clone(), true)));
                if self.clipboard_item.is_some() {
                    v.push(menu_row("Paste", false, Message::PasteItem(p.clone())));
                }
                v.push(menu_row(
                    "Rename",
                    false,
                    Message::RenamePrompt(p.clone(), true),
                ));
                v.push(menu_row(
                    "Settings",
                    false,
                    Message::OpenSettings(p.join("folder.bru"), TabKind::FolderSettings),
                ));
                v.push(menu_row(
                    "Reveal in Explorer",
                    false,
                    Message::RevealItem(p.clone()),
                ));
                v.push(menu_sep());
                v.push(menu_row(
                    "Delete",
                    true,
                    Message::DeletePrompt(p.clone(), true),
                ));
            }
            MenuTarget::Collection => {
                if let Some(dir) = self.collection_dir.clone() {
                    v.push(menu_row(
                        "New Request",
                        false,
                        Message::NewRequestPrompt(dir.clone()),
                    ));
                    v.push(menu_row(
                        "New Folder",
                        false,
                        Message::NewFolderPrompt(dir.clone()),
                    ));
                    v.push(menu_row("Run", false, Message::RunFolder(dir.clone())));
                    v.push(menu_sep());
                    if self.clipboard_item.is_some() {
                        v.push(menu_row("Paste", false, Message::PasteItem(dir.clone())));
                    }
                    v.push(menu_row(
                        "Settings",
                        false,
                        Message::OpenSettings(
                            dir.join("collection.bru"),
                            TabKind::CollectionSettings,
                        ),
                    ));
                    v.push(menu_row("Collapse All", false, Message::CollapseAll));
                    v.push(menu_row(
                        "Reveal in Explorer",
                        false,
                        Message::RevealItem(dir),
                    ));
                }
            }
            MenuTarget::Tab(i) => {
                let i = *i;
                let (has_path, dirty) = self
                    .tabs
                    .get(i)
                    .map(|t| (t.path.is_some(), t.dirty))
                    .unwrap_or((false, false));
                v.push(menu_row("Close", false, Message::CloseTab(i)));
                v.push(menu_row("Close Others", false, Message::CloseOthers(i)));
                v.push(menu_row(
                    "Close to the Right",
                    false,
                    Message::CloseRight(i),
                ));
                v.push(menu_row("Close to the Left", false, Message::CloseLeft(i)));
                v.push(menu_row("Close Saved", false, Message::CloseSaved));
                v.push(menu_row("Close All", false, Message::CloseAll));
                v.push(menu_sep());
                if dirty {
                    v.push(menu_row("Revert Changes", false, Message::RevertTab(i)));
                }
                if has_path {
                    v.push(menu_row("Clone", false, Message::CloneTab(i)));
                    v.push(menu_row("Copy Path", false, Message::CopyTabPath(i)));
                }
            }
            MenuTarget::Response => {
                let html = self
                    .active
                    .and_then(|i| self.tabs.get(i))
                    .and_then(|t| t.result.as_ref())
                    .and_then(|o| o.response.as_ref())
                    .map(|r| is_html_response(&r.headers))
                    .unwrap_or(false);
                v.push(menu_row("Copy", false, Message::CopyResponse));
                v.push(menu_row("Save to File", false, Message::DownloadResponse));
                v.push(menu_row(
                    "Save as Example",
                    false,
                    Message::SaveExamplePrompt,
                ));
                if html {
                    v.push(menu_row("Open in Browser", false, Message::OpenInBrowser));
                }
                v.push(menu_sep());
                v.push(menu_row("Clear", true, Message::ClearResponse));
            }
        }
        v
    }

    fn modal_overlay<'a>(&'a self, modal: &'a Modal) -> Element<'a, Message> {
        let card: Element<'a, Message> = match modal {
            Modal::NewRequest {
                name,
                method,
                url,
                error,
                ..
            } => modal_card_view(
                "New Request",
                column![
                    labeled(
                        "Name",
                        text_input("Request name", name)
                            .on_input(Message::ModalName)
                            .on_submit(Message::ModalSubmit)
                            .padding(8)
                            .style(input_style)
                    ),
                    labeled(
                        "Method",
                        dropdown(
                            pairs(METHODS),
                            method,
                            Length::Fixed(140.0),
                            Message::ModalMethod
                        )
                    ),
                    labeled(
                        "URL",
                        text_input("https://...", url)
                            .on_input(Message::ModalUrl)
                            .on_submit(Message::ModalSubmit)
                            .padding(8)
                            .font(MONO)
                            .style(input_style)
                    ),
                    modal_error(error),
                ]
                .spacing(10)
                .into(),
                "Create",
                false,
            ),
            Modal::NewFolder { name, error, .. } => modal_card_view(
                "New Folder",
                column![
                    labeled(
                        "Name",
                        text_input("Folder name", name)
                            .on_input(Message::ModalName)
                            .on_submit(Message::ModalSubmit)
                            .padding(8)
                            .style(input_style)
                    ),
                    modal_error(error),
                ]
                .spacing(10)
                .into(),
                "Create",
                false,
            ),
            Modal::Rename { name, error, .. } => modal_card_view(
                "Rename",
                column![
                    text_input("New name", name)
                        .on_input(Message::ModalName)
                        .on_submit(Message::ModalSubmit)
                        .padding(8)
                        .style(input_style),
                    modal_error(error),
                ]
                .spacing(10)
                .into(),
                "Rename",
                false,
            ),
            Modal::Clone { name, error, .. } => modal_card_view(
                "Clone",
                column![
                    text_input("Name for the copy", name)
                        .on_input(Message::ModalName)
                        .on_submit(Message::ModalSubmit)
                        .padding(8)
                        .style(input_style),
                    modal_error(error),
                ]
                .spacing(10)
                .into(),
                "Clone",
                false,
            ),
            Modal::Delete {
                name, is_folder, ..
            } => modal_card_view(
                "Delete",
                text(format!(
                    "Delete {} \"{}\"? This removes it from disk.",
                    if *is_folder { "folder" } else { "request" },
                    name
                ))
                .size(13)
                .color(TEXT())
                .into(),
                "Delete",
                true,
            ),
            Modal::ConfirmClose { id } => {
                let name = self
                    .tabs
                    .iter()
                    .find(|t| t.id == *id)
                    .map(|t| t.title())
                    .unwrap_or_default();
                modal_card_view(
                    "Unsaved Changes",
                    text(format!(
                        "\"{name}\" has unsaved changes. Close without saving?"
                    ))
                    .size(13)
                    .color(TEXT())
                    .into(),
                    "Don't Save",
                    true,
                )
            }
            Modal::Palette { query, selected } => self.palette_view(query, *selected),
            Modal::SaveExample { name } => modal_card_view(
                "Save as Example",
                text_input("example name", name)
                    .on_input(Message::ModalName)
                    .on_submit(Message::ModalSubmit)
                    .padding(8)
                    .style(input_style)
                    .into(),
                "Save",
                false,
            ),
            Modal::Prefs => modal_card_view(
                "Preferences",
                column![
                    labeled(
                        "Request timeout (seconds)",
                        text_input("30", &self.prefs.timeout_secs.to_string())
                            .on_input(Message::PrefTimeout)
                            .padding(8)
                            .style(input_style)
                            .width(Length::Fixed(120.0)),
                    ),
                    checkbox(self.prefs.insecure)
                        .label("Disable TLS certificate verification (insecure)")
                        .on_toggle(Message::PrefInsecure)
                        .size(15)
                        .text_size(13)
                        .style(checkbox_style),
                    checkbox(self.prefs.light)
                        .label("Light theme")
                        .on_toggle(Message::ToggleTheme)
                        .size(15)
                        .text_size(13)
                        .style(checkbox_style),
                    text("Saved automatically to ~/.bruno-rs.json")
                        .size(11)
                        .color(MUTED()),
                ]
                .spacing(12)
                .into(),
                "Done",
                false,
            ),
            Modal::Code { code } => {
                let footer = row![
                    fill_x(),
                    button(text("Copy").size(13).color(TEXT()))
                        .style(|_, s| ghost_button(s))
                        .padding(Padding::from([6, 14]))
                        .on_press(Message::CopyText(code.clone())),
                    button(text("Close").size(13).color(BLACK))
                        .style(|_, _| solid_button(ACCENT(), BLACK))
                        .padding(Padding::from([6, 16]))
                        .on_press(Message::ModalCancel),
                ]
                .spacing(8);
                container(
                    column![
                        text("Generate Code \u{00B7} curl")
                            .size(15)
                            .color(TEXT())
                            .font(BOLD),
                        container(scrollable(code_block(code)).height(Length::Fixed(300.0)))
                            .width(Fill),
                        footer,
                    ]
                    .spacing(12),
                )
                .style(|_| modal_card())
                .width(Length::Fixed(620.0))
                .padding(16)
                .into()
            }
        };

        let backdrop = opaque(
            mouse_area(
                container(Space::new())
                    .width(Fill)
                    .height(Fill)
                    .style(|_| scrim()),
            )
            .on_press(Message::ModalCancel),
        );
        stack![backdrop, container(opaque(card)).center(Fill).padding(40)].into()
    }

    fn palette_view<'a>(&'a self, query: &str, selected: usize) -> Element<'a, Message> {
        let results = self.palette_results();
        let mut list = Column::new().spacing(1);
        if results.is_empty() {
            list = list.push(text("No matching requests").size(12).color(MUTED()));
        }
        for (idx, (name, path)) in results.iter().enumerate() {
            let active = idx == selected;
            list = list.push(
                button(
                    text(name.clone())
                        .size(13)
                        .color(if active { TEXT() } else { SUBTEXT() }),
                )
                .style(move |_, _| {
                    menu_item(
                        if active {
                            button::Status::Hovered
                        } else {
                            button::Status::Active
                        },
                        false,
                    )
                })
                .width(Fill)
                .padding(Padding::from([5, 8]))
                .on_press(Message::OpenRequest(path.clone())),
            );
        }
        let card = column![
            text_input("Search requests...", query)
                .on_input(Message::PaletteQuery)
                .on_submit(Message::ModalSubmit)
                .padding(8)
                .size(14)
                .style(input_style),
            container(scrollable(list).height(Length::Fixed(320.0))).padding(Padding::from([8, 0])),
        ]
        .spacing(4);
        container(column![card])
            .style(|_| modal_card())
            .width(Length::Fixed(520.0))
            .padding(14)
            .into()
    }

    /// The environment-manager overlay: env list (left) + variables table (right).
    fn env_overlay<'a>(&'a self, ed: &'a EnvEditor) -> Element<'a, Message> {
        // Left: environment list with New / per-env duplicate+delete.
        let mut list = Column::new().spacing(2);
        list = list.push(
            button(text("+ New Environment").size(12).color(ACCENT()))
                .style(|_, s| icon_button(s, ACCENT()))
                .width(Fill)
                .padding(Padding::from([5, 8]))
                .on_press(Message::EnvNew),
        );
        for name in &self.envs {
            let active = ed.selected == *name;
            let row_el = row![
                button(
                    text(name.clone())
                        .size(12)
                        .color(if active { TEXT() } else { SUBTEXT() })
                )
                .style(move |_, s| sidebar_item(active, s))
                .width(Fill)
                .padding(Padding::from([4, 8]))
                .on_press(Message::EnvSelect(name.clone())),
                button(text("\u{29C9}").size(11).color(MUTED()))
                    .style(|_, s| icon_button(s, MUTED()))
                    .padding(Padding::from([4, 4]))
                    .on_press(Message::EnvDuplicate(name.clone())),
                button(text("\u{2715}").size(11).color(RED()))
                    .style(|_, s| icon_button(s, RED()))
                    .padding(Padding::from([4, 4]))
                    .on_press(Message::EnvDelete(name.clone())),
            ]
            .spacing(2)
            .align_y(Center);
            list = list.push(row_el);
        }
        let left = container(scrollable(list).height(Fill))
            .width(Length::Fixed(220.0))
            .height(Fill)
            .padding(4);

        // Right: variables table for the selected environment.
        let right: Element<'a, Message> = if ed.selected.is_empty() {
            container(
                text("Select or create an environment.")
                    .size(12)
                    .color(MUTED()),
            )
            .center(Fill)
            .into()
        } else {
            let rename_row = row![
                text("Name").size(11).color(MUTED()),
                text_input("environment name", &ed.rename_buf)
                    .on_input(Message::EnvRenameBuf)
                    .on_submit(Message::EnvRenameApply)
                    .size(12)
                    .padding(Padding::from([4, 8]))
                    .style(input_style)
                    .width(Length::Fixed(240.0)),
                button(text("Rename").size(12).color(TEXT()))
                    .style(|_, s| ghost_button(s))
                    .padding(Padding::from([4, 10]))
                    .on_press(Message::EnvRenameApply),
            ]
            .spacing(8)
            .align_y(Center);

            let mut table = Column::new().spacing(2);
            table = table.push(
                row![
                    hspace(24.0),
                    text("Name")
                        .size(11)
                        .color(MUTED())
                        .width(Length::FillPortion(2)),
                    text("Value")
                        .size(11)
                        .color(MUTED())
                        .width(Length::FillPortion(3)),
                    text("Secret")
                        .size(11)
                        .color(MUTED())
                        .width(Length::Fixed(56.0)),
                    hspace(28.0),
                ]
                .spacing(6)
                .align_y(Center),
            );
            for (i, r) in ed.rows.iter().enumerate() {
                let enabled = checkbox(r.enabled)
                    .on_toggle(move |b| Message::EnvToggle(i, b))
                    .size(14)
                    .style(checkbox_style);
                let name_in = text_input("name", &r.name)
                    .on_input(move |v| Message::EnvName(i, v))
                    .size(12)
                    .font(MONO)
                    .padding(Padding::from([4, 6]))
                    .style(cell_input)
                    .width(Length::FillPortion(2));
                let mut value_in = text_input("value", &r.value)
                    .on_input(move |v| Message::EnvValue(i, v))
                    .size(12)
                    .font(MONO)
                    .padding(Padding::from([4, 6]))
                    .style(cell_input)
                    .width(Length::FillPortion(3));
                if r.secret {
                    value_in = value_in.secure(true);
                }
                let secret = checkbox(r.secret)
                    .on_toggle(move |b| Message::EnvSecret(i, b))
                    .size(14)
                    .style(checkbox_style)
                    .width(Length::Fixed(56.0));
                let del = button(text("\u{2715}").size(12).color(MUTED()))
                    .style(|_, s| icon_button(s, RED()))
                    .padding(Padding::from([2, 6]))
                    .on_press(Message::EnvRemoveRow(i));
                table = table.push(
                    row![enabled, name_in, value_in, secret, del]
                        .spacing(6)
                        .align_y(Center),
                );
            }
            table = table.push(
                button(text("+ Add Variable").size(12).color(ACCENT()))
                    .style(|_, s| icon_button(s, ACCENT()))
                    .padding(Padding::from([4, 8]))
                    .on_press(Message::EnvAddRow),
            );
            column![
                rename_row,
                container(scrollable(table).height(Fill)).height(Fill),
                modal_error(&ed.error),
                row![
                    fill_x(),
                    button(text("Save").size(13).color(BLACK))
                        .style(|_, _| solid_button(ACCENT(), BLACK))
                        .padding(Padding::from([6, 16]))
                        .on_press(Message::EnvSave),
                ],
            ]
            .spacing(8)
            .height(Fill)
            .into()
        };

        let card = container(
            column![
                row![
                    text("Environments").size(15).color(TEXT()).font(BOLD),
                    fill_x(),
                    button(text("Close").size(13).color(TEXT()))
                        .style(|_, s| ghost_button(s))
                        .padding(Padding::from([6, 14]))
                        .on_press(Message::EnvClose),
                ]
                .align_y(Center),
                row![left, container(right).width(Fill).height(Fill).padding(4)]
                    .spacing(8)
                    .height(Fill),
            ]
            .spacing(12),
        )
        .style(|_| modal_card())
        .width(Length::Fixed(760.0))
        .height(Length::Fixed(480.0))
        .padding(16);

        let backdrop = opaque(
            mouse_area(
                container(Space::new())
                    .width(Fill)
                    .height(Fill)
                    .style(|_| scrim()),
            )
            .on_press(Message::EnvClose),
        );
        stack![backdrop, container(opaque(card)).center(Fill).padding(40)].into()
    }

    fn top_bar(&self) -> Element<'_, Message> {
        let name = self
            .collection
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_default();

        // Environment selector: "No Environment" + discovered env names.
        let mut env_pairs = vec![(String::new(), "No Environment".to_string())];
        for e in &self.envs {
            env_pairs.push((e.clone(), e.clone()));
        }
        let env_value = self.selected_env.clone().unwrap_or_default();
        let env_selector = dropdown(env_pairs, &env_value, Length::Fixed(180.0), |s| {
            if s.is_empty() {
                Message::SelectEnv(None)
            } else {
                Message::SelectEnv(Some(s))
            }
        });

        // Git branch chip (only when the collection lives in a repo).
        let branch: Element<'_, Message> = match &self.git_branch {
            Some(b) => row![
                text("\u{2387}").size(12).color(MUTED()),
                text(b.clone()).size(12).color(SUBTEXT()),
            ]
            .spacing(3)
            .align_y(Center)
            .into(),
            None => Space::new().into(),
        };

        // Collection-scoped run / settings icons (only with a collection open).
        let coll_actions: Element<'_, Message> = match &self.collection_dir {
            Some(dir) => row![
                tooltip(
                    button(text("\u{26A1}").size(14).color(SUBTEXT()))
                        .style(|_, s| icon_button(s, SUBTEXT()))
                        .padding(Padding::from([2, 6]))
                        .on_press(Message::RunFolder(dir.clone())),
                    container(text("Run collection").size(11))
                        .style(|_| menu_panel())
                        .padding(4),
                    tooltip::Position::Bottom,
                ),
                tooltip(
                    button(text("\u{2699}").size(14).color(SUBTEXT()))
                        .style(|_, s| icon_button(s, SUBTEXT()))
                        .padding(Padding::from([2, 6]))
                        .on_press(Message::OpenSettings(
                            dir.join("collection.bru"),
                            TabKind::CollectionSettings,
                        )),
                    container(text("Collection settings").size(11))
                        .style(|_| menu_panel())
                        .padding(4),
                    tooltip::Position::Bottom,
                ),
            ]
            .spacing(2)
            .align_y(Center)
            .into(),
            None => Space::new().into(),
        };

        container(
            row![
                button(text("Open Collection").size(13))
                    .style(|_, s| ghost_button(s))
                    .on_press(Message::OpenFolder),
                text(name).size(13).color(ACCENT()).font(BOLD),
                branch,
                coll_actions,
                fill_x(),
                checkbox(self.developer_mode)
                    .label("Dev Mode")
                    .on_toggle(Message::ToggleDevMode)
                    .size(14)
                    .text_size(12)
                    .style(checkbox_style),
                text("Env:").size(12).color(MUTED()),
                env_selector,
                tooltip(
                    button(text("\u{2699}").size(14).color(SUBTEXT()))
                        .style(|_, s| icon_button(s, SUBTEXT()))
                        .padding(Padding::from([2, 6]))
                        .on_press(Message::OpenEnvEditor),
                    container(text("Manage environments").size(11))
                        .style(|_| menu_panel())
                        .padding(4),
                    tooltip::Position::Bottom,
                ),
                button(text("Console").size(12).color(if self.console_open {
                    ACCENT()
                } else {
                    SUBTEXT()
                }))
                .style(|_, s| icon_button(s, SUBTEXT()))
                .padding(Padding::from([2, 8]))
                .on_press(Message::ToggleConsole),
                tooltip(
                    button(
                        text(if theme::is_light() {
                            "\u{263E}"
                        } else {
                            "\u{2600}"
                        })
                        .size(14)
                        .color(SUBTEXT())
                    )
                    .style(|_, s| icon_button(s, SUBTEXT()))
                    .padding(Padding::from([2, 6]))
                    .on_press(Message::ToggleTheme(!theme::is_light())),
                    container(text("Toggle light / dark theme").size(11))
                        .style(|_| menu_panel())
                        .padding(4),
                    tooltip::Position::Bottom,
                ),
                button(text("Prefs").size(12).color(SUBTEXT()))
                    .style(|_, s| icon_button(s, SUBTEXT()))
                    .padding(Padding::from([2, 8]))
                    .on_press(Message::OpenPrefs),
                text(self.status.as_str()).size(12).color(SUBTEXT()),
            ]
            .spacing(12)
            .align_y(Center)
            .padding(Padding::from([6, 12])),
        )
        .style(|_| panel(MANTLE(), Some(BORDER1())))
        .width(Fill)
        .into()
    }

    /// The bottom status bar: quick actions (Search / Cookies / Dev Tools) on the
    /// right and the app version, mirroring Bruno's footer.
    fn status_bar(&self) -> Element<'_, Message> {
        let foot_btn = |label: &str, active: bool, msg: Message| {
            button(text(label.to_string()).size(11).color(if active {
                ACCENT()
            } else {
                SUBTEXT()
            }))
            .style(|_, s| icon_button(s, SUBTEXT()))
            .padding(Padding::from([2, 8]))
            .on_press(msg)
        };
        container(
            row![
                fill_x(),
                foot_btn("Search", false, Message::OpenPalette),
                foot_btn("Cookies", self.cookies_open, Message::OpenCookies),
                foot_btn("Dev Tools", self.console_open, Message::ToggleConsole),
                text(concat!("v", env!("CARGO_PKG_VERSION")))
                    .size(11)
                    .color(MUTED()),
            ]
            .spacing(14)
            .align_y(Center)
            .padding(Padding::from([3, 12])),
        )
        .style(|_| panel(MANTLE(), Some(BORDER1())))
        .width(Fill)
        .into()
    }

    /// The strip of open-request tabs above the request pane, with a "+" button.
    fn request_tabs(&self) -> Element<'_, Message> {
        if self.tabs.is_empty() {
            return Space::new().into();
        }
        let mut strip = row![].spacing(0);
        for (i, tab) in self.tabs.iter().enumerate() {
            let active = self.active == Some(i);
            let method = tab.file.request_method().unwrap_or_default();
            let title = row![
                text(short_method(&method))
                    .size(10)
                    .color(method_color(&method))
                    .font(MONO),
                text(tab.title())
                    .size(12)
                    .color(if active { TEXT() } else { SUBTEXT() }),
                text(if tab.dirty { "\u{25CF}" } else { "" })
                    .size(10)
                    .color(ACCENT()),
            ]
            .spacing(6)
            .align_y(Center);

            let select = button(title)
                .style(move |_, s| request_tab(active, s))
                .padding(Padding::from([6, 10]))
                .on_press(Message::SelectTab(i));
            let close = button(text("\u{00D7}").size(14).color(MUTED()))
                .style(move |_, s| request_tab(active, s))
                .padding(Padding::from([6, 6]))
                .on_press(Message::CloseTab(i));
            // Right-click the tab for its menu; middle-click closes it.
            let tab_el = mouse_area(row![select, close])
                .on_right_press(Message::OpenMenu(MenuTarget::Tab(i)))
                .on_middle_press(Message::CloseTab(i));
            strip = strip.push(tab_el);
        }
        let plus = button(text("+").size(15).color(SUBTEXT()))
            .style(|_, s| icon_button(s, SUBTEXT()))
            .padding(Padding::from([4, 10]))
            .on_press(Message::NewDraft);
        strip = strip.push(plus);

        container(
            scrollable(strip).direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(4).scroller_width(4),
            )),
        )
        .style(|_| panel(MANTLE(), Some(BORDER1())))
        .width(Fill)
        .into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let mut col = Column::new().spacing(1).padding(Padding::from([8, 6]));
        match &self.collection {
            None => col = col.push(text("No collection loaded.").size(12).color(MUTED())),
            Some(tree) => {
                // Collection header: name + "+" new request + right-click menu.
                let header = mouse_area(
                    container(
                        row![
                            text(tree.name.as_str()).size(12).color(MUTED()).font(BOLD),
                            fill_x(),
                            button(text("+").size(14).color(SUBTEXT()))
                                .style(|_, s| icon_button(s, SUBTEXT()))
                                .padding(Padding::from([0, 6]))
                                .on_press_maybe(
                                    self.collection_dir.clone().map(Message::NewRequestPrompt),
                                ),
                        ]
                        .align_y(Center),
                    )
                    .padding(Padding::from([2, 2])),
                )
                .on_right_press(Message::OpenMenu(MenuTarget::Collection));
                col = col.push(header);

                // Search/filter box.
                col = col.push(
                    text_input("Search...", &self.search)
                        .on_input(Message::Search)
                        .size(12)
                        .padding(Padding::from([4, 6]))
                        .style(input_style),
                );
                col = col.push(Space::new().height(4));

                let active_path = self.active.and_then(|i| self.tabs[i].path.clone());
                let query = self.search.to_lowercase();
                let mut rows: Vec<Element<Message>> = Vec::new();
                self.collect_rows(&tree.root, 0, active_path.as_deref(), &query, &mut rows);
                for r in rows {
                    col = col.push(r);
                }
            }
        }
        container(scrollable(col).height(Fill))
            .style(|_| panel(BG(), Some(BORDER1())))
            .width(280)
            .height(Fill)
            .into()
    }

    fn collect_rows<'a>(
        &'a self,
        folder: &'a Folder,
        depth: u16,
        active: Option<&Path>,
        query: &str,
        out: &mut Vec<Element<'a, Message>>,
    ) {
        // Folders, seq then name.
        let mut folders: Vec<&Folder> = folder.folders.iter().collect();
        folders.sort_by_key(|f| f.name.to_lowercase());
        for sub in folders {
            if !query.is_empty() && !folder_matches(sub, query) {
                continue;
            }
            // A query forces folders open so matches are visible.
            let collapsed = query.is_empty() && self.collapsed.contains(&sub.path);
            let chevron = if collapsed { "\u{25B8}" } else { "\u{25BE}" };
            let label = row![
                text(chevron).size(11).color(MUTED()).width(14),
                text(sub.name.clone()).size(12).color(SUBTEXT()).font(BOLD),
            ]
            .spacing(2)
            .align_y(Center);
            let row_btn = button(label)
                .style(move |_, s| sidebar_item(false, s))
                .width(Fill)
                .padding(Padding::from([3, 6]))
                .on_press(Message::ToggleFolder(sub.path.clone()));
            out.push(indent(
                depth,
                mouse_area(row_btn)
                    .on_right_press(Message::OpenMenu(MenuTarget::Folder(sub.path.clone())))
                    .into(),
            ));
            if !collapsed {
                self.collect_rows(sub, depth + 1, active, query, out);
            }
        }
        // Requests, by seq then name.
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
            let method = req.method.clone().unwrap_or_default();
            let is_sel = active == Some(req.path.as_path());
            // Highlight the row currently hovered as a drag-and-drop target.
            let dropping =
                self.dragging.is_some() && self.drag_over.as_deref() == Some(req.path.as_path());
            let label = row![
                text(short_method(&method))
                    .size(11)
                    .color(method_color(&method))
                    .font(MONO)
                    .width(38),
                text(req.name.clone())
                    .size(12)
                    .color(if is_sel { TEXT() } else { SUBTEXT() }),
            ]
            .spacing(4)
            .align_y(Center);
            let hl = is_sel || dropping;
            let row_btn = button(label)
                .style(move |_, s| sidebar_item(hl, s))
                .width(Fill)
                .padding(Padding::from([3, 6]))
                .on_press(Message::OpenRequest(req.path.clone()));
            // A small grip starts a drag without triggering open (separate area).
            let grip = mouse_area(text("\u{283F}").size(11).color(MUTED()))
                .on_press(Message::SidebarDragStart(req.path.clone()));
            let row_el = row![grip, row_btn].spacing(3).align_y(Center);
            out.push(indent(
                depth,
                mouse_area(row_el)
                    .on_right_press(Message::OpenMenu(MenuTarget::Request(req.path.clone())))
                    .on_enter(Message::SidebarDragOver(req.path.clone()))
                    .on_exit(Message::SidebarDragOut(req.path.clone()))
                    .into(),
            ));
        }
    }

    fn main_panel(&self) -> Element<'_, Message> {
        let Some(i) = self.active else {
            return container(text("Select a request.").size(13).color(MUTED()))
                .center(Fill)
                .into();
        };
        let tab = &self.tabs[i];

        if tab.is_settings() {
            return self.settings_panel(tab);
        }

        let req = tab.file.to_request();

        // Editor-dominant tabs fill the pane (Bruno-style); a scrollable would
        // collapse their `Fill` height, so those bypass it. KV-table tabs scroll.
        let fills = match tab.req_tab {
            ReqTab::Source | ReqTab::Script | ReqTab::Tests | ReqTab::Docs => true,
            ReqTab::Body => matches!(
                req.as_ref().map(|r| &r.body),
                Some(
                    Body::Json(_)
                        | Body::Text(_)
                        | Body::Xml(_)
                        | Body::Sparql(_)
                        | Body::GraphQl { .. }
                )
            ),
            _ => false,
        };
        let inner = self.req_content(tab, req.as_ref());
        let req_area = if fills {
            container(inner).padding(10).height(Fill)
        } else {
            container(scrollable(inner).height(Fill))
                .padding(10)
                .height(Fill)
        };
        let resp = self.response_pane(tab);

        let split: Element<'_, Message> = if self.layout_horizontal {
            row![
                container(req_area)
                    .width(Length::FillPortion(1))
                    .height(Fill),
                container(resp).width(Length::FillPortion(1)).height(Fill),
            ]
            .height(Fill)
            .into()
        } else {
            // Draggable divider sets the request/response split ratio.
            let req_p = (self.split * 1000.0) as u16;
            let resp_p = ((1.0 - self.split) * 1000.0) as u16;
            let divider = mouse_area(
                container(Space::new().width(Fill).height(Length::Fixed(5.0)))
                    .style(|_| panel(BORDER2(), None))
                    .width(Fill),
            )
            .on_press(Message::SplitDragStart)
            .on_release(Message::PointerUp);
            column![
                container(req_area).height(Length::FillPortion(req_p)),
                divider,
                container(resp).height(Length::FillPortion(resp_p)),
            ]
            .height(Fill)
            .into()
        };

        let content = column![
            self.url_bar(tab, req.as_ref()),
            self.req_tab_strip(tab, req.as_ref()),
            split,
        ];
        container(content).width(Fill).height(Fill).into()
    }

    /// Collection/folder settings tab: a title+Save bar, a reduced sub-tab strip,
    /// and structured content projected straight from the `.bru` blocks.
    fn settings_panel<'a>(&'a self, tab: &'a Tab) -> Element<'a, Message> {
        let dot = if tab.dirty { "\u{25CF}" } else { "" };
        let bar = container(
            row![
                text(tab.title()).size(13).color(TEXT()).font(BOLD),
                text(dot).size(10).color(ACCENT()),
                fill_x(),
                button(text("Save").size(13).color(TEXT()))
                    .style(|_, s| ghost_button(s))
                    .padding(Padding::from([6, 12]))
                    .on_press(Message::Save),
            ]
            .spacing(8)
            .align_y(Center)
            .padding(8),
        )
        .style(|_| panel(MANTLE(), Some(BORDER1())))
        .width(Fill);

        let content = container(scrollable(self.settings_content(tab)).height(Fill))
            .padding(10)
            .height(Fill);

        container(column![bar, self.settings_tab_strip(tab), content])
            .width(Fill)
            .height(Fill)
            .into()
    }

    fn settings_tab_strip(&self, tab: &Tab) -> Element<'_, Message> {
        const TABS: &[ReqTab] = &[
            ReqTab::Headers,
            ReqTab::Vars,
            ReqTab::Auth,
            ReqTab::Script,
            ReqTab::Tests,
            ReqTab::Docs,
            ReqTab::Source,
        ];
        let mut r = row![]
            .spacing(2)
            .padding(Padding::from([0, 8]))
            .align_y(Center);
        for &t in TABS {
            let active = t == tab.req_tab;
            r = r.push(
                button(
                    text(t.label())
                        .size(12)
                        .color(if active { TEXT() } else { MUTED() }),
                )
                .style(move |_, _| tab_button(active))
                .padding(Padding::from([6, 10]))
                .on_press(Message::ReqTab(t)),
            );
        }
        if tab.req_tab == ReqTab::Auth {
            let mode = tab.file.dict_value("auth", "mode").unwrap_or("none");
            r = r.push(fill_x());
            r = r.push(dropdown(
                pairs(AUTH_MODES),
                mode,
                Length::Fixed(150.0),
                Message::AuthModeChanged,
            ));
        }
        container(r)
            .style(|_| panel(SURFACE0(), Some(BORDER2())))
            .width(Fill)
            .padding(Padding::from([2, 0]))
            .into()
    }

    fn settings_content<'a>(&'a self, tab: &'a Tab) -> Element<'a, Message> {
        match tab.req_tab {
            ReqTab::Headers => self.kv_or_bulk(
                tab,
                KvSection::Headers,
                "Name",
                "Value",
                block_kv_rows(&tab.file, "headers"),
            ),
            ReqTab::Vars => column![
                section("Pre Request"),
                self.vars_or_bulk(
                    tab,
                    KvSection::VarsPre,
                    block_var_rows(&tab.file, "vars:pre-request")
                ),
                vspace(12.0),
                section("Post Response"),
                self.vars_or_bulk(
                    tab,
                    KvSection::VarsPost,
                    block_var_rows(&tab.file, "vars:post-response")
                ),
            ]
            .spacing(4)
            .into(),
            ReqTab::Auth => {
                let mode = tab.file.dict_value("auth", "mode").unwrap_or("none");
                auth_view(&tab.file.project_auth(mode))
            }
            ReqTab::Script => column![
                section("Pre Request"),
                editor_box(
                    &tab.editors.script_pre,
                    EditorField::ScriptPre,
                    "js",
                    FIXED_EDITOR
                ),
                vspace(12.0),
                section("Post Response"),
                editor_box(
                    &tab.editors.script_post,
                    EditorField::ScriptPost,
                    "js",
                    FIXED_EDITOR
                ),
            ]
            .spacing(4)
            .into(),
            ReqTab::Tests => editor_box(&tab.editors.tests, EditorField::Tests, "js", FIXED_EDITOR),
            ReqTab::Docs => editor_box(&tab.editors.docs, EditorField::Docs, "md", FIXED_EDITOR),
            // Inside the settings scrollable, `Fill` collapses to ~1 line, so the
            // Source editor takes a fixed height like its sibling editors above.
            ReqTab::Source => container(
                text_editor(&tab.editors.source)
                    .font(MONO)
                    .height(FIXED_EDITOR)
                    .on_action(Message::SourceEdit),
            )
            .height(FIXED_EDITOR)
            .into(),
            _ => text("Not available here.").size(12).color(MUTED()).into(),
        }
    }

    fn url_bar(&self, tab: &Tab, req: Option<&Request>) -> Element<'_, Message> {
        let method = req.map(|r| r.method.to_uppercase()).unwrap_or_default();
        let url = req.map(|r| r.url.clone()).unwrap_or_default();
        let can_send = !tab.sending;

        let method_dd = dropdown(
            pairs(METHODS),
            &method,
            Length::Fixed(110.0),
            Message::MethodChanged,
        );

        let url_input = text_input("Enter URL", &url)
            .on_input(Message::UrlChanged)
            .font(MONO)
            .size(13)
            .padding(Padding::from([6, 10]))
            .style(input_style)
            .width(Fill);

        let send = button(
            text(if tab.sending {
                "Sending..."
            } else {
                "Send \u{2192}"
            })
            .size(13)
            .color(BLACK),
        )
        .style(|_, _| solid_button(ACCENT(), BLACK))
        .padding(Padding::from([6, 16]))
        .on_press_maybe(can_send.then_some(Message::Send));

        let code_btn = tooltip(
            button(text("</>").size(13).color(SUBTEXT()))
                .style(|_, s| icon_button(s, SUBTEXT()))
                .padding(Padding::from([6, 8]))
                .on_press(Message::GenerateCodeActive),
            container(text("Generate code (curl)").size(11))
                .style(|_| menu_panel())
                .padding(4),
            tooltip::Position::Bottom,
        );

        let bar = container(
            row![
                method_dd,
                url_input,
                code_btn,
                button(text("Save").size(13).color(TEXT()))
                    .style(|_, s| ghost_button(s))
                    .padding(Padding::from([6, 12]))
                    .on_press(Message::Save),
                send,
            ]
            .spacing(8)
            .align_y(Center)
            .padding(8),
        )
        .style(|_| panel(MANTLE(), Some(BORDER1())))
        .width(Fill);

        // Show an interpolated preview of any `{{var}}` in the URL: each token is
        // a pill whose tooltip reveals the resolved value (env + request scope).
        let preview = var_preview(&url, |name: &str| {
            if let Some(r) = req {
                if let Some(v) = r.vars_pre.iter().find(|v| v.enabled && v.name == name) {
                    return Some(v.value.clone());
                }
            }
            self.vars.get(name).cloned()
        });

        match preview {
            Some(p) => column![bar, p].spacing(2).into(),
            None => bar.into(),
        }
    }

    fn req_tab_strip(&self, tab: &Tab, req: Option<&Request>) -> Element<'_, Message> {
        let mut tabs = row![].spacing(2).align_y(Center);
        for t in ReqTab::ALL {
            // Hide the Examples tab unless the request actually has examples.
            if t == ReqTab::Examples && example_count(&tab.file) == 0 {
                continue;
            }
            let active = t == tab.req_tab;
            let mut label =
                row![text(t.label())
                    .size(12)
                    .color(if active { TEXT() } else { MUTED() })]
                .spacing(3)
                .align_y(Center);
            if let Some(ind) = self.tab_indicator(t, tab, req) {
                label = label.push(ind);
            }
            tabs = tabs.push(
                button(label)
                    .style(move |_, _| tab_button(active))
                    .padding(Padding::from([6, 10]))
                    .on_press(Message::ReqTab(t)),
            );
        }

        // The tab strip scrolls horizontally when it can't fit (Bruno's ">>"
        // overflow); the Body/Auth mode selector stays pinned on the right.
        let scroller = container(
            scrollable(tabs).direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new().width(3).scroller_width(3),
            )),
        )
        .width(Fill);
        let mut r = row![scroller]
            .spacing(2)
            .padding(Padding::from([0, 8]))
            .align_y(Center);
        if tab.req_tab == ReqTab::Body {
            let mode = req.map(|x| body_mode_value(&x.body)).unwrap_or("none");
            r = r.push(dropdown(
                pairs(BODY_MODES),
                mode,
                Length::Fixed(160.0),
                Message::BodyModeChanged,
            ));
        } else if tab.req_tab == ReqTab::Auth {
            let mode = req.map(|x| auth_mode_value(&x.auth)).unwrap_or("none");
            r = r.push(dropdown(
                pairs(AUTH_MODES),
                mode,
                Length::Fixed(150.0),
                Message::AuthModeChanged,
            ));
        }

        container(r)
            .style(|_| panel(SURFACE0(), Some(BORDER2())))
            .width(Fill)
            .padding(Padding::from([2, 0]))
            .into()
    }

    /// A count or status dot beside a request sub-tab label.
    fn tab_indicator(
        &self,
        t: ReqTab,
        tab: &Tab,
        req: Option<&Request>,
    ) -> Option<Element<'_, Message>> {
        let count = |n: usize| (n > 0).then(|| text(format!("{n}")).size(9).color(ACCENT()).into());
        let dot = || Some(text("\u{2022}").size(12).color(ACCENT()).into());
        let r = req?;
        match t {
            ReqTab::Params => count(
                r.query.iter().filter(|k| k.enabled).count()
                    + r.path_params.iter().filter(|k| k.enabled).count(),
            ),
            ReqTab::Headers => count(r.headers.iter().filter(|k| k.enabled).count()),
            ReqTab::Assert => count(r.assertions.iter().filter(|a| a.enabled).count()),
            ReqTab::Vars => count(
                r.vars_pre.iter().filter(|v| v.enabled).count()
                    + r.vars_post.iter().filter(|v| v.enabled).count(),
            ),
            ReqTab::Body => (!matches!(r.body, Body::None)).then(dot).flatten(),
            ReqTab::Auth => (!matches!(r.auth, Auth::None | Auth::Inherit))
                .then(dot)
                .flatten(),
            ReqTab::Script => (tab.file.script_pre().is_some_and(|s| !s.trim().is_empty())
                || tab.file.script_post().is_some_and(|s| !s.trim().is_empty()))
            .then(dot)
            .flatten(),
            ReqTab::Tests => tab
                .file
                .tests_script()
                .is_some_and(|s| !s.trim().is_empty())
                .then(dot)
                .flatten(),
            ReqTab::Docs => (!docs_text(&tab.file).trim().is_empty())
                .then(dot)
                .flatten(),
            ReqTab::Examples => count(example_count(&tab.file)),
            _ => None,
        }
    }

    fn req_content<'a>(&'a self, tab: &'a Tab, req: Option<&Request>) -> Element<'a, Message> {
        if tab.req_tab == ReqTab::Source {
            return text_editor(&tab.editors.source)
                .font(MONO)
                .height(Fill)
                .on_action(Message::SourceEdit)
                .into();
        }
        let Some(req) = req else {
            return column![
                text("This file has no HTTP method block - editing via Source.")
                    .size(12)
                    .color(RED()),
                text_editor(&tab.editors.source)
                    .font(MONO)
                    .height(Fill)
                    .on_action(Message::SourceEdit),
            ]
            .spacing(6)
            .into();
        };
        match tab.req_tab {
            ReqTab::Params => column![
                section("Query Params"),
                self.kv_or_bulk(tab, KvSection::Query, "Name", "Value", kv_rows(&req.query)),
                vspace(12.0),
                section("Path Params"),
                self.kv_or_bulk(
                    tab,
                    KvSection::Path,
                    "Name",
                    "Value",
                    kv_rows(&req.path_params)
                ),
            ]
            .spacing(4)
            .into(),
            ReqTab::Headers => self.kv_or_bulk(
                tab,
                KvSection::Headers,
                "Name",
                "Value",
                kv_rows(&req.headers),
            ),
            ReqTab::Body => self.body_view(tab, &req.body),
            ReqTab::Auth => auth_view(&req.auth),
            ReqTab::Vars => column![
                section("Pre Request"),
                self.vars_or_bulk(tab, KvSection::VarsPre, var_rows_local(&req.vars_pre)),
                vspace(12.0),
                section("Post Response"),
                self.vars_or_bulk(tab, KvSection::VarsPost, var_rows_local(&req.vars_post)),
            ]
            .spacing(4)
            .into(),
            ReqTab::Assert => column![
                section("Assertions"),
                assert_table(assert_rows(&req.assertions)),
            ]
            .spacing(4)
            .into(),
            ReqTab::Script => column![
                section("Pre Request"),
                editor_box(&tab.editors.script_pre, EditorField::ScriptPre, "js", Fill),
                vspace(12.0),
                section("Post Response"),
                editor_box(
                    &tab.editors.script_post,
                    EditorField::ScriptPost,
                    "js",
                    Fill
                ),
            ]
            .spacing(4)
            .height(Fill)
            .into(),
            ReqTab::Tests => editor_box(&tab.editors.tests, EditorField::Tests, "js", Fill),
            ReqTab::Docs => editor_box(&tab.editors.docs, EditorField::Docs, "md", Fill),
            ReqTab::Settings => self.request_settings_view(tab),
            ReqTab::Examples => {
                let examples = request_examples(&tab.file);
                let mut col = Column::new().spacing(10);
                if examples.is_empty() {
                    col = col.push(text("No saved examples.").size(12).color(MUTED()));
                }
                for (name, content) in examples {
                    col = col.push(section(&name));
                    col = col.push(code_block(&content));
                }
                col.into()
            }
            ReqTab::Source => unreachable!(),
        }
    }

    /// The raw bulk-edit textarea for a section.
    fn bulk_view<'a>(&'a self, tab: &'a Tab, section: KvSection) -> Element<'a, Message> {
        column![
            row![
                text("Bulk edit (one `name: value` per line, ~ disables)")
                    .size(11)
                    .color(MUTED()),
                fill_x(),
                button(text("Done").size(11).color(ACCENT()))
                    .style(|_, s| icon_button(s, ACCENT()))
                    .padding(Padding::from([2, 8]))
                    .on_press(Message::ToggleBulk(section)),
            ]
            .align_y(Center),
            container(
                text_editor(&tab.bulk_editor)
                    .font(MONO)
                    .height(Length::Fixed(200.0))
                    .on_action(Message::BulkEdit),
            )
            .style(|_| rounded_panel(INPUT_BG(), BORDER1()))
            .width(Fill),
        ]
        .spacing(4)
        .into()
    }

    /// A KV section rendered as a table, or as a raw bulk editor when toggled.
    fn kv_or_bulk<'a>(
        &'a self,
        tab: &'a Tab,
        section: KvSection,
        col1: &str,
        col2: &str,
        rows: Vec<(String, String, bool)>,
    ) -> Element<'a, Message> {
        if tab.bulk == Some(section) {
            self.bulk_view(tab, section)
        } else {
            kv_table(section, col1, col2, rows)
        }
    }

    /// A vars section: the KV table plus a `local` (@) toggle column.
    fn vars_or_bulk<'a>(
        &'a self,
        tab: &'a Tab,
        section: KvSection,
        rows: Vec<(String, String, bool, bool)>,
    ) -> Element<'a, Message> {
        if tab.bulk == Some(section) {
            self.bulk_view(tab, section)
        } else {
            vars_table(section, rows)
        }
    }

    /// The request Settings tab: request options + a tags editor.
    fn request_settings_view<'a>(&'a self, tab: &'a Tab) -> Element<'a, Message> {
        let tags = edit::meta_tags(&tab.file);
        let mut chips = row![].spacing(6).align_y(Center);
        for (i, t) in tags.iter().enumerate() {
            chips = chips.push(
                button(text(format!("{t}  \u{00D7}")).size(11).color(TEXT()))
                    .style(|_, s| ghost_button(s))
                    .padding(Padding::from([2, 8]))
                    .on_press(Message::RemoveTag(i)),
            );
        }
        column![
            settings_view(&tab.file),
            vspace(8.0),
            section("Tags"),
            chips,
            text_input("add tag, press Enter", &tab.tag_input)
                .on_input(Message::TagInput)
                .on_submit(Message::AddTag)
                .size(12)
                .padding(Padding::from([5, 8]))
                .style(input_style)
                .width(Length::Fixed(260.0)),
        ]
        .spacing(8)
        .into()
    }

    /// A strip of clickable `{{var}}` pills for the distinct variables in `raw`,
    /// shown under the body editor (where inline per-token hover isn't possible).
    /// `None` when there are no variables. Resolution mirrors the URL preview:
    /// request pre-request vars override the cached environment map.
    fn var_strip(&self, tab: &Tab, raw: &str) -> Option<Element<'static, Message>> {
        let names = distinct_vars(raw);
        if names.is_empty() {
            return None;
        }
        let pre = tab
            .file
            .to_request()
            .map(|r| r.vars_pre)
            .unwrap_or_default();
        let lookup = |name: &str| -> Option<String> {
            if let Some(v) = pre.iter().find(|v| v.enabled && v.name == name) {
                return Some(v.value.clone());
            }
            self.vars.get(name).cloned()
        };
        let mut row = row![text("\u{21B3} ").size(12).color(MUTED())]
            .spacing(6)
            .align_y(Center);
        for n in &names {
            row = row.push(var_pill(n, lookup(n)));
        }
        Some(container(row).padding(Padding::from([4, 10])).into())
    }

    fn body_view<'a>(&'a self, tab: &'a Tab, body: &Body) -> Element<'a, Message> {
        match body {
            Body::None => text("No body").size(12).color(MUTED()).into(),
            Body::Json(_) | Body::Text(_) | Body::Xml(_) | Body::Sparql(_) => {
                let editor = editor_box(
                    &tab.editors.body,
                    EditorField::Body,
                    body_syntax(&tab.editors.body_kind),
                    Fill,
                );
                match self.var_strip(tab, &tab.editors.body.text()) {
                    Some(strip) => column![strip, editor].spacing(2).height(Fill).into(),
                    None => editor,
                }
            }
            Body::GraphQl { .. } => {
                let inner = column![
                    section("Query"),
                    editor_box(&tab.editors.gql_query, EditorField::GqlQuery, "js", Fill),
                    vspace(12.0),
                    section("Variables"),
                    editor_box(&tab.editors.gql_vars, EditorField::GqlVars, "json", Fill),
                ]
                .spacing(4)
                .height(Fill);
                let combined = format!(
                    "{} {}",
                    tab.editors.gql_query.text(),
                    tab.editors.gql_vars.text()
                );
                match self.var_strip(tab, &combined) {
                    Some(strip) => column![strip, inner].spacing(2).height(Fill).into(),
                    None => inner.into(),
                }
            }
            Body::FormUrlEncoded(fields) => {
                self.kv_or_bulk(tab, KvSection::Form, "Name", "Value", kv_rows(fields))
            }
            Body::MultipartForm(fields) => {
                let rows: Vec<(String, String, bool)> = fields
                    .iter()
                    .map(|f| {
                        // Round-trip the file decorator so inline edits don't drop
                        // it (parse_multipart_field reads exactly this surface form).
                        let v = match (&f.value, &f.content_type) {
                            (MultipartValue::Text(t), _) => t.clone(),
                            (MultipartValue::File(p), Some(ct)) => {
                                format!("@file({p}) @contentType({ct})")
                            }
                            (MultipartValue::File(p), None) => format!("@file({p})"),
                        };
                        (f.name.clone(), v, f.enabled)
                    })
                    .collect();
                multipart_table(rows)
            }
            Body::File(items) => {
                let selected = items.iter().find(|i| i.selected).or_else(|| items.first());
                let path = selected.map(|i| i.path.clone()).unwrap_or_default();
                let ct = selected
                    .and_then(|i| i.content_type.clone())
                    .unwrap_or_default();
                let label = if path.is_empty() {
                    "No file selected".to_string()
                } else {
                    path.clone()
                };
                column![
                    section("Binary File Body"),
                    row![
                        button(text("Choose File\u{2026}").size(12).color(TEXT()))
                            .style(|_, s| ghost_button(s))
                            .padding(Padding::from([5, 12]))
                            .on_press(Message::BrowseFileBody),
                        text(label).size(12).color(SUBTEXT()).font(MONO),
                    ]
                    .spacing(10)
                    .align_y(Center),
                    labeled(
                        "Content-Type (optional)",
                        text_input("auto", &ct)
                            .on_input(Message::FileBodyContentType)
                            .size(12)
                            .padding(Padding::from([5, 8]))
                            .style(input_style)
                            .width(Length::Fixed(280.0)),
                    ),
                ]
                .spacing(10)
                .into()
            }
        }
    }

    fn response_pane<'a>(&'a self, tab: &'a Tab) -> Element<'a, Message> {
        // Sub-tab strip + status/time/size on the right.
        let mut strip = row![]
            .spacing(2)
            .padding(Padding::from([0, 8]))
            .align_y(Center);
        for t in RespTab::ALL {
            let active = t == tab.resp_tab;
            let mut label =
                row![text(t.label())
                    .size(12)
                    .color(if active { TEXT() } else { MUTED() })]
                .spacing(3)
                .align_y(Center);
            if let Some(ind) = self.resp_indicator(t, tab) {
                label = label.push(ind);
            }
            strip = strip.push(
                button(label)
                    .style(move |_, _| tab_button(active))
                    .padding(Padding::from([6, 10]))
                    .on_press(Message::RespTab(t)),
            );
        }
        strip = strip.push(fill_x());

        // Action buttons (layout toggle always; copy/download/clear once there's
        // a response).
        let layout_glyph = if self.layout_horizontal {
            "\u{2926}"
        } else {
            "\u{2925}"
        };
        strip = strip.push(tooltip(
            button(text(layout_glyph).size(13).color(SUBTEXT()))
                .style(|_, s| icon_button(s, SUBTEXT()))
                .padding(Padding::from([2, 6]))
                .on_press(Message::ToggleLayout),
            container(text("Toggle layout").size(11))
                .style(|_| menu_panel())
                .padding(4),
            tooltip::Position::Bottom,
        ));
        if let Some(r) = tab.result.as_ref().and_then(|o| o.response.as_ref()) {
            if tab.resp_tab == RespTab::Response && !is_image_response(&r.headers) {
                // JSONPath filter: funnel input that extracts from the JSON body.
                strip = strip.push(tooltip(
                    text_input("\u{2315} $.path filter", &tab.resp_filter)
                        .on_input(Message::RespFilter)
                        .font(MONO)
                        .size(12)
                        .padding(Padding::from([2, 6]))
                        .width(Length::Fixed(180.0))
                        .style(input_style),
                    container(text("Filter JSON by path, e.g. $.items[0].name").size(11))
                        .style(|_| menu_panel())
                        .padding(4),
                    tooltip::Position::Bottom,
                ));
                let fmt = match tab.resp_format {
                    RespFormat::Pretty => "pretty",
                    RespFormat::Tree => "tree",
                    RespFormat::Raw => "raw",
                    RespFormat::Hex => "hex",
                };
                strip = strip.push(dropdown(
                    pairs(RESP_FORMATS),
                    fmt,
                    Length::Fixed(90.0),
                    Message::RespFormatChanged,
                ));
            }
            // Response actions collapse into a single kebab (⋯) menu.
            strip = strip.push(
                button(text("\u{22EF}").size(15).color(SUBTEXT()))
                    .style(|_, s| icon_button(s, SUBTEXT()))
                    .padding(Padding::from([2, 8]))
                    .on_press(Message::OpenMenu(MenuTarget::Response)),
            );
            strip = strip.push(
                row![
                    text(format!("{} {}", r.status, r.status_text))
                        .size(12)
                        .color(status_color(r.status))
                        .font(BOLD),
                    text(format!("{} ms", r.duration_ms))
                        .size(12)
                        .color(SUBTEXT()),
                    tooltip(
                        text(human_size(r.body.len())).size(12).color(SUBTEXT()),
                        container(text(format!("{} bytes", r.body.len())).size(11))
                            .style(|_| menu_panel())
                            .padding(4),
                        tooltip::Position::Bottom,
                    ),
                ]
                .spacing(14)
                .align_y(Center),
            );
        }

        let content = self.resp_content(tab);
        container(
            column![
                container(strip)
                    .style(|_| panel(SURFACE0(), Some(BORDER2())))
                    .width(Fill)
                    .padding(Padding::from([2, 0])),
                // resp_content manages its own scrolling per view (the response
                // editor scrolls itself; tables/tree wrap in a scrollable).
                container(content).padding(10).height(Fill),
            ]
            .height(Fill),
        )
        .style(|_| panel(BG(), Some(BORDER2())))
        .width(Fill)
        .height(Fill)
        .into()
    }

    fn resp_indicator(&self, t: RespTab, tab: &Tab) -> Option<Element<'_, Message>> {
        let o = tab.result.as_ref()?;
        match t {
            RespTab::Headers => {
                let n = o.response.as_ref().map(|r| r.headers.len()).unwrap_or(0);
                (n > 0).then(|| text(format!("{n}")).size(9).color(ACCENT()).into())
            }
            RespTab::Tests => {
                let total = o.assertions.len() + o.tests.len();
                if total == 0 {
                    return None;
                }
                let passed = o.assertions.iter().filter(|a| a.passed).count()
                    + o.tests.iter().filter(|t| t.passed).count();
                let c = if passed == total { GREEN() } else { RED() };
                Some(text(format!("{passed}/{total}")).size(9).color(c).into())
            }
            _ => None,
        }
    }

    fn resp_content<'a>(&'a self, tab: &'a Tab) -> Element<'a, Message> {
        let Some(o) = &tab.result else {
            return text("No response yet - press Send.")
                .size(12)
                .color(MUTED())
                .into();
        };
        if let Some(err) = &o.error {
            return text(format!("Error: {err}")).size(12).color(RED()).into();
        }
        match tab.resp_tab {
            RespTab::Response => {
                let Some(r) = &o.response else {
                    return text("(no response)").size(12).color(MUTED()).into();
                };
                const LARGE: usize = 10 * 1024 * 1024;
                let body: Element<'a, Message> = if is_image_response(&r.headers) {
                    container(
                        column![
                            text(format!("Image response ({})", human_size(r.body.len())))
                                .size(13)
                                .color(SUBTEXT()),
                            button(text("Save to file").size(12).color(TEXT()))
                                .style(|_, s| ghost_button(s))
                                .padding(Padding::from([5, 12]))
                                .on_press(Message::DownloadResponse),
                        ]
                        .spacing(10),
                    )
                    .style(|_| rounded_panel(SURFACE0(), BORDER1()))
                    .padding(14)
                    .into()
                } else if r.body.len() > LARGE
                    && !tab.reveal_large
                    && tab.resp_format != RespFormat::Hex
                {
                    container(
                        column![
                            text(format!(
                                "Large response ({}). Rendering may be slow.",
                                human_size(r.body.len())
                            ))
                            .size(13)
                            .color(ORANGE()),
                            row![
                                button(text("View anyway").size(12).color(BLACK))
                                    .style(|_, _| solid_button(ACCENT(), BLACK))
                                    .padding(Padding::from([5, 12]))
                                    .on_press(Message::RevealLarge),
                                button(text("Save to file").size(12).color(TEXT()))
                                    .style(|_, s| ghost_button(s))
                                    .padding(Padding::from([5, 12]))
                                    .on_press(Message::DownloadResponse),
                            ]
                            .spacing(8),
                        ]
                        .spacing(10),
                    )
                    .style(|_| rounded_panel(SURFACE0(), BORDER1()))
                    .padding(14)
                    .into()
                } else if !tab.resp_filter.trim().is_empty() {
                    // JSONPath filter active: show the extracted value.
                    let q = tab.resp_filter.trim();
                    match r.json() {
                        Some(v) => match json_path(&v, q) {
                            Some(fv) => scrollable(
                                text(serde_json::to_string_pretty(&fv).unwrap_or_default())
                                    .size(12)
                                    .font(MONO)
                                    .color(TEXT()),
                            )
                            .height(Fill)
                            .into(),
                            None => text(format!("No match for `{q}`"))
                                .size(12)
                                .color(MUTED())
                                .into(),
                        },
                        None => text("Filter needs a JSON response.")
                            .size(12)
                            .color(MUTED())
                            .into(),
                    }
                } else if tab.resp_format == RespFormat::Tree {
                    match r.json() {
                        Some(v) => scrollable(json_tree(&v, &tab.resp_expanded))
                            .height(Fill)
                            .into(),
                        None => text("Response is not JSON.").size(12).color(MUTED()).into(),
                    }
                } else {
                    // Read-only, selectable, syntax-highlighted response body.
                    text_editor(&tab.resp_editor)
                        .font(MONO)
                        .height(Fill)
                        .highlight(resp_syntax(&r.headers), highlight_theme())
                        .on_action(Message::RespEditorAction)
                        .into()
                };
                if o.console.is_empty() {
                    body
                } else {
                    let mut con = Column::new().spacing(2);
                    for line in &o.console {
                        con =
                            con.push(text(format!("| {line}")).size(12).color(MUTED()).font(MONO));
                    }
                    column![con, body].spacing(6).height(Fill).into()
                }
            }
            RespTab::Headers => match &o.response {
                Some(r) => scrollable(header_table(&r.headers)).height(Fill).into(),
                None => text("(no response)").size(12).color(MUTED()).into(),
            },
            RespTab::Timeline => scrollable(code_block(&timeline_text(tab, o)))
                .height(Fill)
                .into(),
            RespTab::Tests => {
                let mut col = Column::new().spacing(4);
                if o.assertions.is_empty() && o.tests.is_empty() {
                    col = col.push(text("No assertions or tests.").size(12).color(MUTED()));
                }
                for a in &o.assertions {
                    col = col.push(check_row(
                        a.passed,
                        &format!("{} {} {}", a.expr, a.operator, a.expected),
                    ));
                }
                for t in &o.tests {
                    col = col.push(check_row(t.passed, &format!("test: {}", t.name)));
                }
                scrollable(col).height(Fill).into()
            }
        }
    }
}

// === reusable widgets ========================================================

/// A styled `pick_list` over `(value, label)` pairs; `on` receives the value.
fn dropdown<'a>(
    pairs: Vec<(String, String)>,
    value: &str,
    width: Length,
    on: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let opts: Vec<Opt> = pairs.into_iter().map(|(v, l)| Opt(v, l)).collect();
    let selected = opts
        .iter()
        .find(|o| o.0 == value)
        .cloned()
        .unwrap_or_else(|| Opt(value.to_string(), value.to_string()));
    pick_list(opts, Some(selected), move |o: Opt| on(o.0))
        .style(picklist_style)
        .text_size(12)
        .padding(Padding::from([5, 8]))
        .width(width)
        .into()
}

/// A `(value, label)` pick-list option that displays its label but compares by value.
#[derive(Clone)]
struct Opt(String, String);
impl PartialEq for Opt {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}
impl fmt::Display for Opt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.1)
    }
}

/// An editable key/value table: enable checkbox, name, value, delete, with a
/// trailing "+ Add" button. Rows map 1:1 to the dict-block entries by index.
fn kv_table<'a>(
    section: KvSection,
    col1: &str,
    col2: &str,
    rows: Vec<(String, String, bool)>,
) -> Element<'a, Message> {
    let mut col = Column::new().spacing(2);
    col = col.push(
        row![
            hspace(24.0),
            text(col1.to_string())
                .size(11)
                .color(MUTED())
                .width(Length::FillPortion(2)),
            text(col2.to_string())
                .size(11)
                .color(MUTED())
                .width(Length::FillPortion(3)),
            hspace(28.0),
        ]
        .spacing(6)
        .align_y(Center),
    );
    for (i, (name, value, enabled)) in rows.into_iter().enumerate() {
        let check = checkbox(enabled)
            .on_toggle(move |b| Message::KvToggle(section, i, b))
            .size(14)
            .style(checkbox_style);
        let name_in = text_input("name", &name)
            .on_input(move |v| Message::KvName(section, i, v))
            .size(12)
            .font(MONO)
            .padding(Padding::from([4, 6]))
            .style(cell_input)
            .width(Length::FillPortion(2));
        let value_in = text_input("value", &value)
            .on_input(move |v| Message::KvValue(section, i, v))
            .size(12)
            .font(MONO)
            .padding(Padding::from([4, 6]))
            .style(cell_input)
            .width(Length::FillPortion(3));
        let del = button(text("\u{2715}").size(12).color(MUTED()))
            .style(move |_, s| icon_button(s, RED()))
            .padding(Padding::from([2, 6]))
            .on_press(Message::KvRemove(section, i));
        col = col.push(
            row![check, name_in, value_in, del]
                .spacing(6)
                .align_y(Center),
        );
    }
    col = col.push(
        row![
            button(text("+ Add").size(12).color(ACCENT()))
                .style(move |_, s| icon_button(s, ACCENT()))
                .padding(Padding::from([4, 8]))
                .on_press(Message::KvAdd(section)),
            button(text("Bulk Edit").size(12).color(SUBTEXT()))
                .style(move |_, s| icon_button(s, SUBTEXT()))
                .padding(Padding::from([4, 8]))
                .on_press(Message::ToggleBulk(section)),
        ]
        .spacing(4),
    );
    col.into()
}

/// A vars table: enable · name · value · `local`(@) toggle · delete, with "+ Add".
fn vars_table<'a>(
    section: KvSection,
    rows: Vec<(String, String, bool, bool)>,
) -> Element<'a, Message> {
    let mut col = Column::new().spacing(2);
    col = col.push(
        row![
            hspace(24.0),
            text("Name")
                .size(11)
                .color(MUTED())
                .width(Length::FillPortion(2)),
            text("Value")
                .size(11)
                .color(MUTED())
                .width(Length::FillPortion(3)),
            text("Local")
                .size(11)
                .color(MUTED())
                .width(Length::Fixed(48.0)),
            hspace(28.0),
        ]
        .spacing(6)
        .align_y(Center),
    );
    for (i, (name, value, enabled, local)) in rows.into_iter().enumerate() {
        let check = checkbox(enabled)
            .on_toggle(move |b| Message::KvToggle(section, i, b))
            .size(14)
            .style(checkbox_style);
        let name_in = text_input("name", &name)
            .on_input(move |v| Message::KvName(section, i, v))
            .size(12)
            .font(MONO)
            .padding(Padding::from([4, 6]))
            .style(cell_input)
            .width(Length::FillPortion(2));
        let value_in = text_input("value", &value)
            .on_input(move |v| Message::KvValue(section, i, v))
            .size(12)
            .font(MONO)
            .padding(Padding::from([4, 6]))
            .style(cell_input)
            .width(Length::FillPortion(3));
        let local_box = checkbox(local)
            .on_toggle(move |b| Message::KvLocal(section, i, b))
            .size(14)
            .style(checkbox_style)
            .width(Length::Fixed(48.0));
        let del = button(text("\u{2715}").size(12).color(MUTED()))
            .style(move |_, s| icon_button(s, RED()))
            .padding(Padding::from([2, 6]))
            .on_press(Message::KvRemove(section, i));
        col = col.push(
            row![check, name_in, value_in, local_box, del]
                .spacing(6)
                .align_y(Center),
        );
    }
    col = col.push(
        row![
            button(text("+ Add").size(12).color(ACCENT()))
                .style(move |_, s| icon_button(s, ACCENT()))
                .padding(Padding::from([4, 8]))
                .on_press(Message::KvAdd(section)),
            button(text("Bulk Edit").size(12).color(SUBTEXT()))
                .style(move |_, s| icon_button(s, SUBTEXT()))
                .padding(Padding::from([4, 8]))
                .on_press(Message::ToggleBulk(section)),
        ]
        .spacing(4),
    );
    col.into()
}

/// The multipart-form table: like `kv_table` but with a per-row file picker.
fn multipart_table<'a>(rows: Vec<(String, String, bool)>) -> Element<'a, Message> {
    let section = KvSection::Multipart;
    let mut col = Column::new().spacing(2);
    col = col.push(
        row![
            hspace(24.0),
            text("Name")
                .size(11)
                .color(MUTED())
                .width(Length::FillPortion(2)),
            text("Value (text or @file)")
                .size(11)
                .color(MUTED())
                .width(Length::FillPortion(3)),
            hspace(56.0),
        ]
        .spacing(6)
        .align_y(Center),
    );
    for (i, (name, value, enabled)) in rows.into_iter().enumerate() {
        let check = checkbox(enabled)
            .on_toggle(move |b| Message::KvToggle(section, i, b))
            .size(14)
            .style(checkbox_style);
        let name_in = text_input("name", &name)
            .on_input(move |v| Message::KvName(section, i, v))
            .size(12)
            .font(MONO)
            .padding(Padding::from([4, 6]))
            .style(cell_input)
            .width(Length::FillPortion(2));
        let value_in = text_input("value", &value)
            .on_input(move |v| Message::KvValue(section, i, v))
            .size(12)
            .font(MONO)
            .padding(Padding::from([4, 6]))
            .style(cell_input)
            .width(Length::FillPortion(3));
        let browse = button(text("\u{1F4C1}").size(12).color(SUBTEXT()))
            .style(move |_, s| icon_button(s, SUBTEXT()))
            .padding(Padding::from([2, 6]))
            .on_press(Message::BrowseMultipartFile(i));
        let del = button(text("\u{2715}").size(12).color(MUTED()))
            .style(move |_, s| icon_button(s, RED()))
            .padding(Padding::from([2, 6]))
            .on_press(Message::KvRemove(section, i));
        col = col.push(
            row![check, name_in, value_in, browse, del]
                .spacing(6)
                .align_y(Center),
        );
    }
    col = col.push(
        button(text("+ Add").size(12).color(ACCENT()))
            .style(move |_, s| icon_button(s, ACCENT()))
            .padding(Padding::from([4, 8]))
            .on_press(Message::KvAdd(section)),
    );
    col.into()
}

/// The Assert tab table: enable · expression · operator dropdown · operand
/// (disabled for unary ops) · delete, with a trailing "+ Add".
fn assert_table<'a>(rows: Vec<(String, String, bool)>) -> Element<'a, Message> {
    let mut col = Column::new().spacing(2);
    col = col.push(
        row![
            hspace(24.0),
            text("Expression")
                .size(11)
                .color(MUTED())
                .width(Length::FillPortion(3)),
            text("Operator")
                .size(11)
                .color(MUTED())
                .width(Length::Fixed(150.0)),
            text("Value")
                .size(11)
                .color(MUTED())
                .width(Length::FillPortion(3)),
            hspace(28.0),
        ]
        .spacing(6)
        .align_y(Center),
    );
    for (i, (expr, raw, enabled)) in rows.into_iter().enumerate() {
        let (op, operand) = split_assert(&raw);
        let unary = is_unary_op(&op);

        let check = checkbox(enabled)
            .on_toggle(move |b| Message::KvToggle(KvSection::Assert, i, b))
            .size(14)
            .style(checkbox_style);
        let expr_in = text_input("res.status", &expr)
            .on_input(move |v| Message::KvName(KvSection::Assert, i, v))
            .size(12)
            .font(MONO)
            .padding(Padding::from([4, 6]))
            .style(cell_input)
            .width(Length::FillPortion(3));
        let operand_for_op = operand.clone();
        let op_dd = dropdown(pairs(ASSERT_OPS), &op, Length::Fixed(150.0), move |newop| {
            Message::KvValue(
                KvSection::Assert,
                i,
                combine_assert(&newop, &operand_for_op),
            )
        });
        let op_for_operand = op.clone();
        let mut operand_in = text_input("expected", &operand)
            .size(12)
            .font(MONO)
            .padding(Padding::from([4, 6]))
            .style(cell_input)
            .width(Length::FillPortion(3));
        if !unary {
            operand_in = operand_in.on_input(move |v| {
                Message::KvValue(KvSection::Assert, i, combine_assert(&op_for_operand, &v))
            });
        }
        let del = button(text("\u{2715}").size(12).color(MUTED()))
            .style(move |_, s| icon_button(s, RED()))
            .padding(Padding::from([2, 6]))
            .on_press(Message::KvRemove(KvSection::Assert, i));
        col = col.push(
            row![check, expr_in, op_dd, operand_in, del]
                .spacing(6)
                .align_y(Center),
        );
    }
    col = col.push(
        button(text("+ Add").size(12).color(ACCENT()))
            .style(move |_, s| icon_button(s, ACCENT()))
            .padding(Padding::from([4, 8]))
            .on_press(Message::KvAdd(KvSection::Assert)),
    );
    col.into()
}

/// A bordered, syntax-highlighted multiline editor for body/script/docs payloads.
/// Height for editors that live inside a scrollable pane (collection/folder
/// Settings tabs), where `Fill` would collapse to nothing.
const FIXED_EDITOR: Length = Length::Fixed(220.0);

/// A single `{{name}}` pill. Hovering it opens the value popover (with a Copy
/// button) anchored at the cursor. Gold when resolved, red when unset.
/// Self-contained (owned), so it lives `'static`.
fn var_pill(name: &str, value: Option<String>) -> Element<'static, Message> {
    let color = if value.is_some() { ACCENT() } else { RED() };
    let mut label = String::from("{{");
    label.push_str(name);
    label.push_str("}}");
    mouse_area(
        container(text(label).size(12).font(MONO).color(color)).padding(Padding::from([0, 2])),
    )
    .on_enter(Message::OpenVarPopup(name.to_string(), value))
    .into()
}

/// Distinct `{{name}}` tokens in `raw`, in first-seen order.
fn distinct_vars(raw: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = raw;
    while let Some(open) = rest.find("{{") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else { break };
        let name = after[..close].trim().to_string();
        if !name.is_empty() && !out.contains(&name) {
            out.push(name);
        }
        rest = &after[close + 2..];
    }
    out
}

/// Render a `{{var}}`-bearing string as an inline preview: literal text plus a
/// pill for each `{{name}}` token (hover = value, click = copy popover).
/// Returns `None` when the string has no balanced token, so callers can hide
/// the preview entirely. `lookup` resolves a variable against env + request.
fn var_preview<F>(raw: &str, lookup: F) -> Option<Element<'static, Message>>
where
    F: Fn(&str) -> Option<String>,
{
    if !raw.contains("{{") {
        return None;
    }
    let literal = |s: &str| -> Element<'static, Message> {
        text(s.to_string())
            .size(12)
            .font(MONO)
            .color(SUBTEXT())
            .into()
    };

    let mut segments: Vec<Element<'static, Message>> = Vec::new();
    let mut rest = raw;
    let mut resolved_any = false;
    while let Some(open) = rest.find("{{") {
        let before = &rest[..open];
        let after = &rest[open + 2..];
        let Some(close) = after.find("}}") else {
            // Unbalanced `{{`: emit the remainder verbatim and stop.
            segments.push(literal(&rest[open..]));
            rest = "";
            break;
        };
        if !before.is_empty() {
            segments.push(literal(before));
        }
        let name = after[..close].trim();
        segments.push(var_pill(name, lookup(name)));
        resolved_any = true;
        rest = &after[close + 2..];
    }
    if !resolved_any {
        return None;
    }
    if !rest.is_empty() {
        segments.push(literal(rest));
    }

    let mut row = row![text("\u{21B3} ").size(12).color(MUTED())].align_y(Center);
    for seg in segments {
        row = row.push(seg);
    }
    Some(container(row).padding(Padding::from([2, 10])).into())
}

fn editor_box<'a>(
    content: &'a text_editor::Content,
    field: EditorField,
    syntax: &'static str,
    height: Length,
) -> Element<'a, Message> {
    container(
        text_editor(content)
            .font(MONO)
            .height(height)
            .highlight(syntax, highlight_theme())
            .on_action(move |a| Message::EditField(field, a)),
    )
    .style(|_| rounded_panel(INPUT_BG(), BORDER1()))
    .width(Fill)
    .height(height)
    .into()
}

/// Map a `body:*` block name to a syntect syntax token for highlighting.
fn body_syntax(body_kind: &str) -> &'static str {
    match body_kind {
        "body:json" => "json",
        "body:xml" => "xml",
        "body:sparql" => "sql",
        _ => "txt",
    }
}

/// One labelled credential input row in the Auth tab.
fn auth_field(label: &str, value: &str, f: AuthField) -> Element<'static, Message> {
    row![
        text(label.to_string()).size(12).color(MUTED()).width(150),
        text_input("", value)
            .on_input(move |v| Message::AuthEdit(f, v))
            .size(12)
            .font(MONO)
            .padding(Padding::from([5, 8]))
            .style(input_style)
            .width(Fill),
    ]
    .spacing(8)
    .align_y(Center)
    .into()
}

fn auth_view(auth: &Auth) -> Element<'static, Message> {
    let mut col = Column::new().spacing(8);
    match auth {
        Auth::None => col = col.push(text("No authentication.").size(12).color(MUTED())),
        Auth::Inherit => {
            col = col.push(
                text("Inherits auth from the collection/folder.")
                    .size(12)
                    .color(MUTED()),
            )
        }
        Auth::Basic { username, password } => {
            col = col
                .push(auth_field("Username", username, AuthField::BasicUser))
                .push(auth_field("Password", password, AuthField::BasicPass));
        }
        Auth::Bearer { token } => {
            col = col.push(auth_field("Token", token, AuthField::BearerToken))
        }
        Auth::ApiKey {
            key,
            value,
            placement,
        } => {
            let pl = match placement {
                bru_core::ApiKeyPlacement::Header => "header",
                bru_core::ApiKeyPlacement::Query => "queryparams",
            };
            col = col
                .push(auth_field("Key", key, AuthField::ApiKeyKey))
                .push(auth_field("Value", value, AuthField::ApiKeyValue))
                .push(
                    row![
                        text("Placement").size(12).color(MUTED()).width(150),
                        dropdown(pairs(API_KEY_PLACEMENTS), pl, Length::Fixed(180.0), |s| {
                            Message::AuthEdit(AuthField::ApiKeyPlacement, s)
                        }),
                    ]
                    .spacing(8)
                    .align_y(Center),
                );
        }
        Auth::Digest { username, password } => {
            col = col
                .push(auth_field("Username", username, AuthField::DigestUser))
                .push(auth_field("Password", password, AuthField::DigestPass));
        }
        Auth::AwsV4 {
            access_key_id,
            secret_access_key,
            session_token,
            service,
            region,
            profile_name,
        } => {
            col = col
                .push(auth_field(
                    "Access Key Id",
                    access_key_id,
                    AuthField::AwsAccessKey,
                ))
                .push(auth_field(
                    "Secret Access Key",
                    secret_access_key,
                    AuthField::AwsSecretKey,
                ))
                .push(auth_field(
                    "Session Token",
                    session_token,
                    AuthField::AwsSessionToken,
                ))
                .push(auth_field("Service", service, AuthField::AwsService))
                .push(auth_field("Region", region, AuthField::AwsRegion))
                .push(auth_field("Profile", profile_name, AuthField::AwsProfile));
        }
        Auth::OAuth2(o) => {
            col = col
                .push(
                    row![
                        text("Grant Type").size(12).color(MUTED()).width(150),
                        dropdown(
                            pairs(OAUTH2_GRANTS),
                            &o.grant_type,
                            Length::Fixed(220.0),
                            |s| Message::AuthEdit(AuthField::Oauth2GrantType, s),
                        ),
                    ]
                    .spacing(8)
                    .align_y(Center),
                )
                .push(auth_field(
                    "Access Token URL",
                    &o.access_token_url,
                    AuthField::Oauth2TokenUrl,
                ))
                .push(auth_field(
                    "Client Id",
                    &o.client_id,
                    AuthField::Oauth2ClientId,
                ))
                .push(auth_field(
                    "Client Secret",
                    &o.client_secret,
                    AuthField::Oauth2ClientSecret,
                ))
                .push(auth_field("Scope", &o.scope, AuthField::Oauth2Scope));
            if o.grant_type == "password" {
                col = col
                    .push(auth_field(
                        "Username",
                        &o.username,
                        AuthField::Oauth2Username,
                    ))
                    .push(auth_field(
                        "Password",
                        &o.password,
                        AuthField::Oauth2Password,
                    ));
            }
        }
    }
    col.into()
}

fn setting_bool(label: &str, key: &'static str, on: bool) -> Element<'static, Message> {
    checkbox(on)
        .label(label.to_string())
        .on_toggle(move |b| Message::SettingBool(key, b))
        .size(15)
        .text_size(13)
        .style(checkbox_style)
        .into()
}

fn setting_num(label: &str, key: &'static str, value: &str) -> Element<'static, Message> {
    row![
        text(label.to_string()).size(12).color(MUTED()).width(180),
        text_input("", value)
            .on_input(move |v| Message::SettingText(key, v))
            .size(12)
            .padding(Padding::from([5, 8]))
            .style(input_style)
            .width(Length::Fixed(160.0)),
    ]
    .spacing(8)
    .align_y(Center)
    .into()
}

fn settings_view(file: &BruFile) -> Element<'static, Message> {
    let val = |key: &str| file.dict_value("settings", key).unwrap_or("").to_string();
    let is_true = |key: &str| file.dict_value("settings", key) == Some("true");
    column![
        section("Request Settings"),
        setting_bool("Encode URL", "encodeUrl", is_true("encodeUrl")),
        setting_bool(
            "Follow Redirects",
            "followRedirects",
            is_true("followRedirects")
        ),
        setting_num("Max Redirects", "maxRedirects", &val("maxRedirects")),
        setting_num("Timeout (ms)", "timeout", &val("timeout")),
    ]
    .spacing(10)
    .into()
}

fn header_table(headers: &[(String, String)]) -> Element<'static, Message> {
    let mut col = Column::new().spacing(2);
    col = col.push(
        row![
            text("Name")
                .size(11)
                .color(MUTED())
                .width(Length::FillPortion(2)),
            text("Value")
                .size(11)
                .color(MUTED())
                .width(Length::FillPortion(3)),
        ]
        .spacing(12),
    );
    for (k, v) in headers {
        col = col.push(
            row![
                text(k.clone())
                    .size(12)
                    .color(TEXT())
                    .font(MONO)
                    .width(Length::FillPortion(2)),
                text(v.clone())
                    .size(12)
                    .color(SUBTEXT())
                    .font(MONO)
                    .width(Length::FillPortion(3)),
            ]
            .spacing(12),
        );
    }
    col.into()
}

fn section(title: &str) -> Element<'static, Message> {
    text(title.to_string())
        .size(11)
        .color(MUTED())
        .font(BOLD)
        .into()
}

fn menu_row<'a>(label: &str, danger: bool, msg: Message) -> Element<'a, Message> {
    button(
        text(label.to_string())
            .size(12)
            .color(if danger { RED() } else { TEXT() }),
    )
    .style(move |_, s| menu_item(s, danger))
    .width(Fill)
    .padding(Padding::from([5, 8]))
    .on_press(msg)
    .into()
}

fn menu_sep<'a>() -> Element<'a, Message> {
    container(container(Space::new().height(1).width(Fill)).style(|_| separator()))
        .padding(Padding::from([3, 2]))
        .into()
}

fn labeled<'a>(label: &str, control: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    column![
        text(label.to_string()).size(11).color(MUTED()),
        control.into()
    ]
    .spacing(3)
    .into()
}

fn modal_error<'a>(error: &Option<String>) -> Element<'a, Message> {
    match error {
        Some(e) => text(e.clone()).size(12).color(RED()).into(),
        None => Space::new().into(),
    }
}

fn modal_card_view<'a>(
    title: &str,
    body: Element<'a, Message>,
    confirm: &str,
    danger: bool,
) -> Element<'a, Message> {
    let confirm_btn =
        button(
            text(confirm.to_string())
                .size(13)
                .color(if danger { WHITE } else { BLACK }),
        )
        .style(move |_, s| {
            if danger {
                danger_button(s)
            } else {
                solid_button(ACCENT(), BLACK)
            }
        })
        .padding(Padding::from([6, 16]))
        .on_press(Message::ModalSubmit);

    let cancel = button(text("Cancel").size(13).color(TEXT()))
        .style(|_, s| ghost_button(s))
        .padding(Padding::from([6, 14]))
        .on_press(Message::ModalCancel);

    container(
        column![
            text(title.to_string()).size(15).color(TEXT()).font(BOLD),
            body,
            row![fill_x(), cancel, confirm_btn].spacing(8),
        ]
        .spacing(14),
    )
    .style(|_| modal_card())
    .width(Length::Fixed(440.0))
    .padding(18)
    .into()
}

fn code_block(s: &str) -> Element<'static, Message> {
    container(text(s.to_string()).size(12).color(TEXT()).font(MONO))
        .style(|_| rounded_panel(SURFACE0(), BORDER1()))
        .width(Fill)
        .padding(8)
        .into()
}

fn check_row(passed: bool, label: &str) -> Element<'static, Message> {
    let (mark, c) = if passed {
        ("\u{2713}", GREEN())
    } else {
        ("\u{2717}", RED())
    };
    row![
        text(mark).size(12).color(c),
        text(label.to_string()).size(12).color(TEXT()).font(MONO),
    ]
    .spacing(8)
    .into()
}

fn fill_x() -> Space {
    Space::new().width(Fill)
}
fn hspace(w: f32) -> Space {
    Space::new().width(w)
}
fn vspace(h: f32) -> Space {
    Space::new().height(h)
}

fn indent(depth: u16, content: Element<'_, Message>) -> Element<'_, Message> {
    let pad = Padding {
        left: f32::from(depth) * 12.0,
        ..Padding::ZERO
    };
    container(content).padding(pad).into()
}

// === projection helpers ======================================================

/// Number of saved `example` blocks in a file.
fn example_count(file: &BruFile) -> usize {
    file.blocks.iter().filter(|b| b.name == "example").count()
}

/// Saved examples as `(name, raw_content)` — examples are stored verbatim.
fn request_examples(file: &BruFile) -> Vec<(String, String)> {
    file.blocks
        .iter()
        .filter(|b| b.name == "example")
        .filter_map(|b| match &b.content {
            bru_core::BlockContent::Text(t) => {
                // Read the example's own `name:` (top level), not a nested
                // `request:`/`response:` field that also happens to be `name`.
                let name = t
                    .lines()
                    .take_while(|l| {
                        let s = l.trim_start();
                        !(s.starts_with("request:") || s.starts_with("response:"))
                    })
                    .find_map(|l| l.trim().strip_prefix("name:").map(|s| s.trim().to_string()))
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "example".to_string());
                Some((name, t.trim_matches('\n').to_string()))
            }
            _ => None,
        })
        .collect()
}

/// Build the verbatim text for a new `example` block capturing the current
/// request + response (Bruno's saved-response format, 2-space base indent).
fn build_example_text(name: &str, req: &Request, resp: &HttpResponse) -> String {
    let mut s = String::new();
    s.push_str(&format!("  name: {name}\n"));
    s.push_str("  request: {\n");
    s.push_str(&format!("    url: {}\n", req.url));
    s.push_str(&format!("    method: {}\n", req.method.to_lowercase()));
    s.push_str(&format!("    mode: {}\n", body_mode_value(&req.body)));
    let headers: Vec<&KeyVal> = req.headers.iter().filter(|h| h.enabled).collect();
    if !headers.is_empty() {
        s.push_str("    headers: {\n");
        for h in headers {
            s.push_str(&format!("      {}: {}\n", h.name, h.value));
        }
        s.push_str("    }\n");
    }
    s.push_str("  }\n\n");
    s.push_str("  response: {\n");
    s.push_str("    status: {\n");
    s.push_str(&format!("      code: {}\n", resp.status));
    s.push_str(&format!("      text: {}\n", resp.status_text));
    s.push_str("    }\n\n");
    let kind = if resp.json().is_some() {
        "json"
    } else {
        "text"
    };
    s.push_str("    body: {\n");
    s.push_str(&format!("      type: {kind}\n"));
    s.push_str("      content: '''\n");
    for line in pretty_body(resp).lines() {
        s.push_str(&format!("        {line}\n"));
    }
    s.push_str("      '''\n");
    s.push_str("    }\n");
    s.push_str("  }");
    s
}

/// Serialize a dict block's entries as raw lines for bulk editing: a leading `~`
/// marks a disabled row and a leading `@` marks a local var (so both survive a
/// rename round-trip).
fn bulk_text(file: &BruFile, block: &str) -> String {
    block_var_rows(file, block)
        .into_iter()
        .map(|(n, v, en, loc)| {
            format!(
                "{}{}{}: {}",
                if en { "" } else { "~" },
                if loc { "@" } else { "" },
                n,
                v
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse bulk-edit text into `(name, value, enabled, local)` rows. Blank lines and
/// lines without a `:` are skipped; a leading `~` disables, a leading `@` (after
/// `~`) marks local.
fn parse_bulk(text: &str) -> Vec<(String, String, bool, bool)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let (enabled, line) = match line.strip_prefix('~') {
                Some(rest) => (false, rest.trim_start()),
                None => (true, line),
            };
            let (local, line) = match line.strip_prefix('@') {
                Some(rest) => (true, rest.trim_start()),
                None => (false, line),
            };
            let (name, value) = line.split_once(':')?;
            Some((
                name.trim().to_string(),
                value.trim().to_string(),
                enabled,
                local,
            ))
        })
        .collect()
}

/// Project a dictionary block's entries to editable `(name, value, enabled)`
/// rows directly (used by settings tabs, which have no method block to project).
fn block_kv_rows(file: &BruFile, block: &str) -> Vec<(String, String, bool)> {
    match file.block(block).map(|b| &b.content) {
        Some(bru_core::BlockContent::Dict(entries)) => entries
            .iter()
            .map(|e| {
                (
                    e.key.name().to_string(),
                    e.value.as_inline().to_string(),
                    !e.disabled,
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn kv_rows(items: &[bru_core::KeyVal]) -> Vec<(String, String, bool)> {
    items
        .iter()
        .map(|k| (k.name.clone(), k.value.clone(), k.enabled))
        .collect()
}

/// Var rows including the `local` flag, for the vars table's @ column.
fn var_rows_local(items: &[bru_core::Var]) -> Vec<(String, String, bool, bool)> {
    items
        .iter()
        .map(|v| (v.name.clone(), v.value.clone(), v.enabled, v.local))
        .collect()
}

/// Project a `vars:*` block's entries directly (with the @-local flag) for
/// settings tabs, which have no method block to project.
fn block_var_rows(file: &BruFile, block: &str) -> Vec<(String, String, bool, bool)> {
    match file.block(block).map(|b| &b.content) {
        Some(bru_core::BlockContent::Dict(entries)) => entries
            .iter()
            .map(|e| {
                (
                    e.key.name().to_string(),
                    e.value.as_inline().to_string(),
                    !e.disabled,
                    e.local,
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn assert_rows(items: &[bru_core::Assertion]) -> Vec<(String, String, bool)> {
    items
        .iter()
        .map(|a| (a.expr.clone(), a.value.clone(), a.enabled))
        .collect()
}

fn body_block_name(body: &Body) -> &'static str {
    match body {
        Body::Json(_) => "body:json",
        Body::Text(_) => "body:text",
        Body::Xml(_) => "body:xml",
        Body::Sparql(_) => "body:sparql",
        _ => "",
    }
}

fn body_mode_value(body: &Body) -> &'static str {
    match body {
        Body::None => "none",
        Body::Json(_) => "json",
        Body::Text(_) => "text",
        Body::Xml(_) => "xml",
        Body::Sparql(_) => "sparql",
        Body::FormUrlEncoded(_) => "formUrlEncoded",
        Body::MultipartForm(_) => "multipartForm",
        Body::GraphQl { .. } => "graphql",
        Body::File(_) => "file",
    }
}

fn auth_mode_value(auth: &Auth) -> &'static str {
    match auth {
        Auth::None => "none",
        Auth::Inherit => "inherit",
        Auth::Basic { .. } => "basic",
        Auth::Bearer { .. } => "bearer",
        Auth::ApiKey { .. } => "apikey",
        Auth::OAuth2(_) => "oauth2",
        Auth::Digest { .. } => "digest",
        Auth::AwsV4 { .. } => "awsv4",
    }
}

/// The `docs` block payload, outdented to its source form.
fn docs_text(file: &BruFile) -> String {
    match file.block("docs").map(|b| &b.content) {
        Some(bru_core::BlockContent::Text(t)) => t
            .split('\n')
            .map(|l| {
                let l = l.strip_suffix('\r').unwrap_or(l);
                l.strip_prefix("  ").unwrap_or(l)
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn timeline_text(tab: &Tab, o: &RunOutcome) -> String {
    let mut s = String::new();
    if let Some(req) = tab.file.to_request() {
        s.push_str(&format!("> {} {}\n", req.method.to_uppercase(), req.url));
        for h in req.headers.iter().filter(|h| h.enabled) {
            s.push_str(&format!("> {}: {}\n", h.name, h.value));
        }
    }
    s.push('\n');
    if let Some(r) = &o.response {
        s.push_str(&format!("< {} {}\n", r.status, r.status_text));
        for (k, v) in &r.headers {
            s.push_str(&format!("< {k}: {v}\n"));
        }
        s.push_str(&format!(
            "\n({} ms, {})\n",
            r.duration_ms,
            human_size(r.body.len())
        ));
    }
    s
}

// === (value, label) tables for the dropdowns =================================

const METHODS: &[(&str, &str)] = &[
    ("GET", "GET"),
    ("POST", "POST"),
    ("PUT", "PUT"),
    ("PATCH", "PATCH"),
    ("DELETE", "DELETE"),
    ("HEAD", "HEAD"),
    ("OPTIONS", "OPTIONS"),
];

const BODY_MODES: &[(&str, &str)] = &[
    ("none", "No Body"),
    ("json", "JSON"),
    ("xml", "XML"),
    ("text", "Text"),
    ("sparql", "SPARQL"),
    ("formUrlEncoded", "Form URL Encoded"),
    ("multipartForm", "Multipart Form"),
    ("graphql", "GraphQL"),
    ("file", "File / Binary"),
];

const AUTH_MODES: &[(&str, &str)] = &[
    ("none", "No Auth"),
    ("inherit", "Inherit"),
    ("basic", "Basic Auth"),
    ("bearer", "Bearer Token"),
    ("apikey", "API Key"),
    ("oauth2", "OAuth 2.0"),
    ("digest", "Digest Auth"),
    ("awsv4", "AWS Sig v4"),
];

const API_KEY_PLACEMENTS: &[(&str, &str)] = &[("header", "Header"), ("queryparams", "Query Param")];

const RESP_FORMATS: &[(&str, &str)] = &[
    ("pretty", "Pretty"),
    ("tree", "Tree"),
    ("raw", "Raw"),
    ("hex", "Hex"),
];

const OAUTH2_GRANTS: &[(&str, &str)] = &[
    ("client_credentials", "Client Credentials"),
    ("password", "Password Credentials"),
];

/// Assertion operators (value, label), mirroring Bruno's set.
const ASSERT_OPS: &[(&str, &str)] = &[
    ("eq", "equals"),
    ("neq", "not equals"),
    ("gt", "greater than"),
    ("gte", "greater or equal"),
    ("lt", "less than"),
    ("lte", "less or equal"),
    ("in", "in"),
    ("notIn", "not in"),
    ("contains", "contains"),
    ("notContains", "not contains"),
    ("length", "length"),
    ("matches", "matches"),
    ("notMatches", "not matches"),
    ("startsWith", "starts with"),
    ("endsWith", "ends with"),
    ("between", "between"),
    ("isEmpty", "is empty"),
    ("isNotEmpty", "is not empty"),
    ("isNull", "is null"),
    ("isUndefined", "is undefined"),
    ("isDefined", "is defined"),
    ("isTruthy", "is truthy"),
    ("isFalsy", "is falsy"),
    ("isJson", "is json"),
    ("isNumber", "is number"),
    ("isString", "is string"),
    ("isBoolean", "is boolean"),
    ("isArray", "is array"),
];

/// Operators that take no operand (the value field is disabled for these).
const UNARY_OPS: &[&str] = &[
    "isEmpty",
    "isNotEmpty",
    "isNull",
    "isUndefined",
    "isDefined",
    "isTruthy",
    "isFalsy",
    "isJson",
    "isNumber",
    "isString",
    "isBoolean",
    "isArray",
];

fn is_unary_op(op: &str) -> bool {
    UNARY_OPS.contains(&op)
}

/// Split a stored assertion value (`"eq 200"`, `"200"`, `"isNumber"`) into
/// `(operator, operand)`. A leading known operator wins; otherwise it's `eq`.
fn split_assert(value: &str) -> (String, String) {
    let v = value.trim();
    if let Some((head, rest)) = v.split_once(char::is_whitespace) {
        if ASSERT_OPS.iter().any(|(op, _)| *op == head) {
            return (head.to_string(), rest.trim().to_string());
        }
    }
    // A lone known operator (unary, or a binary one selected before the operand
    // is typed) keeps its identity instead of collapsing to `eq <op-name>`.
    if ASSERT_OPS.iter().any(|(op, _)| *op == v) {
        return (v.to_string(), String::new());
    }
    ("eq".to_string(), v.to_string())
}

/// Recombine an operator + operand into the stored assertion value form.
fn combine_assert(op: &str, operand: &str) -> String {
    if is_unary_op(op) {
        op.to_string()
    } else {
        format!("{op} {operand}").trim().to_string()
    }
}

fn pairs(table: &[(&str, &str)]) -> Vec<(String, String)> {
    table
        .iter()
        .map(|(v, l)| (v.to_string(), l.to_string()))
        .collect()
}

// === misc helpers ============================================================

fn scan_envs(dir: &Path) -> Vec<String> {
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir.join("environments")) {
        for e in rd.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("bru") {
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    v.push(stem.to_string());
                }
            }
        }
    }
    v.sort();
    v
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("request")
        .to_string()
}

/// `meta.name` of a `.bru` (or folder via its `folder.bru`), falling back to the
/// file/dir stem — used to seed Rename/Delete dialogs.
/// Open the OS file manager with `path` selected (Windows Explorer).
fn reveal_in_explorer(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer")
            .arg("/select,")
            .arg(path)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        if let Some(dir) = path.parent() {
            let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
        }
    }
}

fn collect_folder_paths(folder: &Folder, out: &mut Vec<PathBuf>) {
    for sub in &folder.folders {
        out.push(sub.path.clone());
        collect_folder_paths(sub, out);
    }
}

/// Flatten every request into `(name, path)` for the command palette.
fn collect_request_index(folder: &Folder, out: &mut Vec<(String, PathBuf)>) {
    for req in &folder.requests {
        out.push((req.name.clone(), req.path.clone()));
    }
    for sub in &folder.folders {
        collect_request_index(sub, out);
    }
}

/// Find the folder node whose path is `dir`.
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

/// Collect request paths under a folder in the SAME order the sidebar shows them
/// (sub-folders first, then this level's requests — matching `collect_rows`), so
/// the runner's variable chaining matches what the user sees.
fn collect_folder_requests(folder: &Folder, out: &mut Vec<PathBuf>) {
    for sub in &folder.folders {
        collect_folder_requests(sub, out);
    }
    for req in &folder.requests {
        out.push(req.path.clone());
    }
}

/// Whether a folder, or any descendant, matches the sidebar filter.
fn folder_matches(folder: &Folder, query: &str) -> bool {
    folder.name.to_lowercase().contains(query)
        || folder
            .requests
            .iter()
            .any(|r| r.name.to_lowercase().contains(query))
        || folder.folders.iter().any(|f| folder_matches(f, query))
}

fn short_method(m: &str) -> String {
    let m = m.to_uppercase();
    match m.as_str() {
        "DELETE" => "DEL".to_string(),
        "OPTIONS" => "OPT".to_string(),
        "" => "?".to_string(),
        other => other.chars().take(4).collect(),
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

// === async + formatting ======================================================

/// Run the given request files sequentially, chaining variables through one
/// shared context (Bruno's folder/collection runner). `vars_base` is any path
/// inside the collection (used to resolve collection + env vars).
async fn run_folder(
    files: Vec<PathBuf>,
    vars_base: PathBuf,
    env: Option<String>,
    developer_mode: bool,
    opts: SendOptions,
) -> Vec<RunResult> {
    let client = match HttpClient::new(&opts) {
        Ok(c) => c,
        Err(e) => {
            return vec![RunResult {
                name: "client".to_string(),
                passed: false,
                status: 0,
                ms: 0,
                error: Some(e.to_string()),
            }]
        }
    };
    let mut ctx = RunContext {
        vars: base_vars(&vars_base, env.as_deref()),
        client,
        send_options: opts,
        developer_mode,
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
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                results.push(RunResult {
                    name: fname,
                    passed: false,
                    status: 0,
                    ms: 0,
                    error: Some(e.to_string()),
                });
                continue;
            }
        };
        let file = match bru_lang::parse(&text) {
            Ok(f) => f,
            Err(e) => {
                results.push(RunResult {
                    name: fname,
                    passed: false,
                    status: 0,
                    ms: 0,
                    error: Some(e.to_string()),
                });
                continue;
            }
        };
        if file.to_request().is_none() {
            continue; // not an HTTP request file
        }
        let outcome = run_request(&file, &mut ctx).await;
        let status = outcome.response.as_ref().map(|r| r.status).unwrap_or(0);
        let ms = outcome
            .response
            .as_ref()
            .map(|r| r.duration_ms)
            .unwrap_or(0);
        results.push(RunResult {
            name: outcome.name.clone(),
            passed: outcome.passed(),
            status,
            ms,
            error: outcome.error.clone(),
        });
    }
    results
}

async fn send_request(
    file: BruFile,
    vars_path: Option<PathBuf>,
    script_dir: Option<PathBuf>,
    env: Option<String>,
    developer_mode: bool,
    opts: SendOptions,
) -> Box<RunOutcome> {
    let name = file
        .request_name()
        .map(str::to_string)
        .unwrap_or_else(|| "request".to_string());
    let vars = vars_path
        .as_deref()
        .map(|p| base_vars(p, env.as_deref()))
        .unwrap_or_default();
    let client = match HttpClient::new(&opts) {
        Ok(c) => c,
        Err(e) => return Box::new(RunOutcome::errored(name, format!("{e}"))),
    };
    let mut ctx = RunContext {
        vars,
        client,
        send_options: opts,
        script_dir,
        developer_mode,
        ..Default::default()
    };
    Box::new(run_request(&file, &mut ctx).await)
}

fn summarize(outcome: &RunOutcome) -> String {
    if let Some(err) = &outcome.error {
        return format!("Error: {err}");
    }
    let checks: Vec<bool> = outcome
        .assertions
        .iter()
        .map(|a| a.passed)
        .chain(outcome.tests.iter().map(|t| t.passed))
        .collect();
    let passed = checks.iter().filter(|p| **p).count();
    match &outcome.response {
        Some(r) => format!(
            "{} {} - {} ms - {passed}/{} checks",
            r.status,
            r.status_text,
            r.duration_ms,
            checks.len()
        ),
        None => "No response".to_string(),
    }
}

/// Best-effort current git branch for `dir` (or an ancestor), read straight from
/// `.git/HEAD` — no git dependency. Returns the branch name (`main`), a short
/// commit hash for a detached HEAD, or `None` when `dir` isn't inside a repo.
fn git_branch(dir: &Path) -> Option<String> {
    let mut cur = Some(dir);
    while let Some(d) = cur {
        let git = d.join(".git");
        let head = if git.is_dir() {
            std::fs::read_to_string(git.join("HEAD")).ok()
        } else if git.is_file() {
            // Worktree/submodule: the `.git` file points at the real gitdir.
            let line = std::fs::read_to_string(&git).ok()?;
            let gd = line.trim().strip_prefix("gitdir:")?.trim();
            std::fs::read_to_string(d.join(gd).join("HEAD")).ok()
        } else {
            None
        };
        if let Some(h) = head {
            let h = h.trim();
            return Some(match h.strip_prefix("ref: refs/heads/") {
                Some(b) => b.to_string(),
                None => h.chars().take(7).collect(),
            });
        }
        cur = d.parent();
    }
    None
}

/// Extract the host from a URL string without pulling in the `url` crate:
/// strips scheme, path, userinfo, and port.
fn host_of(u: &str) -> String {
    let s = u.split("://").nth(1).unwrap_or(u);
    let s = s.split('/').next().unwrap_or(s);
    let s = s.rsplit('@').next().unwrap_or(s);
    s.split(':').next().unwrap_or(s).to_string()
}

/// Parse a single `Set-Cookie` header value into a [`CookieEntry`], defaulting
/// the domain to the responding host. Returns None if there's no `name=value`.
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

/// Insert or replace a cookie, keyed by (domain, path, name) like a real jar.
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

/// One step of a JSONPath query.
enum PathStep {
    Key(String),
    Index(usize),
    Wild,
}

/// Tokenize a JSONPath-ish query (`$.a.b[0].c[*]`) into steps. Supports `.key`,
/// `["key"]`, `[index]`, `[*]`, and `.*`.
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

/// Apply a small JSONPath subset to `v`. Returns the matched value (an array
/// when a wildcard fans out to several), or None on a miss.
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

/// Generate a `curl` command for a projected request (single-quote escaped).
/// Mirrors the real send pipeline: substitutes path params + query (via the same
/// resolver bru-http uses), folds in a query api-key, parses GraphQL variables to
/// an object, and adds a default Content-Type only when none is declared.
fn gen_curl(req: &Request) -> String {
    let q = |s: &str| s.replace('\'', "'\\''");
    let mut url = bru_http::resolve_url(req).unwrap_or_else(|_| req.url.clone());
    if let Auth::ApiKey {
        key,
        value,
        placement: bru_core::ApiKeyPlacement::Query,
    } = &req.auth
    {
        let sep = if url.contains('?') { '&' } else { '?' };
        url = format!("{url}{sep}{key}={value}");
    }
    let mut parts = vec![format!(
        "curl -X {} '{}'",
        req.method.to_uppercase(),
        q(&url)
    )];
    for h in req.headers.iter().filter(|h| h.enabled) {
        parts.push(format!("-H '{}: {}'", q(&h.name), q(&h.value)));
    }
    let has_ct = req
        .headers
        .iter()
        .any(|h| h.enabled && h.name.eq_ignore_ascii_case("content-type"));
    let default_ct = |parts: &mut Vec<String>, ct: &str| {
        if !has_ct {
            parts.push(format!("-H 'Content-Type: {ct}'"));
        }
    };
    match &req.auth {
        Auth::Basic { username, password } => {
            parts.push(format!("-u '{}:{}'", q(username), q(password)))
        }
        Auth::Bearer { token } => parts.push(format!("-H 'Authorization: Bearer {}'", q(token))),
        Auth::ApiKey {
            key,
            value,
            placement: bru_core::ApiKeyPlacement::Header,
        } => parts.push(format!("-H '{}: {}'", q(key), q(value))),
        _ => {}
    }
    match &req.body {
        Body::None => {}
        Body::Json(b) => {
            default_ct(&mut parts, "application/json");
            parts.push(format!("-d '{}'", q(b)));
        }
        Body::Text(b) => {
            default_ct(&mut parts, "text/plain");
            parts.push(format!("-d '{}'", q(b)));
        }
        Body::Xml(b) => {
            default_ct(&mut parts, "application/xml");
            parts.push(format!("-d '{}'", q(b)));
        }
        Body::Sparql(b) => {
            default_ct(&mut parts, "application/sparql-query");
            parts.push(format!("-d '{}'", q(b)));
        }
        Body::GraphQl { query, variables } => {
            // Match apply_body: variables are a JSON object, not a JSON string.
            let vars: serde_json::Value = if variables.trim().is_empty() {
                serde_json::json!({})
            } else {
                serde_json::from_str(variables).unwrap_or_else(|_| serde_json::json!({}))
            };
            let payload = serde_json::json!({ "query": query, "variables": vars }).to_string();
            default_ct(&mut parts, "application/json");
            parts.push(format!("-d '{}'", q(&payload)));
        }
        Body::FormUrlEncoded(fields) => {
            for f in fields.iter().filter(|f| f.enabled) {
                parts.push(format!("--data-urlencode '{}={}'", q(&f.name), q(&f.value)));
            }
        }
        Body::MultipartForm(fields) => {
            for f in fields.iter().filter(|f| f.enabled) {
                let v = match &f.value {
                    MultipartValue::Text(t) => t.clone(),
                    MultipartValue::File(p) => format!("@{p}"),
                };
                parts.push(format!("-F '{}={}'", q(&f.name), q(&v)));
            }
        }
        Body::File(items) => {
            if let Some(it) = items.iter().find(|i| i.selected).or_else(|| items.first()) {
                if let Some(ct) = &it.content_type {
                    if !ct.is_empty() {
                        parts.push(format!("-H 'Content-Type: {}'", q(ct)));
                    }
                }
                parts.push(format!("--data-binary '@{}'", q(&it.path)));
            }
        }
    }
    parts.join(" \\\n  ")
}

fn content_type_contains(headers: &[(String, String)], needle: &str) -> bool {
    headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("content-type") && v.to_ascii_lowercase().contains(needle)
    })
}

fn is_image_response(headers: &[(String, String)]) -> bool {
    content_type_contains(headers, "image/")
}

fn is_html_response(headers: &[(String, String)]) -> bool {
    content_type_contains(headers, "text/html")
}

/// Syntect syntax token for a response body, from its content-type.
fn resp_syntax(headers: &[(String, String)]) -> &'static str {
    if content_type_contains(headers, "json") {
        "json"
    } else if content_type_contains(headers, "html") {
        "html"
    } else if content_type_contains(headers, "xml") {
        "xml"
    } else if content_type_contains(headers, "javascript") {
        "js"
    } else if content_type_contains(headers, "css") {
        "css"
    } else {
        "txt"
    }
}

/// A collapsible JSON tree view. Expanded object/array paths live in `expanded`.
fn json_tree<'a>(value: &serde_json::Value, expanded: &HashSet<String>) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    json_node("$", "root", value, expanded, 0, &mut rows);
    Column::with_children(rows).spacing(1).into()
}

fn json_node<'a>(
    path: &str,
    label: &str,
    value: &serde_json::Value,
    expanded: &HashSet<String>,
    depth: u16,
    out: &mut Vec<Element<'a, Message>>,
) {
    use serde_json::Value as J;
    match value {
        J::Object(map) => {
            let open = expanded.contains(path);
            let chev = if open { "\u{25BE}" } else { "\u{25B8}" };
            let head = format!("{chev} {label}  {{{}}}", map.len());
            out.push(indent(
                depth,
                button(text(head).size(12).color(ACCENT()).font(MONO))
                    .style(|_, s| icon_button(s, ACCENT()))
                    .padding(Padding::from([1, 4]))
                    .on_press(Message::ToggleJsonNode(path.to_string()))
                    .into(),
            ));
            if open {
                for (k, v) in map {
                    // `o:`/`a:` sigils keep object keys (which may contain `/`) from
                    // colliding with array indices or sibling paths.
                    json_node(&format!("{path}/o:{k}"), k, v, expanded, depth + 1, out);
                }
            }
        }
        J::Array(arr) => {
            let open = expanded.contains(path);
            let chev = if open { "\u{25BE}" } else { "\u{25B8}" };
            let head = format!("{chev} {label}  [{}]", arr.len());
            out.push(indent(
                depth,
                button(text(head).size(12).color(ACCENT()).font(MONO))
                    .style(|_, s| icon_button(s, ACCENT()))
                    .padding(Padding::from([1, 4]))
                    .on_press(Message::ToggleJsonNode(path.to_string()))
                    .into(),
            ));
            if open {
                for (idx, v) in arr.iter().enumerate() {
                    json_node(
                        &format!("{path}/a:{idx}"),
                        &format!("[{idx}]"),
                        v,
                        expanded,
                        depth + 1,
                        out,
                    );
                }
            }
        }
        leaf => {
            let (val, color) = match leaf {
                J::String(s) => (format!("\"{s}\""), GREEN()),
                J::Number(n) => (n.to_string(), ORANGE()),
                J::Bool(b) => (b.to_string(), BLUE()),
                _ => ("null".to_string(), MUTED()),
            };
            out.push(indent(
                depth,
                row![
                    text(format!("{label}: "))
                        .size(12)
                        .color(SUBTEXT())
                        .font(MONO),
                    text(val).size(12).color(color).font(MONO),
                ]
                .into(),
            ));
        }
    }
}

/// A classic offset/hex/ascii dump (capped so a huge body can't hang the UI).
fn hex_dump(bytes: &[u8]) -> String {
    let mut out = String::new();
    for (i, chunk) in bytes.chunks(16).take(4096).enumerate() {
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
    if bytes.len() > 4096 * 16 {
        out.push_str("... (truncated)\n");
    }
    out
}

/// Write an HTML response to a temp file and open it in the OS browser.
fn open_response_in_browser(resp: &HttpResponse) {
    use std::io::Write;
    use std::sync::atomic::{AtomicU32, Ordering};
    // A unique, freshly-created file — not a fixed name in the world-shared temp
    // dir. `create_new` refuses to follow a pre-planted symlink (so a local user
    // can't redirect the write), and the per-process-unique name avoids leaking
    // the response body via a predictable leftover file.
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "bruno-rs-response-{}-{}.html",
        std::process::id(),
        n
    ));
    let open_excl = |p: &std::path::Path| {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(p)
    };
    let mut file = match open_excl(&path) {
        Ok(f) => f,
        Err(_) => {
            // A leftover from a crashed prior run (or a planted symlink): remove
            // the entry — remove_file does not follow the link — and retry once.
            let _ = std::fs::remove_file(&path);
            match open_excl(&path) {
                Ok(f) => f,
                Err(_) => return,
            }
        }
    };
    if file.write_all(&resp.body).is_err() {
        return;
    }
    drop(file);
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", ""])
        .arg(&path)
        .spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(&path).spawn();
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(&path).spawn();
}

fn pretty_body(resp: &HttpResponse) -> String {
    match resp.json() {
        Some(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|_| resp.text()),
        None => resp.text(),
    }
}

#[cfg(test)]
mod tests_helpers;
#[cfg(test)]
mod tests_update;
#[cfg(test)]
mod tests_view;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_example_round_trips() {
        let src = "meta {\n  name: X\n  type: http\n}\n\nget {\n  url: https://a.test\n  body: none\n  auth: none\n}\n";
        let mut f = bru_lang::parse(src).unwrap();
        let req = f.to_request().unwrap();
        let resp = HttpResponse {
            status: 200,
            status_text: "OK".to_string(),
            headers: vec![("content-type".to_string(), "application/json".to_string())],
            body: b"{\"a\":1}".to_vec(),
            duration_ms: 5,
        };
        edit::push_text_block(
            &mut f,
            "example",
            build_example_text("My Example", &req, &resp),
        );
        // The nested-brace example block must reparse and be readable back.
        let reparsed = bru_lang::parse(&bru_lang::serialize(&f)).expect("example must reparse");
        assert_eq!(example_count(&reparsed), 1);
        let ex = request_examples(&reparsed);
        assert_eq!(ex.len(), 1);
        assert_eq!(ex[0].0, "My Example");
    }
}
