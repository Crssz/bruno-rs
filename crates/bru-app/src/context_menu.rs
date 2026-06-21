//! Sidebar navigation, right-click context menu, rename and delete.

use crate::*;
use gpui::prelude::*;

impl BruApp {
    pub(crate) fn go_home(&mut self, cx: &mut Context<Self>) {
        self.home = !self.home;
        cx.notify();
    }

    /// Create a new request file in the collection root and open it.
    pub(crate) fn new_request(&mut self, cx: &mut Context<Self>) {
        let dir = self.dir.clone();
        self.new_request_in(&dir, cx);
    }

    /// Create a new request file in `dir` (a folder) and open it.
    pub(crate) fn new_request_in(&mut self, dir: &Path, cx: &mut Context<Self>) {
        let mut n = 1;
        let mut path = dir.join("New Request.bru");
        while path.exists() {
            n += 1;
            path = dir.join(format!("New Request {n}.bru"));
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("New Request");
        let body = format!(
            "meta {{\n  name: {stem}\n  type: http\n  seq: 1\n}}\n\nget {{\n  url: \n  body: none\n  auth: none\n}}\n"
        );
        if std::fs::write(&path, body).is_ok() {
            self.reload_collection(cx);
            self.open_request(path, cx);
        }
        cx.notify();
    }

    /// Re-read the on-disk collection tree into the sidebar.
    pub(crate) fn reload_collection(&mut self, cx: &mut Context<Self>) {
        if let Ok(tree) = bru_lang::load_collection(&self.dir) {
            self.collection = Some(tree);
        }
        self.refresh_vars();
        cx.notify();
    }

    // ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ sidebar context menu ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬
    pub(crate) fn open_ctx_menu(
        &mut self,
        target: PathBuf,
        is_dir: bool,
        name: String,
        pos: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.ctx_menu = Some(CtxMenu {
            target,
            is_dir,
            root: false,
            name,
            pos,
        });
        cx.notify();
    }

    /// Open the collection-root context menu (anchored at the click point).
    pub(crate) fn open_root_menu(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let name = self
            .collection
            .as_ref()
            .map(|c| c.name.clone())
            .unwrap_or_default();
        self.ctx_menu = Some(CtxMenu {
            target: self.dir.clone(),
            is_dir: true,
            root: true,
            name,
            pos,
        });
        cx.notify();
    }

    pub(crate) fn close_ctx_menu(&mut self, cx: &mut Context<Self>) {
        if self.ctx_menu.take().is_some() {
            cx.notify();
        }
    }

    /// Close every open tab whose path is `path` or sits under it (for a folder).
    pub(crate) fn close_tabs_under(&mut self, path: &Path) {
        self.tabs.retain(|t| !t.path.starts_with(path));
        if self.tabs.is_empty() {
            self.active = None;
        } else {
            let i = self.active.unwrap_or(0).min(self.tabs.len() - 1);
            self.active = Some(i);
        }
    }

    /// Duplicate the menu's target (a `.bru` file or a folder) alongside itself.
    pub(crate) fn ctx_duplicate(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.ctx_menu.take() else {
            return;
        };
        let Some(parent) = menu.target.parent() else {
            return;
        };
        if menu.is_dir {
            let mut dest = parent.join(format!("{} copy", menu.name));
            let mut n = 1;
            while dest.exists() {
                n += 1;
                dest = parent.join(format!("{} copy {n}", menu.name));
            }
            let _ = copy_dir_recursive(&menu.target, &dest);
        } else {
            let mut dest = parent.join(format!("{} copy.bru", menu.name));
            let mut n = 1;
            while dest.exists() {
                n += 1;
                dest = parent.join(format!("{} copy {n}.bru", menu.name));
            }
            let _ = std::fs::copy(&menu.target, &dest);
        }
        self.reload_collection(cx);
    }

    /// Run the menu's target: a whole folder, or open + nothing for a request
    /// (the request is opened so the user can Send it).
    pub(crate) fn ctx_run(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.ctx_menu.take() else {
            return;
        };
        if menu.is_dir {
            self.run_folder(menu.target, cx);
        } else {
            self.open_request(menu.target, cx);
        }
        cx.notify();
    }

    /// Run a single request from its row: open it, then send.
    pub(crate) fn ctx_run_request(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.ctx_menu.take() else {
            return;
        };
        self.open_request(menu.target, cx);
        self.send(cx);
        cx.notify();
    }

    /// Generate code (a curl command) for the menu's request and copy it to the
    /// clipboard. Opens the request first so its latest edits are reflected.
    pub(crate) fn ctx_generate_code(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.ctx_menu.take() else {
            return;
        };
        let target = menu.target.clone();
        self.open_request(menu.target, cx);
        // Only generate when the just-opened tab IS the requested one (open_request
        // leaves the prior tab active if the file is unreadable/unparseable).
        let curl = self
            .active_tab()
            .filter(|t| t.path == target)
            .map(|tab| to_curl(tab, cx));
        self.status = match curl {
            Some(curl) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(curl));
                "Copied curl to clipboard".into()
            }
            None => "Couldn't open request".into(),
        };
        cx.notify();
    }

    /// Copy the menu target onto the sidebar clipboard for a later Paste.
    pub(crate) fn ctx_copy(&mut self, cx: &mut Context<Self>) {
        if let Some(menu) = self.ctx_menu.take() {
            self.clipboard_item = Some((menu.target, menu.is_dir));
            self.status = "Copied to sidebar clipboard".into();
            cx.notify();
        }
    }

    /// Paste the clipboard item into the menu's folder (dedup name).
    pub(crate) fn ctx_paste(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.ctx_menu.take() else {
            return;
        };
        let dest_dir = if menu.is_dir {
            menu.target.clone()
        } else {
            menu.target
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or(menu.target)
        };
        let Some((src, is_dir)) = self.clipboard_item.clone() else {
            return;
        };
        let stem = src
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Item")
            .to_string();
        if is_dir {
            let mut dest = dest_dir.join(&stem);
            let mut n = 1;
            while dest.exists() {
                n += 1;
                dest = dest_dir.join(format!("{stem} {n}"));
            }
            let _ = copy_dir_recursive(&src, &dest);
        } else {
            let mut dest = dest_dir.join(format!("{stem}.bru"));
            let mut n = 1;
            while dest.exists() {
                n += 1;
                dest = dest_dir.join(format!("{stem} {n}.bru"));
            }
            let _ = std::fs::copy(&src, &dest);
        }
        self.reload_collection(cx);
    }

    /// Reveal the menu target in the OS file manager.
    pub(crate) fn ctx_reveal(&mut self, cx: &mut Context<Self>) {
        if let Some(menu) = self.ctx_menu.take() {
            reveal_in_file_manager(&menu.target);
            cx.notify();
        }
    }

    /// Open a folder's `folder.bru` as a tab (folder-level headers/auth/vars/
    /// scripts), creating a minimal one if absent.
    pub(crate) fn ctx_folder_settings(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.ctx_menu.take() else {
            return;
        };
        let bru = menu.target.join("folder.bru");
        if !bru.exists() {
            let name = menu.name;
            let _ = std::fs::write(&bru, format!("meta {{\n  name: {name}\n  seq: 1\n}}\n"));
            self.reload_collection(cx);
        }
        self.open_request(bru, cx);
        cx.notify();
    }

    /// Open the collection's `collection.bru` settings as a tab.
    pub(crate) fn open_collection_settings(&mut self, cx: &mut Context<Self>) {
        let bru = self.dir.join("collection.bru");
        if !bru.exists() {
            let _ = std::fs::write(&bru, "meta {\n  name: Collection\n}\n");
            self.reload_collection(cx);
        }
        self.open_request(bru, cx);
        cx.notify();
    }

    /// Move the menu's request up/down among its siblings (rewrites meta.seq).
    pub(crate) fn ctx_move(&mut self, delta: i64, cx: &mut Context<Self>) {
        let Some(menu) = self.ctx_menu.take() else {
            return;
        };
        let path = menu.target;
        let Some(dir) = path.parent() else { return };
        // Sibling requests in display order, via the loaded tree.
        let mut reqs: Vec<(PathBuf, i64, String)> = {
            let Some(tree) = &self.collection else { return };
            let folder = if dir == self.dir {
                Some(&tree.root)
            } else {
                find_folder(&tree.root, dir)
            };
            let Some(folder) = folder else { return };
            folder
                .requests
                .iter()
                .map(|r| (r.path.clone(), r.seq.unwrap_or(i64::MAX), r.name.clone()))
                .collect()
        };
        reqs.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| a.2.to_lowercase().cmp(&b.2.to_lowercase()))
        });
        let Some(idx) = reqs.iter().position(|(p, _, _)| *p == path) else {
            return;
        };
        let target = (idx as i64 + delta).clamp(0, reqs.len() as i64 - 1) as usize;
        if target == idx {
            return;
        }
        let item = reqs.remove(idx);
        reqs.insert(target, item);
        for (i, (p, _, _)) in reqs.iter().enumerate() {
            set_seq_in_file(p, (i + 1) as i64);
        }
        self.reload_collection(cx);
    }

    pub(crate) fn start_rename(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.ctx_menu.take() else {
            return;
        };
        let input = cx.new(|cx| CodeEditor::single_line(cx, &menu.name));
        self.rename = Some(RenameState {
            target: menu.target,
            is_dir: menu.is_dir,
            input,
        });
        cx.notify();
    }

    pub(crate) fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.rename = None;
        cx.notify();
    }

    pub(crate) fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.rename.take() else {
            return;
        };
        let new_name = state.input.read(cx).text().trim().to_string();
        let Some(parent) = state.target.parent().map(Path::to_path_buf) else {
            return;
        };
        if new_name.is_empty() {
            return;
        }
        if state.is_dir {
            let dest = parent.join(&new_name);
            if dest != state.target && std::fs::rename(&state.target, &dest).is_ok() {
                self.close_tabs_under(&state.target);
            }
        } else {
            // Rewrite meta.name so the tree label follows, then move the file.
            let dest = parent.join(format!("{new_name}.bru"));
            if let Ok(text) = std::fs::read_to_string(&state.target) {
                if let Ok(mut file) = bru_lang::parse(&text) {
                    edit::set_meta_name(&mut file, &new_name);
                    let _ = std::fs::write(&state.target, bru_lang::serialize(&file));
                }
            }
            if dest != state.target && std::fs::rename(&state.target, &dest).is_ok() {
                // Re-point any open tab at the new path.
                for t in &mut self.tabs {
                    if t.path == state.target {
                        t.path = dest.clone();
                    }
                }
            }
        }
        self.reload_collection(cx);
    }

    pub(crate) fn start_delete(&mut self, cx: &mut Context<Self>) {
        let Some(menu) = self.ctx_menu.take() else {
            return;
        };
        self.confirm_delete = Some((menu.target, menu.is_dir, menu.name));
        cx.notify();
    }

    pub(crate) fn cancel_delete(&mut self, cx: &mut Context<Self>) {
        self.confirm_delete = None;
        cx.notify();
    }

    pub(crate) fn commit_delete(&mut self, cx: &mut Context<Self>) {
        let Some((target, is_dir, _)) = self.confirm_delete.take() else {
            return;
        };
        let ok = if is_dir {
            std::fs::remove_dir_all(&target).is_ok()
        } else {
            std::fs::remove_file(&target).is_ok()
        };
        if ok {
            self.close_tabs_under(&target);
        }
        self.reload_collection(cx);
    }

    /// The anchored right-click menu over a sidebar entry.
    pub(crate) fn ctx_menu_overlay(&self, cx: &mut Context<Self>) -> Div {
        let Some(menu) = &self.ctx_menu else {
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
        let mut card = div()
            .id("ctx-menu")
            .absolute()
            .left(menu.pos.x)
            .top(menu.pos.y)
            .occlude()
            .flex()
            .flex_col()
            .py_1()
            .w(px(180.))
            .max_h(px(420.))
            .overflow_y_scroll()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2());
        if menu.root {
            // Collection root: a reduced set (no rename/clone/delete).
            card = card
                .child(item("New Request").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                        if let Some(m) = this.ctx_menu.take() {
                            this.new_request_in(&m.target, cx);
                        }
                    }),
                ))
                .child(item("New Folder").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                        if let Some(m) = this.ctx_menu.take() {
                            this.new_folder_in(&m.target, cx);
                        }
                    }),
                ))
                .child(item("Run").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_run(cx)),
                ))
                .child(item("Settings").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                        this.ctx_menu = None;
                        this.open_collection_settings(cx);
                    }),
                ))
                .child(item("Reveal in Explorer").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_reveal(cx)),
                ));
        } else if menu.is_dir {
            card = card
                .child(item("New Request").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                        if let Some(m) = this.ctx_menu.take() {
                            this.new_request_in(&m.target, cx);
                        }
                    }),
                ))
                .child(item("New Folder").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| {
                        if let Some(m) = this.ctx_menu.take() {
                            this.new_folder_in(&m.target, cx);
                        }
                    }),
                ))
                .child(item("Run Folder").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_run(cx)),
                ))
                .child(item("Settings").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_folder_settings(cx)),
                ));
            if self.clipboard_item.is_some() {
                card = card.child(item("Paste").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_paste(cx)),
                ));
            }
        } else {
            card = card
                .child(item("Open").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_run(cx)),
                ))
                .child(item("Run").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_run_request(cx)),
                ))
                .child(item("Generate Code").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_generate_code(cx)),
                ))
                .child(item("Move Up").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_move(-1, cx)),
                ))
                .child(item("Move Down").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_move(1, cx)),
                ));
        }
        if !menu.root {
            card = card
                .child(item("Rename").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.start_rename(cx)),
                ))
                .child(item("Clone").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_duplicate(cx)),
                ))
                .child(item("Copy").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_copy(cx)),
                ))
                .child(item("Reveal in Explorer").on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.ctx_reveal(cx)),
                ))
                .child(item("Delete").text_color(theme::red()).on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.start_delete(cx)),
                ));
        }
        // Full-screen transparent catcher: any click outside closes the menu.
        div()
            .absolute()
            .inset_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _e: &MouseDownEvent, _w, cx| this.close_ctx_menu(cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _e: &MouseDownEvent, _w, cx| this.close_ctx_menu(cx)),
            )
            .child(card)
    }

    /// The inline rename prompt (modal).
    pub(crate) fn rename_overlay(&self, cx: &mut Context<Self>) -> Div {
        let Some(state) = &self.rename else {
            return div();
        };
        let title = if state.is_dir {
            "Rename Folder"
        } else {
            "Rename Request"
        };
        let card = div()
            .w(px(420.))
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
                    .child(title),
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
                    .text_size(px(13.))
                    .child(state.input.clone()),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(ghost_btn("Cancel").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.cancel_rename(cx)),
                    ))
                    .child(solid_btn("Rename").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.commit_rename(cx)),
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

    /// The delete-confirmation modal.
    pub(crate) fn delete_overlay(&self, cx: &mut Context<Self>) -> Div {
        let Some((_, is_dir, name)) = &self.confirm_delete else {
            return div();
        };
        let kind = if *is_dir { "folder" } else { "request" };
        let msg = format!("Delete {kind} \u{201c}{name}\u{201d}? This cannot be undone.");
        let card = div()
            .w(px(420.))
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
                    .child("Confirm Delete"),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::subtext())
                    .child(msg),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap_2()
                    .child(ghost_btn("Cancel").on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.cancel_delete(cx)),
                    ))
                    .child(solid_btn("Delete").bg(theme::red()).on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _e: &MouseUpEvent, _w, cx| this.commit_delete(cx)),
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
}

#[cfg(test)]
mod cov_tests {
    use super::*;
    use crate::test_support::{app_on_temp, temp_collection};

    /// A throwaway anchor point for menus/overlays.
    fn pos() -> Point<Pixels> {
        gpui::point(px(10.), px(20.))
    }

    #[gpui::test]
    fn go_home_toggles(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            assert!(!app.home);
            app.go_home(cx);
            assert!(app.home);
            app.go_home(cx);
            assert!(!app.home);
        });
    }

    #[gpui::test]
    fn new_request_creates_and_opens_file(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let dir = tc.dir.clone();
        app.update(cx, |app, cx| app.new_request(cx));
        // The first free name is "New Request 3.bru" (the sample already ships
        // New Request.bru + New Request 2.bru), so a fresh file must appear and
        // a tab must be open for it.
        app.update(cx, |app, _| {
            assert!(!app.tabs.is_empty());
            let opened = app.active_tab().map(|t| t.path.clone()).unwrap();
            assert!(opened.starts_with(&dir));
            assert!(opened.exists());
            assert!(opened.extension().and_then(|e| e.to_str()) == Some("bru"));
        });
    }

    #[gpui::test]
    fn new_request_in_subfolder(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let folder = tc.dir.join("Repository");
        app.update(cx, |app, cx| app.new_request_in(&folder, cx));
        let made = folder.join("New Request.bru");
        assert!(made.exists());
        // A second call in the same folder must dedup the name.
        app.update(cx, |app, cx| app.new_request_in(&folder, cx));
        assert!(folder.join("New Request 2.bru").exists());
    }

    #[gpui::test]
    fn reload_collection_repopulates(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.collection = None;
            app.reload_collection(cx);
            assert!(app.collection.is_some());
        });
    }

    #[gpui::test]
    fn open_and_close_ctx_menu(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let target = tc.dir.join("Repository Info.bru");
        app.update(cx, |app, cx| {
            app.open_ctx_menu(target.clone(), false, "Repository Info".into(), pos(), cx);
        });
        app.update(cx, |app, _| {
            let m = app.ctx_menu.as_ref().unwrap();
            assert!(!m.is_dir);
            assert!(!m.root);
            assert!(m.target == target);
        });
        app.update(cx, |app, cx| app.close_ctx_menu(cx));
        app.update(cx, |app, _| assert!(app.ctx_menu.is_none()));
        // Closing again is a no-op (the take() guard).
        app.update(cx, |app, cx| app.close_ctx_menu(cx));
    }

    #[gpui::test]
    fn open_root_menu_marks_root(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let dir = tc.dir.clone();
        app.update(cx, |app, cx| app.open_root_menu(pos(), cx));
        app.update(cx, |app, _| {
            let m = app.ctx_menu.as_ref().unwrap();
            assert!(m.root);
            assert!(m.is_dir);
            assert!(m.target == dir);
        });
    }

    #[gpui::test]
    fn close_tabs_under_folder_and_file(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let root_req = tc.dir.join("Repository Info.bru");
        let sub_req = tc.dir.join("Repository").join("Create Issue.bru");
        app.update(cx, |app, cx| {
            app.open_request(root_req.clone(), cx);
            app.open_request(sub_req.clone(), cx);
        });
        app.update(cx, |app, _| assert_eq!(app.tabs.len(), 2));
        // Closing the folder drops the sub tab but keeps the root one.
        app.update(cx, |app, _| {
            app.close_tabs_under(&tc.dir.join("Repository"))
        });
        app.update(cx, |app, _| {
            assert_eq!(app.tabs.len(), 1);
            assert!(app.tabs[0].path == root_req);
            assert!(app.active.is_some());
        });
        // Closing under the remaining tab empties the set and clears active.
        app.update(cx, |app, _| app.close_tabs_under(&root_req));
        app.update(cx, |app, _| {
            assert!(app.tabs.is_empty());
            assert!(app.active.is_none());
        });
    }

    #[gpui::test]
    fn ctx_duplicate_file(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let target = tc.dir.join("Repository Info.bru");
        app.update(cx, |app, cx| {
            app.open_ctx_menu(target, false, "Repository Info".into(), pos(), cx);
            app.ctx_duplicate(cx);
        });
        assert!(tc.dir.join("Repository Info copy.bru").exists());
    }

    #[gpui::test]
    fn ctx_duplicate_dir(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let target = tc.dir.join("Repository");
        app.update(cx, |app, cx| {
            app.open_ctx_menu(target, true, "Repository".into(), pos(), cx);
            app.ctx_duplicate(cx);
        });
        let copy = tc.dir.join("Repository copy");
        assert!(copy.is_dir());
        assert!(copy.join("folder.bru").exists());
    }

    #[gpui::test]
    fn ctx_duplicate_without_menu_is_noop(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| app.ctx_duplicate(cx));
        app.update(cx, |app, _| assert!(app.ctx_menu.is_none()));
    }

    #[gpui::test]
    fn ctx_run_file_opens_request(cx: &mut gpui::TestAppContext) {
        // ctx_run on a non-dir target just opens the request (no worker thread).
        let (app, tc) = app_on_temp(cx);
        let target = tc.dir.join("Repository Info.bru");
        app.update(cx, |app, cx| {
            app.open_ctx_menu(target.clone(), false, "Repository Info".into(), pos(), cx);
            app.ctx_run(cx);
        });
        app.update(cx, |app, _| {
            assert!(app.ctx_menu.is_none());
            assert!(app.active_tab().map(|t| t.path.clone()) == Some(target));
        });
    }

    #[gpui::test]
    fn ctx_generate_code_copies_curl(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let target = tc.dir.join("Repository Info.bru");
        app.update(cx, |app, cx| {
            app.open_ctx_menu(target, false, "Repository Info".into(), pos(), cx);
            app.ctx_generate_code(cx);
        });
        app.update(cx, |app, _| {
            assert!(app.ctx_menu.is_none());
            assert!(app.status == "Copied curl to clipboard");
        });
    }

    #[gpui::test]
    fn ctx_copy_then_paste_file(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let src = tc.dir.join("Repository Info.bru");
        // Copy a file onto the sidebar clipboard.
        app.update(cx, |app, cx| {
            app.open_ctx_menu(src.clone(), false, "Repository Info".into(), pos(), cx);
            app.ctx_copy(cx);
        });
        app.update(cx, |app, _| {
            assert!(app.clipboard_item.is_some());
            assert!(app.status == "Copied to sidebar clipboard");
        });
        // Paste into the Repository folder (a dir target -> paste into it).
        let folder = tc.dir.join("Repository");
        app.update(cx, |app, cx| {
            app.open_ctx_menu(folder.clone(), true, "Repository".into(), pos(), cx);
            app.ctx_paste(cx);
        });
        assert!(folder.join("Repository Info.bru").exists());
    }

    #[gpui::test]
    fn ctx_paste_dir_into_file_target(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        // Copy the Repository folder, then paste with a FILE target so the dest
        // resolves to the file's parent dir.
        let folder = tc.dir.join("Repository");
        app.update(cx, |app, cx| {
            app.open_ctx_menu(folder, true, "Repository".into(), pos(), cx);
            app.ctx_copy(cx);
        });
        let file_target = tc.dir.join("Repository Info.bru");
        app.update(cx, |app, cx| {
            app.open_ctx_menu(file_target, false, "Repository Info".into(), pos(), cx);
            app.ctx_paste(cx);
        });
        // Pasting the folder back into root dedups to "Repository 2".
        assert!(tc.dir.join("Repository 2").is_dir());
    }

    #[gpui::test]
    fn ctx_folder_settings_creates_and_opens(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        // Make a folder with no folder.bru so ctx_folder_settings must scaffold one.
        let folder = tc.dir.join("FreshFolder");
        std::fs::create_dir_all(&folder).unwrap();
        app.update(cx, |app, cx| app.reload_collection(cx));
        app.update(cx, |app, cx| {
            app.open_ctx_menu(folder.clone(), true, "FreshFolder".into(), pos(), cx);
            app.ctx_folder_settings(cx);
        });
        let bru = folder.join("folder.bru");
        assert!(bru.exists());
        app.update(cx, |app, _| {
            assert!(app.active_tab().map(|t| t.path.clone()) == Some(bru.clone()));
        });
        // A second call (folder.bru now present) just re-opens the existing tab.
        app.update(cx, |app, cx| {
            app.open_ctx_menu(folder.clone(), true, "FreshFolder".into(), pos(), cx);
            app.ctx_folder_settings(cx);
        });
    }

    #[gpui::test]
    fn open_collection_settings_scaffolds_when_missing(cx: &mut gpui::TestAppContext) {
        // The sample ships a collection.bru, so remove it to hit the create path.
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let _ = std::fs::remove_file(dir.join("collection.bru"));
        let app = crate::test_support::build_app(cx, dir.clone());
        app.update(cx, |app, cx| app.open_collection_settings(cx));
        let bru = dir.join("collection.bru");
        assert!(bru.exists());
        app.update(cx, |app, _| {
            assert!(app.active_tab().map(|t| t.path.clone()) == Some(bru));
        });
    }

    #[gpui::test]
    fn ctx_move_reorders_siblings(cx: &mut gpui::TestAppContext) {
        // Build a clean folder with three seq'd requests so move is deterministic.
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let folder = dir.join("Ordered");
        std::fs::create_dir_all(&folder).unwrap();
        for (i, n) in [(1, "A"), (2, "B"), (3, "C")] {
            let body = format!(
                "meta {{\n  name: {n}\n  type: http\n  seq: {i}\n}}\n\nget {{\n  url: \n}}\n"
            );
            std::fs::write(folder.join(format!("{n}.bru")), body).unwrap();
        }
        let app = crate::test_support::build_app(cx, dir.clone());
        // Move "A" down: it should swap with "B" (seq rewritten on disk).
        let a = folder.join("A.bru");
        app.update(cx, |app, cx| {
            app.open_ctx_menu(a.clone(), false, "A".into(), pos(), cx);
            app.ctx_move(1, cx);
        });
        let a_text = std::fs::read_to_string(&a).unwrap();
        // A now has seq 2 (was 1) after moving down one slot.
        assert!(a_text.contains("seq: 2"));
        // Moving the top item up is clamped (no panic, target == idx early return).
        app.update(cx, |app, cx| {
            let b = folder.join("B.bru");
            app.open_ctx_menu(b, false, "B".into(), pos(), cx);
            app.ctx_move(-5, cx);
        });
    }

    #[gpui::test]
    fn rename_file_round_trip(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let target = tc.dir.join("Repository Info.bru");
        // Open the request first so commit_rename re-points the open tab.
        app.update(cx, |app, cx| app.open_request(target.clone(), cx));
        app.update(cx, |app, cx| {
            app.open_ctx_menu(target.clone(), false, "Repository Info".into(), pos(), cx);
            app.start_rename(cx);
        });
        app.update(cx, |app, _| assert!(app.rename.is_some()));
        // Set the rename input's text, then commit.
        app.update(cx, |app, cx| {
            let input = app.rename.as_ref().unwrap().input.clone();
            input.update(cx, |ed, cx| {
                ed.set_text("Renamed Info", crate::editor::Lang::Plain, cx)
            });
            app.commit_rename(cx);
        });
        let dest = tc.dir.join("Renamed Info.bru");
        assert!(dest.exists());
        assert!(!target.exists());
        app.update(cx, |app, _| {
            // The open tab now points at the new path.
            assert!(app.tabs.iter().any(|t| t.path == dest));
            assert!(app.rename.is_none());
        });
    }

    #[gpui::test]
    fn rename_dir_round_trip(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let target = tc.dir.join("Repository");
        app.update(cx, |app, cx| {
            app.open_ctx_menu(target.clone(), true, "Repository".into(), pos(), cx);
            app.start_rename(cx);
            let input = app.rename.as_ref().unwrap().input.clone();
            input.update(cx, |ed, cx| {
                ed.set_text("Repo2", crate::editor::Lang::Plain, cx)
            });
            app.commit_rename(cx);
        });
        assert!(tc.dir.join("Repo2").is_dir());
        assert!(!target.exists());
    }

    #[gpui::test]
    fn rename_empty_name_is_noop(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let target = tc.dir.join("Repository Info.bru");
        app.update(cx, |app, cx| {
            app.open_ctx_menu(target.clone(), false, "Repository Info".into(), pos(), cx);
            app.start_rename(cx);
            let input = app.rename.as_ref().unwrap().input.clone();
            input.update(cx, |ed, cx| {
                ed.set_text("   ", crate::editor::Lang::Plain, cx)
            });
            app.commit_rename(cx);
        });
        // Empty (trimmed) name: the original file is untouched.
        assert!(target.exists());
    }

    #[gpui::test]
    fn cancel_rename_clears_state(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let target = tc.dir.join("Repository Info.bru");
        app.update(cx, |app, cx| {
            app.open_ctx_menu(target, false, "Repository Info".into(), pos(), cx);
            app.start_rename(cx);
            app.cancel_rename(cx);
        });
        app.update(cx, |app, _| assert!(app.rename.is_none()));
    }

    #[gpui::test]
    fn delete_file_confirm_flow(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let target = tc.dir.join("Repository Info.bru");
        app.update(cx, |app, cx| app.open_request(target.clone(), cx));
        app.update(cx, |app, cx| {
            app.open_ctx_menu(target.clone(), false, "Repository Info".into(), pos(), cx);
            app.start_delete(cx);
        });
        app.update(cx, |app, _| assert!(app.confirm_delete.is_some()));
        app.update(cx, |app, cx| app.commit_delete(cx));
        assert!(!target.exists());
        app.update(cx, |app, _| {
            assert!(app.confirm_delete.is_none());
            // The open tab for the deleted file was closed.
            assert!(!app.tabs.iter().any(|t| t.path == target));
        });
    }

    #[gpui::test]
    fn delete_dir_and_cancel(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let target = tc.dir.join("Repository");
        // Cancel path first: arms then clears, folder untouched.
        app.update(cx, |app, cx| {
            app.open_ctx_menu(target.clone(), true, "Repository".into(), pos(), cx);
            app.start_delete(cx);
            app.cancel_delete(cx);
        });
        app.update(cx, |app, _| assert!(app.confirm_delete.is_none()));
        assert!(target.is_dir());
        // Now actually delete the folder.
        app.update(cx, |app, cx| {
            app.open_ctx_menu(target.clone(), true, "Repository".into(), pos(), cx);
            app.start_delete(cx);
            app.commit_delete(cx);
        });
        assert!(!target.exists());
    }

    // —— windowed render: exercise the overlay view-builders ——

    #[gpui::test]
    fn renders_root_ctx_menu_overlay(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        window
            .update(cx, |app, _w, cx| app.open_root_menu(pos(), cx))
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _w, _cx| assert!(app.ctx_menu.is_some()))
            .unwrap();
    }

    #[gpui::test]
    fn renders_dir_ctx_menu_with_paste(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let folder = dir.join("Repository");
        let src = dir.join("Repository Info.bru");
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir.clone()));
        cx.run_until_parked();
        // Put something on the clipboard so the dir menu shows the Paste item.
        window
            .update(cx, |app, _w, cx| {
                app.open_ctx_menu(src, false, "Repository Info".into(), pos(), cx);
                app.ctx_copy(cx);
                app.open_ctx_menu(folder, true, "Repository".into(), pos(), cx);
            })
            .unwrap();
        cx.run_until_parked();
    }

    #[gpui::test]
    fn renders_file_ctx_menu_overlay(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let target = dir.join("Repository Info.bru");
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir.clone()));
        cx.run_until_parked();
        window
            .update(cx, |app, _w, cx| {
                app.open_ctx_menu(target, false, "Repository Info".into(), pos(), cx);
            })
            .unwrap();
        cx.run_until_parked();
    }

    #[gpui::test]
    fn renders_rename_overlay(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let target = dir.join("Repository Info.bru");
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir.clone()));
        cx.run_until_parked();
        window
            .update(cx, |app, _w, cx| {
                app.open_ctx_menu(target, false, "Repository Info".into(), pos(), cx);
                app.start_rename(cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _w, _cx| assert!(app.rename.is_some()))
            .unwrap();
    }

    #[gpui::test]
    fn renders_delete_overlay(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let target = dir.join("Repository");
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir.clone()));
        cx.run_until_parked();
        window
            .update(cx, |app, _w, cx| {
                app.open_ctx_menu(target, true, "Repository".into(), pos(), cx);
                app.start_delete(cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _w, _cx| assert!(app.confirm_delete.is_some()))
            .unwrap();
    }
}
