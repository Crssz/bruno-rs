//! Folder/collection creation and the method/mode/env dropdown menus.

use crate::*;
use gpui::prelude::*;

impl BruApp {
    pub(crate) fn toggle_folder(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.collapsed.remove(&path) {
            self.collapsed.insert(path);
        }
        cx.notify();
    }

    /// Create a new sub-folder under `dir` and reload the tree.
    pub(crate) fn new_folder_in(&mut self, dir: &Path, cx: &mut Context<Self>) {
        let mut n = 1;
        let mut path = dir.join("New Folder");
        while path.exists() {
            n += 1;
            path = dir.join(format!("New Folder {n}"));
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("New Folder");
        if std::fs::create_dir_all(&path).is_ok() {
            // Bruno folder metadata lives in folder.bru.
            let meta = format!("meta {{\n  name: {name}\n  seq: 1\n}}\n");
            let _ = std::fs::write(path.join("folder.bru"), meta);
            self.reload_collection(cx);
        }
    }

    /// Scaffold a new Bruno collection under `parent` (bruno.json + an empty
    /// environments/ dir) and open it.
    pub(crate) fn create_collection(&mut self, parent: &Path, cx: &mut Context<Self>) {
        let mut dir = parent.join("New Collection");
        let mut n = 1;
        while dir.exists() {
            n += 1;
            dir = parent.join(format!("New Collection {n}"));
        }
        let name = dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("New Collection");
        if std::fs::create_dir_all(&dir).is_ok() {
            let bruno =
                format!("{{\n  \"version\": \"1\",\n  \"name\": \"{name}\",\n  \"type\": \"collection\"\n}}\n");
            let _ = std::fs::write(dir.join("bruno.json"), bruno);
            let _ = std::fs::create_dir_all(dir.join("environments"));
            self.load_collection(dir, cx);
        }
    }

    // ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ active environment selector ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬
    pub(crate) fn open_env_menu(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        self.env_menu = Some(pos);
        cx.notify();
    }
    pub(crate) fn close_env_menu(&mut self, cx: &mut Context<Self>) {
        if self.env_menu.take().is_some() {
            cx.notify();
        }
    }
    pub(crate) fn select_env(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        self.selected_env = name;
        self.env_menu = None;
        self.refresh_vars();
        cx.notify();
    }
    pub(crate) fn select_global_env(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        self.selected_global_env = name;
        self.env_menu = None;
        self.refresh_vars();
        cx.notify();
    }

    pub(crate) fn open_method_menu(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        self.method_menu = Some(pos);
        cx.notify();
    }
    pub(crate) fn close_method_menu(&mut self, cx: &mut Context<Self>) {
        if self.method_menu.take().is_some() {
            cx.notify();
        }
    }
    pub(crate) fn pick_method(&mut self, m: &str, cx: &mut Context<Self>) {
        if let Some(i) = self.active {
            edit::set_method(&mut self.tabs[i].file, m);
            self.tabs[i].method = m.to_string();
        }
        self.method_menu = None;
        cx.notify();
    }

    pub(crate) fn open_mode_menu(
        &mut self,
        pos: Point<Pixels>,
        is_body: bool,
        cx: &mut Context<Self>,
    ) {
        self.mode_menu = Some((pos, is_body));
        cx.notify();
    }
    pub(crate) fn close_mode_menu(&mut self, cx: &mut Context<Self>) {
        if self.mode_menu.take().is_some() {
            cx.notify();
        }
    }
    pub(crate) fn pick_mode(&mut self, mode: &str, is_body: bool, cx: &mut Context<Self>) {
        if is_body {
            self.set_body_mode(mode, cx);
        } else {
            self.set_auth_mode(mode, cx);
        }
        self.mode_menu = None;
        cx.notify();
    }

    /// The body/auth mode dropdown (anchored under the sub-tab-strip chip).
    pub(crate) fn mode_menu_overlay(&self, cx: &mut Context<Self>) -> Div {
        let Some((pos, is_body)) = self.mode_menu else {
            return div();
        };
        let (list, field) = if is_body {
            (BODY_MODES, "body")
        } else {
            (AUTH_MODES, "auth")
        };
        let cur = self
            .active_tab()
            .and_then(|t| edit::method_field(&t.file, field))
            .unwrap_or_else(|| "none".into());
        let mut card = div()
            .absolute()
            .left(pos.x)
            .top(pos.y)
            .occlude()
            .flex()
            .flex_col()
            .py_1()
            .w(px(170.))
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2());
        for m in list {
            let m = *m;
            let active = m == cur;
            card = card.child(
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
                    .child(m)
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                            this.pick_mode(m, is_body, cx)
                        }),
                    ),
            );
        }
        div()
            .absolute()
            .inset_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseDownEvent, _w, cx| this.close_mode_menu(cx)),
            )
            .child(card)
    }

    /// The method-picker dropdown (anchored under the URL-bar method badge).
    pub(crate) fn method_menu_overlay(&self, cx: &mut Context<Self>) -> Div {
        let Some(pos) = self.method_menu else {
            return div();
        };
        let mut card = div()
            .absolute()
            .left(pos.x)
            .top(pos.y)
            .occlude()
            .flex()
            .flex_col()
            .py_1()
            .w(px(120.))
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2());
        for m in ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"] {
            card = card.child(
                div()
                    .px_3()
                    .py_1()
                    .text_size(px(12.))
                    .font_family("monospace")
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(theme::method_color(m))
                    .hover(|s| s.bg(theme::surface0()))
                    .child(m)
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _e: &MouseUpEvent, _w, cx| this.pick_method(m, cx)),
                    ),
            );
        }
        // Full-screen catcher so a click outside the card dismisses it.
        div()
            .absolute()
            .inset_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseDownEvent, _w, cx| this.close_method_menu(cx)),
            )
            .child(card)
    }

    /// The active-environment dropdown (anchored under the toolbar chip).
    pub(crate) fn env_menu_overlay(&self, cx: &mut Context<Self>) -> Div {
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
            .id("env-menu")
            .absolute()
            .left(pos.x)
            .top(pos.y)
            .occlude()
            .flex()
            .flex_col()
            .py_1()
            .w(px(200.))
            .max_h(px(360.))
            .overflow_y_scroll()
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
        // Global (app-level) environments overlay collection vars beneath them.
        let globals = envfs::scan_envs(&globals_root());
        if !globals.is_empty() {
            card = card.child(
                div()
                    .px_3()
                    .py_1()
                    .text_size(px(10.))
                    .text_color(theme::muted())
                    .child("GLOBAL"),
            );
            card = card.child(
                item("No Global Env".into(), self.selected_global_env.is_none()).on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.select_global_env(None, cx)),
                ),
            );
            for name in globals {
                let active = self.selected_global_env.as_deref() == Some(name.as_str());
                let n = name.clone();
                card = card.child(item(name, active).on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                        this.select_global_env(Some(n.clone()), cx)
                    }),
                ));
            }
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
}
