//! Modal overlays: vault, devtools, prefs, curl, cookies, runner, env.

use crate::*;
use gpui::prelude::*;

impl BruApp {
    /// The secrets-vault overlay (unlock or manage).
    pub(crate) fn vault_overlay(&self, cx: &mut Context<Self>) -> Div {
        let unlocked = self.vault.is_some();
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .child(
                div()
                    .flex_1()
                    .text_size(px(15.))
                    .text_color(theme::text())
                    .child("Secrets Vault"),
            )
            .when(unlocked, |d| {
                let eye = if self.reveal_secrets {
                    "\u{1F441} Hide"
                } else {
                    "\u{1F441} Reveal"
                };
                d.child(ghost_btn(eye).on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.toggle_reveal_secrets(cx)),
                ))
                .child(ghost_btn("Lock").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.vault_lock(cx)),
                ))
            })
            .child(ghost_btn("Close").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.close_vault(cx)),
            ));
        let body: Div =
            if !unlocked {
                let prompt = if vault::exists() {
                    "Enter your master password to unlock."
                } else {
                    "No vault yet \u{2014} set a master password to create one."
                };
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::subtext())
                            .child(prompt),
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
                            .child(self.vault_input.clone()),
                    )
                    .child(solid_btn("Unlock").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.vault_unlock(cx)),
                    ))
            } else {
                let cell = |child: Entity<CodeEditor>| {
                    div()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(theme::input_bg())
                        .border_1()
                        .border_color(theme::border1())
                        .text_size(px(12.))
                        .font_family("monospace")
                        .child(child)
                };
                let mut table = div().flex().flex_col().gap_1();
                for (i, (k, v)) in self.vault_rows.iter().enumerate() {
                    table = table.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(cell(k.clone()).w(px(200.)))
                            .child(cell(v.clone()).flex_1())
                            .child(
                                div()
                                    .px_1()
                                    .text_color(theme::red())
                                    .child("\u{2715}")
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                            this.vault_remove_row(i, cx)
                                        }),
                                    ),
                            ),
                    );
                }
                table = table.child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::accent())
                        .child("+ Add Secret")
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.vault_add_row(cx)),
                        ),
                );
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .child("Secrets resolve into {{name}} at send (lowest precedence)."),
                    )
                    .child(
                        div()
                            .id("vault-table")
                            .overflow_y_scroll()
                            .min_h_0()
                            .flex_1()
                            .child(table),
                    )
                    .child(div().flex().flex_row().justify_end().child(
                        solid_btn("Save").on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.vault_save(cx)),
                        ),
                    ))
            };
        let card = div()
            .flex()
            .flex_col()
            .gap_3()
            .w(px(620.))
            .h(if unlocked { px(440.) } else { px(220.) })
            .p_4()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .overflow_hidden()
            .occlude()
            .child(header)
            .child(body)
            .children(self.vault_error.as_ref().map(|e| {
                div()
                    .text_size(px(12.))
                    .text_color(theme::red())
                    .child(e.clone())
            }));
        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x00000099))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.close_vault(cx)),
            )
            .child(card)
    }

    /// The devtools dock (Console / Network), pinned to the bottom.
    pub(crate) fn devtools_overlay(&self, cx: &mut Context<Self>) -> Div {
        let tab_btn = |label: &'static str, net: bool, active: bool, cx: &mut Context<Self>| {
            tab_chip(label, active).on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                    this.devtools_net = net;
                    cx.notify();
                }),
            )
        };
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .child(tab_btn("Console", false, !self.devtools_net, cx))
            .child(tab_btn("Network", true, self.devtools_net, cx))
            .child(div().flex_1())
            .child(ghost_btn("Clear").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.clear_devtools(cx)),
            ))
            .child(ghost_btn("Close").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.toggle_devtools(cx)),
            ));
        let body: Div = if self.devtools_net {
            let mut col = div().flex().flex_col().gap_1();
            if self.network.is_empty() {
                col = col.child(
                    div()
                        .text_color(theme::muted())
                        .text_size(px(12.))
                        .child("No requests yet."),
                );
            }
            for e in &self.network {
                let sc = if e.ok {
                    status_color(e.status)
                } else {
                    theme::red()
                };
                col =
                    col.child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(50.))
                                    .font_family("monospace")
                                    .text_size(px(11.))
                                    .text_color(theme::method_color(&e.method))
                                    .child(short_method(&e.method)),
                            )
                            .child(div().w(px(40.)).text_size(px(11.)).text_color(sc).child(
                                if e.ok {
                                    e.status.to_string()
                                } else {
                                    "ERR".to_string()
                                },
                            ))
                            .child(
                                div()
                                    .w(px(64.))
                                    .text_size(px(11.))
                                    .text_color(theme::subtext())
                                    .child(format!("{} ms", e.ms)),
                            )
                            .child(
                                div()
                                    .w(px(80.))
                                    .text_size(px(11.))
                                    .text_color(theme::subtext())
                                    .child(human_size(e.size)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .font_family("monospace")
                                    .text_size(px(11.))
                                    .text_color(theme::text())
                                    .child(e.url.clone()),
                            ),
                    );
            }
            col
        } else {
            let mut col = div().flex().flex_col().gap_1();
            if self.console.is_empty() {
                col = col.child(
                    div()
                        .text_color(theme::muted())
                        .text_size(px(12.))
                        .child("Console is empty."),
                );
            }
            for line in &self.console {
                col = col.child(
                    div()
                        .font_family("monospace")
                        .text_size(px(12.))
                        .text_color(theme::subtext())
                        .child(line.clone()),
                );
            }
            col
        };
        div()
            .absolute()
            .left(px(0.))
            .right(px(0.))
            .bottom(px(0.))
            .h(px(220.))
            .bg(theme::mantle())
            .border_t_1()
            .border_color(theme::border1())
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .occlude()
            .child(header)
            .child(
                div()
                    .id("devtools-body")
                    .overflow_y_scroll()
                    .min_h_0()
                    .flex_1()
                    .w_full()
                    .child(body),
            )
    }

    /// The preferences overlay (timeout + TLS-insecure).
    pub(crate) fn prefs_overlay(&self, cx: &mut Context<Self>) -> Div {
        let card = div()
            .flex()
            .flex_col()
            .gap_3()
            .w(px(440.))
            .p_4()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .occlude()
            .child(
                div()
                    .text_size(px(15.))
                    .text_color(theme::text())
                    .child("Preferences"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(150.))
                            .text_size(px(12.))
                            .text_color(theme::subtext())
                            .child("Timeout (seconds)"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(theme::input_bg())
                            .border_1()
                            .border_color(theme::border1())
                            .font_family("monospace")
                            .text_size(px(12.))
                            .child(self.timeout_input.clone()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(check_box(self.pref_insecure).on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.toggle_insecure(cx)),
                    ))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::subtext())
                            .child("Disable TLS verification (insecure)"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(check_box(self.pref_developer).on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.toggle_developer(cx)),
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme::subtext())
                                    .child("Developer Mode"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(theme::muted())
                                    .child("Allow scripts to require() local files"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(ghost_btn("Close").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.close_prefs(cx)),
                    ))
                    .child(solid_btn("Apply").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.apply_prefs(cx)),
                    )),
            );
        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x00000099))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.close_prefs(cx)),
            )
            .child(card)
    }

    /// The curl-import overlay (paste a curl command).
    pub(crate) fn curl_overlay(&self, cx: &mut Context<Self>) -> Div {
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .child(
                div()
                    .flex_1()
                    .text_size(px(15.))
                    .text_color(theme::text())
                    .child("Import curl"),
            )
            .child(solid_btn("Import").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.import_curl(cx)),
            ))
            .child(ghost_btn("Close").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.close_curl(cx)),
            ));
        let editor = div()
            .id("curl-input")
            .overflow_y_scroll()
            .min_h_0()
            .flex_1()
            .w_full()
            .p_2()
            .rounded_md()
            .bg(theme::input_bg())
            .border_1()
            .border_color(theme::border1())
            .font_family("monospace")
            .text_size(px(12.))
            .child(self.curl_input.clone());
        let card = div()
            .flex()
            .flex_col()
            .gap_2()
            .w(px(680.))
            .h(px(340.))
            .p_4()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .occlude()
            .child(header)
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::muted())
                    .child("Paste a curl command:"),
            )
            .child(editor);
        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x00000099))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.close_curl(cx)),
            )
            .child(card)
    }

    /// The cookies overlay (captured from response Set-Cookie headers).
    pub(crate) fn cookies_overlay(&self, cx: &mut Context<Self>) -> Div {
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .w_full()
            .child(
                div()
                    .flex_1()
                    .text_size(px(15.))
                    .text_color(theme::text())
                    .child("Cookies"),
            )
            .child(ghost_btn("Clear All").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.clear_cookies(cx)),
            ))
            .child(ghost_btn("Close").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.close_cookies(cx)),
            ));
        let mut list = div()
            .id("cookies-list")
            .overflow_y_scroll()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_1()
            .flex_1()
            .w_full();
        if self.cookies.is_empty() {
            list = list.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child("No cookies yet \u{2014} send a request that returns Set-Cookie."),
            );
        }
        for (i, c) in self.cookies.iter().enumerate() {
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(180.))
                            .font_family("monospace")
                            .text_size(px(12.))
                            .text_color(theme::subtext())
                            .child(c.domain.clone()),
                    )
                    .child(
                        div()
                            .w(px(160.))
                            .text_size(px(12.))
                            .text_color(theme::accent())
                            .child(c.name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .font_family("monospace")
                            .text_size(px(12.))
                            .text_color(theme::text())
                            .child(c.value.clone()),
                    )
                    .child(
                        div()
                            .px_1()
                            .text_color(theme::red())
                            .child("\u{2715}")
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                    this.delete_cookie(i, cx)
                                }),
                            ),
                    ),
            );
        }
        let card = div()
            .flex()
            .flex_col()
            .gap_3()
            .w(px(720.))
            .h(px(440.))
            .p_4()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .occlude()
            .child(header)
            .child(list);
        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x00000099))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.close_cookies(cx)),
            )
            .child(card)
    }

    /// The collection-runner overlay (scrim + results card).
    pub(crate) fn runner_overlay(&self, cx: &mut Context<Self>) -> Div {
        let passed = self.runner_results.iter().filter(|x| x.passed).count();
        let total = self.runner_results.len();
        let status_text = if self.runner_running {
            "running\u{2026}".to_string()
        } else {
            format!("{passed}/{total} passed")
        };
        let status_color = if self.runner_running {
            theme::accent()
        } else if passed == total {
            theme::green()
        } else {
            theme::red()
        };
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .w_full()
            .child(
                div()
                    .text_size(px(15.))
                    .text_color(theme::text())
                    .child(format!("Run: {}", self.runner_title)),
            )
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(status_color)
                    .child(status_text),
            )
            .child(ghost_btn("Close").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                    this.runner_open = false;
                    cx.notify();
                }),
            ));
        let mut list = div()
            .id("runner-list")
            .overflow_y_scroll()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_1()
            .flex_1()
            .w_full();
        if self.runner_running && self.runner_results.is_empty() {
            list = list.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child("Running requests\u{2026}"),
            );
        }
        for res in &self.runner_results {
            let (mark, c) = if res.passed {
                ("\u{2713}", theme::green())
            } else {
                ("\u{2717}", theme::red())
            };
            let detail = match &res.error {
                Some(e) => e.clone(),
                None => format!("{} \u{00B7} {} ms", res.status, res.ms),
            };
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .w(px(14.))
                            .text_size(px(12.))
                            .text_color(c)
                            .child(mark),
                    )
                    .child(
                        div()
                            .w(px(220.))
                            .text_size(px(12.))
                            .text_color(theme::text())
                            .child(res.name.clone()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .font_family("monospace")
                            .text_size(px(12.))
                            .text_color(theme::subtext())
                            .child(detail),
                    ),
            );
        }
        let card = div()
            .flex()
            .flex_col()
            .gap_3()
            .w(px(620.))
            .h(px(460.))
            .p_4()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .occlude()
            .child(header)
            .child(list);
        div()
            .absolute()
            .inset_0()
            .bg(gpui::rgba(0x00000099))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                    this.runner_open = false;
                    cx.notify();
                }),
            )
            .child(card)
    }

    /// The environment-manager overlay (scrim + card).
    pub(crate) fn env_overlay(&self, cx: &mut Context<Self>) -> Div {
        let ed = self.env.as_ref().expect("env overlay with env=None");

        // Left: env list with New / per-env duplicate + delete.
        let mut list = div().flex().flex_col().gap_1().w(px(220.)).child(
            div()
                .px_2()
                .py_1()
                .rounded_md()
                .text_size(px(12.))
                .text_color(theme::accent())
                .child("+ New Environment")
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.env_new(cx)),
                ),
        );
        for name in &ed.names {
            let active = ed.selected == *name;
            let (n_sel, n_dup, n_del) = (name.clone(), name.clone(), name.clone());
            list = list.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_size(px(12.))
                            .when(active, |d| d.bg(theme::surface0()))
                            .text_color(if active {
                                theme::text()
                            } else {
                                theme::subtext()
                            })
                            .child(name.clone())
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                    this.env_select(n_sel.clone(), cx)
                                }),
                            ),
                    )
                    .child(
                        div()
                            .px_1()
                            .text_size(px(11.))
                            .text_color(theme::muted())
                            .child("\u{29C9}")
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                    this.env_duplicate(n_dup.clone(), cx)
                                }),
                            ),
                    )
                    .child(
                        div()
                            .px_1()
                            .text_size(px(11.))
                            .text_color(theme::red())
                            .child("\u{2715}")
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                    this.env_delete(n_del.clone(), cx)
                                }),
                            ),
                    ),
            );
        }
        let left = div()
            .id("env-list")
            .overflow_y_scroll()
            .min_h_0()
            .w(px(220.))
            .h_full()
            .child(list);

        // Right: rename + variables table + error + Save.
        let right: Div =
            if ed.selected.is_empty() {
                div().flex_1().flex().items_center().justify_center().child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::muted())
                        .child("Select or create an environment."),
                )
            } else {
                let rename_row = div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(240.))
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(theme::input_bg())
                            .border_1()
                            .border_color(theme::border1())
                            .text_size(px(12.))
                            .font_family("monospace")
                            .child(ed.rename.clone()),
                    )
                    .child(ghost_btn("Rename").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.env_rename_apply(cx)),
                    ));
                let cell = |child: Entity<CodeEditor>| {
                    div()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(theme::input_bg())
                        .border_1()
                        .border_color(theme::border1())
                        .text_size(px(12.))
                        .font_family("monospace")
                        .child(child)
                };
                let mut table = div().flex().flex_col().gap_1();
                for (i, r) in ed.rows.iter().enumerate() {
                    table = table.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(check_box(r.enabled).on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                    this.env_toggle_enabled(i, cx)
                                }),
                            ))
                            .child(cell(r.name.clone()).w(px(180.)))
                            .child(cell(r.value.clone()).flex_1())
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_1()
                                    .child(check_box(r.secret).on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                            this.env_toggle_secret(i, cx)
                                        }),
                                    ))
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .text_color(theme::muted())
                                            .child("secret"),
                                    ),
                            )
                            .child(
                                div()
                                    .px_1()
                                    .text_color(theme::red())
                                    .child("\u{2715}")
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                            this.env_remove_row(i, cx)
                                        }),
                                    ),
                            ),
                    );
                }
                table = table.child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme::accent())
                        .child("+ Add Variable")
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.env_add_row(cx)),
                        ),
                );
                let mut col = div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .child(rename_row)
                    .child(
                        div()
                            .id("env-table")
                            .overflow_y_scroll()
                            .min_h_0()
                            .flex_1()
                            .child(table),
                    );
                if let Some(err) = &ed.error {
                    col = col.child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::red())
                            .child(err.clone()),
                    );
                }
                col.child(div().flex().flex_row().justify_end().child(
                    solid_btn("Save").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.env_save(cx)),
                    ),
                ))
            };

        let card = div()
            .w(px(800.))
            .h(px(480.))
            .p_4()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .overflow_hidden()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(15.))
                            .text_color(theme::text())
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("Environments"),
                    )
                    .child({
                        let global = self.env.as_ref().map(|e| e.global).unwrap_or(false);
                        div()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .mr_2()
                            .child(
                                ghost_btn("Collection")
                                    .when(!global, |d| d.text_color(theme::accent()))
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                                            this.env_set_scope(false, cx)
                                        }),
                                    ),
                            )
                            .child(
                                ghost_btn("Global")
                                    .when(global, |d| d.text_color(theme::accent()))
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                                            this.env_set_scope(true, cx)
                                        }),
                                    ),
                            )
                    })
                    .child({
                        let eye = if self.reveal_secrets {
                            "\u{1F441} Hide"
                        } else {
                            "\u{1F441} Reveal"
                        };
                        ghost_btn(eye).on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                                this.toggle_reveal_secrets(cx)
                            }),
                        )
                    })
                    .child(ghost_btn("Close").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.env_close(cx)),
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_3()
                    .flex_1()
                    .min_h_0()
                    .child(left)
                    .child(right),
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
