//! Top bar, collection header, sidebar tree and URL bar.

use crate::*;
use gpui::prelude::*;

impl BruApp {
    /// The slim app title bar (Bruno's AppTitleBar): Home + collection switcher
    /// on the left, theme toggle on the right. Collection actions live in the
    /// collection header below, not here.
    pub(crate) fn top_bar(&self, cx: &mut Context<Self>) -> Div {
        let name = self
            .collection
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_else(|| "No collection".into());
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .px_3()
            .py_2()
            .bg(theme::bg())
            .border_b_1()
            .border_color(theme::border1())
            .child(svg_chip("home").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.go_home(cx)),
            ))
            .child(
                // Collection/workspace switcher: name + chevron (opens Home).
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .hover(|s| s.bg(theme::surface0()))
                    .child(
                        div()
                            .text_color(theme::text())
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(name),
                    )
                    .child(
                        icons::icon("chevron-down")
                            .size(px(12.))
                            .text_color(theme::muted()),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.go_home(cx)),
                    ),
            )
            .child(
                div()
                    .text_color(theme::muted())
                    .text_size(px(12.))
                    .child("\u{2022} main"),
            )
            .child(div().flex_1())
            .child(
                icon_chip(if theme::is_dark() {
                    "\u{2600}" // ÃƒÂ¢Ã‹Å“Ã¢â€šÂ¬ ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Â click for light
                } else {
                    "\u{263E}" // ÃƒÂ¢Ã‹Å“Ã‚Â¾ ÃƒÂ¢Ã¢â€šÂ¬Ã¢â‚¬Â click for dark
                })
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                        theme::toggle();
                        this.persist_prefs();
                        cx.notify();
                    }),
                ),
            )
    }

    /// The per-collection toolbar (Bruno's CollectionHeader), shown atop the main
    /// pane when a collection is open: management on the left; Run / Settings /
    /// Vault / Prefs / Environments + the environment selector on the right.
    /// (Cookies + DevTools live in the status bar, as in Bruno.)
    pub(crate) fn collection_header(&self, cx: &mut Context<Self>) -> Div {
        if self.collection.is_none() || self.home {
            return div();
        }
        let (env_label, env_has) = match &self.selected_env {
            Some(e) => (e.clone(), true),
            None => ("No Environment".to_string(), false),
        };
        let env_pill = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .px_3()
            .py_1()
            .rounded_md()
            .bg(theme::surface0())
            .border_1()
            .border_color(theme::border1())
            .text_size(px(12.))
            .child(div().w(px(7.)).h(px(7.)).rounded_full().bg(if env_has {
                theme::green()
            } else {
                theme::muted()
            }))
            .child(
                div()
                    .text_color(if env_has {
                        theme::text()
                    } else {
                        theme::muted()
                    })
                    .child(env_label),
            )
            .child(
                div()
                    .text_color(theme::muted())
                    .text_size(px(10.))
                    .child("\u{25BE}"),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _w, cx| {
                    this.open_env_menu(ev.position, cx);
                }),
            );
        let run = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .px_3()
            .py_1()
            .rounded_md()
            .text_color(theme::text())
            .text_size(px(13.))
            .hover(|s| s.bg(theme::surface0()))
            .child(icons::icon("play").size(px(12.)).text_color(theme::green()))
            .child("Run")
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                    let dir = this.dir.clone();
                    this.run_folder(dir, cx);
                    cx.notify();
                }),
            );
        let row = div()
            .id("collection-header")
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .overflow_x_scroll()
            .px_3()
            .py_1()
            .bg(theme::bg())
            .border_b_1()
            .border_color(theme::border1())
            .child(chip("Open").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        this.load_collection(dir, cx);
                    }
                }),
            ))
            .child(chip("New").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                    if let Some(parent) = rfd::FileDialog::new().pick_folder() {
                        this.create_collection(&parent, cx);
                    }
                }),
            ))
            .child(chip("Import").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.import_postman(cx)),
            ))
            .child(chip("curl").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.open_curl(cx)),
            ))
            .child(div().flex_1())
            .child(run)
            .child(svg_chip("settings").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.open_collection_settings(cx)),
            ))
            .child(
                chip("Vault")
                    .text_color(if self.vault.is_some() {
                        theme::green()
                    } else {
                        theme::text()
                    })
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.open_vault(cx)),
                    ),
            )
            .child(chip("Prefs").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.open_prefs(cx)),
            ))
            .child(chip("Environments").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.env_open(cx)),
            ))
            .child(env_pill);
        div().w_full().child(row)
    }

    pub(crate) fn sidebar(&self, cx: &mut Context<Self>) -> Div {
        let q = self.search_query.clone();
        let mut rows: Vec<Div> = Vec::new();
        if let Some(tree) = &self.collection {
            self.push_folder(&tree.root, 0, &q, cx, &mut rows);
        } else {
            rows.push(
                div()
                    .p_2()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child("No collection loaded."),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_1()
            .w(px(self.sidebar_w))
            .h_full()
            .bg(theme::bg())
            .border_r_1()
            .border_color(theme::border1())
            .p_2()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_1()
                    .child(
                        div()
                            .flex_1()
                            .text_color(theme::muted())
                            .text_size(px(12.))
                            .child(
                                self.collection
                                    .as_ref()
                                    .map(|c| c.name.to_uppercase())
                                    .unwrap_or_default(),
                            )
                            // Right-click the collection name for the root menu.
                            .when(self.collection.is_some(), |d| {
                                d.on_mouse_down(
                                    MouseButton::Right,
                                    cx.listener(|this, ev: &MouseDownEvent, _w, cx| {
                                        this.open_root_menu(ev.position, cx)
                                    }),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_1()
                            .items_center()
                            .child(
                                div()
                                    .px_1()
                                    .rounded_md()
                                    .hover(|s| s.bg(theme::surface0()))
                                    .child(
                                        icons::icon("folder")
                                            .size(px(15.))
                                            .text_color(theme::accent()),
                                    )
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                                            let dir = this.dir.clone();
                                            this.new_folder_in(&dir, cx);
                                        }),
                                    ),
                            )
                            .child(
                                div()
                                    .px_1()
                                    .rounded_md()
                                    .hover(|s| s.bg(theme::surface0()))
                                    .child(
                                        icons::icon("plus")
                                            .size(px(15.))
                                            .text_color(theme::accent()),
                                    )
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                                            this.new_request(cx)
                                        }),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .w_full()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(theme::input_bg())
                    .border_1()
                    .border_color(theme::border1())
                    .text_size(px(12.))
                    .child(
                        icons::icon("search")
                            .size(px(14.))
                            .text_color(theme::muted()),
                    )
                    .child(div().flex_1().min_w_0().child(self.search.clone())),
            )
            .child(
                div()
                    .id("sidebar-rows")
                    .flex_1()
                    .overflow_y_scroll()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .children(rows),
            )
    }

    pub(crate) fn push_folder(
        &self,
        folder: &Folder,
        depth: usize,
        query: &str,
        cx: &mut Context<Self>,
        out: &mut Vec<Div>,
    ) {
        let mut subs: Vec<&Folder> = folder.folders.iter().collect();
        subs.sort_by_key(|f| f.name.to_lowercase());
        for sub in subs {
            if !query.is_empty() && !folder_matches(sub, query) {
                continue;
            }
            // A search query forces every branch open so matches are visible.
            let collapsed = query.is_empty() && self.collapsed.contains(&sub.path);
            let fpath = sub.path.clone();
            let fname = sub.name.clone();
            let tpath = sub.path.clone();
            out.push(
                folder_row(&sub.name, depth, collapsed)
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseUpEvent, _win, cx| {
                            this.toggle_folder(tpath.clone(), cx);
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, ev: &MouseDownEvent, _win, cx| {
                            this.open_ctx_menu(fpath.clone(), true, fname.clone(), ev.position, cx);
                        }),
                    ),
            );
            if !collapsed {
                self.push_folder(sub, depth + 1, query, cx, out);
            }
        }
        let mut reqs: Vec<&bru_core::RequestItem> = folder.requests.iter().collect();
        reqs.sort_by(|a, b| {
            a.seq
                .unwrap_or(i64::MAX)
                .cmp(&b.seq.unwrap_or(i64::MAX))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        for req in reqs {
            if !query.is_empty() && !req.name.to_lowercase().contains(query) {
                continue;
            }
            let path = req.path.clone();
            let active = self.active_tab().map(|t| t.path.as_path()) == Some(path.as_path());
            let method = req.method.clone().unwrap_or_default();
            let rpath = path.clone();
            let rname = req.name.clone();
            let row = req_row(&method, &req.name, active, depth)
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, _ev: &MouseUpEvent, _win, cx| {
                        this.open_request(path.clone(), cx);
                        cx.notify();
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, ev: &MouseDownEvent, _win, cx| {
                        this.open_ctx_menu(rpath.clone(), false, rname.clone(), ev.position, cx);
                    }),
                );
            out.push(row);
        }
    }

    pub(crate) fn url_bar(&self, tab: &OpenTab, cx: &mut Context<Self>) -> Div {
        let method = if tab.method.is_empty() {
            "GET".to_string()
        } else {
            tab.method.to_uppercase()
        };
        let dirty = self.dirty.contains(&tab.path);
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .px_2()
            .py_2()
            .bg(theme::bg())
            .border_b_1()
            .border_color(theme::border1())
            .child(
                // Method + URL share one bordered, rounded input group â€” Bruno's URL bar.
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    .rounded_md()
                    .bg(theme::input_bg())
                    .border_1()
                    .border_color(theme::border1())
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .border_r_1()
                            .border_color(theme::border1())
                            .text_color(theme::method_color(&method))
                            .text_size(px(12.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .font_family("monospace")
                            .child(method)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, ev: &MouseDownEvent, _w, cx| {
                                    this.open_method_menu(ev.position, cx);
                                }),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px_2()
                            .py_1()
                            .text_color(theme::text())
                            .text_size(px(13.))
                            .font_family("monospace")
                            .child(tab.url_input.clone()),
                    ),
            )
            .child(icon_chip("</>").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                    let curl = this.active_tab().map(|tab| to_curl(tab, cx));
                    if let Some(curl) = curl {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(curl));
                        this.status = "Copied curl to clipboard".into();
                        cx.notify();
                    }
                }),
            ))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .bg(theme::surface0())
                    .text_color(theme::text())
                    .text_size(px(13.))
                    .child("Save")
                    .when(dirty, |d| {
                        d.child(
                            div()
                                .w(px(6.))
                                .h(px(6.))
                                .rounded_full()
                                .bg(theme::draft_dot()),
                        )
                    })
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _ev: &MouseUpEvent, _w, cx| {
                            this.save(cx);
                            cx.notify();
                        }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_1()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .bg(theme::accent())
                    .text_color(theme::bg())
                    .text_size(px(13.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .hover(|s| s.opacity(0.92))
                    .child(icons::icon("send").size(px(13.)).text_color(theme::bg()))
                    .child(if tab.sending {
                        "Sending\u{2026}".to_string()
                    } else {
                        "Send".to_string()
                    })
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _ev: &MouseUpEvent, _w, cx| {
                            this.send(cx);
                            cx.notify();
                        }),
                    ),
            )
    }
}
