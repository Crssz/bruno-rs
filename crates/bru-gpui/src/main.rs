// Phase 3a: real collection data. Loads a Bruno collection (the bundled sample,
// or a path arg), renders a clickable recursive sidebar, and shows the opened
// request's real method/URL/body (JSON bodies tree-sitter-highlighted).
mod highlight;
mod theme;

use std::path::{Path, PathBuf};

use bru_core::{Body, CollectionTree, Folder};
use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, Div, MouseButton, MouseUpEvent, SharedString,
    StyledText, Window, WindowBounds, WindowOptions,
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
fn tab(label: &str, active: bool) -> Div {
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
    selected: Option<PathBuf>,
    method: String,
    url: String,
    body: SharedString,
    body_is_json: bool,
}

impl BruApp {
    fn new(dir: PathBuf) -> Self {
        let collection = bru_lang::load_collection(&dir).ok();
        Self {
            dir,
            collection,
            selected: None,
            method: String::new(),
            url: String::new(),
            body: "".into(),
            body_is_json: false,
        }
    }

    /// Open a request file: project its method/URL/body for display.
    fn open_request(&mut self, path: PathBuf) {
        let Some(file) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| bru_lang::parse(&t).ok())
        else {
            return;
        };
        let req = file.to_request();
        self.method = req.as_ref().map(|r| r.method.clone()).unwrap_or_default();
        self.url = req.as_ref().map(|r| r.url.clone()).unwrap_or_default();
        match req.as_ref().map(|r| &r.body) {
            Some(Body::Json(s)) => {
                self.body = s.clone().into();
                self.body_is_json = true;
            }
            _ => {
                // No JSON body: show the raw .bru source (plain).
                self.body = bru_lang::serialize(&file).into();
                self.body_is_json = false;
            }
        }
        self.selected = Some(path);
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
            let active = self.selected.as_deref() == Some(path.as_path());
            let method = req.method.clone().unwrap_or_default();
            let row = req_row(&method, &req.name, active, depth).on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _ev: &MouseUpEvent, _win, cx| {
                    this.open_request(path.clone());
                    cx.notify();
                }),
            );
            out.push(row);
        }
    }

    fn url_bar(&self) -> Div {
        let method = if self.method.is_empty() {
            "GET".to_string()
        } else {
            self.method.to_uppercase()
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
                    .child(if self.url.is_empty() {
                        "Select a request".to_string()
                    } else {
                        self.url.clone()
                    }),
            )
            .child(icon_chip("</>"))
            .child(chip("Save"))
            .child(
                div()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .bg(theme::accent())
                    .text_color(theme::bg())
                    .text_size(px(13.))
                    .child("Send \u{2192}"),
            )
    }

    fn body_pane(&self, window: &mut Window) -> Div {
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
            .child(tab(if self.body_is_json { "Body (JSON)" } else { "Source" }, true));

        let content = if self.body_is_json {
            let mut base = window.text_style();
            base.font_family = "monospace".into();
            base.color = theme::text();
            base.font_size = px(13.).into();
            let spans = highlight::json(&self.body);
            div()
                .id("body")
                .overflow_y_scroll()
                .flex_1()
                .w_full()
                .p_3()
                .font_family("monospace")
                .text_size(px(13.))
                .line_height(px(19.))
                .child(StyledText::new(self.body.clone()).with_default_highlights(&base, spans))
        } else {
            div()
                .id("body")
                .overflow_y_scroll()
                .flex_1()
                .w_full()
                .p_3()
                .font_family("monospace")
                .text_size(px(13.))
                .line_height(px(19.))
                .text_color(theme::subtext())
                .child(self.body.clone())
        };

        div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .bg(theme::bg())
            .child(header)
            .child(content)
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
            .child(div().flex_1())
            .child(icon_chip("Search"))
            .child(icon_chip("Cookies"))
            .child(icon_chip("Dev Tools"))
            .child(div().text_color(theme::muted()).text_size(px(11.)).child(
                format!("{} \u{00B7} v0.0.0", self.dir.display()),
            ))
    }
}

impl Render for BruApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let center = div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .child(self.url_bar())
            .child(self.body_pane(window));

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
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|_| BruApp::new(dir.clone())),
        )
        .unwrap();
        cx.activate(true);
    });
}
