//! Open-tab management, tab context menu and close confirmations.

use crate::*;
use gpui::prelude::*;

impl BruApp {
    pub(crate) fn active_tab(&self) -> Option<&OpenTab> {
        self.active.and_then(|i| self.tabs.get(i))
    }

    /// Remove tab `i`, fixing up the active index.
    pub(crate) fn close_tab(&mut self, i: usize) {
        if i >= self.tabs.len() {
            return;
        }
        let p = self.tabs[i].path.clone();
        self.dirty.remove(&p);
        self.tabs.remove(i);
        self.active = if self.tabs.is_empty() {
            None
        } else {
            match self.active {
                Some(a) if a > i => Some(a - 1),
                Some(a) if a == i => Some(i.min(self.tabs.len() - 1)),
                other => other,
            }
        };
    }

    /// Close tab `i`, but prompt first if it has unsaved edits (data-loss guard).
    pub(crate) fn request_close_tab(&mut self, i: usize, cx: &mut Context<Self>) {
        let dirty = self
            .tabs
            .get(i)
            .map(|t| self.dirty.contains(&t.path))
            .unwrap_or(false);
        if dirty {
            self.confirm_close = Some(i);
        } else {
            self.close_tab(i);
        }
        cx.notify();
    }

    // ── open-tab context menu ──────────────────────────────────────────────────
    pub(crate) fn open_tab_menu(&mut self, i: usize, pos: Point<Pixels>, cx: &mut Context<Self>) {
        self.tab_menu = Some((i, pos));
        cx.notify();
    }
    pub(crate) fn close_tab_menu(&mut self, cx: &mut Context<Self>) {
        if self.tab_menu.take().is_some() {
            cx.notify();
        }
    }
    /// Close every tab whose index satisfies `target`, skipping ones with unsaved
    /// edits (they're kept open so bulk-close never silently discards work).
    pub(crate) fn close_tabs_matching(
        &mut self,
        target: impl Fn(usize) -> bool,
        cx: &mut Context<Self>,
    ) {
        self.tab_menu = None;
        let mut kept = 0;
        let mut i = self.tabs.len();
        while i > 0 {
            i -= 1;
            if target(i) {
                if self.dirty.contains(&self.tabs[i].path) {
                    kept += 1;
                } else {
                    self.close_tab(i);
                }
            }
        }
        self.status = if kept > 0 {
            format!("Kept {kept} unsaved tab(s) open")
        } else {
            "Closed tabs".into()
        };
        cx.notify();
    }
    /// Copy the tab's file path to the clipboard.
    pub(crate) fn copy_tab_path(&mut self, i: usize, cx: &mut Context<Self>) {
        self.tab_menu = None;
        if let Some(t) = self.tabs.get(i) {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                t.path.to_string_lossy().into_owned(),
            ));
            self.status = "Copied path to clipboard".into();
        }
        cx.notify();
    }
    /// "Close All": close everything. If any tab has unsaved edits, confirm once
    /// before discarding them (unlike "Close Saved", which keeps them).
    pub(crate) fn request_close_all(&mut self, cx: &mut Context<Self>) {
        self.tab_menu = None;
        let any_dirty = self.tabs.iter().any(|t| self.dirty.contains(&t.path));
        if any_dirty {
            self.confirm_close_all = true;
        } else {
            self.force_close_all(cx);
        }
        cx.notify();
    }
    /// Close every tab unconditionally (discarding unsaved edits).
    pub(crate) fn force_close_all(&mut self, cx: &mut Context<Self>) {
        for t in &self.tabs {
            self.dirty.remove(&t.path);
        }
        self.tabs.clear();
        self.active = None;
        self.confirm_close_all = false;
        self.status = "Closed all tabs".into();
        cx.notify();
    }

    /// The open-tab right-click menu (Close / Close Others / … / Copy Path).
    pub(crate) fn tab_menu_overlay(&self, cx: &mut Context<Self>) -> Div {
        let Some((i, pos)) = self.tab_menu else {
            return div();
        };
        let item = |label: &str| {
            div()
                .px_3()
                .py_1()
                .text_size(px(13.))
                .text_color(theme::text())
                .hover(|s| s.bg(theme::surface0()))
                .child(label.to_string())
        };
        let card = div()
            .id("tab-menu")
            .absolute()
            .left(pos.x)
            .top(pos.y)
            .occlude()
            .flex()
            .flex_col()
            .py_1()
            .w(px(180.))
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .child(item("Close").on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                    this.tab_menu = None;
                    this.request_close_tab(i, cx);
                }),
            ))
            .child(item("Close Others").on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                    // Keep the right-clicked tab active across the closes.
                    this.active = Some(i);
                    this.close_tabs_matching(move |j| j != i, cx)
                }),
            ))
            .child(item("Close to the Right").on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                    this.close_tabs_matching(move |j| j > i, cx)
                }),
            ))
            .child(item("Close Saved").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                    this.close_tabs_matching(|_| true, cx)
                }),
            ))
            .child(item("Close All").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.request_close_all(cx)),
            ))
            .child(item("Copy Path").on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| this.copy_tab_path(i, cx)),
            ));
        div()
            .absolute()
            .inset_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseDownEvent, _w, cx| this.close_tab_menu(cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _e: &MouseDownEvent, _w, cx| this.close_tab_menu(cx)),
            )
            .child(card)
    }

    /// The unsaved-changes confirmation modal for closing a dirty tab.
    pub(crate) fn close_confirm_overlay(&self, cx: &mut Context<Self>) -> Div {
        let Some(i) = self.confirm_close else {
            return div();
        };
        let name = self
            .tabs
            .get(i)
            .map(|t| t.title())
            .unwrap_or_else(|| "this tab".into());
        let card = div()
            .w(px(440.))
            .p_4()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .occlude()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(px(15.))
                    .text_color(theme::text())
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Unsaved Changes"),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::subtext())
                    .child(format!("{name} has unsaved edits.")),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(ghost_btn("Cancel").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                            this.confirm_close = None;
                            cx.notify();
                        }),
                    ))
                    .child(ghost_btn("Discard").text_color(theme::red()).on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                            this.close_tab(i);
                            this.confirm_close = None;
                            cx.notify();
                        }),
                    ))
                    .child(solid_btn("Save & Close").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                            this.active = Some(i);
                            this.save(cx);
                            this.close_tab(i);
                            this.confirm_close = None;
                            cx.notify();
                        }),
                    )),
            );
        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x00000066))
            .flex()
            .items_center()
            .justify_center()
            .child(card)
    }

    /// The "Close All" confirmation modal (shown only when tabs are unsaved).
    pub(crate) fn close_all_overlay(&self, cx: &mut Context<Self>) -> Div {
        let n = self
            .tabs
            .iter()
            .filter(|t| self.dirty.contains(&t.path))
            .count();
        let card = div()
            .w(px(440.))
            .p_4()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .occlude()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(px(15.))
                    .text_color(theme::text())
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Close All Tabs"),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::subtext())
                    .child(format!(
                        "{n} tab(s) have unsaved edits. Close all and discard them?"
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(ghost_btn("Cancel").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                            this.confirm_close_all = false;
                            cx.notify();
                        }),
                    ))
                    .child(
                        ghost_btn("Close All & Discard")
                            .text_color(theme::red())
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                                    this.force_close_all(cx)
                                }),
                            ),
                    ),
            );
        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x00000066))
            .flex()
            .items_center()
            .justify_center()
            .child(card)
    }
}
