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
    pub(crate) fn on_goto_definition(&mut self, target: &editor::GotoTarget, cx: &mut Context<Self>) {
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
            resolve_module_in(&d.path, "data").unwrap().file_name().unwrap(),
            "data.json"
        );
        assert_eq!(
            resolve_module_in(&d.path, "pkg").unwrap().file_name().unwrap(),
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
