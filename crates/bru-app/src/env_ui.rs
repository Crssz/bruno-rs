//! Environment-manager state and CRUD operations.

use crate::*;
use gpui::prelude::*;

impl BruApp {
    // ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ environment manager ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬
    /// The dir the env manager currently operates on (collection or globals).
    pub(crate) fn env_dir(&self) -> PathBuf {
        if self.env.as_ref().map(|e| e.global).unwrap_or(false) {
            globals_root()
        } else {
            self.dir.clone()
        }
    }

    pub(crate) fn env_build_rows(&self, name: &str, cx: &mut Context<Self>) -> Vec<EnvRowState> {
        let reveal = self.reveal_secrets;
        let dir = self.env_dir();
        envfs::load_env_rows(&dir, name)
            .into_iter()
            .map(|r| EnvRowState {
                name: cx.new(|cx| CodeEditor::single_line(cx, &r.name)),
                value: cx.new(|cx| {
                    if r.secret && !reveal {
                        CodeEditor::masked_line(cx, &r.value)
                    } else {
                        CodeEditor::single_line(cx, &r.value)
                    }
                }),
                enabled: r.enabled,
                secret: r.secret,
            })
            .collect()
    }

    pub(crate) fn env_collect_rows(&self, ed: &EnvEditor, cx: &App) -> Vec<envfs::EnvRow> {
        ed.rows
            .iter()
            .map(|r| envfs::EnvRow {
                name: r.name.read(cx).text().trim().to_string(),
                value: r.value.read(cx).text().to_string(),
                enabled: r.enabled,
                secret: r.secret,
            })
            .filter(|r| !r.name.is_empty())
            .collect()
    }

    pub(crate) fn env_open(&mut self, cx: &mut Context<Self>) {
        let names = envfs::scan_envs(&self.dir);
        let first = names.first().cloned().unwrap_or_default();
        let rows = self.env_build_rows(&first, cx);
        let rename = cx.new(|cx| CodeEditor::single_line(cx, &first));
        self.env = Some(EnvEditor {
            names,
            selected: first,
            rename,
            rows,
            error: None,
            global: false,
        });
        cx.notify();
    }

    /// Switch the env manager between Collection and Global scope.
    pub(crate) fn env_set_scope(&mut self, global: bool, cx: &mut Context<Self>) {
        if let Some(ed) = &mut self.env {
            ed.global = global;
        }
        let dir = self.env_dir();
        let names = envfs::scan_envs(&dir);
        let first = names.first().cloned().unwrap_or_default();
        let rows = self.env_build_rows(&first, cx);
        if let Some(ed) = &mut self.env {
            ed.names = names;
            ed.selected = first.clone();
            ed.rows = rows;
            ed.error = None;
            ed.rename.update(cx, |li, cx| li.set_line(&first, cx));
        }
        cx.notify();
    }

    pub(crate) fn env_close(&mut self, cx: &mut Context<Self>) {
        self.env = None;
        cx.notify();
    }

    pub(crate) fn env_select(&mut self, name: String, cx: &mut Context<Self>) {
        let rows = self.env_build_rows(&name, cx);
        if let Some(ed) = &mut self.env {
            ed.rename.update(cx, |li, cx| li.set_line(&name, cx));
            ed.selected = name;
            ed.rows = rows;
            ed.error = None;
        }
        cx.notify();
    }

    pub(crate) fn env_add_row(&mut self, cx: &mut Context<Self>) {
        let name = cx.new(|cx| CodeEditor::single_line(cx, ""));
        let value = cx.new(|cx| CodeEditor::single_line(cx, ""));
        if let Some(ed) = &mut self.env {
            ed.rows.push(EnvRowState {
                name,
                value,
                enabled: true,
                secret: false,
            });
        }
        cx.notify();
    }

    pub(crate) fn env_remove_row(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(ed) = &mut self.env {
            if i < ed.rows.len() {
                ed.rows.remove(i);
            }
        }
        cx.notify();
    }

    pub(crate) fn env_toggle_enabled(&mut self, i: usize, cx: &mut Context<Self>) {
        if let Some(ed) = &mut self.env {
            if let Some(r) = ed.rows.get_mut(i) {
                r.enabled = !r.enabled;
            }
        }
        cx.notify();
    }

    pub(crate) fn env_toggle_secret(&mut self, i: usize, cx: &mut Context<Self>) {
        let reveal = self.reveal_secrets;
        if let Some(ed) = &mut self.env {
            if let Some(r) = ed.rows.get_mut(i) {
                r.secret = !r.secret;
                let mask = r.secret && !reveal;
                r.value.update(cx, |ed, cx| ed.set_masked(mask, cx));
            }
        }
        cx.notify();
    }

    pub(crate) fn env_save(&mut self, cx: &mut Context<Self>) {
        let Some(ed) = self.env.as_ref() else { return };
        if ed.selected.is_empty() {
            if let Some(ed) = &mut self.env {
                ed.error = Some("Select or create an environment first".into());
            }
            cx.notify();
            return;
        }
        let rows = self.env_collect_rows(ed, cx);
        let sel = ed.selected.clone();
        let res = envfs::save_env(&self.env_dir(), &sel, &rows);
        if let Some(ed) = &mut self.env {
            ed.error = res.err();
        }
        self.refresh_vars();
        cx.notify();
    }

    pub(crate) fn env_new(&mut self, cx: &mut Context<Self>) {
        let dir = self.env_dir();
        let existing = envfs::scan_envs(&dir);
        let mut name = "New Environment".to_string();
        let mut n = 1;
        while existing.iter().any(|e| e == &name) {
            n += 1;
            name = format!("New Environment {n}");
        }
        match envfs::create_env(&dir, &name) {
            Ok(()) => {
                let names = envfs::scan_envs(&dir);
                let rows = self.env_build_rows(&name, cx);
                let rename = cx.new(|cx| CodeEditor::single_line(cx, &name));
                if let Some(ed) = &mut self.env {
                    ed.names = names;
                    ed.rename = rename;
                    ed.selected = name;
                    ed.rows = rows;
                    ed.error = None;
                }
            }
            Err(e) => {
                if let Some(ed) = &mut self.env {
                    ed.error = Some(e);
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn env_delete(&mut self, name: String, cx: &mut Context<Self>) {
        let dir = self.env_dir();
        let _ = envfs::delete_env(&dir, &name);
        let names = envfs::scan_envs(&dir);
        let reselect = self
            .env
            .as_ref()
            .map(|e| e.selected == name)
            .unwrap_or(false);
        let target = if reselect {
            names.first().cloned().unwrap_or_default()
        } else {
            self.env
                .as_ref()
                .map(|e| e.selected.clone())
                .unwrap_or_default()
        };
        let rows = self.env_build_rows(&target, cx);
        let rename = cx.new(|cx| CodeEditor::single_line(cx, &target));
        if let Some(ed) = &mut self.env {
            ed.names = names;
            ed.selected = target;
            ed.rename = rename;
            ed.rows = rows;
        }
        cx.notify();
    }

    pub(crate) fn env_duplicate(&mut self, name: String, cx: &mut Context<Self>) {
        let dir = self.env_dir();
        let _ = envfs::duplicate_env(&dir, &name);
        let names = envfs::scan_envs(&dir);
        if let Some(ed) = &mut self.env {
            ed.names = names;
        }
        cx.notify();
    }

    pub(crate) fn env_rename_apply(&mut self, cx: &mut Context<Self>) {
        let (old, new) = match self.env.as_ref() {
            Some(ed) => (
                ed.selected.clone(),
                ed.rename.read(cx).text().trim().to_string(),
            ),
            None => return,
        };
        if old.is_empty() || new.is_empty() || old == new {
            return;
        }
        match envfs::rename_env(&self.env_dir(), &old, &new) {
            Ok(()) => {
                let names = envfs::scan_envs(&self.env_dir());
                if let Some(ed) = &mut self.env {
                    ed.names = names;
                    ed.selected = new;
                    ed.error = None;
                }
            }
            Err(e) => {
                if let Some(ed) = &mut self.env {
                    ed.error = Some(e);
                }
            }
        }
        cx.notify();
    }
}
