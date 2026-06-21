//! Home screen, tab strip, status bar and git chip/overlay.

use crate::*;
use gpui::prelude::*;

impl BruApp {
    /// The Home / welcome screen (open / import + recent collections).
    pub(crate) fn home_screen(&self, cx: &mut Context<Self>) -> Div {
        let mut col = div()
            .flex()
            .flex_col()
            .gap_3()
            .items_center()
            .child(
                div()
                    .text_size(px(28.))
                    .text_color(theme::accent())
                    .child("bruno-rs"),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::subtext())
                    .child("Open or import a collection to begin."),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(solid_btn("Open Collection").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                this.load_collection(dir, cx);
                            }
                        }),
                    ))
                    .child(ghost_btn("Import Postman").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.import_postman(cx)),
                    ))
                    .child(ghost_btn("Import curl").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.open_curl(cx)),
                    )),
            );
        if !self.recent.is_empty() {
            col = col.child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::muted())
                    .child("Recent"),
            );
            let mut list = div().flex().flex_col().gap_1().w(px(460.));
            for p in &self.recent {
                let path = PathBuf::from(p);
                let name = path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.clone());
                let pc = path.clone();
                list = list.child(
                    div()
                        .flex()
                        .flex_col()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(theme::surface0())
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(theme::text())
                                .child(name),
                        )
                        .child(
                            div()
                                .text_size(px(10.))
                                .text_color(theme::muted())
                                .child(p.clone()),
                        )
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                this.load_collection(pc.clone(), cx)
                            }),
                        ),
                );
            }
            col = col.child(
                div()
                    .id("home-recent")
                    .overflow_y_scroll()
                    .min_h_0()
                    .h(px(240.))
                    .child(list),
            );
        }
        div()
            .flex()
            .flex_1()
            .w_full()
            .items_center()
            .justify_center()
            .child(col)
    }

    /// The strip of open request tabs (click to focus, ÃƒÆ’Ã¢â‚¬â€ to close).
    pub(crate) fn tab_strip(&self, cx: &mut Context<Self>) -> Div {
        let mut strip = div()
            .flex()
            .flex_row()
            .items_end()
            .w_full()
            .bg(theme::bg())
            .border_b_1()
            .border_color(theme::border0());
        for (i, t) in self.tabs.iter().enumerate() {
            let active = self.active == Some(i);
            let dirty = self.dirty.contains(&t.path);
            let mut tab = div()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .px_3()
                .py_1()
                .when(active, |d| {
                    d.border_b_1().border_color(theme::tab_underline())
                })
                // Right-click opens the tab menu; middle-click closes the tab.
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, ev: &MouseDownEvent, _w, cx| {
                        this.open_tab_menu(i, ev.position, cx)
                    }),
                )
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |this, _ev: &MouseDownEvent, _w, cx| {
                        this.request_close_tab(i, cx)
                    }),
                );
            // Unsaved tabs show Bruno's amber draft dot before the title.
            if dirty {
                tab = tab.child(
                    div()
                        .w(px(7.))
                        .h(px(7.))
                        .rounded_full()
                        .bg(theme::draft_dot()),
                );
            }
            strip = strip.child(
                tab.child(
                    div()
                        .text_size(px(12.))
                        .text_color(if active {
                            theme::text()
                        } else {
                            theme::muted()
                        })
                        .child(t.title())
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _ev: &MouseUpEvent, _w, cx| {
                                this.active = Some(i);
                                cx.notify();
                            }),
                        ),
                )
                .child(
                    div()
                        .px_1()
                        .rounded_md()
                        .hover(|s| s.bg(theme::surface0()))
                        .child(icons::icon("x").size(px(12.)).text_color(theme::muted()))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _ev: &MouseUpEvent, _w, cx| {
                                this.request_close_tab(i, cx);
                            }),
                        ),
                ),
            );
        }
        strip
    }

    pub(crate) fn status_bar(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .w_full()
            .px_3()
            .py_1()
            .bg(theme::statusbar_bg())
            .border_t_1()
            .border_color(theme::statusbar_border())
            .child(
                div()
                    .px_2()
                    .text_color(theme::statusbar_text())
                    .text_size(px(11.))
                    .child(self.status.clone()),
            )
            .children(self.git_chip(cx))
            .child(div().flex_1())
            .child(icon_chip("Search").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, window, cx| {
                    let h = this.search.read(cx).focus_handle(cx);
                    window.focus(&h, cx);
                }),
            ))
            .child(icon_chip("Cookies").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.open_cookies(cx)),
            ))
            .child(icon_chip("Dev Tools").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.toggle_devtools(cx)),
            ))
            .child(
                div()
                    .text_color(theme::statusbar_text())
                    .text_size(px(11.))
                    .child("v0.0.0"),
            )
    }

    /// The status-bar git chip: branch + ahead/behind + dirty dot. None when the
    /// collection isn't a git repo. Shown even if `git status` couldn't be read
    /// (falls back to a "git" label) so the overlay stays reachable. Clicking
    /// opens the git overlay.
    pub(crate) fn git_chip(&self, cx: &mut Context<Self>) -> Option<Div> {
        if !self.git_repo {
            return None;
        }
        let label = match self.git_status.as_ref() {
            Some(st) => {
                let mut label = st.branch.clone();
                if st.behind > 0 {
                    label.push_str(&format!(" \u{2193}{}", st.behind));
                }
                if st.ahead > 0 {
                    label.push_str(&format!(" \u{2191}{}", st.ahead));
                }
                if st.is_dirty() {
                    label.push_str(" \u{2022}");
                }
                label
            }
            None => "git".to_string(),
        };
        Some(
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
                    icons::icon("git-branch")
                        .size(px(13.))
                        .text_color(theme::statusbar_text()),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme::statusbar_text())
                        .child(label),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.open_git(cx)),
                ),
        )
    }

    /// The git overlay: branch summary, changed-file list, commit input, and the
    /// stage/commit/discard/fetch/pull/push actions.
    pub(crate) fn git_overlay(&self, cx: &mut Context<Self>) -> Div {
        let have_status = self.git_status.is_some();
        let st = self.git_status.clone().unwrap_or_default();
        let summary = if have_status {
            let mut s = format!("On branch {}", st.branch);
            if st.ahead > 0 || st.behind > 0 {
                s.push_str(&format!(" (ahead {}, behind {})", st.ahead, st.behind));
            }
            s
        } else {
            "Could not read git status \u{2014} try Fetch, or reopen the collection.".to_string()
        };
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .child(
                icons::icon("git-branch")
                    .size(px(16.))
                    .text_color(theme::accent()),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(15.))
                    .text_color(theme::text())
                    .child("Git"),
            )
            .child(ghost_btn("Close").on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.close_git(cx)),
            ));

        // Changed-file list.
        let mut files = div()
            .id("git-files")
            .overflow_y_scroll()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_1()
            .flex_1()
            .w_full();
        if st.files.is_empty() {
            files = files.child(div().text_size(px(12.)).text_color(theme::muted()).child(
                if have_status {
                    "Working tree clean."
                } else {
                    "Status unavailable."
                },
            ));
        }
        for f in &st.files {
            let color = if f.code.contains('?') {
                theme::green() // untracked
            } else if f.code.starts_with(' ') {
                theme::orange() // unstaged change
            } else {
                theme::blue() // staged
            };
            files = files.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(28.))
                            .font_family("monospace")
                            .text_size(px(12.))
                            .text_color(color)
                            .child(f.code.replace(' ', "\u{00b7}")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .font_family("monospace")
                            .text_size(px(12.))
                            .text_color(theme::text())
                            .child(f.path.clone()),
                    ),
            );
        }

        let busy = self.git_busy;
        // Local actions row.
        let discard_label = if self.git_confirm_discard {
            "Confirm discard"
        } else {
            "Discard all"
        };
        let local_actions = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .child(self.git_btn(cx, "Stage all", busy, |this, cx| {
                this.git_run(git::Op::StageAll, cx)
            }))
            .child(self.git_btn(cx, "Commit", busy, |this, cx| this.git_commit(cx)))
            .child(div().flex_1())
            .child(
                self.git_btn(cx, discard_label, busy, |this, cx| this.git_discard(cx))
                    .text_color(theme::red()),
            );

        // Remote actions row.
        let remote_actions = div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .w_full()
            .child(self.git_btn(cx, "Fetch", busy, |this, cx| {
                this.git_run(git::Op::Fetch, cx)
            }))
            .child(self.git_btn(cx, "Pull", busy, |this, cx| this.git_run(git::Op::Pull, cx)))
            .child(self.git_btn(cx, "Push", busy, |this, cx| this.git_run(git::Op::Push, cx)));

        let card = div()
            .flex()
            .flex_col()
            .gap_3()
            .w(px(640.))
            .h(px(520.))
            .p_4()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .occlude()
            .child(header)
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::subtext())
                    .child(summary),
            )
            .child(files)
            .child(
                div()
                    .w_full()
                    .border_1()
                    .border_color(theme::border1())
                    .rounded_md()
                    .bg(theme::input_bg())
                    .px_2()
                    .py_1()
                    .child(self.git_msg.clone()),
            )
            .child(local_actions)
            .child(remote_actions)
            .child(
                // Fetch and error output can be multi-line; cap + scroll it so it
                // never spills past the fixed-height card.
                div()
                    .id("git-output")
                    .overflow_y_scroll()
                    .max_h(px(120.))
                    .text_size(px(12.))
                    .font_family("monospace")
                    .text_color(if busy {
                        theme::accent()
                    } else {
                        theme::muted()
                    })
                    .child(self.git_output.clone()),
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
                cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.close_git(cx)),
            )
            .child(card)
    }

    /// A git overlay action button; dimmed + inert while a git op is running.
    pub(crate) fn git_btn(
        &self,
        cx: &mut Context<Self>,
        label: &str,
        busy: bool,
        handler: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> Div {
        let mut b = ghost_btn(label);
        if busy {
            b = b.opacity(0.5);
        } else {
            b = b.on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, _e: &MouseUpEvent, _w, cx| handler(this, cx)),
            );
        }
        b
    }
}

#[cfg(test)]
mod cov_tests {
    use super::*;
    use crate::test_support::{app_on_temp, temp_collection};

    /// A `git::Status` with the given branch/ahead/behind and a set of changed
    /// files (each `(code, path)`), to drive the chip + overlay render branches
    /// without touching a real repo.
    fn fake_status(branch: &str, ahead: u32, behind: u32, files: &[(&str, &str)]) -> git::Status {
        git::Status {
            branch: branch.to_string(),
            ahead,
            behind,
            files: files
                .iter()
                .map(|(c, p)| git::FileEntry {
                    code: c.to_string(),
                    path: p.to_string(),
                })
                .collect(),
        }
    }

    // ── git_chip ──────────────────────────────────────────────────────────

    /// Not a git repo → no chip (the early `return None`).
    #[gpui::test]
    fn git_chip_none_when_not_a_repo(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.git_repo = false;
            app.git_status = None;
            assert!(app.git_chip(cx).is_none());
        });
    }

    /// A repo with no parsed status still shows a chip (the "git" fallback label).
    #[gpui::test]
    fn git_chip_some_with_fallback_label(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.git_repo = true;
            app.git_status = None;
            assert!(app.git_chip(cx).is_some());
        });
    }

    /// A repo with a parsed, clean status on a named branch yields a chip
    /// (covers the `Some(st)` label arm with ahead/behind/dirty all zero).
    #[gpui::test]
    fn git_chip_clean_branch(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.git_repo = true;
            app.git_status = Some(fake_status("main", 0, 0, &[]));
            assert!(app.git_chip(cx).is_some());
        });
    }

    /// Ahead + behind + dirty all set exercises every `push_str` branch in the
    /// chip label builder.
    #[gpui::test]
    fn git_chip_ahead_behind_dirty(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.git_repo = true;
            app.git_status = Some(fake_status("feature", 2, 3, &[(" M", "a.bru")]));
            // is_dirty() is true because files is non-empty.
            assert!(app.git_status.as_ref().unwrap().is_dirty());
            assert!(app.git_chip(cx).is_some());
        });
    }

    // ── git_overlay ───────────────────────────────────────────────────────
    //
    // git_overlay() embeds entity handles (the commit-message editor + output
    // viewer), so it must be exercised through a real render pass (template 3);
    // calling it directly and dropping the returned Div leaks those handles and
    // trips gpui's deterministic test scheduler.

    /// Overlay with a parsed status, ahead/behind set, and one of each file
    /// status code so all three color branches (untracked `?`, unstaged leading
    /// space, staged) are taken.
    #[gpui::test]
    fn git_overlay_with_status_and_files(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, _w, cx| {
                app.git_repo = true;
                app.git_status = Some(fake_status(
                    "main",
                    1,
                    1,
                    &[
                        ("??", "new.bru"),
                        (" M", "changed.bru"),
                        ("M ", "staged.bru"),
                    ],
                ));
                app.git_busy = false;
                app.git_confirm_discard = false;
                app.open_git(cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _w, _cx| assert!(app.git_open))
            .unwrap();
    }

    /// Overlay with no parsed status takes the "Could not read git status" /
    /// "Status unavailable." branches.
    #[gpui::test]
    fn git_overlay_without_status(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, _w, cx| {
                app.git_repo = true;
                app.git_status = None;
                app.git_busy = false;
                app.open_git(cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _w, _cx| assert!(app.git_open))
            .unwrap();
    }

    /// Busy + armed-discard exercises the dimmed `git_btn` path, the
    /// "Confirm discard" label, and the busy-colored output region.
    #[gpui::test]
    fn git_overlay_busy_and_confirm_discard(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, _w, cx| {
                app.git_repo = true;
                app.git_status = Some(fake_status("main", 0, 0, &[]));
                app.open_git(cx);
            })
            .unwrap();
        // open_git resets git_confirm_discard, so arm it + busy after opening,
        // then re-park to rebuild the overlay with those branches.
        window
            .update(cx, |app, _w, cx| {
                app.git_busy = true;
                app.git_confirm_discard = true;
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _w, _cx| {
                assert!(app.git_open);
                assert!(app.git_confirm_discard);
            })
            .unwrap();
    }

    // ── status_bar / tab_strip / home_screen via a real window ────────────

    /// Drive the full status bar through render with the git chip open: set a
    /// repo+status, open a request (populates `status`), open the git overlay,
    /// and re-park so every status-bar/overlay builder runs.
    #[gpui::test]
    fn status_bar_and_overlay_render_in_window(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let req = tc.dir.join("Repository Info.bru");
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, _w, cx| {
                app.open_request(req, cx);
                app.git_repo = true;
                app.git_status = Some(fake_status(
                    "main",
                    1,
                    0,
                    &[("??", "x.bru"), (" M", "y.bru")],
                ));
                app.git_open = true;
            })
            .unwrap();
        // Re-park so render rebuilds: status_bar + git_chip(Some) + git_overlay.
        cx.run_until_parked();
    }

    /// The tab strip with an active, dirty tab renders the draft dot + underline
    /// branches; the home screen with recents renders its recent list.
    #[gpui::test]
    fn tab_strip_active_dirty_branches(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let req = tc.dir.join("Repository Info.bru");
        app.update(cx, |app, cx| {
            app.open_request(req.clone(), cx);
            app.active = Some(0);
            app.dirty.insert(req);
            // Both the active-underline and dirty-dot branches now apply.
            let _ = app.tab_strip(cx);
        });
    }
}
