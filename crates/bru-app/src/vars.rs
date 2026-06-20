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

    /// React to an editor's hovered-`{{var}}` change: open/switch the popup, or
    /// (on a click → `None`) dismiss it.
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
            }
            None => self.var_popup = None,
        }
        cx.notify();
    }

    /// The `{{var}}` hover popup: name + scope badge + resolved value + Copy.
    pub(crate) fn var_popup_overlay(&self, window: &Window, cx: &mut Context<Self>) -> Div {
        let Some(p) = &self.var_popup else {
            return div();
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
