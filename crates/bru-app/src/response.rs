//! Response body access and the response pane.

use crate::*;
use gpui::prelude::*;

impl BruApp {
    /// Raw bytes of the active tab's last response, if any.
    pub(crate) fn response_bytes(&self) -> Option<Vec<u8>> {
        self.active_tab()
            .and_then(|t| t.response.as_ref())
            .and_then(|o| o.response.as_ref())
            .map(|r| r.body.clone())
    }

    pub(crate) fn copy_response(&mut self, cx: &mut Context<Self>) {
        if let Some(bytes) = self.response_bytes() {
            let s = String::from_utf8_lossy(&bytes).to_string();
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(s));
            self.status = "Copied response to clipboard".into();
            cx.notify();
        }
    }

    pub(crate) fn save_response(&mut self, cx: &mut Context<Self>) {
        let Some(bytes) = self.response_bytes() else {
            return;
        };
        if let Some(path) = rfd::FileDialog::new().save_file() {
            self.status = match std::fs::write(&path, &bytes) {
                Ok(()) => "Saved response".into(),
                Err(e) => format!("Save failed: {e}"),
            };
        }
        cx.notify();
    }

    pub(crate) fn clear_response(&mut self, cx: &mut Context<Self>) {
        if let Some(i) = self.active {
            self.tabs[i].response = None;
            self.status.clear();
            cx.notify();
        }
    }

    pub(crate) fn response_pane(
        &self,
        tab: &OpenTab,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        // Sub-tab strip + status/time/size summary. Scrolls horizontally so the
        // right-side actions never clip in the narrow split response pane.
        let mut strip = div()
            .id("resp-strip")
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .overflow_x_scroll()
            .px_2()
            .bg(theme::bg())
            .border_b_1()
            .border_color(theme::border1());
        for rt in RespTab::ALL {
            let active = tab.resp_tab == rt;
            // Headers tab shows a count badge; Tests tab shows passed/total.
            let label = match rt {
                RespTab::Headers => {
                    let n = tab
                        .response
                        .as_ref()
                        .and_then(|o| o.response.as_ref())
                        .map(|r| r.headers.len())
                        .unwrap_or(0);
                    if n > 0 {
                        format!("Headers ({n})")
                    } else {
                        "Headers".to_string()
                    }
                }
                RespTab::Tests => match &tab.response {
                    Some(o) if !o.assertions.is_empty() || !o.tests.is_empty() => {
                        let total = o.assertions.len() + o.tests.len();
                        let passed = o.assertions.iter().filter(|a| a.passed).count()
                            + o.tests.iter().filter(|t| t.passed).count();
                        format!("Tests {passed}/{total}")
                    }
                    _ => "Tests".to_string(),
                },
                _ => rt.label().to_string(),
            };
            strip = strip.child(tab_chip(&label, active).on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                    if let Some(i) = this.active {
                        this.tabs[i].resp_tab = rt;
                    }
                    cx.notify();
                }),
            ));
        }
        if let Some(r) = tab.response.as_ref().and_then(|o| o.response.as_ref()) {
            strip = strip
                .child(div().flex_1())
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(status_color(r.status))
                        .child(format!("{} {}", r.status, r.status_text)),
                )
                .child(
                    div()
                        .px_2()
                        .text_size(px(12.))
                        .text_color(theme::subtext())
                        .child(format!("{} ms", r.duration_ms)),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::subtext())
                        .child(human_size(r.body.len())),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme::muted())
                                .font_family("monospace")
                                .child("$"),
                        )
                        .child(
                            div()
                                .w(px(170.))
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .bg(theme::input_bg())
                                .border_1()
                                .border_color(theme::border1())
                                .font_family("monospace")
                                .text_size(px(12.))
                                .child(self.resp_filter.clone()),
                        ),
                )
                .child(
                    ghost_btn(if self.resp_raw { "Pretty" } else { "Raw" }).on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                            this.resp_raw = !this.resp_raw;
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    ghost_btn("Hex")
                        .when(self.resp_hex, |d| d.text_color(theme::accent()))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                                this.resp_hex = !this.resp_hex;
                                cx.notify();
                            }),
                        ),
                )
                .child(ghost_btn("Copy").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.copy_response(cx)),
                ))
                .child(ghost_btn("Save").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.save_response(cx)),
                ))
                .child(ghost_btn("Clear").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.clear_response(cx)),
                ));
        }

        let scroll = |id: &'static str| {
            div()
                .id(id)
                .overflow_y_scroll()
                .min_h_0()
                .flex_1()
                .w_full()
                .p_3()
        };
        let content: gpui::AnyElement = match (&tab.response, tab.resp_tab) {
            (None, _) => div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .items_center()
                .justify_center()
                .gap_1()
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(theme::subtext())
                        .child("No response yet"),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme::muted())
                        .child("Press Send to make a request"),
                )
                .into_any_element(),
            (Some(o), RespTab::Response) => {
                // Compute the displayed body (pretty JSON + JSONPath filter, or
                // raw), then push it into the read-only selectable editor only
                // when it changes (so a live text selection survives re-renders).
                let (displayed, lang) = match o.response.as_ref() {
                    Some(r) if self.resp_hex => (hex_dump(&r.body), Lang::Plain),
                    Some(r) => {
                        let raw = String::from_utf8_lossy(&r.body).to_string();
                        let is_json = !self.resp_raw
                            && r.headers.iter().any(|(k, v)| {
                                k.eq_ignore_ascii_case("content-type") && v.contains("json")
                            });
                        if is_json {
                            match serde_json::from_str::<serde_json::Value>(&raw) {
                                Ok(v) => {
                                    let shown = if self.resp_filter_query.is_empty() {
                                        Some(v)
                                    } else {
                                        json_path(&v, &self.resp_filter_query)
                                    };
                                    match shown {
                                        Some(val) => (
                                            serde_json::to_string_pretty(&val).unwrap_or(raw),
                                            Lang::Json,
                                        ),
                                        None => ("(no match)".to_string(), Lang::Plain),
                                    }
                                }
                                Err(_) => (raw, Lang::Plain),
                            }
                        } else {
                            (raw, Lang::Plain)
                        }
                    }
                    None => (format_outcome(o), Lang::Plain),
                };
                if self.resp_editor.read(cx).text() != displayed {
                    self.resp_editor
                        .update(cx, |ed, cx| ed.set_text(&displayed, lang, cx));
                }
                scroll("resp-body")
                    .font_family("monospace")
                    .text_size(px(13.))
                    .line_height(px(19.))
                    .child(self.resp_editor.clone())
                    .into_any_element()
            }
            (Some(o), RespTab::Headers) => {
                let mut col = div()
                    .flex()
                    .flex_col()
                    .border_1()
                    .border_color(theme::border0())
                    .rounded_md()
                    .overflow_hidden();
                match &o.response {
                    Some(r) => {
                        for (i, (k, v)) in r.headers.iter().enumerate() {
                            col = col.child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_2()
                                    .px_2()
                                    .py_1()
                                    .when(i % 2 == 1, |d| d.bg(theme::mantle()))
                                    .child(
                                        div()
                                            .w(px(200.))
                                            .font_family("monospace")
                                            .text_size(px(12.))
                                            .text_color(theme::accent())
                                            .child(k.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .font_family("monospace")
                                            .text_size(px(12.))
                                            .text_color(theme::text())
                                            .child(v.clone()),
                                    ),
                            );
                        }
                    }
                    None => {
                        col = col.child(
                            div()
                                .p_2()
                                .text_color(theme::muted())
                                .child("(no response)"),
                        )
                    }
                }
                scroll("resp-headers").child(col).into_any_element()
            }
            (Some(o), RespTab::Timeline) => {
                // curl-style trace: request line + request headers, then the
                // response status + every response header, then timing/size.
                let mut txt = format!("> {} {}\n", tab.method.to_uppercase(), o.url);
                for line in edit::dict_to_lines(&tab.file, "headers").lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('~') {
                        continue;
                    }
                    if let Some((k, v)) = line.split_once(':') {
                        txt.push_str(&format!("> {}: {}\n", k.trim(), v.trim()));
                    }
                }
                if let Some(e) = &o.error {
                    if !e.is_empty() {
                        txt.push_str(&format!("! {e}\n"));
                    }
                }
                if let Some(r) = o.response.as_ref() {
                    txt.push_str(&format!("\n< {} {}\n", r.status, r.status_text));
                    for (k, v) in &r.headers {
                        txt.push_str(&format!("< {k}: {v}\n"));
                    }
                    txt.push_str(&format!(
                        "\ntime: {} ms\nsize: {}",
                        r.duration_ms,
                        human_size(r.body.len())
                    ));
                }
                scroll("resp-timeline")
                    .font_family("monospace")
                    .text_size(px(12.))
                    .text_color(theme::subtext())
                    .child(txt)
                    .into_any_element()
            }
            (Some(o), RespTab::Tests) => {
                let mut col = div().flex().flex_col().gap_1();
                if o.assertions.is_empty() && o.tests.is_empty() {
                    col = col.child(
                        div()
                            .text_color(theme::muted())
                            .text_size(px(12.))
                            .child("No assertions or tests."),
                    );
                }
                for a in &o.assertions {
                    let (m, c) = if a.passed {
                        ("\u{2713}", theme::green())
                    } else {
                        ("\u{2717}", theme::red())
                    };
                    col = col.child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(div().text_color(c).child(m))
                            .child(
                                div()
                                    .font_family("monospace")
                                    .text_size(px(12.))
                                    .text_color(theme::text())
                                    .child(format!("{} {} {}", a.expr, a.operator, a.expected)),
                            ),
                    );
                }
                for t in &o.tests {
                    let (m, c) = if t.passed {
                        ("\u{2713}", theme::green())
                    } else {
                        ("\u{2717}", theme::red())
                    };
                    col = col.child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(div().text_color(c).child(m))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme::text())
                                    .child(format!("test: {}", t.name)),
                            ),
                    );
                }
                scroll("resp-tests").child(col).into_any_element()
            }
        };

        div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .bg(theme::bg())
            .border_t_1()
            .border_color(theme::border2())
            .child(strip)
            .child(content)
    }
}
