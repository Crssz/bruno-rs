//! Git status/commit/push/pull and the collection runner trigger.

use crate::*;
use gpui::prelude::*;

impl BruApp {
    // â”€â”€ git â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
    /// Recompute the collection's git status on a worker thread (it may touch the
    /// filesystem / index) and store the result. Tracks repo-ness separately from
    /// the parsed status so a transient status error keeps the chip reachable.
    pub(crate) fn refresh_git_status(&mut self, cx: &mut Context<Self>) {
        let dir = self.dir.clone();
        let queried = dir.clone();
        let (tx, rx) = futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            let repo = git::is_repo(&dir);
            let status = if repo { git::status(&dir).ok() } else { None };
            let _ = tx.send((repo, status));
        });
        cx.spawn(async move |this, cx| {
            let Ok((repo, status)) = rx.await else { return };
            let _ = this.update(cx, |this, cx| {
                // Ignore a result for a collection the user has since switched away
                // from (worker threads can finish out of order).
                if this.dir != queried {
                    return;
                }
                this.git_repo = repo;
                this.git_status = status;
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn open_git(&mut self, cx: &mut Context<Self>) {
        self.git_open = true;
        self.git_confirm_discard = false;
        self.refresh_git_status(cx);
        cx.notify();
    }
    pub(crate) fn close_git(&mut self, cx: &mut Context<Self>) {
        self.git_open = false;
        self.git_confirm_discard = false;
        cx.notify();
    }

    /// Run a mutating git operation on a worker thread (network ops would block
    /// the UI otherwise), then refresh the status and surface the result.
    pub(crate) fn git_run(&mut self, op: git::Op, cx: &mut Context<Self>) {
        if self.git_busy {
            return;
        }
        self.git_confirm_discard = false;
        let dir = self.dir.clone();
        self.git_busy = true;
        self.git_output = format!("{}\u{2026}", op.label());
        cx.notify();
        let (tx, rx) = futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(git::run_op(&dir, &op));
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.git_busy = false;
                match result {
                    Ok(Ok(msg)) => {
                        this.git_output = msg;
                        // A successful commit consumes the message input.
                        this.git_msg.update(cx, |ed, cx| ed.set_line("", cx));
                    }
                    Ok(Err(e)) => this.git_output = e,
                    Err(_) => this.git_output = "Operation cancelled".into(),
                }
                this.refresh_git_status(cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Stage everything and commit with the overlay's message.
    pub(crate) fn git_commit(&mut self, cx: &mut Context<Self>) {
        let msg = self.git_msg.read(cx).text().trim().to_string();
        if msg.is_empty() {
            self.git_output = "Commit message is empty".into();
            cx.notify();
            return;
        }
        self.git_run(git::Op::Commit(msg), cx);
    }

    /// Discard tracked changes â€” armed by a first click, executed by the second.
    pub(crate) fn git_discard(&mut self, cx: &mut Context<Self>) {
        if self.git_confirm_discard {
            self.git_run(git::Op::Discard, cx);
        } else {
            self.git_confirm_discard = true;
            cx.notify();
        }
    }

    // ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ collection runner ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬ÃƒÂ¢Ã¢â‚¬ÂÃ¢â€šÂ¬
    pub(crate) fn requests_under(&self, dir: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let Some(tree) = &self.collection else {
            return out;
        };
        let start = if self.dir == dir {
            Some(&tree.root)
        } else {
            find_folder(&tree.root, dir)
        };
        if let Some(folder) = start {
            collect_folder_requests(folder, &mut out);
        }
        out
    }

    /// Run every request under `dir` sequentially on a worker thread; bridge the
    /// batch back via oneshot + cx.spawn (same pattern as send()).
    pub(crate) fn run_folder(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        if self.runner_running {
            return;
        }
        let files = self.requests_under(&dir);
        self.runner_title = dir
            .file_name()
            .and_then(|s| s.to_str())
            .map(str::to_string)
            .unwrap_or_else(|| "Collection".into());
        self.runner_open = true;
        self.runner_running = true;
        self.runner_results.clear();
        let vars_base = self.dir.clone();
        let opts = self.send_options();
        let globals = self.send_globals();
        let env = self.selected_env.clone();
        let developer = self.pref_developer;
        let (tx, rx) = futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(run_folder_blocking(
                files, vars_base, opts, globals, env, developer,
            ));
        });
        cx.spawn(async move |this, cx| {
            let result = rx.await;
            let _ = this.update(cx, |this, cx| {
                this.runner_running = false;
                if let Ok(results) = result {
                    this.runner_results = results;
                }
                cx.notify();
            });
        })
        .detach();
    }
}
