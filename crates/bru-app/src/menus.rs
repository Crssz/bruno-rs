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

#[cfg(test)]
mod cov_tests {
    use super::*;
    use crate::test_support::app_on_temp;

    fn pt() -> gpui::Point<gpui::Pixels> {
        gpui::point(gpui::px(10.), gpui::px(20.))
    }

    // ── folder / collection scaffolding ──────────────────────────────────────

    #[gpui::test]
    fn toggle_folder_inserts_then_removes(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        let p = PathBuf::from("/some/folder");
        app.update(cx, |app, cx| {
            assert!(!app.collapsed.contains(&p));
            // First toggle inserts (collapses).
            app.toggle_folder(p.clone(), cx);
            assert!(app.collapsed.contains(&p));
            // Second toggle removes (expands).
            app.toggle_folder(p.clone(), cx);
            assert!(!app.collapsed.contains(&p));
        });
    }

    #[gpui::test]
    fn new_folder_in_creates_and_increments(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let dir = tc.dir.clone();
        // First call: "New Folder".
        app.update(cx, |app, cx| app.new_folder_in(&dir, cx));
        let first = dir.join("New Folder");
        assert!(first.is_dir());
        assert!(first.join("folder.bru").is_file());
        // Second call must skip to "New Folder 2" since the first now exists.
        app.update(cx, |app, cx| app.new_folder_in(&dir, cx));
        assert!(dir.join("New Folder 2").is_dir());
    }

    // ── environment selector menu ────────────────────────────────────────────

    #[gpui::test]
    fn open_close_env_menu_toggles_state(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            assert!(app.env_menu.is_none());
            app.open_env_menu(pt(), cx);
            assert!(app.env_menu.is_some());
            app.close_env_menu(cx);
            assert!(app.env_menu.is_none());
            // Closing again with nothing open hits the no-notify branch.
            app.close_env_menu(cx);
            assert!(app.env_menu.is_none());
        });
    }

    #[gpui::test]
    fn select_env_sets_name_and_clears_menu(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.open_env_menu(pt(), cx);
            app.select_env(Some("New Environment".to_string()), cx);
            assert_eq!(app.selected_env.as_deref(), Some("New Environment"));
            assert!(app.env_menu.is_none());
            // And clearing back to None.
            app.select_env(None, cx);
            assert!(app.selected_env.is_none());
        });
    }

    #[gpui::test]
    fn select_global_env_sets_name_and_clears_menu(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.open_env_menu(pt(), cx);
            app.select_global_env(Some("Prod".to_string()), cx);
            assert_eq!(app.selected_global_env.as_deref(), Some("Prod"));
            assert!(app.env_menu.is_none());
            app.select_global_env(None, cx);
            assert!(app.selected_global_env.is_none());
        });
    }

    // ── method-picker menu ───────────────────────────────────────────────────

    #[gpui::test]
    fn open_close_method_menu_toggles_state(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.open_method_menu(pt(), cx);
            assert!(app.method_menu.is_some());
            app.close_method_menu(cx);
            assert!(app.method_menu.is_none());
            // No-op close branch.
            app.close_method_menu(cx);
            assert!(app.method_menu.is_none());
        });
    }

    #[gpui::test]
    fn pick_method_updates_active_tab_and_closes_menu(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.open_request(tc.dir.join("Repository Info.bru"), cx)
        });
        app.update(cx, |app, cx| {
            app.open_method_menu(pt(), cx);
            app.pick_method("POST", cx);
            assert_eq!(app.tabs[0].method, "POST");
            assert!(app.method_menu.is_none());
        });
    }

    #[gpui::test]
    fn pick_method_with_no_active_tab_just_closes_menu(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            // No request open: `active` is None, so the if-let body is skipped.
            assert!(app.active.is_none());
            app.open_method_menu(pt(), cx);
            app.pick_method("PUT", cx);
            assert!(app.method_menu.is_none());
        });
    }

    // ── body/auth mode menu ──────────────────────────────────────────────────

    #[gpui::test]
    fn open_close_mode_menu_toggles_state(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.open_mode_menu(pt(), true, cx);
            assert!(app.mode_menu.is_some());
            app.close_mode_menu(cx);
            assert!(app.mode_menu.is_none());
            // No-op close branch.
            app.close_mode_menu(cx);
            assert!(app.mode_menu.is_none());
        });
    }

    #[gpui::test]
    fn pick_mode_body_sets_body_mode_and_closes(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.open_request(tc.dir.join("Repository Info.bru"), cx)
        });
        app.update(cx, |app, cx| {
            app.open_mode_menu(pt(), true, cx);
            app.pick_mode("json", true, cx);
            assert!(app.mode_menu.is_none());
            // The body field on the method block should now read "json".
            let field = app
                .active_tab()
                .and_then(|t| edit::method_field(&t.file, "body"));
            assert_eq!(field.as_deref(), Some("json"));
        });
    }

    #[gpui::test]
    fn pick_mode_auth_sets_auth_mode_and_closes(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.open_request(tc.dir.join("Repository Info.bru"), cx)
        });
        app.update(cx, |app, cx| {
            app.open_mode_menu(pt(), false, cx);
            app.pick_mode("bearer", false, cx);
            assert!(app.mode_menu.is_none());
            let field = app
                .active_tab()
                .and_then(|t| edit::method_field(&t.file, "auth"));
            assert_eq!(field.as_deref(), Some("bearer"));
        });
    }

    // ── overlay builders (build the Div directly via an entity update) ───────
    // These take only `&self, cx: &mut Context<Self>` and return a `Div`, so they
    // run without a Window. Driving them open then building the overlay exercises
    // the item/listener-construction loops inside each builder.

    #[gpui::test]
    fn builds_env_menu_overlay_with_collection_envs(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            // Closed: builder returns an empty placeholder div (early-return branch).
            let _empty = app.env_menu_overlay(cx);
            // Open: builder loops over scanned collection envs (sample has one) and
            // adds the No-Environment row + per-env rows.
            app.open_env_menu(pt(), cx);
            app.select_env(Some("New Environment".to_string()), cx);
            app.open_env_menu(pt(), cx);
            let _card = app.env_menu_overlay(cx);
        });
    }

    #[gpui::test]
    fn builds_method_menu_overlay(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.open_request(tc.dir.join("Repository Info.bru"), cx)
        });
        app.update(cx, |app, cx| {
            // Early-return branch.
            let _empty = app.method_menu_overlay(cx);
            // Open: loops over the seven HTTP verbs building rows.
            app.open_method_menu(pt(), cx);
            let _card = app.method_menu_overlay(cx);
        });
    }

    #[gpui::test]
    fn builds_mode_menu_overlay_body_and_auth(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.open_request(tc.dir.join("Repository Info.bru"), cx)
        });
        app.update(cx, |app, cx| {
            // Early-return branch.
            let _empty = app.mode_menu_overlay(cx);
            // Body variant: loops over BODY_MODES, marking the active one.
            app.open_mode_menu(pt(), true, cx);
            let _body = app.mode_menu_overlay(cx);
            // Auth variant: loops over AUTH_MODES.
            app.close_mode_menu(cx);
            app.open_mode_menu(pt(), false, cx);
            let _auth = app.mode_menu_overlay(cx);
        });
    }
}
