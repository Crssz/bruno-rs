//! bruno-rs — iced (wgpu) desktop app.
//!
//! Open a Bruno collection, browse its request tree, and **send** the selected
//! request — the response (status, timing, assertions, body) renders in the
//! detail pane. Sending is async via iced `Task::perform` over `bru-engine`, so
//! the UI never blocks on the network.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use bru_core::{CollectionTree, Folder};
use bru_engine::{base_vars, run_request, RunContext, RunOutcome};
use bru_http::SendOptions;
use iced::widget::{button, column, container, row, scrollable, text, Column};
use iced::{Center, Element, Fill, Font, Padding, Task};

fn main() -> iced::Result {
    iced::application(App::boot, App::update, App::view)
        .title("bruno-rs")
        .run()
}

#[derive(Default)]
struct App {
    collection: Option<CollectionTree>,
    selected: Option<PathBuf>,
    detail: Option<String>,
    result: Option<RunOutcome>,
    sending: bool,
    status: String,
}

#[derive(Debug, Clone)]
enum Message {
    OpenFolder,
    Select(PathBuf),
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
                self.detail = None;
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
                self.detail = std::fs::read_to_string(&path).ok();
                self.selected = Some(path);
                self.result = None;
                Task::none()
            }
            Message::Send => {
                let (Some(path), Some(text)) = (self.selected.clone(), self.detail.clone()) else {
                    return Task::none();
                };
                self.sending = true;
                self.result = None;
                self.status = "Sending…".to_string();
                let vars = base_vars(&path, None);
                let options = SendOptions::default();
                Task::perform(
                    async move {
                        match bru_lang::parse(&text) {
                            Ok(file) => {
                                let mut ctx = RunContext { vars, options };
                                Box::new(run_request(&file, &mut ctx).await)
                            }
                            Err(e) => Box::new(RunOutcome::errored("request", format!("{e}"))),
                        }
                    },
                    Message::Sent,
                )
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
        let header = row![
            button("Open Collection…").on_press(Message::OpenFolder),
            text(self.status.as_str()),
        ]
        .spacing(12)
        .padding(10)
        .align_y(Center);

        let sidebar = container(scrollable(self.tree_view()).height(Fill))
            .width(340)
            .height(Fill)
            .padding(8);

        let detail = container(self.detail_view())
            .width(Fill)
            .height(Fill)
            .padding(8);

        column![header, row![sidebar, detail].height(Fill)].into()
    }

    fn detail_view(&self) -> Element<'_, Message> {
        let can_send = self.selected.is_some() && !self.sending;
        let send_btn = button(text(if self.sending { "Sending…" } else { "Send" }))
            .on_press_maybe(can_send.then_some(Message::Send));

        let title = self
            .selected
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "No request selected".to_string());

        let toolbar = row![text(title), send_btn].spacing(12).align_y(Center);

        // Body pane shows the response (when present) else the raw .bru.
        let body: Element<Message> = match &self.result {
            Some(outcome) => self.response_view(outcome),
            None => text(
                self.detail
                    .as_deref()
                    .unwrap_or("Select a request to view its .bru."),
            )
            .font(Font::MONOSPACE)
            .into(),
        };

        column![toolbar, scrollable(body).height(Fill)]
            .spacing(10)
            .into()
    }

    fn response_view<'a>(&self, outcome: &'a RunOutcome) -> Element<'a, Message> {
        let mut col = Column::new().spacing(6);

        if let Some(err) = &outcome.error {
            return text(format!("Error: {err}")).into();
        }
        if let Some(resp) = &outcome.response {
            col = col.push(text(format!(
                "{} {}   {} ms   {} bytes",
                resp.status,
                resp.status_text,
                resp.duration_ms,
                resp.body.len()
            )));
            for a in &outcome.assertions {
                let mark = if a.passed { "PASS" } else { "FAIL" };
                col = col.push(
                    text(format!("[{mark}] {} {} {}", a.expr, a.operator, a.expected))
                        .font(Font::MONOSPACE),
                );
            }
            col = col.push(text(pretty_body(resp)).font(Font::MONOSPACE));
        }
        col.into()
    }

    fn tree_view(&self) -> Element<'_, Message> {
        match &self.collection {
            None => text("No collection loaded.").into(),
            Some(tree) => {
                let mut rows: Vec<Element<Message>> = Vec::new();
                collect_rows(&tree.root, 0, self.selected.as_deref(), &mut rows);
                if rows.is_empty() {
                    rows.push(text("(no requests found)").into());
                }
                Column::with_children(rows).spacing(2).into()
            }
        }
    }
}

fn summarize(outcome: &RunOutcome) -> String {
    if let Some(err) = &outcome.error {
        return format!("Error: {err}");
    }
    let passed = outcome.assertions.iter().filter(|a| a.passed).count();
    let total = outcome.assertions.len();
    match &outcome.response {
        Some(r) => format!(
            "{} {} · {} ms · {}/{} assertions passed",
            r.status, r.status_text, r.duration_ms, passed, total
        ),
        None => "No response".to_string(),
    }
}

/// Pretty-print a JSON response body; fall back to raw text.
fn pretty_body(resp: &bru_http::HttpResponse) -> String {
    match resp.json() {
        Some(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|_| resp.text()),
        None => resp.text(),
    }
}

fn collect_rows(
    folder: &Folder,
    depth: u16,
    selected: Option<&Path>,
    out: &mut Vec<Element<'_, Message>>,
) {
    for sub in &folder.folders {
        out.push(indent(depth, text(format!("📁 {}", sub.name)).into()));
        collect_rows(sub, depth + 1, selected, out);
    }
    for req in &folder.requests {
        let method = req.method.as_deref().unwrap_or("");
        let label = format!("{method:<6} {}", req.name);
        let is_selected = selected == Some(req.path.as_path());
        let btn = button(text(label).font(Font::MONOSPACE))
            .on_press(Message::Select(req.path.clone()))
            .width(Fill)
            .style(if is_selected {
                button::primary
            } else {
                button::text
            });
        out.push(indent(depth, btn.into()));
    }
}

fn indent(depth: u16, content: Element<'_, Message>) -> Element<'_, Message> {
    let pad = Padding {
        left: f32::from(depth) * 16.0,
        ..Padding::ZERO
    };
    container(content).padding(pad).into()
}
