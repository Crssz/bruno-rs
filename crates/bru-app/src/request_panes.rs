//! Request sub-tabs: params/headers grid, auth form, vars tables.

use crate::*;
use gpui::prelude::*;

impl BruApp {
    /// The clickable request sub-tab strip.
    pub(crate) fn req_subtabs(&self, tab: &OpenTab, cx: &mut Context<Self>) -> gpui::Stateful<Div> {
        let mut strip = div()
            .id("req-subtabs")
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .overflow_x_scroll()
            .px_2()
            .bg(theme::bg())
            .border_b_1()
            .border_color(theme::border1());
        for t in ReqTab::ALL {
            let active = tab.req_tab == t;
            strip = strip.child(tab_chip(t.label(), active).on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _ev: &MouseUpEvent, _w, cx| {
                    if let Some(i) = this.active {
                        this.tabs[i].switch_tab(t, cx);
                    }
                    cx.notify();
                }),
            ));
        }
        // A mode-cycle chip pinned right when the Body/Auth tab is active.
        if matches!(tab.req_tab, ReqTab::Body | ReqTab::Auth) {
            let is_body = tab.req_tab == ReqTab::Body;
            let (field, prefix) = if is_body {
                ("body", "Body")
            } else {
                ("auth", "Auth")
            };
            let cur = edit::method_field(&tab.file, field).unwrap_or_else(|| "none".into());
            strip =
                strip
                    .child(div().flex_1())
                    .child(chip(&format!("{prefix}: {cur}")).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseDownEvent, _w, cx| {
                            this.open_mode_menu(ev.position, is_body, cx);
                        }),
                    ));
        }
        strip
    }

    /// The content for the active request sub-tab (the shared editor).
    /// Full-pane editor for a plain-text file tab (a `require`d module opened via
    /// Ctrl+click). A path header sits above the editable, scrollable editor.
    pub(crate) fn text_pane(&self, tab: &OpenTab, _cx: &mut Context<Self>) -> Div {
        let Some(editor) = tab.text.clone() else {
            return div();
        };
        let rel = tab
            .path
            .strip_prefix(&self.dir)
            .unwrap_or(&tab.path)
            .to_string_lossy()
            .into_owned();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .w_full()
            .bg(theme::bg())
            .child(
                div()
                    .px_3()
                    .py_2()
                    .w_full()
                    .border_b_1()
                    .border_color(theme::border1())
                    .font_family("monospace")
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child(rel),
            )
            .child(
                div()
                    .id("text-file")
                    .overflow_y_scroll()
                    .track_scroll(&tab.text_scroll)
                    .min_h_0()
                    .flex_1()
                    .w_full()
                    .p_3()
                    .font_family("monospace")
                    .text_size(px(13.))
                    .line_height(px(19.))
                    .child(editor),
            )
    }

    pub(crate) fn req_content(&self, tab: &OpenTab, cx: &mut Context<Self>) -> Div {
        if matches!(tab.edit_kind, EditKind::Kv(_)) {
            return self.kv_grid(tab, cx);
        }
        if matches!(tab.edit_kind, EditKind::Vars) {
            return self.vars_pane(tab, cx);
        }
        if matches!(tab.edit_kind, EditKind::AuthForm(_)) {
            return self.auth_form(tab, cx);
        }
        if matches!(tab.edit_kind, EditKind::GraphQl) {
            let pane = |label: &str, id: &'static str, ed: Entity<CodeEditor>| {
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .child(
                        div()
                            .px_3()
                            .pt_2()
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .child(label.to_string()),
                    )
                    .child(
                        div()
                            .id(id)
                            .overflow_y_scroll()
                            .min_h_0()
                            .flex_1()
                            .w_full()
                            .p_3()
                            .font_family("monospace")
                            .text_size(px(13.))
                            .line_height(px(19.))
                            .child(ed),
                    )
            };
            return div()
                .flex()
                .flex_col()
                .flex_1()
                .w_full()
                .bg(theme::bg())
                .child(pane("QUERY", "gql-query", tab.body_editor.clone()))
                .child(pane("VARIABLES", "gql-vars", tab.body_vars_editor.clone()));
        }
        let body_box = div()
            .id("body")
            .overflow_y_scroll()
            .track_scroll(&tab.body_scroll)
            .min_h_0()
            .flex_1()
            .w_full()
            .p_3()
            .font_family("monospace")
            .text_size(px(13.))
            .line_height(px(19.))
            .child(tab.body_editor.clone());
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .bg(theme::bg())
            // The single "Script" tab carries an inner Pre Request / Post Response
            // sub-tab strip (Bruno's layout); other body tabs render just the editor.
            .when(tab.req_tab == ReqTab::Script, |d| {
                d.child(self.script_subtabs(tab, cx))
            })
            .child(body_box)
    }

    /// The inner Pre Request / Post Response strip shown inside the Script tab.
    fn script_subtabs(&self, tab: &OpenTab, cx: &mut Context<Self>) -> Div {
        let chip = |label: &'static str, active: bool, post: bool, cx: &mut Context<Self>| {
            tab_chip(label, active).on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                    if let Some(i) = this.active {
                        this.tabs[i].switch_script_tab(post, cx);
                    }
                    cx.notify();
                }),
            )
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .px_2()
            .bg(theme::bg())
            .border_b_1()
            .border_color(theme::border1())
            .child(chip("Pre Request", !tab.script_post, false, cx))
            .child(chip("Post Response", tab.script_post, true, cx))
    }

    /// The structured params/headers grid (enable toggle + name + value + ÃƒÂ¢Ã…â€œÃ¢â‚¬Â¢).
    /// Structured Auth form: a labeled single-line input per field of the mode.
    pub(crate) fn auth_form(&self, tab: &OpenTab, _cx: &mut Context<Self>) -> Div {
        let mut col = div().flex().flex_col().gap_3().w_full();
        for r in &tab.auth_rows {
            col = col.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .w(px(420.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .child(r.label.clone()),
                    )
                    .child(
                        div()
                            .w_full()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(theme::input_bg())
                            .border_1()
                            .border_color(theme::border1())
                            .font_family("monospace")
                            .text_size(px(12.))
                            .child(r.editor.clone()),
                    ),
            );
        }
        if tab.auth_rows.is_empty() {
            col = col.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child("No fields for this auth mode."),
            );
        }
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .bg(theme::bg())
            .child(
                div()
                    .id("auth-form")
                    .overflow_y_scroll()
                    .min_h_0()
                    .flex_1()
                    .w_full()
                    .p_3()
                    .child(col),
            )
    }

    pub(crate) fn kv_grid(&self, tab: &OpenTab, cx: &mut Context<Self>) -> Div {
        // Cells are borderless and sit inside one bordered table (Bruno's grid);
        // `divider` draws the vertical gridline on the right of the name column.
        let cell = |child: Entity<CodeEditor>, w: Option<Pixels>, divider: bool| {
            let d = div()
                .px_2()
                .py_1()
                .font_family("monospace")
                .text_size(px(12.))
                .when(divider, |x| x.border_r_1().border_color(theme::border0()))
                .child(child);
            match w {
                Some(w) => d.w(w),
                None => d.flex_1(),
            }
        };
        let block = match &tab.edit_kind {
            EditKind::Kv(b) => b.as_str(),
            _ => "",
        };
        let (col1, col2) = if block == "assert" {
            (
                "Expression  (e.g. res.status)",
                "Operator + Value  (e.g. eq 200)",
            )
        } else {
            ("Name", "Value")
        };
        // One bordered table: a header row, then a gridlined row per entry.
        let mut grid = div()
            .flex()
            .flex_col()
            .border_1()
            .border_color(theme::border0())
            .rounded_md()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .bg(theme::mantle())
                    .border_b_1()
                    .border_color(theme::border0())
                    .text_size(px(10.))
                    .text_color(theme::muted())
                    .child(
                        div()
                            .w(px(234.))
                            .px_2()
                            .py_1()
                            .border_r_1()
                            .border_color(theme::border0())
                            .child(col1),
                    )
                    .child(div().flex_1().px_2().py_1().child(col2)),
            );
        let kv_len = tab.kv_rows.len();
        for (idx, row) in tab.kv_rows.iter().enumerate() {
            let at_top = idx == 0;
            let at_bottom = idx + 1 >= kv_len;
            grid = grid.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .border_b_1()
                    .border_color(theme::border0())
                    .hover(|s| s.bg(theme::mantle()))
                    .child(div().w(px(22.)).flex().justify_center().child(
                        check_box(row.enabled).on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                this.kv_toggle_row(idx, cx)
                            }),
                        ),
                    ))
                    .child(cell(row.name.clone(), Some(px(212.)), true))
                    .child(cell(row.value.clone(), None, false))
                    // Reorder arrows: dimmed + inert at the list bounds.
                    .child({
                        let up = div()
                            .px_1()
                            .text_size(px(11.))
                            .text_color(if at_top {
                                theme::border2()
                            } else {
                                theme::muted()
                            })
                            .child("\u{2191}");
                        if at_top {
                            up
                        } else {
                            up.on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                    this.kv_move_row(idx, true, cx)
                                }),
                            )
                        }
                    })
                    .child({
                        let down = div()
                            .px_1()
                            .text_size(px(11.))
                            .text_color(if at_bottom {
                                theme::border2()
                            } else {
                                theme::muted()
                            })
                            .child("\u{2193}");
                        if at_bottom {
                            down
                        } else {
                            down.on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                    this.kv_move_row(idx, false, cx)
                                }),
                            )
                        }
                    })
                    .child(
                        div()
                            .px_2()
                            .text_color(theme::muted())
                            .child("\u{2715}")
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                    this.kv_remove_row(idx, cx)
                                }),
                            ),
                    ),
            );
        }
        let table = div().flex().flex_col().gap_2().child(grid).child(
            div()
                .text_size(px(12.))
                .text_color(theme::accent())
                .child("+ Add")
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.kv_add_row(cx)),
                ),
        );
        let mut inner = div().flex().flex_col().gap_3().child(table);
        // URL-derived path params (Params tab only): read-only name label +
        // editable, dirty-tracked value.
        if !tab.path_rows.is_empty() {
            let mut pt = div().flex().flex_col().gap_1().child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::muted())
                    .child("PATH PARAMS"),
            );
            for (name, ed) in &tab.path_rows {
                pt = pt.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .w(px(220.))
                                .font_family("monospace")
                                .text_size(px(12.))
                                .text_color(theme::accent())
                                .child(format!(":{name}")),
                        )
                        .child(cell(ed.clone(), None, false)),
                );
            }
            inner = inner.child(pt);
        }
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .bg(theme::bg())
            .child(
                div()
                    .id("kv-grid")
                    .overflow_y_scroll()
                    .min_h_0()
                    .flex_1()
                    .w_full()
                    .p_3()
                    .child(inner),
            )
    }

    /// The Vars tab: two stacked structured tables (Pre Request + Post Response),
    /// matching Bruno's single-page layout.
    pub(crate) fn vars_pane(&self, tab: &OpenTab, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .bg(theme::bg())
            .child(
                div()
                    .id("vars-pane")
                    .overflow_y_scroll()
                    .min_h_0()
                    .flex_1()
                    .w_full()
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(self.var_table(tab, false, cx))
                    .child(self.var_table(tab, true, cx)),
            )
    }

    /// One Vars table â€” pre-request (`post == false`) or post-response. The
    /// post-response value column reads "Expr" (values are JS expressions). The
    /// `@local` flag is preserved per row but not shown (matching Bruno).
    pub(crate) fn var_table(&self, tab: &OpenTab, post: bool, cx: &mut Context<Self>) -> Div {
        let rows = if post {
            &tab.var_post_rows
        } else {
            &tab.var_pre_rows
        };
        let (title, value_col, hint) = if post {
            (
                "Post Response",
                "Expr",
                Some("JS expressions, evaluated against the response."),
            )
        } else {
            ("Pre Request", "Value", None)
        };
        let cell = |child: Entity<CodeEditor>, w: Option<Pixels>, divider: bool| {
            let d = div()
                .px_2()
                .py_1()
                .font_family("monospace")
                .text_size(px(12.))
                .when(divider, |x| x.border_r_1().border_color(theme::border0()))
                .child(child);
            match w {
                Some(w) => d.w(w),
                None => d.flex_1(),
            }
        };
        // One bordered table: a header row, then a gridlined row per entry.
        let mut grid = div()
            .flex()
            .flex_col()
            .border_1()
            .border_color(theme::border0())
            .rounded_md()
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .bg(theme::mantle())
                    .border_b_1()
                    .border_color(theme::border0())
                    .text_size(px(10.))
                    .text_color(theme::muted())
                    .child(
                        div()
                            .w(px(234.))
                            .px_2()
                            .py_1()
                            .border_r_1()
                            .border_color(theme::border0())
                            .child("Name"),
                    )
                    .child(div().flex_1().px_2().py_1().child(value_col)),
            );
        let len = rows.len();
        let mut any_invalid = false;
        for (idx, row) in rows.iter().enumerate() {
            // Live name validation (Bruno's variableNameRegex). Validate the
            // TRIMMED name â€” that's what gets persisted â€” so a stray trailing
            // space isn't flagged as an error it would actually save fine.
            let name = row.name.read(cx).text();
            let trimmed = name.trim();
            let name_invalid = !trimmed.is_empty() && !valid_var_name(trimmed);
            any_invalid |= name_invalid;
            let at_top = idx == 0;
            let at_bottom = idx + 1 >= len;
            // Signal invalid with a red background tint, not a thicker border, so
            // the cell's box size (and the row height) stays constant.
            let name_cell = div()
                .w(px(212.))
                .px_2()
                .py_1()
                .font_family("monospace")
                .text_size(px(12.))
                .border_r_1()
                .border_color(theme::border0())
                .when(name_invalid, |d| d.bg(gpui::rgba(0xe0655233)))
                .child(row.name.clone());
            grid = grid.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .border_b_1()
                    .border_color(theme::border0())
                    .hover(|s| s.bg(theme::mantle()))
                    .child(div().w(px(22.)).flex().justify_center().child(
                        check_box(row.enabled).on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                this.var_toggle_row(post, idx, cx)
                            }),
                        ),
                    ))
                    .child(name_cell)
                    .child(cell(row.value.clone(), None, false))
                    // Reorder arrows: dimmed + inert at the list bounds.
                    .child({
                        let up = div()
                            .px_1()
                            .text_size(px(11.))
                            .text_color(if at_top {
                                theme::border2()
                            } else {
                                theme::muted()
                            })
                            .child("\u{2191}");
                        if at_top {
                            up
                        } else {
                            up.on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                    this.var_move_row(post, idx, true, cx)
                                }),
                            )
                        }
                    })
                    .child({
                        let down = div()
                            .px_1()
                            .text_size(px(11.))
                            .text_color(if at_bottom {
                                theme::border2()
                            } else {
                                theme::muted()
                            })
                            .child("\u{2193}");
                        if at_bottom {
                            down
                        } else {
                            down.on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                    this.var_move_row(post, idx, false, cx)
                                }),
                            )
                        }
                    })
                    .child(
                        div()
                            .px_2()
                            .text_color(theme::muted())
                            .child("\u{2715}")
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                    this.var_remove_row(post, idx, cx)
                                }),
                            ),
                    ),
            );
        }
        let mut col = div().flex().flex_col().gap_2().w_full().child(
            div()
                .text_size(px(11.))
                .text_color(theme::muted())
                .child(title),
        );
        if let Some(h) = hint {
            col = col.child(div().text_size(px(10.)).text_color(theme::muted()).child(h));
        }
        col = col.child(grid).child(
            div()
                .text_size(px(12.))
                .text_color(theme::accent())
                .child("+ Add")
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, _e: &MouseUpEvent, _w, cx| this.var_add_row(post, cx)),
                ),
        );
        if any_invalid {
            col = col.child(
                div()
                    .text_size(px(10.))
                    .text_color(theme::red())
                    .child("Variable names may only contain letters, numbers, and - _ ."),
            );
        }
        col
    }
}
