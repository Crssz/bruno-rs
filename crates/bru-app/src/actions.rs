//! App key actions, command palette, and overlay dismissal.

use crate::*;
use gpui::prelude::*;

impl BruApp {
    pub(crate) fn on_save_action(&mut self, _: &SaveTab, _w: &mut Window, cx: &mut Context<Self>) {
        self.save(cx);
        cx.notify();
    }
    pub(crate) fn on_send_action(&mut self, _: &SendReq, _w: &mut Window, cx: &mut Context<Self>) {
        self.send(cx);
        cx.notify();
    }
    pub(crate) fn on_escape_action(
        &mut self,
        _: &CloseOverlay,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // An open in-editor find bar closes first, refocusing its editor so typing
        // resumes in the buffer (the query box had focus while the bar was open).
        if let Some(ed) = self.find_editor.take() {
            ed.update(cx, |e, cx| e.close_find(cx));
            let h = ed.read(cx).focus_handle(cx);
            window.focus(&h, cx);
            cx.notify();
            return;
        }
        self.close_topmost_overlay(cx);
    }
    pub(crate) fn on_palette_action(
        &mut self,
        _: &OpenPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.palette_open = true;
        let h = self.palette_input.read(cx).focus_handle(cx);
        window.focus(&h, cx);
        cx.notify();
    }

    /// The Ctrl+K jump-to-request command palette.
    pub(crate) fn palette_overlay(&self, cx: &mut Context<Self>) -> Div {
        let q = self.palette_query.to_lowercase();
        let mut items: Vec<(String, PathBuf)> = Vec::new();
        if let Some(tree) = &self.collection {
            flatten_requests(&tree.root, &mut items);
        }
        let filtered: Vec<(String, PathBuf)> = items
            .into_iter()
            .filter(|(n, p)| {
                q.is_empty()
                    || n.to_lowercase().contains(&q)
                    || p.to_string_lossy().to_lowercase().contains(&q)
            })
            .take(60)
            .collect();
        let mut list = div().flex().flex_col().gap_1();
        for (name, path) in filtered {
            let hint = path
                .parent()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let p = path.clone();
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .hover(|s| s.bg(theme::surface0()))
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(theme::text())
                            .child(name),
                    )
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .child(hint),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                            this.open_request(p.clone(), cx);
                            this.palette_open = false;
                            cx.notify();
                        }),
                    ),
            );
        }
        let card = div()
            .w(px(520.))
            .max_h(px(440.))
            .p_3()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .occlude()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .w_full()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(theme::input_bg())
                    .border_1()
                    .border_color(theme::border1())
                    .text_size(px(13.))
                    .child(self.palette_input.clone()),
            )
            .child(
                div()
                    .id("palette-list")
                    .overflow_y_scroll()
                    .min_h_0()
                    .flex_1()
                    .child(list),
            );
        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x00000066))
            .flex()
            .flex_col()
            .items_center()
            .pt(px(80.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseDownEvent, _w, cx| {
                    this.palette_open = false;
                    cx.notify();
                }),
            )
            .child(card)
    }

    /// Esc closes the topmost open overlay/menu (in priority order).
    pub(crate) fn close_topmost_overlay(&mut self, cx: &mut Context<Self>) {
        if self.palette_open {
            self.palette_open = false;
        } else if self.var_popup.take().is_some()
            || self.editor_menu.take().is_some()
            || self.confirm_close.take().is_some()
            || std::mem::take(&mut self.confirm_close_all)
            || self.confirm_delete.take().is_some()
            || self.rename.take().is_some()
            || self.ctx_menu.take().is_some()
            || self.tab_menu.take().is_some()
            || self.env_menu.take().is_some()
            || self.method_menu.take().is_some()
            || self.mode_menu.take().is_some()
        {
            // one of the lightweight popovers was closed
        } else if self.curl_open {
            self.curl_open = false;
        } else if self.git_open {
            self.git_open = false;
            self.git_confirm_discard = false;
        } else if self.vault_open {
            self.vault_open = false;
        } else if self.prefs_open {
            self.prefs_open = false;
        } else if self.cookies_open {
            self.cookies_open = false;
        } else if self.devtools_open {
            self.devtools_open = false;
        } else if self.runner_open {
            self.runner_open = false;
        } else if self.env.is_some() {
            self.env = None;
        } else {
            return;
        }
        cx.notify();
    }
}
