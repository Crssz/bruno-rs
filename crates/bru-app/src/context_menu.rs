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
