//! Secrets-vault unlock/lock and row editing.

use crate::*;
use gpui::prelude::*;

impl BruApp {
    pub(crate) fn open_vault(&mut self, cx: &mut Context<Self>) {
        self.vault_open = true;
        self.vault_error = None;
        cx.notify();
    }
    pub(crate) fn close_vault(&mut self, cx: &mut Context<Self>) {
        self.vault_open = false;
        cx.notify();
    }
    pub(crate) fn vault_unlock(&mut self, cx: &mut Context<Self>) {
        let pw = self.vault_input.read(cx).text().to_string();
        let reveal = self.reveal_secrets;
        match vault::load(&pw) {
            Ok(map) => {
                let mut rows: Vec<(String, String)> =
                    map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                rows.sort_by(|a, b| a.0.cmp(&b.0));
                self.vault_rows = rows
                    .into_iter()
                    .map(|(k, v)| {
                        (
                            cx.new(|cx| CodeEditor::single_line(cx, &k)),
                            cx.new(|cx| {
                                if reveal {
                                    CodeEditor::single_line(cx, &v)
                                } else {
                                    CodeEditor::masked_line(cx, &v)
                                }
                            }),
                        )
                    })
                    .collect();
                self.vault = Some(map);
                self.vault_pw = Some(pw);
                self.vault_error = None;
            }
            Err(e) => self.vault_error = Some(e),
        }
        self.refresh_vars();
        cx.notify();
    }
    pub(crate) fn vault_lock(&mut self, cx: &mut Context<Self>) {
        self.vault = None;
        self.vault_pw = None;
        self.vault_rows.clear();
        self.refresh_vars();
        cx.notify();
    }
    pub(crate) fn vault_add_row(&mut self, cx: &mut Context<Self>) {
        let reveal = self.reveal_secrets;
        self.vault_rows.push((
            cx.new(|cx| CodeEditor::single_line(cx, "")),
            cx.new(|cx| {
                if reveal {
                    CodeEditor::single_line(cx, "")
                } else {
                    CodeEditor::masked_line(cx, "")
                }
            }),
        ));
        cx.notify();
    }

    /// Flip the reveal-secrets eye and re-mask/unmask every value editor live.
    pub(crate) fn toggle_reveal_secrets(&mut self, cx: &mut Context<Self>) {
        self.reveal_secrets = !self.reveal_secrets;
        let reveal = self.reveal_secrets;
        for (_, v) in &self.vault_rows {
            v.update(cx, |ed, cx| ed.set_masked(!reveal, cx));
        }
        if let Some(env) = &self.env {
            for row in &env.rows {
                if row.secret {
                    row.value.update(cx, |ed, cx| ed.set_masked(!reveal, cx));
                }
            }
        }
        cx.notify();
    }
    pub(crate) fn vault_remove_row(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.vault_rows.len() {
            self.vault_rows.remove(i);
        }
        cx.notify();
    }
    pub(crate) fn vault_save(&mut self, cx: &mut Context<Self>) {
        let map: HashMap<String, String> = self
            .vault_rows
            .iter()
            .map(|(k, v)| {
                (
                    k.read(cx).text().trim().to_string(),
                    v.read(cx).text().to_string(),
                )
            })
            .filter(|(k, _)| !k.is_empty())
            .collect();
        if let Some(pw) = &self.vault_pw {
            match vault::save(pw, &map) {
                Ok(()) => {
                    self.vault = Some(map);
                    self.vault_error = None;
                }
                Err(e) => self.vault_error = Some(e),
            }
        }
        self.refresh_vars();
        cx.notify();
    }
}
