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
    pub(crate) fn text_pane(&self, tab: &OpenTab, cx: &mut Context<Self>) -> Div {
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
            .children(CodeEditor::find_bar(&editor, cx))
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
            // Find bars are computed up front (each borrows `cx`), then handed to
            // the panes so the closure needn't capture `cx`.
            let q_bar = CodeEditor::find_bar(&tab.body_editor, cx);
            let v_bar = CodeEditor::find_bar(&tab.body_vars_editor, cx);
            let pane = |label: &str,
                        id: &'static str,
                        ed: Entity<CodeEditor>,
                        bar: Option<gpui::AnyElement>| {
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
                    .children(bar)
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
                .child(pane("QUERY", "gql-query", tab.body_editor.clone(), q_bar))
                .child(pane(
                    "VARIABLES",
                    "gql-vars",
                    tab.body_vars_editor.clone(),
                    v_bar,
                ));
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
            // Ctrl+F / Ctrl+H find/replace bar, above the scrolling editor.
            .children(CodeEditor::find_bar(&tab.body_editor, cx))
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

#[cfg(test)]
mod cov_tests {
    use super::*;
    use crate::test_support::temp_collection;
    use gpui::TestAppContext;

    /// Open a windowed app on a throwaway sample collection, with one request
    /// already opened and rendered. Returns the window + the live TempCollection
    /// (which must be kept alive until the test ends).
    fn windowed_with_request(
        cx: &mut TestAppContext,
        file: &str,
    ) -> (
        gpui::WindowHandle<BruApp>,
        crate::test_support::TempCollection,
    ) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        let req = tc.dir.join(file);
        window
            .update(cx, |app, _w, cx| app.open_request(req, cx))
            .unwrap();
        cx.run_until_parked();
        (window, tc)
    }

    /// Switching through every ReqTab (re-parking each) renders `req_subtabs` +
    /// `req_content` for all of Params/Body/Headers/Auth/Assert/Vars/Script/
    /// Tests/Docs/Source on a request that carries a JSON body and bearer auth.
    #[gpui::test]
    fn renders_every_req_tab(cx: &mut TestAppContext) {
        let (window, _tc) = windowed_with_request(cx, "Repository/Create Issue.bru");
        for t in ReqTab::ALL {
            window
                .update(cx, |app, _w, cx| {
                    if let Some(i) = app.active {
                        app.tabs[i].switch_tab(t, cx);
                    }
                })
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |app, _w, _cx| {
                    assert_eq!(app.tabs[app.active.unwrap()].req_tab, t);
                })
                .unwrap();
        }
    }

    /// Cycling the body mode through every entry of BODY_MODES re-loads the active
    /// tab and re-renders: this hits the kv-grid body branch (form/multipart), the
    /// GraphQL two-editor branch, the plain Body-editor branch, and the `none`
    /// fall-through in `req_content`.
    #[gpui::test]
    fn body_mode_cycle_renders_all_content_shapes(cx: &mut TestAppContext) {
        let (window, _tc) = windowed_with_request(cx, "Repository/Create Issue.bru");
        // Ensure we're on the Body tab so req_content builds the body shape.
        window
            .update(cx, |app, _w, cx| {
                if let Some(i) = app.active {
                    app.tabs[i].switch_tab(ReqTab::Body, cx);
                }
            })
            .unwrap();
        cx.run_until_parked();
        for mode in BODY_MODES {
            window
                .update(cx, |app, _w, cx| app.set_body_mode(mode, cx))
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |app, _w, _cx| {
                    let i = app.active.unwrap();
                    assert_eq!(
                        edit::method_field(&app.tabs[i].file, "body").as_deref(),
                        Some(*mode)
                    );
                })
                .unwrap();
        }
    }

    /// Cycling the auth mode renders the structured `auth_form` for each mode that
    /// has fields (basic/bearer/apikey/oauth2/digest/awsv4) and the "No fields"
    /// empty form for none/inherit.
    #[gpui::test]
    fn auth_mode_cycle_renders_auth_form(cx: &mut TestAppContext) {
        let (window, _tc) = windowed_with_request(cx, "Repository/Create Issue.bru");
        window
            .update(cx, |app, _w, cx| {
                if let Some(i) = app.active {
                    app.tabs[i].switch_tab(ReqTab::Auth, cx);
                }
            })
            .unwrap();
        cx.run_until_parked();
        for mode in AUTH_MODES {
            window
                .update(cx, |app, _w, cx| app.set_auth_mode(mode, cx))
                .unwrap();
            cx.run_until_parked();
            window
                .update(cx, |app, _w, _cx| {
                    let i = app.active.unwrap();
                    assert_eq!(
                        edit::method_field(&app.tabs[i].file, "auth").as_deref(),
                        Some(*mode)
                    );
                })
                .unwrap();
        }
    }

    /// Opening the body and auth mode menus toggles the `mode_menu` state and
    /// re-renders the sub-tab strip's pinned mode chip.
    #[gpui::test]
    fn open_body_and_auth_mode_menus(cx: &mut TestAppContext) {
        let (window, _tc) = windowed_with_request(cx, "Repository/Create Issue.bru");
        // Body mode menu.
        window
            .update(cx, |app, _w, cx| {
                if let Some(i) = app.active {
                    app.tabs[i].switch_tab(ReqTab::Body, cx);
                }
                app.open_mode_menu(gpui::point(px(10.), px(10.)), true, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _w, _cx| assert!(app.mode_menu.is_some()))
            .unwrap();
        // Auth mode menu.
        window
            .update(cx, |app, _w, cx| {
                if let Some(i) = app.active {
                    app.tabs[i].switch_tab(ReqTab::Auth, cx);
                }
                app.open_mode_menu(gpui::point(px(20.), px(20.)), false, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _w, _cx| {
                assert!(matches!(app.mode_menu, Some((_, false))));
            })
            .unwrap();
    }

    /// A request with `params:query` rows renders the kv grid on the Params tab.
    /// Adding/toggling/moving/removing rows exercises the grid's row-bounds
    /// branches (top arrow inert on row 0, bottom arrow inert on last row).
    #[gpui::test]
    fn params_kv_grid_row_ops(cx: &mut TestAppContext) {
        let (window, _tc) = windowed_with_request(cx, "Search Repos copy.bru");
        window
            .update(cx, |app, _w, cx| {
                if let Some(i) = app.active {
                    app.tabs[i].switch_tab(ReqTab::Params, cx);
                }
            })
            .unwrap();
        cx.run_until_parked();
        // Start with the two sample params; add a third, then exercise moves.
        window
            .update(cx, |app, _w, cx| {
                let i = app.active.unwrap();
                let start = app.tabs[i].kv_rows.len();
                app.kv_add_row(cx);
                assert_eq!(app.tabs[i].kv_rows.len(), start + 1);
                app.kv_toggle_row(0, cx);
                app.kv_move_row(1, true, cx); // move row 1 up
                app.kv_move_row(0, true, cx); // inert (already at top)
                let last = app.tabs[i].kv_rows.len() - 1;
                app.kv_move_row(last, false, cx); // inert (already at bottom)
                app.kv_move_row(0, false, cx); // move row 0 down
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _w, cx| {
                let i = app.active.unwrap();
                let len = app.tabs[i].kv_rows.len();
                app.kv_remove_row(len - 1, cx);
                assert_eq!(app.tabs[i].kv_rows.len(), len - 1);
            })
            .unwrap();
        cx.run_until_parked();
    }

    /// The Assert tab uses the kv grid with the "Expression"/"Operator + Value"
    /// header labels (the `block == "assert"` branch of `kv_grid`).
    #[gpui::test]
    fn assert_tab_renders_kv_grid(cx: &mut TestAppContext) {
        let (window, _tc) = windowed_with_request(cx, "Repository/Create Issue.bru");
        window
            .update(cx, |app, _w, cx| {
                let i = app.active.unwrap();
                app.tabs[i].switch_tab(ReqTab::Assert, cx);
                assert!(matches!(app.tabs[i].edit_kind, EditKind::Kv(_)));
                // Add a row so the grid has at least one entry to render.
                app.kv_add_row(cx);
            })
            .unwrap();
        cx.run_until_parked();
    }

    /// The Vars tab renders the two stacked var tables. Adding rows with a valid
    /// and an invalid name exercises the live name-validation branch (the red tint
    /// + the trailing error line in `var_table`).
    #[gpui::test]
    fn vars_tables_validate_names(cx: &mut TestAppContext) {
        let (window, _tc) = windowed_with_request(cx, "Repository/Create Issue.bru");
        window
            .update(cx, |app, _w, cx| {
                let i = app.active.unwrap();
                app.tabs[i].switch_tab(ReqTab::Vars, cx);
                assert!(matches!(app.tabs[i].edit_kind, EditKind::Vars));
            })
            .unwrap();
        cx.run_until_parked();
        // Pre-request: one valid, one invalid name.
        window
            .update(cx, |app, _w, cx| {
                app.var_add_row(false, cx);
                app.var_add_row(false, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _w, cx| {
                let i = app.active.unwrap();
                app.tabs[i].var_pre_rows[0]
                    .name
                    .update(cx, |ed, cx| ed.set_line("valid_name", cx));
                app.tabs[i].var_pre_rows[1]
                    .name
                    .update(cx, |ed, cx| ed.set_line("bad name!", cx));
            })
            .unwrap();
        // Re-park so var_table re-renders with the now-invalid name (red tint).
        cx.run_until_parked();
        // Post-response table row ops + toggle/move/remove.
        window
            .update(cx, |app, _w, cx| {
                app.var_add_row(true, cx);
                app.var_add_row(true, cx);
                app.var_toggle_row(true, 0, cx);
                app.var_move_row(true, 1, true, cx);
                app.var_move_row(true, 0, true, cx); // inert at top
                app.var_move_row(true, 1, false, cx); // inert at bottom
                app.var_remove_row(true, 0, cx);
            })
            .unwrap();
        cx.run_until_parked();
    }

    /// Editing the URL to carry `:tokens` and re-loading the Params tab populates
    /// `path_rows`, so `kv_grid` renders the PATH PARAMS section.
    #[gpui::test]
    fn params_tab_renders_path_params(cx: &mut TestAppContext) {
        let (window, _tc) = windowed_with_request(cx, "Repository Info.bru");
        window
            .update(cx, |app, _w, cx| {
                let i = app.active.unwrap();
                // Put a path token into the URL editor.
                app.tabs[i].url_input.update(cx, |ed, cx| {
                    ed.set_line("https://api.example.com/users/:id", cx)
                });
                // switch_tab applies edits (syncing path params) then reloads.
                app.tabs[i].switch_tab(ReqTab::Params, cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _w, _cx| {
                let i = app.active.unwrap();
                assert!(app.tabs[i].path_rows.iter().any(|(n, _)| n == "id"));
            })
            .unwrap();
    }

    /// The Script tab carries an inner Pre Request / Post Response strip
    /// (`script_subtabs`). Switching the inner sub-tab re-renders it.
    #[gpui::test]
    fn script_tab_inner_subtabs(cx: &mut TestAppContext) {
        let (window, _tc) = windowed_with_request(cx, "Repository/Create Issue.bru");
        window
            .update(cx, |app, _w, cx| {
                let i = app.active.unwrap();
                app.tabs[i].switch_tab(ReqTab::Script, cx);
                assert!(!app.tabs[i].script_post);
                app.tabs[i].switch_script_tab(true, cx);
                assert!(app.tabs[i].script_post);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _w, cx| {
                let i = app.active.unwrap();
                app.tabs[i].switch_script_tab(false, cx);
                assert!(!app.tabs[i].script_post);
            })
            .unwrap();
        cx.run_until_parked();
    }

    /// Opening a plain-text file routes the render to `text_pane` (path header +
    /// find bar + scrolling editor) instead of the request panes.
    #[gpui::test]
    fn text_pane_renders_for_plain_file(cx: &mut TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let js = tc.dir.join("helper.js");
        std::fs::write(&js, "function add(a, b) { return a + b; }\n").unwrap();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, _w, cx| app.open_text_file(js, cx))
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _w, _cx| {
                let i = app.active.unwrap();
                assert!(app.tabs[i].text.is_some());
            })
            .unwrap();
    }

    /// The GraphQL body mode renders the two-editor (QUERY + VARIABLES) pane in
    /// `req_content`. Drive it directly via `set_body_mode("graphql")`.
    #[gpui::test]
    fn graphql_body_renders_two_editors(cx: &mut TestAppContext) {
        let (window, _tc) = windowed_with_request(cx, "Repository/Create Issue.bru");
        window
            .update(cx, |app, _w, cx| {
                let i = app.active.unwrap();
                app.tabs[i].switch_tab(ReqTab::Body, cx);
                app.set_body_mode("graphql", cx);
                assert!(matches!(app.tabs[i].edit_kind, EditKind::GraphQl));
            })
            .unwrap();
        cx.run_until_parked();
    }

    /// `formUrlEncoded` and `multipartForm` body modes render the kv grid inside
    /// `req_content` (the `EditKind::Kv` early return for a body block).
    #[gpui::test]
    fn form_body_renders_kv_grid(cx: &mut TestAppContext) {
        let (window, _tc) = windowed_with_request(cx, "Repository/Create Issue.bru");
        for mode in ["formUrlEncoded", "multipartForm"] {
            window
                .update(cx, |app, _w, cx| {
                    let i = app.active.unwrap();
                    app.tabs[i].switch_tab(ReqTab::Body, cx);
                    app.set_body_mode(mode, cx);
                    assert!(matches!(app.tabs[i].edit_kind, EditKind::Kv(_)));
                    // Add a row so the body grid has an entry to render.
                    app.kv_add_row(cx);
                })
                .unwrap();
            cx.run_until_parked();
        }
    }
}
