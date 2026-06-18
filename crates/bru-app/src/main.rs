//! bruno-rs — iced (wgpu) desktop app.
//!
//! M0 scope: open a Bruno collection folder and render its request tree in a
//! sidebar; selecting a request shows its raw `.bru`. The async request engine
//! and editors arrive in M1/M2; for now `update` is synchronous.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use bru_core::{CollectionTree, Folder};
use iced::widget::{button, column, container, row, scrollable, text, Column};
use iced::{Center, Element, Fill, Font, Padding};

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
    status: String,
}

#[derive(Debug, Clone)]
enum Message {
    OpenFolder,
    Select(PathBuf),
}

impl App {
    /// Initial state. If a folder path is passed on the command line, load it;
    /// otherwise prompt the user to open one.
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
            }
            Err(e) => self.status = format!("Failed to open {}: {e}", dir.display()),
        }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::OpenFolder => {
                if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                    self.load(dir);
                }
            }
            Message::Select(path) => {
                self.detail = std::fs::read_to_string(&path).ok();
                self.selected = Some(path);
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

        let detail = container(
            scrollable(
                text(
                    self.detail
                        .as_deref()
                        .unwrap_or("Select a request to view its .bru."),
                )
                .font(Font::MONOSPACE),
            )
            .height(Fill),
        )
        .width(Fill)
        .height(Fill)
        .padding(8);

        column![header, row![sidebar, detail].height(Fill)].into()
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

/// Flatten the (fully-expanded) tree into indented rows: folders as labels,
/// requests as selectable buttons.
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
