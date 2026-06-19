// Phase 3a: real collection data. Loads a Bruno collection (the bundled sample,
// or a path arg), renders a clickable recursive sidebar, and shows the opened
// request's real method/URL/body (JSON bodies tree-sitter-highlighted).
mod editor;
mod highlight;
mod theme;

use std::path::{Path, PathBuf};

use bru_core::{BlockContent, Body, CollectionTree, Folder};
use editor::{CodeEditor, Lang};

/// What the body editor is currently editing, for Save.
enum EditTarget {
    /// The whole `.bru` source (write verbatim).
    Source,
    /// A specific body block (e.g. `body:json`): set its raw text and re-serialize.
    Body(String),
}
use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, Div, Entity, MouseButton, MouseUpEvent,
    Window, WindowBounds, WindowOptions,
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
    #[allow(dead_code)] // kept for reload/refresh later
    dir: PathBuf,
    collection: Option<CollectionTree>,
    selected: Option<PathBuf>,
    method: String,
    url: String,
    body_label: &'static str,
    body_editor: Entity<CodeEditor>,
    edit_target: EditTarget,
    status: String,
    sending: bool,
    response: Option<String>,
}

/// Run a request to completion on a fresh tokio runtime (called on a worker
/// thread). Returns the formatted response or an error string.
fn run_blocking(path: PathBuf, dir: PathBuf) -> Result<String, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let file = bru_lang::parse(&text).map_err(|e| e.to_string())?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    rt.block_on(async move {
        let opts = bru_http::SendOptions::default();
        let client = bru_http::HttpClient::new(&opts).map_err(|e| e.to_string())?;
        let mut ctx = bru_engine::RunContext {
            vars: bru_engine::base_vars(&dir, None),
            client,
            send_options: opts,
            script_dir: path.parent().map(Path::to_path_buf),
            ..Default::default()
        };
        let outcome = bru_engine::run_request(&file, &mut ctx).await;
        Ok(format_outcome(&outcome))
    })
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

impl BruApp {
    fn new(cx: &mut Context<Self>, dir: PathBuf) -> Self {
        let collection = bru_lang::load_collection(&dir).ok();
        let body_editor = cx.new(|cx| CodeEditor::new(cx, ""));
        Self {
            dir,
            collection,
            selected: None,
            method: String::new(),
            url: String::new(),
            body_label: "Body",
            body_editor,
            edit_target: EditTarget::Source,
            status: String::new(),
            sending: false,
            response: None,
        }
    }

    /// Send the selected request: run it on a worker thread (its own tokio
    /// runtime) and deliver the result back to the UI via a oneshot + cx.spawn.
    fn send(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        let dir = self.dir.clone();
        self.sending = true;
        self.status = "Sending\u{2026}".into();
        let (tx, rx) = futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(run_blocking(path, dir));
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.sending = false;
                match result {
                    Ok(Ok(body)) => {
                        this.response = Some(body);
                        this.status = "Response received".into();
                    }
                    Ok(Err(e)) => {
                        this.response = Some(format!("Error: {e}"));
                        this.status = "Send failed".into();
                    }
                    Err(_) => this.status = "Send cancelled".into(),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Write the editor's content back to disk (reconstructing the `.bru` when
    /// editing an isolated body block).
    fn save(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        let text = self.body_editor.read(cx).text().to_string();
        let ok = match &self.edit_target {
            EditTarget::Source => std::fs::write(&path, text).is_ok(),
            EditTarget::Body(block) => match std::fs::read_to_string(&path)
                .ok()
                .and_then(|t| bru_lang::parse(&t).ok())
            {
                Some(mut file) => {
                    if let Some(b) = file.blocks.iter_mut().find(|b| &b.name == block) {
                        b.content = BlockContent::Text(text);
                    }
                    std::fs::write(&path, bru_lang::serialize(&file)).is_ok()
                }
                None => false,
            },
        };
        self.status = if ok { "Saved".into() } else { "Save failed".into() };
    }

    /// Open a request file: project its method/URL/body and load the editor.
    fn open_request(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(file) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| bru_lang::parse(&t).ok())
        else {
            return;
        };
        let req = file.to_request();
        self.method = req.as_ref().map(|r| r.method.clone()).unwrap_or_default();
        self.url = req.as_ref().map(|r| r.url.clone()).unwrap_or_default();
        let (text, label, lang, target) = match req.as_ref().map(|r| &r.body) {
            Some(Body::Json(s)) => (
                s.clone(),
                "Body (JSON)",
                Lang::Json,
                EditTarget::Body("body:json".into()),
            ),
            Some(Body::Text(s)) => (
                s.clone(),
                "Body",
                Lang::Plain,
                EditTarget::Body("body:text".into()),
            ),
            Some(Body::Xml(s)) => (
                s.clone(),
                "Body",
                Lang::Plain,
                EditTarget::Body("body:xml".into()),
            ),
            // No isolated body: edit the raw .bru source.
            _ => (
                bru_lang::serialize(&file),
                "Source",
                Lang::Plain,
                EditTarget::Source,
            ),
        };
        self.body_label = label;
        self.edit_target = target;
        self.status.clear();
        self.body_editor
            .update(cx, |ed, cx| ed.set_text(&text, lang, cx));
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
                    this.open_request(path.clone(), cx);
                    cx.notify();
                }),
            );
            out.push(row);
        }
    }

    fn url_bar(&self, cx: &mut Context<Self>) -> Div {
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
                    .child(if self.sending {
                        "Sending\u{2026}".to_string()
                    } else {
                        "Send \u{2192}".to_string()
                    })
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _ev: &MouseUpEvent, _w, cx| {
                            if !this.sending {
                                this.send(cx);
                                cx.notify();
                            }
                        }),
                    ),
            )
    }

    fn response_pane(&self) -> Div {
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
            .child(tab("Response", true));
        let body = div()
            .id("resp")
            .overflow_y_scroll()
            .flex_1()
            .w_full()
            .p_3()
            .font_family("monospace")
            .text_size(px(13.))
            .line_height(px(19.))
            .text_color(theme::subtext())
            .child(
                self.response
                    .clone()
                    .unwrap_or_else(|| "No response yet \u{2014} press Send.".to_string()),
            );
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .bg(theme::bg())
            .border_t_1()
            .border_color(theme::border2())
            .child(header)
            .child(body)
    }

    fn body_pane(&self) -> Div {
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
            .child(tab(self.body_label, true));

        let content = div()
            .id("body")
            .overflow_y_scroll()
            .flex_1()
            .w_full()
            .p_3()
            .font_family("monospace")
            .text_size(px(13.))
            .line_height(px(19.))
            .child(self.body_editor.clone());

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
            .child(
                div()
                    .px_2()
                    .text_color(theme::green())
                    .text_size(px(11.))
                    .child(self.status.clone()),
            )
            .child(div().flex_1())
            .child(icon_chip("Search"))
            .child(icon_chip("Cookies"))
            .child(icon_chip("Dev Tools"))
            .child(
                div()
                    .text_color(theme::muted())
                    .text_size(px(11.))
                    .child("v0.0.0"),
            )
    }
}

impl Render for BruApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let center = div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .child(self.url_bar(cx))
            .child(self.body_pane())
            .child(self.response_pane());

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
        editor::bind_keys(cx);
        let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| BruApp::new(cx, dir.clone())),
        )
        .unwrap();
        cx.activate(true);
    });
}
