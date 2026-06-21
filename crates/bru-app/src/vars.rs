//! Template-variable resolution, scope lookup and the hover popup.

use crate::*;
use gpui::prelude::*;

impl BruApp {
    // ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ secrets vault ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬
    /// Vault secrets as the lowest-precedence base vars for sends.
    pub(crate) fn vault_vars(&self) -> HashMap<String, String> {
        self.vault.clone().unwrap_or_default()
    }

    /// The send base layer: vault secrets overlaid by the active GLOBAL env's
    /// vars (collection + collection-env vars are layered on top in
    /// run_blocking via base_vars).
    pub(crate) fn send_globals(&self) -> HashMap<String, String> {
        let mut vars = self.vault_vars();
        if let Some(name) = &self.selected_global_env {
            for r in envfs::load_env_rows(&globals_root(), name) {
                if r.enabled {
                    vars.insert(r.name, r.value);
                }
            }
        }
        vars
    }

    // ── template-variable hover popup ─────────────────────────────────────────
    /// Rebuild the cached non-request variable scopes (vault < global <
    /// collection < env, so a later scope overwrites an earlier one and its
    /// scope label wins). Cheap file reads, so call only on collection/env/vault
    /// changes — never per hover.
    pub(crate) fn refresh_vars(&mut self) {
        let mut m: HashMap<String, (VarScope, String, bool)> = HashMap::new();
        for (k, v) in self.vault_vars() {
            m.insert(k, (VarScope::Vault, v, true));
        }
        if let Some(name) = self.selected_global_env.clone() {
            for r in envfs::load_env_rows(&globals_root(), &name) {
                if r.enabled {
                    m.insert(r.name, (VarScope::Global, r.value, r.secret));
                }
            }
        }
        for (k, v) in collection_vars(&self.dir) {
            m.insert(k, (VarScope::Collection, v, false));
        }
        if let Some(name) = self.selected_env.clone() {
            for r in envfs::load_env_rows(&self.dir, &name) {
                // Match the send path (base_vars): secret collection-env vars are
                // NOT applied, so they must not overwrite a lower scope here.
                if r.enabled && !r.secret {
                    m.insert(r.name, (VarScope::Env, r.value, false));
                }
            }
        }
        self.var_scopes = m;
    }

    /// Resolve a variable to `(scope, value, secret)`, matching send precedence.
    /// Request-level vars (the active tab's `vars:pre-request`) win; on the Vars
    /// tab they're read LIVE from the grid editors (else from the in-memory file).
    pub(crate) fn resolve_var(
        &self,
        name: &str,
        cx: &Context<Self>,
    ) -> (Option<VarScope>, String, bool) {
        if DYNAMIC_VARS.contains(&name) {
            return (
                Some(VarScope::Dynamic),
                "(generated per request)".into(),
                false,
            );
        }
        if let Some(tab) = self.active_tab() {
            if tab.var_pre_rows.is_empty() {
                for (k, v, enabled, _local) in edit::var_block_rows(&tab.file, "vars:pre-request") {
                    if enabled && k == name {
                        return (Some(VarScope::Request), v, false);
                    }
                }
            } else {
                // The Vars tab is open: the grid cells are the source of truth.
                for row in &tab.var_pre_rows {
                    if row.enabled && row.name.read(cx).text().trim() == name {
                        return (
                            Some(VarScope::Request),
                            row.value.read(cx).text().to_string(),
                            false,
                        );
                    }
                }
            }
        }
        match self.var_scopes.get(name) {
            Some((scope, val, secret)) => (Some(*scope), val.clone(), *secret),
            None => (None, String::new(), false),
        }
    }

    // ── Ctrl/Cmd+click "go to definition" ─────────────────────────────────────
    /// Route a Ctrl+click target: a `{{var}}` to its defining scope, or a
    /// `require(...)` specifier to the file it resolves to.
    pub(crate) fn on_goto_definition(
        &mut self,
        target: &editor::GotoTarget,
        cx: &mut Context<Self>,
    ) {
        match target {
            editor::GotoTarget::Var(name) => self.goto_var_definition(name, cx),
            editor::GotoTarget::Module { spec, symbol } => {
                self.goto_module(spec, symbol.as_deref(), cx)
            }
            editor::GotoTarget::Local(name) => self.goto_local(name, cx),
        }
    }

    /// Jump to a symbol defined in the active editor's own buffer (a local
    /// function/const/class) — select its declaration and scroll it into view.
    fn goto_local(&mut self, symbol: &str, cx: &mut Context<Self>) {
        let Some(i) = self.active else { return };
        let (editor, scroll) = match &self.tabs[i].text {
            Some(ed) => (ed.clone(), self.tabs[i].text_scroll.clone()),
            None => (
                self.tabs[i].body_editor.clone(),
                self.tabs[i].body_scroll.clone(),
            ),
        };
        if !reveal_symbol(&editor, &scroll, symbol, cx) {
            self.status = format!("No local definition for '{symbol}'");
            cx.notify();
        }
    }

    /// Open the editor/modal where `name`'s effective scope defines it.
    fn goto_var_definition(&mut self, name: &str, cx: &mut Context<Self>) {
        let (scope, _value, _secret) = self.resolve_var(name, cx);
        match scope {
            Some(VarScope::Request) => {
                if let Some(i) = self.active {
                    self.tabs[i].switch_tab(ReqTab::Vars, cx);
                }
                self.status = format!("'{name}' is a request variable (Vars tab)");
            }
            Some(VarScope::Env) => {
                self.env_open(cx);
                if let Some(env) = self.selected_env.clone() {
                    self.env_select(env, cx);
                }
                self.status = format!("'{name}' is defined in the active environment");
            }
            Some(VarScope::Global) => {
                self.env_open(cx);
                self.env_set_scope(true, cx);
                if let Some(env) = self.selected_global_env.clone() {
                    self.env_select(env, cx);
                }
                self.status = format!("'{name}' is defined in the active global environment");
            }
            Some(VarScope::Collection) => {
                self.open_collection_settings(cx);
                self.status = format!("'{name}' is defined in collection settings");
            }
            Some(VarScope::Vault) => {
                self.open_vault(cx);
                self.status = format!("'{name}' is defined in the secrets vault");
            }
            Some(VarScope::Dynamic) => {
                self.status = format!("'{name}' is a dynamic variable (generated per request)");
            }
            None => self.status = format!("'{name}' is not defined in any scope"),
        }
        cx.notify();
    }

    /// Resolve a `require(spec)` to a file (relative to the active request's
    /// folder) and open it in an editable in-app tab. When `symbol` is set (the
    /// click was on an imported identifier), also scroll to and select that
    /// symbol's definition via a tree-sitter parse of the opened file.
    fn goto_module(&mut self, spec: &str, symbol: Option<&str>, cx: &mut Context<Self>) {
        let Some(p) = self.resolve_module_path(spec) else {
            let base = self
                .active_tab()
                .and_then(|t| t.path.parent().map(|p| p.display().to_string()))
                .unwrap_or_default();
            self.status = format!("Cannot find module '{spec}' in {base} or the collection root");
            cx.notify();
            return;
        };
        self.open_text_file(p, cx);
        let Some(symbol) = symbol else { return };
        let Some(i) = self.active else { return };
        let Some(editor) = self.tabs[i].text.clone() else {
            return;
        };
        let scroll = self.tabs[i].text_scroll.clone();
        reveal_symbol(&editor, &scroll, symbol, cx);
        cx.notify();
    }

    /// Resolve a `require` specifier to a file. Relative specifiers are tried
    /// against the active request's own folder first (CommonJS), then the
    /// collection root (where shared scripts are commonly kept). Each base does
    /// node-style resolution: exact path, then `.js`/`.json`, then `index.js`.
    /// Bare specifiers (npm packages) return `None` — there's no `node_modules`.
    fn resolve_module_path(&self, spec: &str) -> Option<PathBuf> {
        if Path::new(spec).is_absolute() {
            return resolve_module_file(Path::new(spec));
        }
        if !(spec.starts_with("./") || spec.starts_with("../")) {
            return None;
        }
        // Drop a leading `./` so the common case joins to a clean path.
        let rel = spec.strip_prefix("./").unwrap_or(spec);
        let base = self.active_tab()?.path.parent()?.to_path_buf();
        resolve_module_in(&base, rel).or_else(|| resolve_module_in(&self.dir, rel))
    }

    /// React to an editor's hovered-`{{var}}` change: open/switch the popup on
    /// entering a var, or schedule its dismissal on leaving one (or on a click).
    pub(crate) fn on_hover_var(&mut self, ev: &editor::HoverVar, cx: &mut Context<Self>) {
        match &ev.name {
            Some(name) => {
                let (scope, value, secret) = self.resolve_var(name, cx);
                self.var_popup = Some(VarPopup {
                    name: name.clone(),
                    value,
                    scope,
                    secret,
                    pos: ev.pos,
                });
                self.var_popup_hovered = false;
                // A fresh hover cancels any pending dismiss from a prior var.
                self.var_popup_gen += 1;
                cx.notify();
            }
            // Pointer left the `{{var}}`: don't close immediately — give it a short
            // grace to reach the popup (e.g. to click Copy). The popup's own hover
            // tracking cancels this if the pointer lands on it.
            None => self.schedule_var_popup_dismiss(cx),
        }
    }

    /// Dismiss the var popup after a short grace period, unless the pointer has
    /// since entered the popup (`var_popup_hovered`) or a newer hover superseded
    /// this one (`var_popup_gen` changed). Bridges the gap between the `{{var}}`
    /// and the card so moving onto the popup doesn't dismiss it first.
    fn schedule_var_popup_dismiss(&mut self, cx: &mut Context<Self>) {
        if self.var_popup.is_none() {
            return;
        }
        self.var_popup_gen += 1;
        let generation = self.var_popup_gen;
        cx.spawn(async move |this, cx| {
            let timer = cx
                .background_executor()
                .timer(std::time::Duration::from_millis(200));
            timer.await;
            let _ = this.update(cx, |this, cx| {
                if this.var_popup_gen == generation && !this.var_popup_hovered {
                    this.var_popup = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The `{{var}}` hover popup: name + scope badge + resolved value + Copy.
    pub(crate) fn var_popup_overlay(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let Some(p) = &self.var_popup else {
            return div().id("var-popup");
        };
        let resolved = p.scope.is_some();
        let badge = match p.scope {
            Some(s) => (s.label(), s.color()),
            None => ("unset", theme::red()),
        };
        let display_value = if !resolved {
            "(unset)".to_string()
        } else if p.secret && !self.reveal_secrets {
            "\u{2022}".repeat(8)
        } else {
            p.value.clone()
        };
        // Clamp to the window so a var near the right/bottom edge stays on-screen.
        let (card_w, est_h) = (260.0_f32, 96.0_f32);
        let vw = f32::from(window.viewport_size().width);
        let vh = f32::from(window.viewport_size().height);
        let left = f32::from(p.pos.x).min(vw - card_w - 8.0).max(8.0);
        let raw_top = f32::from(p.pos.y);
        let top = if raw_top + 18.0 + est_h > vh {
            (raw_top - est_h).max(8.0)
        } else {
            raw_top + 18.0
        };
        let mut card = div()
            .id("var-popup")
            // Track the pointer over the card so a dismiss scheduled when it left
            // the `{{var}}` is cancelled here, and so leaving the card dismisses it.
            .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                this.var_popup_hovered = *hovered;
                if *hovered {
                    this.var_popup_gen += 1; // cancel the pending dismiss
                } else {
                    this.schedule_var_popup_dismiss(cx);
                }
            }))
            .absolute()
            .left(px(left))
            .top(px(top))
            .occlude()
            .flex()
            .flex_col()
            .gap_1()
            .w(px(260.))
            .p_2()
            .rounded_md()
            .bg(theme::mantle())
            .border_1()
            .border_color(theme::border2())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_family("monospace")
                            .text_size(px(12.))
                            .text_color(theme::text())
                            .child(p.name.clone()),
                    )
                    .child(
                        div()
                            .px_1()
                            .rounded_sm()
                            .bg(theme::surface0())
                            .text_size(px(9.))
                            .text_color(badge.1)
                            .child(badge.0),
                    ),
            )
            .child(
                div()
                    .font_family("monospace")
                    .text_size(px(12.))
                    .text_color(if resolved {
                        theme::text()
                    } else {
                        theme::red()
                    })
                    .child(display_value),
            );
        if resolved && p.scope != Some(VarScope::Dynamic) && !p.value.is_empty() {
            let val = p.value.clone();
            card = card.child(
                div().flex().flex_row().justify_end().child(
                    div()
                        .px_2()
                        .rounded_md()
                        .text_size(px(11.))
                        .text_color(theme::accent())
                        .hover(|s| s.bg(theme::surface0()))
                        .child("Copy")
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _e: &MouseUpEvent, _w, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(val.clone()));
                                this.var_popup = None;
                                this.status = "Copied value to clipboard".into();
                                cx.notify();
                            }),
                        ),
                ),
            );
        }
        card
    }
}

/// Select `symbol`'s definition in `editor` (located via a tree-sitter parse) and
/// scroll it into view with `scroll`. Returns false if no definition was found.
fn reveal_symbol(
    editor: &Entity<CodeEditor>,
    scroll: &ScrollHandle,
    symbol: &str,
    cx: &mut Context<BruApp>,
) -> bool {
    let content = editor.read(cx).text().to_string();
    let Some(range) = highlight::js_symbol_range(&content, symbol) else {
        return false;
    };
    let line = content[..range.start].matches('\n').count();
    editor.update(cx, |ed, cx| ed.select_byte_range(range, cx));
    // Bring the definition near the top, keeping ~2 lines of context above.
    let y = line.saturating_sub(2) as f32 * 19.0;
    scroll.set_offset(gpui::point(px(0.), px(-y)));
    true
}

/// Node-style resolution of an already-joined module path: the exact file, then
/// the path with `.js`/`.json` appended, then `index.js` inside it (a directory).
fn resolve_module_file(joined: &Path) -> Option<PathBuf> {
    if joined.is_file() {
        return Some(joined.to_path_buf());
    }
    for ext in ["js", "json"] {
        let mut s = joined.as_os_str().to_os_string();
        s.push(".");
        s.push(ext);
        let p = PathBuf::from(s);
        if p.is_file() {
            return Some(p);
        }
    }
    let idx = joined.join("index.js");
    idx.is_file().then_some(idx)
}

/// Resolve `rel` (a `./`-stripped relative specifier) against `base`.
fn resolve_module_in(base: &Path, rel: &str) -> Option<PathBuf> {
    resolve_module_file(&base.join(rel))
}

#[cfg(test)]
mod tests {
    use super::{resolve_module_file, resolve_module_in};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);

    struct TempDir {
        path: PathBuf,
    }
    impl TempDir {
        fn new() -> Self {
            let n = N.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("bru-mod-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            TempDir { path }
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn resolves_sibling_with_and_without_extension() {
        let d = TempDir::new();
        std::fs::write(d.path.join("helper.js"), "module.exports = 1;").unwrap();
        // Exact name, and bare name with `.js` inferred, both resolve to the file.
        assert!(resolve_module_in(&d.path, "helper.js").unwrap().is_file());
        let hit = resolve_module_in(&d.path, "helper").unwrap();
        assert_eq!(hit.file_name().unwrap(), "helper.js");
    }

    #[test]
    fn resolves_json_and_directory_index() {
        let d = TempDir::new();
        std::fs::write(d.path.join("data.json"), "{}").unwrap();
        let pkg = d.path.join("pkg");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("index.js"), "module.exports = {};").unwrap();
        assert_eq!(
            resolve_module_in(&d.path, "data")
                .unwrap()
                .file_name()
                .unwrap(),
            "data.json"
        );
        assert_eq!(
            resolve_module_in(&d.path, "pkg")
                .unwrap()
                .file_name()
                .unwrap(),
            "index.js"
        );
    }

    #[test]
    fn missing_file_resolves_to_none() {
        let d = TempDir::new();
        assert!(resolve_module_in(&d.path, "nope").is_none());
        assert!(resolve_module_file(&d.path.join("nope.js")).is_none());
    }
}

#[cfg(test)]
mod cov_tests {
    use super::*;
    use crate::test_support::{app_on_temp, temp_collection};
    use std::collections::HashMap;

    // ── small helpers ─────────────────────────────────────────────────────────

    /// Open `Repository Info.bru` from the temp collection as the active tab.
    fn open_repo_info(
        app: &gpui::Entity<BruApp>,
        dir: &std::path::Path,
        cx: &mut gpui::TestAppContext,
    ) {
        let p = dir.join("Repository Info.bru");
        app.update(cx, |app, cx| app.open_request(p, cx));
    }

    /// Write a populated collection environment into the temp copy so the Env
    /// scope has something real to resolve. Returns the env name (file stem).
    fn write_env(dir: &std::path::Path, name: &str, rows: &[(&str, &str)]) -> String {
        let rows: Vec<crate::envfs::EnvRow> = rows
            .iter()
            .map(|(k, v)| crate::envfs::EnvRow {
                name: (*k).to_string(),
                value: (*v).to_string(),
                enabled: true,
                secret: false,
            })
            .collect();
        crate::envfs::save_env(dir, name, &rows).unwrap();
        name.to_string()
    }

    // ── vault_vars / send_globals ─────────────────────────────────────────────

    #[gpui::test]
    fn vault_vars_is_empty_when_locked_and_reflects_unlocked_map(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        // Locked vault → empty base vars.
        app.update(cx, |app, _| assert!(app.vault_vars().is_empty()));
        // Unlocked (in-memory only — never saved to disk) → returns a clone.
        app.update(cx, |app, _| {
            let mut m = HashMap::new();
            m.insert("apiKey".to_string(), "sekret".to_string());
            app.vault = Some(m);
        });
        app.update(cx, |app, _| {
            let v = app.vault_vars();
            assert_eq!(v.get("apiKey").map(String::as_str), Some("sekret"));
        });
    }

    #[gpui::test]
    fn send_globals_with_no_global_env_is_just_the_vault(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, _| {
            let mut m = HashMap::new();
            m.insert("base".to_string(), "v".to_string());
            app.vault = Some(m);
            app.selected_global_env = None;
        });
        app.update(cx, |app, _| {
            let g = app.send_globals();
            assert_eq!(g.get("base").map(String::as_str), Some("v"));
            assert_eq!(g.len(), 1);
        });
    }

    // ── refresh_vars + resolve_var, per scope ─────────────────────────────────

    #[gpui::test]
    fn refresh_vars_picks_up_vault_scope(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            let mut m = HashMap::new();
            m.insert("vaultVar".to_string(), "vaultVal".to_string());
            app.vault = Some(m);
            app.refresh_vars();
            let (scope, val, secret) = app.resolve_var("vaultVar", cx);
            assert!(scope == Some(VarScope::Vault));
            assert_eq!(val, "vaultVal");
            assert!(secret); // vault vars are always flagged secret
        });
    }

    #[gpui::test]
    fn refresh_vars_picks_up_collection_scope(cx: &mut gpui::TestAppContext) {
        // The sample collection.bru defines `baseUrl` as a collection pre-request var.
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.refresh_vars();
            let (scope, val, secret) = app.resolve_var("baseUrl", cx);
            assert!(scope == Some(VarScope::Collection));
            assert_eq!(val, "https://api.github.com");
            assert!(!secret);
        });
    }

    #[gpui::test]
    fn refresh_vars_picks_up_env_scope(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let env = write_env(&tc.dir, "CovEnv", &[("host", "example.test")]);
        // select_env sets selected_env and calls refresh_vars for us.
        app.update(cx, |app, cx| app.select_env(Some(env), cx));
        app.update(cx, |app, cx| {
            let (scope, val, _secret) = app.resolve_var("host", cx);
            assert!(scope == Some(VarScope::Env));
            assert_eq!(val, "example.test");
        });
    }

    #[gpui::test]
    fn env_scope_overrides_collection_scope(cx: &mut gpui::TestAppContext) {
        // An env var named `baseUrl` must win over the collection's `baseUrl`,
        // mirroring send precedence (Env > Collection).
        let (app, tc) = app_on_temp(cx);
        let env = write_env(&tc.dir, "Override", &[("baseUrl", "https://override.test")]);
        app.update(cx, |app, cx| app.select_env(Some(env), cx));
        app.update(cx, |app, cx| {
            let (scope, val, _secret) = app.resolve_var("baseUrl", cx);
            assert!(scope == Some(VarScope::Env));
            assert_eq!(val, "https://override.test");
        });
    }

    #[gpui::test]
    fn refresh_vars_skips_secret_env_vars(cx: &mut gpui::TestAppContext) {
        // Secret collection-env vars are NOT applied in the send path, so they must
        // not shadow a lower scope. Here there's no lower scope, so it resolves None.
        let (app, tc) = app_on_temp(cx);
        let rows = vec![crate::envfs::EnvRow {
            name: "secretOnly".into(),
            value: "hidden".into(),
            enabled: true,
            secret: true,
        }];
        crate::envfs::save_env(&tc.dir, "SecretEnv", &rows).unwrap();
        app.update(cx, |app, cx| {
            app.select_env(Some("SecretEnv".to_string()), cx)
        });
        app.update(cx, |app, cx| {
            let (scope, _val, _secret) = app.resolve_var("secretOnly", cx);
            assert!(scope.is_none());
        });
    }

    // ── resolve_var: dynamic / unset / request scopes ─────────────────────────

    #[gpui::test]
    fn resolve_var_dynamic_branch(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            let (scope, val, secret) = app.resolve_var("$guid", cx);
            assert!(scope == Some(VarScope::Dynamic));
            assert_eq!(val, "(generated per request)");
            assert!(!secret);
        });
    }

    #[gpui::test]
    fn resolve_var_unset_returns_none(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            let (scope, val, secret) = app.resolve_var("definitelyNotDefined", cx);
            assert!(scope.is_none());
            assert_eq!(val, String::new());
            assert!(!secret);
        });
    }

    #[gpui::test]
    fn resolve_var_request_scope_from_file_when_grid_empty(cx: &mut gpui::TestAppContext) {
        // With no Vars-tab grid open (var_pre_rows empty), request vars are read
        // from the file's `vars:pre-request` block. Write one into the active tab's
        // file on disk and re-open so OpenTab::load picks it up.
        let (app, tc) = app_on_temp(cx);
        let path = tc.dir.join("Repository Info.bru");
        let mut src = std::fs::read_to_string(&path).unwrap();
        src.push_str("\nvars:pre-request {\n  reqVar: reqVal\n}\n");
        std::fs::write(&path, src).unwrap();
        open_repo_info(&app, &tc.dir, cx);
        app.update(cx, |app, cx| {
            assert!(app.active_tab().is_some());
            let (scope, val, _secret) = app.resolve_var("reqVar", cx);
            assert!(scope == Some(VarScope::Request));
            assert_eq!(val, "reqVal");
        });
    }

    #[gpui::test]
    fn resolve_var_request_scope_from_live_grid(cx: &mut gpui::TestAppContext) {
        // When the Vars tab grid is populated (var_pre_rows non-empty) the grid
        // cells are the source of truth, overriding the file.
        let (app, tc) = app_on_temp(cx);
        open_repo_info(&app, &tc.dir, cx);
        // Add a live grid row and fill its name/value editors.
        app.update(cx, |app, cx| app.var_add_row(false, cx));
        app.update(cx, |app, cx| {
            let i = app.active.unwrap();
            let row = &app.tabs[i].var_pre_rows[0];
            row.name.update(cx, |ed, cx| ed.set_line("gridVar", cx));
            row.value.update(cx, |ed, cx| ed.set_line("gridVal", cx));
        });
        app.update(cx, |app, cx| {
            let (scope, val, _secret) = app.resolve_var("gridVar", cx);
            assert!(scope == Some(VarScope::Request));
            assert_eq!(val, "gridVal");
        });
    }

    #[gpui::test]
    fn resolve_var_live_grid_ignores_disabled_rows(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        open_repo_info(&app, &tc.dir, cx);
        app.update(cx, |app, cx| app.var_add_row(false, cx));
        app.update(cx, |app, cx| {
            let i = app.active.unwrap();
            let row = &mut app.tabs[i].var_pre_rows[0];
            row.enabled = false;
            row.name.update(cx, |ed, cx| ed.set_line("offVar", cx));
            row.value.update(cx, |ed, cx| ed.set_line("offVal", cx));
        });
        app.update(cx, |app, cx| {
            // Disabled grid row contributes nothing → falls through to scope map.
            let (scope, _val, _secret) = app.resolve_var("offVar", cx);
            assert!(scope.is_none());
        });
    }

    // ── on_hover_var ──────────────────────────────────────────────────────────

    #[gpui::test]
    fn on_hover_var_some_opens_popup_with_resolved_scope(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, _| app.refresh_vars()); // baseUrl → Collection
        app.update(cx, |app, cx| {
            let ev = editor::HoverVar {
                name: Some("baseUrl".to_string()),
                pos: gpui::point(px(10.), px(20.)),
            };
            app.on_hover_var(&ev, cx);
        });
        app.update(cx, |app, _| {
            let p = app.var_popup.as_ref().expect("popup should be open");
            assert_eq!(p.name, "baseUrl");
            assert!(p.scope == Some(VarScope::Collection));
            assert_eq!(p.value, "https://api.github.com");
            assert!(!app.var_popup_hovered);
        });
    }

    #[gpui::test]
    fn on_hover_var_unset_marks_popup_unresolved(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            let ev = editor::HoverVar {
                name: Some("nope".to_string()),
                pos: gpui::point(px(1.), px(2.)),
            };
            app.on_hover_var(&ev, cx);
        });
        app.update(cx, |app, _| {
            let p = app.var_popup.as_ref().unwrap();
            assert!(p.scope.is_none());
            assert_eq!(p.value, String::new());
        });
    }

    #[gpui::test]
    fn on_hover_var_none_schedules_dismiss_without_immediate_close(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        // Open a popup first.
        app.update(cx, |app, cx| {
            let ev = editor::HoverVar {
                name: Some("baseUrl".to_string()),
                pos: gpui::point(px(0.), px(0.)),
            };
            app.on_hover_var(&ev, cx);
        });
        let gen_before = app.update(cx, |app, _| app.var_popup_gen);
        // Pointer left the {{var}}: schedules a *delayed* dismiss, popup stays for now.
        app.update(cx, |app, cx| {
            let ev = editor::HoverVar {
                name: None,
                pos: gpui::point(px(0.), px(0.)),
            };
            app.on_hover_var(&ev, cx);
        });
        app.update(cx, |app, _| {
            assert!(app.var_popup.is_some()); // not closed synchronously
            assert!(app.var_popup_gen > gen_before); // generation bumped
        });
    }

    // ── on_goto_definition / goto_var_definition ──────────────────────────────

    #[gpui::test]
    fn goto_var_definition_request_scope_switches_to_vars_tab(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        open_repo_info(&app, &tc.dir, cx);
        // Populate a live request var so it resolves to the Request scope.
        app.update(cx, |app, cx| app.var_add_row(false, cx));
        app.update(cx, |app, cx| {
            let i = app.active.unwrap();
            let row = &app.tabs[i].var_pre_rows[0];
            row.name.update(cx, |ed, cx| ed.set_line("reqv", cx));
            row.value.update(cx, |ed, cx| ed.set_line("x", cx));
        });
        app.update(cx, |app, cx| {
            app.on_goto_definition(&editor::GotoTarget::Var("reqv".to_string()), cx);
        });
        app.update(cx, |app, _| {
            let i = app.active.unwrap();
            assert!(app.tabs[i].req_tab == ReqTab::Vars);
            assert!(app.status.contains("request variable"));
        });
    }

    #[gpui::test]
    fn goto_var_definition_collection_scope_opens_settings(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, _| app.refresh_vars());
        app.update(cx, |app, cx| {
            app.on_goto_definition(&editor::GotoTarget::Var("baseUrl".to_string()), cx);
        });
        app.update(cx, |app, _| {
            assert!(app.status.contains("collection settings"))
        });
    }

    #[gpui::test]
    fn goto_var_definition_env_scope_opens_env_manager(cx: &mut gpui::TestAppContext) {
        let (app, tc) = app_on_temp(cx);
        let env = write_env(&tc.dir, "GotoEnv", &[("envv", "e")]);
        app.update(cx, |app, cx| app.select_env(Some(env), cx));
        app.update(cx, |app, cx| {
            app.on_goto_definition(&editor::GotoTarget::Var("envv".to_string()), cx);
        });
        app.update(cx, |app, _| {
            assert!(app.env.is_some()); // env manager overlay opened
            assert!(app.status.contains("active environment"));
        });
    }

    #[gpui::test]
    fn goto_var_definition_vault_scope_opens_vault(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, _| {
            let mut m = HashMap::new();
            m.insert("vk".to_string(), "vv".to_string());
            app.vault = Some(m);
            app.refresh_vars();
        });
        app.update(cx, |app, cx| {
            app.on_goto_definition(&editor::GotoTarget::Var("vk".to_string()), cx);
        });
        app.update(cx, |app, _| {
            assert!(app.vault_open);
            assert!(app.status.contains("secrets vault"));
        });
    }

    #[gpui::test]
    fn goto_var_definition_dynamic_and_unset_set_status(cx: &mut gpui::TestAppContext) {
        let (app, _tc) = app_on_temp(cx);
        app.update(cx, |app, cx| {
            app.on_goto_definition(&editor::GotoTarget::Var("$timestamp".to_string()), cx);
        });
        app.update(cx, |app, _| {
            assert!(app.status.contains("dynamic variable"))
        });
        app.update(cx, |app, cx| {
            app.on_goto_definition(&editor::GotoTarget::Var("missing".to_string()), cx);
        });
        app.update(cx, |app, _| {
            assert!(app.status.contains("not defined in any scope"))
        });
    }

    #[gpui::test]
    fn on_goto_definition_local_missing_symbol_sets_status(cx: &mut gpui::TestAppContext) {
        // A request tab uses its body_editor (no `text` editor). A Local target for
        // a symbol that isn't in the (empty) body should report "No local definition".
        let (app, tc) = app_on_temp(cx);
        open_repo_info(&app, &tc.dir, cx);
        app.update(cx, |app, cx| {
            app.on_goto_definition(&editor::GotoTarget::Local("nonexistentFn".to_string()), cx);
        });
        app.update(cx, |app, _| {
            assert!(app.status.contains("No local definition"))
        });
    }

    #[gpui::test]
    fn on_goto_definition_local_no_active_tab_is_noop(cx: &mut gpui::TestAppContext) {
        // No request open → goto_local early-returns without touching status.
        let tc = temp_collection();
        let app = crate::test_support::build_app(cx, tc.dir.clone());
        app.update(cx, |app, _| assert!(app.active.is_none()));
        app.update(cx, |app, cx| {
            app.on_goto_definition(&editor::GotoTarget::Local("whatever".to_string()), cx);
        });
        // Status unchanged (still the construction-time default).
        app.update(cx, |app, _| {
            assert!(!app.status.contains("No local definition"))
        });
    }
}
