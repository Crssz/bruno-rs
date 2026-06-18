//! bruno-rs — iced (wgpu) desktop app, styled to match Bruno's dark theme.
//!
//! Layout mirrors Bruno: a top bar, a collection sidebar with method-colored
//! request rows, a method + URL + Send bar, a request sub-tab strip
//! (Params/Headers/Body/Auth/…), and a response pane with a colored status.
//! Colors are Bruno's own dark-theme palette (`themes/dark/dark.js`).
//!
//! The structured sub-tabs render the parsed request read-only; the **Source**
//! tab is the editable raw `.bru` (validated on Save). Structured editing that
//! writes back per-field is a tracked follow-up. Sending is async over
//! `bru-engine`, so the network never blocks the UI.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use bru_core::{Auth, Body, BruFile, CollectionTree, Folder, Request};
use bru_engine::{base_vars, run_request, RunContext, RunOutcome};
use bru_http::{HttpClient, SendOptions};
use iced::widget::{button, column, container, row, scrollable, text, text_editor, Column};
use iced::{color, Background, Border, Center, Color, Element, Fill, Font, Padding, Task};

// ── Bruno dark-theme palette (themes/dark/dark.js) ──────────────────────────
const BG: Color = color!(0x1a1a1a); // background.BASE  hsl(0,0%,10%)
const MANTLE: Color = color!(0x222224);
const SURFACE0: Color = color!(0x26292b);
const SURFACE1: Color = color!(0x2f3133);
const BORDER1: Color = color!(0x333333);
const BORDER2: Color = color!(0x444444);
const TEXT: Color = color!(0xcccccc); // text.BASE  hsl(0,0%,80%)
const SUBTEXT: Color = color!(0xaaaaaa);
const MUTED: Color = color!(0x808080);
const ACCENT: Color = color!(0xd9a342); // Bruno gold
const GREEN: Color = color!(0x73e899);
const BLUE: Color = color!(0x79c8f6);
const ORANGE: Color = color!(0xf6ab79);
const RED: Color = color!(0xe06552);
const TEAL: Color = color!(0x57d6bf);
const CYAN: Color = color!(0x7cdcf0);
const BLACK: Color = color!(0x000000);

const MONO: Font = Font::MONOSPACE;

fn main() -> iced::Result {
    iced::application(App::boot, App::update, App::view)
        .title("bruno-rs")
        .theme(app_theme)
        .run()
}

fn app_theme(_: &App) -> iced::Theme {
    iced::Theme::Dark
}

#[derive(Default)]
struct App {
    collection: Option<CollectionTree>,
    selected: Option<PathBuf>,
    on_disk: Option<String>,
    editor: text_editor::Content,
    req_tab: ReqTab,
    result: Option<RunOutcome>,
    sending: bool,
    status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ReqTab {
    Params,
    #[default]
    Body,
    Headers,
    Auth,
    Assert,
    Script,
    Docs,
    Source,
}

impl ReqTab {
    const ALL: [ReqTab; 8] = [
        ReqTab::Params,
        ReqTab::Body,
        ReqTab::Headers,
        ReqTab::Auth,
        ReqTab::Assert,
        ReqTab::Script,
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
            ReqTab::Script => "Script",
            ReqTab::Docs => "Docs",
            ReqTab::Source => "Source",
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    OpenFolder,
    Select(PathBuf),
    Edit(text_editor::Action),
    Tab(ReqTab),
    Save,
    Send,
    Sent(Box<RunOutcome>),
}

impl App {
    fn boot() -> App {
        let mut app = App::default();
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
                self.selected = None;
                self.on_disk = None;
                self.editor = text_editor::Content::new();
                self.result = None;
            }
            Err(e) => self.status = format!("Failed to open {}: {e}", dir.display()),
        }
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenFolder => {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    self.load(dir);
                }
                Task::none()
            }
            Message::Select(path) => {
                match std::fs::read_to_string(&path) {
                    Ok(text) => {
                        self.editor = text_editor::Content::with_text(&text);
                        self.on_disk = Some(text);
                        self.selected = Some(path);
                        self.result = None;
                        self.status.clear();
                    }
                    Err(e) => self.status = format!("Failed to read {}: {e}", path.display()),
                }
                Task::none()
            }
            Message::Edit(action) => {
                self.editor.perform(action);
                self.result = None;
                Task::none()
            }
            Message::Tab(t) => {
                self.req_tab = t;
                Task::none()
            }
            Message::Save => {
                let Some(path) = self.selected.clone() else {
                    return Task::none();
                };
                let text = self.editor.text();
                match validate_and_save(&path, &text) {
                    Ok(()) => {
                        self.on_disk = Some(text);
                        self.status = "Saved".to_string();
                    }
                    Err(e) => self.status = format!("Not saved — {e}"),
                }
                Task::none()
            }
            Message::Send => {
                let Some(path) = self.selected.clone() else {
                    return Task::none();
                };
                self.sending = true;
                self.result = None;
                self.status = "Sending…".to_string();
                Task::perform(send_request(path), Message::Sent)
            }
            Message::Sent(outcome) => {
                self.sending = false;
                self.status = summarize(&outcome);
                self.result = Some(*outcome);
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let body = column![
            self.top_bar(),
            row![self.sidebar(), self.main_panel()].height(Fill)
        ];
        container(body)
            .style(|_| panel(BG, None))
            .width(Fill)
            .height(Fill)
            .into()
    }

    fn top_bar(&self) -> Element<'_, Message> {
        let name = self
            .collection
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_default();
        container(
            row![
                button(text("Open Collection…").size(13))
                    .style(|_, s| ghost_button(s))
                    .on_press(Message::OpenFolder),
                text(name).size(13).color(ACCENT),
                container(text("")).width(Fill),
                text(self.status.as_str()).size(12).color(SUBTEXT),
            ]
            .spacing(12)
            .align_y(Center)
            .padding(Padding::from([6, 12])),
        )
        .style(|_| panel(MANTLE, Some(BORDER1)))
        .width(Fill)
        .into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let mut col = Column::new().spacing(1).padding(Padding::from([8, 6]));
        match &self.collection {
            None => col = col.push(text("No collection loaded.").size(12).color(MUTED)),
            Some(tree) => {
                col = col.push(text(tree.name.as_str()).size(12).color(MUTED).font(Font {
                    weight: iced::font::Weight::Bold,
                    ..Font::default()
                }));
                let mut rows: Vec<Element<Message>> = Vec::new();
                collect_rows(&tree.root, 0, self.selected.as_deref(), &mut rows);
                for r in rows {
                    col = col.push(r);
                }
            }
        }
        container(scrollable(col).height(Fill))
            .style(|_| panel(BG, Some(BORDER1)))
            .width(280)
            .height(Fill)
            .into()
    }

    fn main_panel(&self) -> Element<'_, Message> {
        if self.selected.is_none() {
            return container(text("Select a request.").size(13).color(MUTED))
                .center(Fill)
                .into();
        }
        // Parse the current editor buffer for the structured sub-tab views.
        let parsed = bru_lang::parse(&self.editor.text()).ok();
        let req = parsed.as_ref().and_then(|f| f.to_request());

        let content = column![
            self.url_bar(req.as_ref()),
            self.tab_strip(),
            container(scrollable(self.tab_content(parsed.as_ref(), req.as_ref())).height(Fill))
                .padding(10)
                .height(Fill),
            self.response_pane(),
        ];
        container(content).width(Fill).height(Fill).into()
    }

    fn url_bar(&self, req: Option<&Request>) -> Element<'_, Message> {
        let method = req.map(|r| r.method.to_uppercase()).unwrap_or_default();
        let url = req.map(|r| r.url.clone()).unwrap_or_default();
        let can_send = self.selected.is_some() && !self.sending;

        let send = button(
            text(if self.sending { "Sending…" } else { "Send" })
                .size(13)
                .color(BLACK),
        )
        .style(|_, _| solid_button(ACCENT, BLACK))
        .on_press_maybe(can_send.then_some(Message::Send));

        container(
            row![
                text(method).size(13).color(method_color(req)).font(Font {
                    weight: iced::font::Weight::Bold,
                    ..Font::default()
                }),
                text(url).size(13).color(TEXT).font(MONO).width(Fill),
                button(text("Save").size(13).color(TEXT))
                    .style(|_, s| ghost_button(s))
                    .on_press(Message::Save),
                send,
            ]
            .spacing(12)
            .align_y(Center)
            .padding(8),
        )
        .style(|_| panel(MANTLE, Some(BORDER1)))
        .width(Fill)
        .into()
    }

    fn tab_strip(&self) -> Element<'_, Message> {
        let mut r = row![].spacing(2).padding(Padding::from([0, 8]));
        for t in ReqTab::ALL {
            let active = t == self.req_tab;
            r = r.push(
                button(
                    text(t.label())
                        .size(12)
                        .color(if active { TEXT } else { MUTED }),
                )
                .style(move |_, _| tab_button(active))
                .padding(Padding::from([6, 10]))
                .on_press(Message::Tab(t)),
            );
        }
        container(r)
            .style(|_| panel(SURFACE0, Some(BORDER2)))
            .width(Fill)
            .into()
    }

    fn tab_content<'a>(
        &'a self,
        file: Option<&BruFile>,
        req: Option<&Request>,
    ) -> Element<'a, Message> {
        if self.req_tab == ReqTab::Source {
            return text_editor(&self.editor)
                .font(MONO)
                .height(Fill)
                .on_action(Message::Edit)
                .into();
        }
        let Some(req) = req else {
            return text("(could not parse request)").size(12).color(RED).into();
        };
        match self.req_tab {
            ReqTab::Params => kv_view(
                req.query
                    .iter()
                    .map(|k| (k.name.as_str(), k.value.as_str(), k.enabled))
                    .chain(
                        req.path_params
                            .iter()
                            .map(|k| (k.name.as_str(), k.value.as_str(), k.enabled)),
                    ),
            ),
            ReqTab::Headers => kv_view(
                req.headers
                    .iter()
                    .map(|k| (k.name.as_str(), k.value.as_str(), k.enabled)),
            ),
            ReqTab::Body => body_view(&req.body),
            ReqTab::Auth => auth_view(&req.auth),
            ReqTab::Assert => kv_view(
                req.assertions
                    .iter()
                    .map(|a| (a.expr.as_str(), a.value.as_str(), a.enabled)),
            ),
            ReqTab::Script => {
                let pre = file.and_then(|f| f.script_pre()).unwrap_or_default();
                let post = file.and_then(|f| f.script_post()).unwrap_or_default();
                let tests = file.and_then(|f| f.tests_script()).unwrap_or_default();
                column![
                    section("pre-request"),
                    code_block(&pre),
                    section("post-response"),
                    code_block(&post),
                    section("tests"),
                    code_block(&tests),
                ]
                .spacing(4)
                .into()
            }
            ReqTab::Docs => {
                let docs = file
                    .and_then(|f| f.block("docs"))
                    .and_then(|b| match &b.content {
                        bru_core::BlockContent::Text(t) => Some(t.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                code_block(docs.trim_start())
            }
            ReqTab::Source => unreachable!(),
        }
    }

    fn response_pane(&self) -> Element<'_, Message> {
        let inner: Element<Message> = match &self.result {
            None => text("No response yet — press Send.")
                .size(12)
                .color(MUTED)
                .into(),
            Some(o) if o.error.is_some() => text(format!("Error: {}", o.error.as_deref().unwrap()))
                .size(12)
                .color(RED)
                .into(),
            Some(o) => {
                let mut col = Column::new().spacing(6);
                if let Some(r) = &o.response {
                    col = col.push(
                        row![
                            text(format!("{} {}", r.status, r.status_text))
                                .size(13)
                                .color(status_color(r.status))
                                .font(Font {
                                    weight: iced::font::Weight::Bold,
                                    ..Font::default()
                                }),
                            text(format!("{} ms", r.duration_ms))
                                .size(12)
                                .color(SUBTEXT),
                            text(format!("{} B", r.body.len())).size(12).color(SUBTEXT),
                        ]
                        .spacing(16),
                    );
                    for a in &o.assertions {
                        col = col.push(check_row(
                            a.passed,
                            &format!("{} {} {}", a.expr, a.operator, a.expected),
                        ));
                    }
                    for t in &o.tests {
                        col = col.push(check_row(t.passed, &format!("test: {}", t.name)));
                    }
                    for line in &o.console {
                        col = col.push(text(format!("| {line}")).size(12).color(MUTED).font(MONO));
                    }
                    col = col.push(code_block(&pretty_body(r)));
                }
                scrollable(col).height(Fill).into()
            }
        };
        container(container(inner).padding(10).height(Fill))
            .style(|_| panel(BG, Some(BORDER2)))
            .width(Fill)
            .height(iced::Length::FillPortion(2))
            .into()
    }
}

// ── content helpers ─────────────────────────────────────────────────────────

fn section(title: &str) -> Element<'static, Message> {
    text(title.to_string()).size(11).color(MUTED).into()
}

fn code_block(s: &str) -> Element<'static, Message> {
    container(text(s.to_string()).size(12).color(TEXT).font(MONO))
        .style(|_| panel(SURFACE0, Some(BORDER1)))
        .width(Fill)
        .padding(8)
        .into()
}

fn kv_view<'a>(items: impl Iterator<Item = (&'a str, &'a str, bool)>) -> Element<'static, Message> {
    let mut col = Column::new().spacing(3);
    let mut any = false;
    for (k, v, enabled) in items {
        any = true;
        let c = if enabled { TEXT } else { MUTED };
        col = col.push(
            row![
                text(k.to_string())
                    .size(12)
                    .color(c)
                    .font(MONO)
                    .width(iced::Length::FillPortion(2)),
                text(v.to_string())
                    .size(12)
                    .color(c)
                    .font(MONO)
                    .width(iced::Length::FillPortion(3)),
            ]
            .spacing(12),
        );
    }
    if !any {
        col = col.push(text("(none)").size(12).color(MUTED));
    }
    col.into()
}

fn body_view(body: &Body) -> Element<'static, Message> {
    match body {
        Body::None => text("No body").size(12).color(MUTED).into(),
        Body::Json(s) | Body::Text(s) | Body::Xml(s) | Body::Sparql(s) => code_block(s),
        Body::GraphQl { query, variables } => column![
            section("query"),
            code_block(query),
            section("variables"),
            code_block(variables),
        ]
        .spacing(4)
        .into(),
        Body::FormUrlEncoded(fields) => kv_view(
            fields
                .iter()
                .map(|f| (f.name.as_str(), f.value.as_str(), f.enabled)),
        ),
        Body::MultipartForm(fields) => {
            let mut col = Column::new().spacing(3);
            for f in fields {
                let v = match &f.value {
                    bru_core::MultipartValue::Text(t) => t.clone(),
                    bru_core::MultipartValue::File(p) => format!("@file({p})"),
                };
                col = col.push(
                    row![
                        text(f.name.clone()).size(12).color(TEXT).font(MONO),
                        text(v).size(12).color(SUBTEXT).font(MONO),
                    ]
                    .spacing(12),
                );
            }
            col.into()
        }
    }
}

fn auth_view(auth: &Auth) -> Element<'static, Message> {
    let mut col = Column::new().spacing(3);
    let mode = match auth {
        Auth::None => "none",
        Auth::Inherit => "inherit",
        Auth::Basic { .. } => "basic",
        Auth::Bearer { .. } => "bearer",
        Auth::ApiKey { .. } => "api-key",
        Auth::OAuth2(_) => "oauth2",
        Auth::Digest { .. } => "digest",
        Auth::AwsV4 { .. } => "awsv4",
    };
    col = col.push(
        row![
            text("mode").size(12).color(MUTED),
            text(mode).size(12).color(ACCENT)
        ]
        .spacing(12),
    );
    let field = |k: &str, v: &str| {
        row![
            text(k.to_string()).size(12).color(MUTED).font(MONO),
            text(mask(v)).size(12).color(TEXT).font(MONO),
        ]
        .spacing(12)
    };
    match auth {
        Auth::Basic { username, password } => {
            col = col
                .push(field("username", username))
                .push(field("password", password));
        }
        Auth::Bearer { token } => col = col.push(field("token", token)),
        Auth::ApiKey { key, value, .. } => {
            col = col.push(field("key", key)).push(field("value", value));
        }
        Auth::Digest { username, password } => {
            col = col
                .push(field("username", username))
                .push(field("password", password));
        }
        Auth::OAuth2(o) => {
            col = col
                .push(field("grant_type", &o.grant_type))
                .push(field("access_token_url", &o.access_token_url))
                .push(field("client_id", &o.client_id));
        }
        Auth::AwsV4 {
            region, service, ..
        } => {
            col = col
                .push(field("region", region))
                .push(field("service", service));
        }
        _ => {}
    }
    col.into()
}

/// Mask a credential value, keeping the look of Bruno's secret fields.
fn mask(v: &str) -> String {
    if v.is_empty() {
        String::new()
    } else {
        "•".repeat(v.chars().count().min(12))
    }
}

fn check_row(passed: bool, label: &str) -> Element<'static, Message> {
    let (mark, c) = if passed { ("✓", GREEN) } else { ("✗", RED) };
    row![
        text(mark).size(12).color(c),
        text(label.to_string()).size(12).color(TEXT).font(MONO),
    ]
    .spacing(8)
    .into()
}

fn collect_rows(
    folder: &Folder,
    depth: u16,
    selected: Option<&Path>,
    out: &mut Vec<Element<'_, Message>>,
) {
    for sub in &folder.folders {
        out.push(indent(
            depth,
            text(format!("▸ {}", sub.name))
                .size(12)
                .color(SUBTEXT)
                .into(),
        ));
        collect_rows(sub, depth + 1, selected, out);
    }
    for req in &folder.requests {
        let method = req.method.clone().unwrap_or_default();
        let is_sel = selected == Some(req.path.as_path());
        let mc = method_color_str(&method);
        let label = row![
            text(short_method(&method))
                .size(11)
                .color(mc)
                .font(MONO)
                .width(38),
            text(req.name.clone())
                .size(12)
                .color(if is_sel { TEXT } else { SUBTEXT }),
        ]
        .spacing(4)
        .align_y(Center);
        out.push(indent(
            depth,
            button(label)
                .style(move |_, s| sidebar_item(is_sel, s))
                .width(Fill)
                .padding(Padding::from([3, 6]))
                .on_press(Message::Select(req.path.clone()))
                .into(),
        ));
    }
}

fn indent(depth: u16, content: Element<'_, Message>) -> Element<'_, Message> {
    let pad = Padding {
        left: f32::from(depth) * 12.0,
        ..Padding::ZERO
    };
    container(content).padding(pad).into()
}

fn short_method(m: &str) -> String {
    let m = m.to_uppercase();
    match m.as_str() {
        "DELETE" => "DEL".to_string(),
        "OPTIONS" => "OPT".to_string(),
        other => other.chars().take(4).collect(),
    }
}

// ── async + formatting ──────────────────────────────────────────────────────

async fn send_request(path: PathBuf) -> Box<RunOutcome> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "request".to_string());
    let vars = base_vars(&path, None);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => return Box::new(RunOutcome::errored(name, format!("read error: {e}"))),
    };
    let file = match bru_lang::parse(&text) {
        Ok(f) => f,
        Err(e) => return Box::new(RunOutcome::errored(name, format!("parse error: {e}"))),
    };
    let client = match HttpClient::new(&SendOptions::default()) {
        Ok(c) => c,
        Err(e) => return Box::new(RunOutcome::errored(name, format!("{e}"))),
    };
    let mut ctx = RunContext {
        vars,
        client,
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
            "{} {} · {} ms · {passed}/{} checks",
            r.status,
            r.status_text,
            r.duration_ms,
            checks.len()
        ),
        None => "No response".to_string(),
    }
}

fn pretty_body(resp: &bru_http::HttpResponse) -> String {
    match resp.json() {
        Some(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|_| resp.text()),
        None => resp.text(),
    }
}

fn validate_and_save(path: &Path, text: &str) -> Result<(), String> {
    bru_lang::parse(text).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

// ── colors ──────────────────────────────────────────────────────────────────

fn method_color(req: Option<&Request>) -> Color {
    req.map(|r| method_color_str(&r.method)).unwrap_or(TEXT)
}

fn method_color_str(m: &str) -> Color {
    match m.to_uppercase().as_str() {
        "GET" => GREEN,
        "POST" => BLUE,
        "PUT" | "PATCH" => ORANGE,
        "DELETE" => RED,
        "OPTIONS" => TEAL,
        "HEAD" => CYAN,
        _ => SUBTEXT,
    }
}

fn status_color(status: u16) -> Color {
    match status {
        200..=299 => GREEN,
        300..=399 => ACCENT,
        400..=599 => RED,
        _ => TEXT,
    }
}

// ── widget styles ───────────────────────────────────────────────────────────

fn panel(bg: Color, border: Option<Color>) -> container::Style {
    container::Style {
        background: Some(Background::Color(bg)),
        text_color: Some(TEXT),
        border: Border {
            color: border.unwrap_or(Color::TRANSPARENT),
            width: if border.is_some() { 1.0 } else { 0.0 },
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

fn solid_button(bg: Color, fg: Color) -> button::Style {
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: fg,
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn ghost_button(status: button::Status) -> button::Style {
    let bg = match status {
        button::Status::Hovered => SURFACE1,
        _ => SURFACE0,
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: TEXT,
        border: Border {
            color: BORDER2,
            width: 1.0,
            radius: 4.0.into(),
        },
        ..Default::default()
    }
}

fn tab_button(active: bool) -> button::Style {
    button::Style {
        background: Some(Background::Color(Color::TRANSPARENT)),
        text_color: if active { TEXT } else { MUTED },
        border: Border {
            color: if active { ACCENT } else { Color::TRANSPARENT },
            // Approximate Bruno's active-tab underline with a bottom-only feel
            // via a full thin border in the accent color.
            width: if active { 1.0 } else { 0.0 },
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

fn sidebar_item(selected: bool, status: button::Status) -> button::Style {
    let bg = if selected {
        SURFACE0
    } else if matches!(status, button::Status::Hovered) {
        MANTLE
    } else {
        Color::TRANSPARENT
    };
    button::Style {
        background: Some(Background::Color(bg)),
        text_color: TEXT,
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::validate_and_save;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A temp file that removes itself on drop (no `tempfile` dependency).
    struct TempFile(PathBuf);
    impl TempFile {
        fn new(tag: &str) -> Self {
            static N: AtomicU32 = AtomicU32::new(0);
            let p = std::env::temp_dir().join(format!(
                "bru-app-{tag}-{}-{}.bru",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            TempFile(p)
        }
    }
    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[test]
    fn valid_text_is_written() {
        let f = TempFile::new("valid");
        let src = "meta {\n  name: X\n  type: http\n}\n";
        assert!(validate_and_save(&f.0, src).is_ok());
        assert_eq!(std::fs::read_to_string(&f.0).unwrap(), src);
    }

    #[test]
    fn invalid_text_errors_and_does_not_clobber() {
        let f = TempFile::new("invalid");
        let good = "meta {\n  name: X\n  type: http\n}\n";
        validate_and_save(&f.0, good).unwrap();
        // An unterminated block fails to parse — the existing file must survive.
        assert!(validate_and_save(&f.0, "meta {\n  name: X\n").is_err());
        assert_eq!(std::fs::read_to_string(&f.0).unwrap(), good);
    }
}
