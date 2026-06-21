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

#[cfg(test)]
mod cov_tests {
    use super::*;
    use crate::test_support::app_on_temp;

    /// Open three top-level sample requests into tabs; returns their paths in the
    /// order they were opened (which is also their tab index order).
    fn open_three(
        app: &Entity<BruApp>,
        dir: &std::path::Path,
        cx: &mut gpui::TestAppContext,
    ) -> Vec<PathBuf> {
        let paths = vec![
            dir.join("Repository Info.bru"),
            dir.join("New Request.bru"),
            dir.join("New Request 2.bru"),
        ];
        for p in &paths {
            let p = p.clone();
            app.update(cx, |app, cx| app.open_request(p, cx));
        }
        paths
    }

    #[gpui::test]
    fn active_tab_tracks_open_request(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let paths = open_three(&app, &tc.dir, cx);
        app.update(cx, |app, _| {
            assert_eq!(app.tabs.len(), 3);
            // Last-opened tab is active and is what `active_tab` returns.
            assert_eq!(app.active, Some(2));
            let got = app.active_tab().map(|t| t.path.clone());
            assert_eq!(got, Some(paths[2].clone()));
        });
    }

    #[gpui::test]
    fn active_tab_none_when_empty(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, _| {
            assert!(app.active_tab().is_none());
            assert!(app.tabs.is_empty());
            assert!(app.active.is_none());
        });
    }

    #[gpui::test]
    fn close_tab_out_of_bounds_is_noop(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        open_three(&app, &tc.dir, cx);
        app.update(cx, |app, _| {
            app.close_tab(99);
            assert_eq!(app.tabs.len(), 3);
            assert_eq!(app.active, Some(2));
        });
    }

    #[gpui::test]
    fn close_tab_after_active_keeps_active(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        open_three(&app, &tc.dir, cx);
        app.update(cx, |app, _| {
            // Active is index 2; close index 0 (before active) -> active shifts down.
            app.close_tab(0);
            assert_eq!(app.tabs.len(), 2);
            assert_eq!(app.active, Some(1));
        });
    }

    #[gpui::test]
    fn close_active_tab_clamps_index(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        open_three(&app, &tc.dir, cx);
        app.update(cx, |app, _| {
            // Active = 2 (last). Closing it clamps active to the new last (1).
            app.close_tab(2);
            assert_eq!(app.tabs.len(), 2);
            assert_eq!(app.active, Some(1));
            // Close the middle one (which is now also active=1).
            app.close_tab(1);
            assert_eq!(app.tabs.len(), 1);
            assert_eq!(app.active, Some(0));
            // Close the last remaining -> no active.
            app.close_tab(0);
            assert!(app.tabs.is_empty());
            assert!(app.active.is_none());
        });
    }

    #[gpui::test]
    fn close_tab_before_active_shifts_active_down(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        open_three(&app, &tc.dir, cx);
        app.update(cx, |app, _| {
            // Make the middle tab active, then close index 0.
            app.active = Some(1);
            app.close_tab(0);
            assert_eq!(app.active, Some(0));
        });
    }

    #[gpui::test]
    fn request_close_clean_tab_closes_immediately(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        open_three(&app, &tc.dir, cx);
        app.update(cx, |app, cx| {
            app.request_close_tab(1, cx);
            assert_eq!(app.tabs.len(), 2);
            // No confirmation modal for a clean tab.
            assert!(app.confirm_close.is_none());
        });
    }

    #[gpui::test]
    fn request_close_dirty_tab_prompts(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let paths = open_three(&app, &tc.dir, cx);
        app.update(cx, |app, cx| {
            // Mark tab 1 dirty, then request close -> confirmation, not removal.
            app.dirty.insert(paths[1].clone());
            app.request_close_tab(1, cx);
            assert_eq!(app.tabs.len(), 3);
            assert_eq!(app.confirm_close, Some(1));
        });
    }

    #[gpui::test]
    fn tab_menu_open_and_close(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        open_three(&app, &tc.dir, cx);
        app.update(cx, |app, cx| {
            app.open_tab_menu(1, gpui::point(px(10.), px(20.)), cx);
            assert!(app.tab_menu.is_some());
            assert_eq!(app.tab_menu.map(|(i, _)| i), Some(1));
            app.close_tab_menu(cx);
            assert!(app.tab_menu.is_none());
            // Second close is a no-op (take() returns None).
            app.close_tab_menu(cx);
            assert!(app.tab_menu.is_none());
        });
    }

    #[gpui::test]
    fn close_others_keeps_only_target(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let paths = open_three(&app, &tc.dir, cx);
        app.update(cx, |app, cx| {
            // Emulate the "Close Others" menu action around index 0.
            app.active = Some(0);
            app.close_tabs_matching(move |j| j != 0, cx);
            assert_eq!(app.tabs.len(), 1);
            assert_eq!(app.tabs[0].path, paths[0]);
            // The context menu is cleared by the bulk close.
            assert!(app.tab_menu.is_none());
            assert_eq!(app.status, "Closed tabs");
        });
    }

    #[gpui::test]
    fn close_to_the_right_drops_higher_indices(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let paths = open_three(&app, &tc.dir, cx);
        app.update(cx, |app, cx| {
            app.close_tabs_matching(move |j| j > 0, cx);
            assert_eq!(app.tabs.len(), 1);
            assert_eq!(app.tabs[0].path, paths[0]);
        });
    }

    #[gpui::test]
    fn close_saved_keeps_dirty_tabs(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let paths = open_three(&app, &tc.dir, cx);
        app.update(cx, |app, cx| {
            // Mark one tab dirty; "Close Saved" (target = all) keeps it open.
            app.dirty.insert(paths[1].clone());
            app.close_tabs_matching(|_| true, cx);
            assert_eq!(app.tabs.len(), 1);
            assert_eq!(app.tabs[0].path, paths[1]);
            assert!(app.status.contains("Kept 1 unsaved"));
        });
    }

    #[gpui::test]
    fn copy_tab_path_sets_status_and_closes_menu(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        open_three(&app, &tc.dir, cx);
        app.update(cx, |app, cx| {
            app.open_tab_menu(2, gpui::point(px(1.), px(2.)), cx);
            app.copy_tab_path(2, cx);
            assert!(app.tab_menu.is_none());
            assert_eq!(app.status, "Copied path to clipboard");
        });
    }

    #[gpui::test]
    fn copy_tab_path_out_of_bounds_no_status(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        open_three(&app, &tc.dir, cx);
        app.update(cx, |app, cx| {
            app.status = "unchanged".into();
            app.copy_tab_path(99, cx);
            // No tab at 99 -> status is left as-is (only the menu is cleared).
            assert_eq!(app.status, "unchanged");
        });
    }

    #[gpui::test]
    fn request_close_all_clean_closes_everything(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        open_three(&app, &tc.dir, cx);
        app.update(cx, |app, cx| {
            app.request_close_all(cx);
            assert!(app.tabs.is_empty());
            assert!(app.active.is_none());
            assert!(!app.confirm_close_all);
            assert_eq!(app.status, "Closed all tabs");
        });
    }

    #[gpui::test]
    fn request_close_all_dirty_prompts(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let paths = open_three(&app, &tc.dir, cx);
        app.update(cx, |app, cx| {
            app.dirty.insert(paths[0].clone());
            app.request_close_all(cx);
            // A dirty tab forces a confirmation rather than closing.
            assert!(app.confirm_close_all);
            assert_eq!(app.tabs.len(), 3);
        });
    }

    #[gpui::test]
    fn force_close_all_clears_dirty(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let paths = open_three(&app, &tc.dir, cx);
        app.update(cx, |app, cx| {
            app.dirty.insert(paths[0].clone());
            app.dirty.insert(paths[2].clone());
            app.confirm_close_all = true;
            app.force_close_all(cx);
            assert!(app.tabs.is_empty());
            assert!(app.active.is_none());
            assert!(!app.confirm_close_all);
            // Dirty entries for the closed tabs are cleared.
            assert!(!app.dirty.contains(&paths[0]));
            assert!(!app.dirty.contains(&paths[2]));
            assert_eq!(app.status, "Closed all tabs");
        });
    }

    #[gpui::test]
    fn overlay_builders_run_without_panicking(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let paths = open_three(&app, &tc.dir, cx);
        app.update(cx, |app, cx| {
            // tab_menu_overlay: empty div when no menu, populated card when open.
            let _empty = app.tab_menu_overlay(cx);
            app.open_tab_menu(0, gpui::point(px(5.), px(6.)), cx);
            let _menu = app.tab_menu_overlay(cx);

            // close_confirm_overlay: builds the modal when a dirty tab is pending.
            app.dirty.insert(paths[1].clone());
            app.confirm_close = Some(1);
            let _confirm = app.close_confirm_overlay(cx);

            // close_all_overlay always builds (counts dirty tabs).
            app.confirm_close_all = true;
            let _all = app.close_all_overlay(cx);
        });
    }
}
