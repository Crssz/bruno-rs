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

#[cfg(test)]
mod cov_tests {
    use super::*;
    use crate::test_support::{app_on_temp, temp_collection};

    // ---- env_open / env_dir -------------------------------------------------

    #[gpui::test]
    fn env_open_seeds_state_from_disk(cx: &mut gpui::TestAppContext) {
        // The sample collection ships one env: "New Environment".
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| app.env_open(cx));
        app.update(cx, |app, _cx| {
            let ed = app.env.as_ref().expect("env opened");
            assert!(ed.names.iter().any(|n| n == "New Environment"));
            // First (sorted) env becomes the selection.
            assert_eq!(ed.selected, ed.names.first().cloned().unwrap_or_default());
            assert!(ed.error.is_none());
            assert!(!ed.global);
        });
    }

    #[gpui::test]
    fn env_dir_is_collection_dir_in_collection_scope(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        app.update(cx, |app, cx| app.env_open(cx));
        let dir = app.update(cx, |app, _cx| app.env_dir());
        assert!(dir == tc.dir);
    }

    #[gpui::test]
    fn env_close_clears_state(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| app.env_open(cx));
        app.update(cx, |app, _cx| assert!(app.env.is_some()));
        app.update(cx, |app, cx| app.env_close(cx));
        app.update(cx, |app, _cx| assert!(app.env.is_none()));
    }

    // ---- env_set_scope ------------------------------------------------------

    #[gpui::test]
    fn env_set_scope_global_then_back(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| app.env_open(cx));
        // Global scope only READS globals_root (scan/load); never writes there.
        app.update(cx, |app, cx| app.env_set_scope(true, cx));
        app.update(cx, |app, _cx| {
            assert!(app.env.as_ref().unwrap().global);
            assert!(app.env.as_ref().unwrap().error.is_none());
        });
        // Back to collection scope re-reads the temp collection.
        app.update(cx, |app, cx| app.env_set_scope(false, cx));
        app.update(cx, |app, _cx| {
            let ed = app.env.as_ref().unwrap();
            assert!(!ed.global);
            assert!(ed.names.iter().any(|n| n == "New Environment"));
        });
    }

    // ---- env_new ------------------------------------------------------------

    #[gpui::test]
    fn env_new_creates_unique_name_and_file(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        app.update(cx, |app, cx| app.env_open(cx));
        app.update(cx, |app, cx| app.env_new(cx));
        app.update(cx, |app, _cx| {
            let ed = app.env.as_ref().unwrap();
            // "New Environment" already exists in the sample, so the new one is
            // disambiguated to "New Environment 2".
            assert_eq!(ed.selected, "New Environment 2");
            assert!(ed.names.iter().any(|n| n == "New Environment 2"));
            assert!(ed.error.is_none());
        });
        assert!(tc
            .dir
            .join("environments")
            .join("New Environment 2.bru")
            .exists());
    }

    // ---- env_select ---------------------------------------------------------

    #[gpui::test]
    fn env_select_switches_selection(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| app.env_open(cx));
        app.update(cx, |app, cx| app.env_new(cx)); // adds "New Environment 2"
        app.update(cx, |app, cx| app.env_select("New Environment".into(), cx));
        app.update(cx, |app, cx| {
            assert_eq!(app.env.as_ref().unwrap().selected, "New Environment");
            // The rename field follows the new selection.
            let rename_txt = app.env.as_ref().unwrap().rename.read(cx).text().to_string();
            assert_eq!(rename_txt, "New Environment");
            assert!(app.env.as_ref().unwrap().error.is_none());
        });
    }

    // ---- env_add_row / env_remove_row / toggles -----------------------------

    #[gpui::test]
    fn env_add_and_remove_row(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| app.env_open(cx));
        let before = app.update(cx, |app, _cx| app.env.as_ref().unwrap().rows.len());
        app.update(cx, |app, cx| app.env_add_row(cx));
        app.update(cx, |app, _cx| {
            assert_eq!(app.env.as_ref().unwrap().rows.len(), before + 1);
            let r = app.env.as_ref().unwrap().rows.last().unwrap();
            assert!(r.enabled);
            assert!(!r.secret);
        });
        // Remove the row we just added.
        app.update(cx, |app, cx| app.env_remove_row(before, cx));
        app.update(cx, |app, _cx| {
            assert_eq!(app.env.as_ref().unwrap().rows.len(), before);
        });
        // Out-of-range remove is a no-op.
        app.update(cx, |app, cx| app.env_remove_row(9999, cx));
        app.update(cx, |app, _cx| {
            assert_eq!(app.env.as_ref().unwrap().rows.len(), before);
        });
    }

    #[gpui::test]
    fn env_toggle_enabled_and_secret(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| app.env_open(cx));
        app.update(cx, |app, cx| app.env_add_row(cx));
        let i = app.update(cx, |app, _cx| app.env.as_ref().unwrap().rows.len() - 1);
        app.update(cx, |app, cx| app.env_toggle_enabled(i, cx));
        app.update(cx, |app, _cx| {
            assert!(!app.env.as_ref().unwrap().rows[i].enabled);
        });
        app.update(cx, |app, cx| app.env_toggle_secret(i, cx));
        app.update(cx, |app, _cx| {
            assert!(app.env.as_ref().unwrap().rows[i].secret);
        });
        // Toggle secret back off.
        app.update(cx, |app, cx| app.env_toggle_secret(i, cx));
        app.update(cx, |app, _cx| {
            assert!(!app.env.as_ref().unwrap().rows[i].secret);
        });
        // Out-of-range toggles are no-ops (just must not panic).
        app.update(cx, |app, cx| app.env_toggle_enabled(9999, cx));
        app.update(cx, |app, cx| app.env_toggle_secret(9999, cx));
    }

    // ---- env_save / env_collect_rows ----------------------------------------

    #[gpui::test]
    fn env_save_writes_rows_to_disk(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        app.update(cx, |app, cx| app.env_open(cx));
        // Make sure a concrete env is selected (sample ships "New Environment").
        app.update(cx, |app, cx| app.env_select("New Environment".into(), cx));
        // Add a row and fill its name/value editors.
        app.update(cx, |app, cx| app.env_add_row(cx));
        app.update(cx, |app, cx| {
            let row = app.env.as_ref().unwrap().rows.last().unwrap();
            row.name.update(cx, |e, cx| e.set_line("baseUrl", cx));
            row.value
                .update(cx, |e, cx| e.set_line("https://example.test", cx));
        });
        app.update(cx, |app, cx| app.env_save(cx));
        app.update(cx, |app, _cx| {
            assert!(app.env.as_ref().unwrap().error.is_none())
        });
        let written =
            std::fs::read_to_string(tc.dir.join("environments").join("New Environment.bru"))
                .expect("env file written");
        assert!(written.contains("baseUrl: https://example.test"));
    }

    #[gpui::test]
    fn env_save_with_empty_selection_sets_error(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| app.env_open(cx));
        // Force an empty selection to drive the "select first" error branch.
        app.update(cx, |app, _cx| {
            if let Some(ed) = app.env.as_mut() {
                ed.selected = String::new();
            }
        });
        app.update(cx, |app, cx| app.env_save(cx));
        app.update(cx, |app, _cx| {
            assert_eq!(
                app.env.as_ref().unwrap().error.as_deref(),
                Some("Select or create an environment first")
            );
        });
    }

    #[gpui::test]
    fn env_collect_rows_trims_names_and_drops_empties(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| app.env_open(cx));
        // One named row, one empty-named row (should be filtered out).
        app.update(cx, |app, cx| {
            app.env_add_row(cx);
            app.env_add_row(cx);
        });
        app.update(cx, |app, cx| {
            let ed = app.env.as_ref().unwrap();
            let n = ed.rows.len();
            ed.rows[n - 2]
                .name
                .update(cx, |e, cx| e.set_line("  spaced  ", cx));
            ed.rows[n - 2]
                .value
                .update(cx, |e, cx| e.set_line("v1", cx));
            // last row left with an empty name -> filtered
        });
        let collected = app.update(cx, |app, cx| {
            let ed = app.env.as_ref().unwrap();
            app.env_collect_rows(ed, cx)
        });
        assert!(collected
            .iter()
            .any(|r| r.name == "spaced" && r.value == "v1"));
        assert!(collected.iter().all(|r| !r.name.is_empty()));
    }

    // ---- env_duplicate / env_delete -----------------------------------------

    #[gpui::test]
    fn env_duplicate_then_delete(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        app.update(cx, |app, cx| app.env_open(cx));
        app.update(cx, |app, cx| {
            app.env_duplicate("New Environment".into(), cx)
        });
        app.update(cx, |app, _cx| {
            assert!(app
                .env
                .as_ref()
                .unwrap()
                .names
                .iter()
                .any(|n| n == "New Environment copy"));
        });
        assert!(tc
            .dir
            .join("environments")
            .join("New Environment copy.bru")
            .exists());
        // Delete the copy (not the selected one) -> selection is preserved.
        app.update(cx, |app, cx| {
            app.env_delete("New Environment copy".into(), cx)
        });
        app.update(cx, |app, _cx| {
            assert!(!app
                .env
                .as_ref()
                .unwrap()
                .names
                .iter()
                .any(|n| n == "New Environment copy"));
        });
        assert!(!tc
            .dir
            .join("environments")
            .join("New Environment copy.bru")
            .exists());
    }

    #[gpui::test]
    fn env_delete_selected_reselects_first(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| app.env_open(cx));
        app.update(cx, |app, cx| app.env_new(cx)); // "New Environment 2", now selected
        app.update(cx, |app, _cx| {
            assert_eq!(app.env.as_ref().unwrap().selected, "New Environment 2");
        });
        // Deleting the currently-selected env reselects the first remaining one.
        app.update(cx, |app, cx| app.env_delete("New Environment 2".into(), cx));
        app.update(cx, |app, _cx| {
            let ed = app.env.as_ref().unwrap();
            assert_ne!(ed.selected, "New Environment 2");
            assert_eq!(ed.selected, ed.names.first().cloned().unwrap_or_default());
        });
    }

    // ---- env_rename_apply ---------------------------------------------------

    #[gpui::test]
    fn env_rename_apply_renames_file(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        app.update(cx, |app, cx| app.env_open(cx));
        app.update(cx, |app, cx| app.env_select("New Environment".into(), cx));
        app.update(cx, |app, cx| {
            let ed = app.env.as_ref().unwrap();
            ed.rename.update(cx, |e, cx| e.set_line("Production", cx));
        });
        app.update(cx, |app, cx| app.env_rename_apply(cx));
        app.update(cx, |app, _cx| {
            let ed = app.env.as_ref().unwrap();
            assert_eq!(ed.selected, "Production");
            assert!(ed.error.is_none());
            assert!(ed.names.iter().any(|n| n == "Production"));
        });
        assert!(tc.dir.join("environments").join("Production.bru").exists());
        assert!(!tc
            .dir
            .join("environments")
            .join("New Environment.bru")
            .exists());
    }

    #[gpui::test]
    fn env_rename_apply_noop_when_unchanged(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| app.env_open(cx));
        app.update(cx, |app, cx| app.env_select("New Environment".into(), cx));
        // rename field equals current selection -> early-return branch, no error.
        app.update(cx, |app, cx| app.env_rename_apply(cx));
        app.update(cx, |app, _cx| {
            assert_eq!(app.env.as_ref().unwrap().selected, "New Environment");
            assert!(app.env.as_ref().unwrap().error.is_none());
        });
    }

    #[gpui::test]
    fn env_rename_apply_returns_early_without_editor(cx: &mut gpui::TestAppContext) {
        // With env=None, env_rename_apply hits its `None => return` arm.
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, _cx| assert!(app.env.is_none()));
        app.update(cx, |app, cx| app.env_rename_apply(cx));
        app.update(cx, |app, _cx| assert!(app.env.is_none()));
    }

    // ---- env_overlay render (template 3) ------------------------------------

    #[gpui::test]
    fn env_overlay_renders_in_window(cx: &mut gpui::TestAppContext) {
        let tc = temp_collection();
        let dir = tc.dir.clone();
        let window = cx.add_window(|_w, cx| BruApp::new(cx, dir));
        cx.run_until_parked();
        // Open the env manager, then re-park so render runs the now-visible
        // env_overlay builder (gated on self.env.is_some()).
        window
            .update(cx, |app, _w, cx| {
                app.env_open(cx);
                // Add a row so the overlay's row-builder path is exercised too.
                app.env_add_row(cx);
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |app, _w, _cx| assert!(app.env.is_some()))
            .unwrap();
    }
}
