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
