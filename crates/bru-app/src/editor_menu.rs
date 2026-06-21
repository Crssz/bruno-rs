//! The right-click edit menu (Cut / Copy / Paste / Select All) shown over any
//! code editor. A `CodeEditor` emits `editor::EditorMenu` on right-click; the
//! app stores it (`editor_menu`) and renders this anchored overlay, mirroring
//! the `tab_menu` / sidebar `ctx_menu` pattern. Items act back on the editor the
//! menu was opened over.

use crate::*;
use gpui::prelude::*;

/// Which edit command a menu item runs against the target editor.
#[derive(Clone, Copy)]
enum EditAction {
    Cut,
    Copy,
    Paste,
    SelectAll,
    Format,
}

impl BruApp {
    pub(crate) fn close_editor_menu(&mut self, cx: &mut Context<Self>) {
        if self.editor_menu.take().is_some() {
            cx.notify();
        }
    }

    /// Run an edit command against the editor the menu was opened over, then
    /// close the menu. The editor keeps focus (right-click focused it), so the
    /// command sees the same selection the user right-clicked on.
    fn editor_menu_run(&mut self, act: EditAction, cx: &mut Context<Self>) {
        if let Some(menu) = self.editor_menu.take() {
            match act {
                // Format reports success/failure (e.g. a syntax error) in the status bar.
                EditAction::Format => {
                    self.status = match menu.editor.update(cx, |ed, cx| ed.do_format(cx)) {
                        Ok(()) => "Formatted".into(),
                        Err(e) => e,
                    };
                }
                EditAction::Cut => menu.editor.update(cx, |ed, cx| ed.do_cut(cx)),
                EditAction::Copy => menu.editor.update(cx, |ed, cx| ed.do_copy(cx)),
                EditAction::Paste => menu.editor.update(cx, |ed, cx| ed.do_paste(cx)),
                EditAction::SelectAll => menu.editor.update(cx, |ed, cx| ed.do_select_all(cx)),
            }
            cx.notify();
        }
    }

    /// Navigate to the target the menu was opened over (a `{{var}}` definition or
    /// a `require(...)` module), then close the menu.
    fn editor_menu_goto(&mut self, cx: &mut Context<Self>) {
        if let Some(menu) = self.editor_menu.take() {
            if let Some(target) = menu.goto {
                self.on_goto_definition(&target, cx);
                return;
            }
        }
        cx.notify();
    }

    /// The anchored Cut/Copy/Paste/Select All menu. Items that don't apply (Cut/
    /// Copy without a selection, Cut/Paste on a read-only editor) are greyed out.
    pub(crate) fn editor_menu_overlay(&self, cx: &mut Context<Self>) -> Div {
        let Some(menu) = &self.editor_menu else {
            return div();
        };
        let pos = menu.pos;
        let editable = !menu.read_only;
        let has_sel = menu.has_selection;
        let formattable = menu.formattable;
        // A navigation target under the click → a context-aware "Go to" item.
        let goto_label = menu.goto.as_ref().map(|t| match t {
            editor::GotoTarget::Module { .. } => "Go to Implementation",
            editor::GotoTarget::Var(_) | editor::GotoTarget::Local(_) => "Go to Definition",
        });

        let item = |label: &str, enabled: bool| {
            let mut d = div()
                .px_3()
                .py_1()
                .text_size(px(13.))
                .child(label.to_string());
            if enabled {
                d = d
                    .text_color(theme::text())
                    .hover(|s| s.bg(theme::surface0()));
            } else {
                d = d.text_color(theme::muted());
            }
            d
        };
        let card = div()
            .id("editor-menu")
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
            // Navigation sits at the top (Zed's order), above the edit commands.
            .when(goto_label.is_some(), |c| {
                let label = goto_label.unwrap();
                c.child(item(label, true).on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.editor_menu_goto(cx)),
                ))
                .child(div().my_1().mx_2().h(px(1.)).bg(theme::border2()))
            })
            .child(
                item("Cut", editable && has_sel).when(editable && has_sel, |d| {
                    d.on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                            this.editor_menu_run(EditAction::Cut, cx)
                        }),
                    )
                }),
            )
            .child(item("Copy", has_sel).when(has_sel, |d| {
                d.on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                        this.editor_menu_run(EditAction::Copy, cx)
                    }),
                )
            }))
            .child(item("Paste", editable).when(editable, |d| {
                d.on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                        this.editor_menu_run(EditAction::Paste, cx)
                    }),
                )
            }))
            .child(item("Select All", true).on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                    this.editor_menu_run(EditAction::SelectAll, cx)
                }),
            ))
            // Format (JS/TS or JSON only): a separated group below the edit items.
            .when(editable && formattable, |c| {
                c.child(div().my_1().mx_2().h(px(1.)).bg(theme::border2()))
                    .child(item("Format", true).on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                            this.editor_menu_run(EditAction::Format, cx)
                        }),
                    ))
            });

        // Full-screen catcher: any click outside dismisses the menu.
        div()
            .absolute()
            .inset_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseDownEvent, _w, cx| this.close_editor_menu(cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _e: &MouseDownEvent, _w, cx| this.close_editor_menu(cx)),
            )
            .child(card)
    }
}

#[cfg(test)]
mod cov_tests {
    use super::*;
    use crate::EditorMenuState;
    use editor::{CodeEditor, GotoTarget};
    use gpui::{point, px, AppContext, Entity, Point};

    /// Build a `CodeEditor` entity holding `text`, optionally fully selected.
    fn mk_editor(
        cx: &mut Context<BruApp>,
        text: &str,
        lang: editor::Lang,
        select_all: bool,
    ) -> Entity<CodeEditor> {
        let ed = cx.new(|cx| {
            let mut e = CodeEditor::new(cx, text);
            e.set_text(text, lang, cx);
            e
        });
        if select_all {
            ed.update(cx, |e, cx| e.do_select_all(cx));
        }
        ed
    }

    /// Populate `app.editor_menu` with a state pointing at `editor`.
    fn set_menu(
        app: &mut BruApp,
        editor: Entity<CodeEditor>,
        pos: Point<gpui::Pixels>,
        read_only: bool,
        has_selection: bool,
        formattable: bool,
        goto: Option<GotoTarget>,
    ) {
        app.editor_menu = Some(EditorMenuState {
            editor,
            pos,
            read_only,
            has_selection,
            formattable,
            goto,
        });
    }

    // ── close_editor_menu ──────────────────────────────────────────────────────

    #[gpui::test]
    fn close_editor_menu_clears_an_open_menu(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = crate::test_support::app_on_temp(cx);
        app.update(cx, |app, cx| {
            let ed = mk_editor(cx, "hello", editor::Lang::Plain, false);
            set_menu(app, ed, point(px(10.), px(20.)), false, false, false, None);
            assert!(app.editor_menu.is_some());
            app.close_editor_menu(cx);
            assert!(app.editor_menu.is_none());
        });
    }

    #[gpui::test]
    fn close_editor_menu_is_noop_when_closed(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = crate::test_support::app_on_temp(cx);
        app.update(cx, |app, cx| {
            assert!(app.editor_menu.is_none());
            // Take() returns None → the early branch, no notify, still closed.
            app.close_editor_menu(cx);
            assert!(app.editor_menu.is_none());
        });
    }

    // ── editor_menu_run: each EditAction acts on the editor + closes the menu ───

    #[gpui::test]
    fn run_copy_writes_selection_and_closes(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = crate::test_support::app_on_temp(cx);
        app.update(cx, |app, cx| {
            let ed = mk_editor(cx, "copy me", editor::Lang::Plain, true);
            set_menu(
                app,
                ed.clone(),
                point(px(1.), px(1.)),
                false,
                true,
                false,
                None,
            );
            app.editor_menu_run(EditAction::Copy, cx);
            // Copy leaves the buffer unchanged, but the menu is consumed.
            assert!(app.editor_menu.is_none());
            assert!(ed.read(cx).text() == "copy me");
        });
    }

    #[gpui::test]
    fn run_cut_removes_selection_and_closes(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = crate::test_support::app_on_temp(cx);
        app.update(cx, |app, cx| {
            let ed = mk_editor(cx, "cut all", editor::Lang::Plain, true);
            set_menu(
                app,
                ed.clone(),
                point(px(1.), px(1.)),
                false,
                true,
                false,
                None,
            );
            app.editor_menu_run(EditAction::Cut, cx);
            assert!(app.editor_menu.is_none());
            // The fully-selected buffer is emptied by Cut.
            assert!(ed.read(cx).text().is_empty());
        });
    }

    #[gpui::test]
    fn run_paste_inserts_clipboard_and_closes(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = crate::test_support::app_on_temp(cx);
        app.update(cx, |app, cx| {
            // Seed the clipboard via a Copy run, then Paste it into an empty editor.
            let src = mk_editor(cx, "PAYLOAD", editor::Lang::Plain, true);
            set_menu(app, src, point(px(1.), px(1.)), false, true, false, None);
            app.editor_menu_run(EditAction::Copy, cx);

            let dst = mk_editor(cx, "", editor::Lang::Plain, false);
            set_menu(
                app,
                dst.clone(),
                point(px(1.), px(1.)),
                false,
                false,
                false,
                None,
            );
            app.editor_menu_run(EditAction::Paste, cx);
            assert!(app.editor_menu.is_none());
            assert!(dst.read(cx).text() == "PAYLOAD");
        });
    }

    #[gpui::test]
    fn run_select_all_selects_and_closes(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = crate::test_support::app_on_temp(cx);
        app.update(cx, |app, cx| {
            let ed = mk_editor(cx, "abc\ndef", editor::Lang::Plain, false);
            set_menu(
                app,
                ed.clone(),
                point(px(1.), px(1.)),
                false,
                false,
                false,
                None,
            );
            app.editor_menu_run(EditAction::SelectAll, cx);
            assert!(app.editor_menu.is_none());
            // Buffer content is unchanged by Select All.
            assert!(ed.read(cx).text() == "abc\ndef");
        });
    }

    #[gpui::test]
    fn run_format_ok_sets_formatted_status(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = crate::test_support::app_on_temp(cx);
        app.update(cx, |app, cx| {
            // Messy-but-valid JSON reformats (serde_json, in-process) → success.
            let ed = mk_editor(cx, "{\"a\":1,\"b\":2}", editor::Lang::Json, false);
            set_menu(app, ed, point(px(1.), px(1.)), false, false, true, None);
            app.editor_menu_run(EditAction::Format, cx);
            assert!(app.editor_menu.is_none());
            assert_eq!(app.status, "Formatted");
        });
    }

    #[gpui::test]
    fn run_format_err_sets_error_status(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = crate::test_support::app_on_temp(cx);
        app.update(cx, |app, cx| {
            // Invalid JSON → do_format returns Err, which becomes the status.
            let ed = mk_editor(cx, "{not json", editor::Lang::Json, false);
            set_menu(app, ed, point(px(1.), px(1.)), false, false, true, None);
            app.editor_menu_run(EditAction::Format, cx);
            assert!(app.editor_menu.is_none());
            assert!(app.status.contains("Invalid JSON"));
        });
    }

    #[gpui::test]
    fn run_is_noop_when_menu_closed(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = crate::test_support::app_on_temp(cx);
        app.update(cx, |app, cx| {
            assert!(app.editor_menu.is_none());
            // No menu → the `if let Some` guard skips everything.
            app.editor_menu_run(EditAction::SelectAll, cx);
            assert!(app.editor_menu.is_none());
        });
    }

    // ── editor_menu_goto ───────────────────────────────────────────────────────

    #[gpui::test]
    fn goto_with_var_target_routes_and_closes(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = crate::test_support::app_on_temp(cx);
        app.update(cx, |app, cx| {
            let ed = mk_editor(cx, "{{token}}", editor::Lang::Plain, false);
            set_menu(
                app,
                ed,
                point(px(1.), px(1.)),
                false,
                false,
                false,
                Some(GotoTarget::Var("token".into())),
            );
            // Routes through on_goto_definition (no active tab → resolves to a
            // status, but does not panic) and consumes the menu.
            app.editor_menu_goto(cx);
            assert!(app.editor_menu.is_none());
        });
    }

    #[gpui::test]
    fn goto_without_target_just_closes(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = crate::test_support::app_on_temp(cx);
        app.update(cx, |app, cx| {
            let ed = mk_editor(cx, "plain", editor::Lang::Plain, false);
            set_menu(app, ed, point(px(1.), px(1.)), false, false, false, None);
            // goto is None → take() consumes the menu, then falls through to notify.
            app.editor_menu_goto(cx);
            assert!(app.editor_menu.is_none());
        });
    }

    #[gpui::test]
    fn goto_is_noop_when_menu_closed(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = crate::test_support::app_on_temp(cx);
        app.update(cx, |app, cx| {
            assert!(app.editor_menu.is_none());
            app.editor_menu_goto(cx);
            assert!(app.editor_menu.is_none());
        });
    }

    // ── editor_menu_overlay: windowed render exercises the view builder ─────────

    /// Open the menu with the given flags in a real window and re-park so render
    /// runs `editor_menu_overlay` end-to-end (every item + branch builds).
    fn render_menu_with(
        cx: &mut gpui::TestAppContext,
        read_only: bool,
        has_selection: bool,
        formattable: bool,
        goto: Option<GotoTarget>,
    ) {
        let tc = crate::test_support::temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, _w, cx| {
                let ed = mk_editor(cx, "value {{v}}", editor::Lang::JavaScript, has_selection);
                set_menu(
                    app,
                    ed,
                    point(px(40.), px(60.)),
                    read_only,
                    has_selection,
                    formattable,
                    goto,
                );
            })
            .unwrap();
        // Re-park so the now-open overlay builder runs in render.
        cx.run_until_parked();
        window
            .update(cx, |app, _w, _cx| assert!(app.editor_menu.is_some()))
            .unwrap();
    }

    #[gpui::test]
    fn overlay_renders_editable_with_selection_and_format(cx: &mut gpui::TestAppContext) {
        // editable + has_selection + formattable → Cut/Copy/Paste/Select All all
        // enabled, plus the separated Format group.
        render_menu_with(cx, false, true, true, None);
    }

    #[gpui::test]
    fn overlay_renders_readonly_no_selection(cx: &mut gpui::TestAppContext) {
        // read_only + no selection → Cut/Copy/Paste greyed (disabled branch),
        // Select All still enabled, no Format group (editable is false).
        render_menu_with(cx, true, false, false, None);
    }

    #[gpui::test]
    fn overlay_renders_goto_var_label(cx: &mut gpui::TestAppContext) {
        // A Var target adds the "Go to Definition" row + divider at the top.
        render_menu_with(cx, false, false, false, Some(GotoTarget::Var("v".into())));
    }

    #[gpui::test]
    fn overlay_renders_goto_module_label(cx: &mut gpui::TestAppContext) {
        // A Module target adds the "Go to Implementation" row.
        render_menu_with(
            cx,
            false,
            true,
            false,
            Some(GotoTarget::Module {
                spec: "./mod".into(),
                symbol: None,
            }),
        );
    }

    #[gpui::test]
    fn overlay_renders_goto_local_label(cx: &mut gpui::TestAppContext) {
        // A Local target also yields the "Go to Definition" label branch.
        render_menu_with(
            cx,
            false,
            false,
            false,
            Some(GotoTarget::Local("fn1".into())),
        );
    }

    #[gpui::test]
    fn overlay_is_empty_div_when_closed(cx: &mut gpui::TestAppContext) {
        // With no menu, editor_menu_overlay returns a bare div (the early return).
        let (app, _tc) = crate::test_support::app_on_temp(cx);
        app.update(cx, |app, cx| {
            assert!(app.editor_menu.is_none());
            let _ = app.editor_menu_overlay(cx);
        });
    }
}
