//! bruno-rs — iced (wgpu) desktop app.
//!
//! Open a Bruno collection, browse its request tree, **edit** the selected
//! request's raw `.bru` in place, **save** it (validated by `bru-lang` first),
//! and **send** it — the response (status, timing, assertions, body) renders in
//! the detail pane. Sending is async via iced `Task::perform` over `bru-engine`,
//! so the network never blocks the UI. (Folder open, request preview, and Save
//! do small local file reads/writes on the UI thread; the env picker and
//! fully-async IO are tracked follow-ups.)

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use bru_core::{CollectionTree, Folder};
use bru_engine::{base_vars, run_request, RunContext, RunOutcome};
use bru_http::{HttpClient, SendOptions};
use iced::widget::{button, column, container, row, scrollable, text, text_editor, Column};
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
    /// On-disk text of the selected request, used to detect unsaved edits.
    on_disk: Option<String>,
    /// Editable buffer for the selected request's raw `.bru`.
    editor: text_editor::Content,
    result: Option<RunOutcome>,
    sending: bool,
    status: String,
}

#[derive(Debug, Clone)]
enum Message {
    OpenFolder,
    Select(PathBuf),
    Edit(text_editor::Action),
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
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                self.editor = text_editor::Content::with_text(&text);
                self.on_disk = Some(text);
                self.selected = Some(path);
                self.result = None;
                self.status.clear();
                Task::none()
            }
            Message::Edit(action) => {
                self.editor.perform(action);
                // Editing dismisses any stale response so the editor is visible.
                self.result = None;
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
        let has_selection = self.selected.is_some();
        let can_send = has_selection && !self.sending;
        let send_btn = button(text(if self.sending { "Sending…" } else { "Send" }))
            .on_press_maybe(can_send.then_some(Message::Send));
        let save_btn = button(text("Save")).on_press_maybe(has_selection.then_some(Message::Save));

        let dirty = self.is_modified();
        let base_title = self
            .selected
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "No request selected".to_string());
        let title = if dirty {
            format!("{base_title} •")
        } else {
            base_title
        };

        let toolbar = row![text(title), save_btn, send_btn]
            .spacing(12)
            .align_y(Center);

        // Body pane shows the response (after Send) else the editable raw .bru.
        let body: Element<Message> = match &self.result {
            Some(outcome) => scrollable(self.response_view(outcome)).height(Fill).into(),
            None if has_selection => text_editor(&self.editor)
                .font(Font::MONOSPACE)
                .height(Fill)
                .on_action(Message::Edit)
                .into(),
            None => text("Select a request to view and edit its .bru.").into(),
        };

        column![toolbar, body].spacing(10).into()
    }

    /// True when the editor buffer differs from the on-disk text.
    /// Ignores a trailing newline difference, since `Content::text()` does not
    /// emit the file's final newline.
    fn is_modified(&self) -> bool {
        match &self.on_disk {
            Some(disk) => disk.trim_end_matches('\n') != self.editor.text().trim_end_matches('\n'),
            None => false,
        }
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
            for t in &outcome.tests {
                let mark = if t.passed { "PASS" } else { "FAIL" };
                let extra = match &t.error {
                    Some(e) if !t.passed => format!("  ({e})"),
                    _ => String::new(),
                };
                col = col
                    .push(text(format!("[{mark}] test: {}{extra}", t.name)).font(Font::MONOSPACE));
            }
            for line in &outcome.console {
                col = col.push(text(format!("| {line}")).font(Font::MONOSPACE));
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

/// Validate `text` as a `.bru` file, then write it to `path`.
///
/// Parsing happens first: if it fails, the error is returned and the file on
/// disk is left untouched. Only valid `.bru` text reaches `std::fs::write`.
fn validate_and_save(path: &Path, text: &str) -> Result<(), String> {
    bru_lang::parse(text).map_err(|e| format!("parse error: {e}"))?;
    std::fs::write(path, text).map_err(|e| format!("write error: {e}"))
}

/// Re-read the request file at send time (so on-disk edits are picked up), build
/// a one-off client, run it, and box the outcome for the `Sent` message.
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
    let total = checks.len();
    match &outcome.response {
        Some(r) => format!(
            "{} {} · {} ms · {passed}/{total} checks passed",
            r.status, r.status_text, r.duration_ms
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

#[cfg(test)]
mod tests {
    use super::validate_and_save;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A unique temp path per call; no external deps. The file is created by the
    /// code under test, and the guard removes it on drop.
    struct TempFile(PathBuf);

    impl TempFile {
        fn new() -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let name = format!("bru-app-test-{}-{n}.bru", std::process::id());
            TempFile(std::env::temp_dir().join(name))
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    const VALID_BRU: &str =
        "meta {\n  name: t\n  type: http\n}\n\nget {\n  url: https://example.com\n}\n";

    #[test]
    fn valid_text_is_written() {
        let tmp = TempFile::new();
        validate_and_save(&tmp.0, VALID_BRU).expect("valid .bru should save");
        let on_disk = std::fs::read_to_string(&tmp.0).expect("file should exist");
        assert_eq!(on_disk, VALID_BRU);
    }

    #[test]
    fn invalid_text_errors_and_does_not_write() {
        let tmp = TempFile::new();
        // Bare word with no block opener — rejected by bru_lang::parse.
        let err = validate_and_save(&tmp.0, "this is not valid bru\n")
            .expect_err("invalid .bru should error");
        assert!(err.contains("parse error"), "unexpected error: {err}");
        assert!(!tmp.0.exists(), "file must not be created on parse failure");
    }

    #[test]
    fn invalid_text_does_not_clobber_existing_file() {
        let tmp = TempFile::new();
        validate_and_save(&tmp.0, VALID_BRU).expect("seed valid file");
        // A failing save must leave the previous good contents intact.
        let _ = validate_and_save(&tmp.0, "garbage with no braces\n")
            .expect_err("invalid .bru should error");
        let on_disk = std::fs::read_to_string(&tmp.0).expect("file should still exist");
        assert_eq!(on_disk, VALID_BRU, "existing file must be untouched");
    }
}
