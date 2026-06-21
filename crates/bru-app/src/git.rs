//! Git operations for the active collection.
//!
//! These shell out to the system `git` binary rather than linking a libgit2
//! binding. Reusing the user's installed git means remote operations (fetch /
//! pull / push) transparently use whatever credentials they already have set up
//! — credential helpers / managers, SSH agents, `~/.git-credentials`, etc. — so
//! we never have to reimplement credential discovery or prompt for secrets. The
//! cost is a hard dependency on `git` being on `PATH`, which is reasonable for a
//! developer tool whose collections are version-controlled.
//!
//! Every call runs in the collection directory; the UI runs the slow (network)
//! ones on a worker thread so the foreground never blocks.

use std::path::Path;
use std::process::Command;

/// A parsed `git status` snapshot for the status-bar chip + git overlay.
#[derive(Clone, Debug, Default)]
pub struct Status {
    /// Current branch name (or "HEAD" when detached).
    pub branch: String,
    /// Commits ahead of / behind the upstream, when one is configured.
    pub ahead: u32,
    pub behind: u32,
    /// Changed entries from `git status --porcelain` (staged + unstaged + new).
    pub files: Vec<FileEntry>,
}

impl Status {
    /// Whether the working tree has any changes (staged, unstaged, or untracked).
    pub fn is_dirty(&self) -> bool {
        !self.files.is_empty()
    }
}

/// One changed path with its two-char porcelain status code (e.g. ` M`, `??`).
#[derive(Clone, Debug)]
pub struct FileEntry {
    pub code: String,
    pub path: String,
}

/// A mutating git operation requested from the UI.
#[derive(Clone, Debug)]
pub enum Op {
    /// Stage every change (`git add -A`).
    StageAll,
    /// Stage everything, then commit with the given message.
    Commit(String),
    /// Discard all tracked changes back to HEAD (`git reset --hard`).
    Discard,
    /// Download remote refs without merging (`git fetch`).
    Fetch,
    /// Fetch + integrate the upstream (`git pull`).
    Pull,
    /// Publish local commits (`git push`).
    Push,
}

impl Op {
    /// A short present-tense label for the "running…" status line.
    pub fn label(&self) -> &'static str {
        match self {
            Op::StageAll => "Staging",
            Op::Commit(_) => "Committing",
            Op::Discard => "Discarding",
            Op::Fetch => "Fetching",
            Op::Pull => "Pulling",
            Op::Push => "Pushing",
        }
    }
}

/// Suppress the console window that would otherwise flash when a GUI process
/// spawns a console subprocess on Windows.
#[cfg(windows)]
fn hide_console(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}
#[cfg(not(windows))]
fn hide_console(_cmd: &mut Command) {}

/// Run `git <args>` in `dir`, returning trimmed stdout on success or a combined
/// stdout+stderr message on failure (or if `git` can't be launched at all).
fn run(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(dir);
    hide_console(&mut cmd);
    let out = cmd
        .output()
        .map_err(|e| format!("git not available: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
    } else {
        let mut msg = String::from_utf8_lossy(&out.stdout).into_owned();
        msg.push_str(&String::from_utf8_lossy(&out.stderr));
        let msg = msg.trim().to_string();
        Err(if msg.is_empty() {
            format!("git exited with {}", out.status)
        } else {
            msg
        })
    }
}

/// Whether `dir` is inside a git work tree (false if git is missing too).
pub fn is_repo(dir: &Path) -> bool {
    run(dir, &["rev-parse", "--is-inside-work-tree"])
        .map(|s| s.trim() == "true")
        .unwrap_or(false)
}

/// Read the working-tree status (branch, ahead/behind, changed files).
pub fn status(dir: &Path) -> Result<Status, String> {
    let out = run(dir, &["status", "--porcelain=v1", "--branch"])?;
    Ok(parse_status(&out))
}

/// Parse `git status --porcelain=v1 --branch` output.
fn parse_status(out: &str) -> Status {
    let mut s = Status::default();
    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            // `branch...upstream [ahead N, behind M]` | `branch` |
            // `HEAD (no branch)` (detached) | `No commits yet on branch` (unborn).
            let head = rest.split("...").next().unwrap_or(rest);
            let head = head.split(" [").next().unwrap_or(head).trim();
            s.branch = if let Some(b) = head.strip_prefix("No commits yet on ") {
                b.trim().to_string()
            } else if head.starts_with("HEAD (") {
                "HEAD".to_string()
            } else {
                head.to_string()
            };
            if let Some(b) = rest.find('[') {
                let bracket = &rest[b..];
                s.ahead = field(bracket, "ahead ");
                s.behind = field(bracket, "behind ");
            }
        } else if line.len() >= 3 {
            s.files.push(FileEntry {
                code: line[..2].to_string(),
                path: line[3..].to_string(),
            });
        }
    }
    s
}

/// Extract the integer following `key` (e.g. "ahead ") in a `[ahead 1, behind 2]`
/// fragment; 0 if absent.
fn field(bracket: &str, key: &str) -> u32 {
    bracket
        .find(key)
        .and_then(|i| {
            bracket[i + key.len()..]
                .split(|c: char| !c.is_ascii_digit())
                .next()
        })
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// Run a mutating operation, returning a human-readable result line.
pub fn run_op(dir: &Path, op: &Op) -> Result<String, String> {
    match op {
        Op::StageAll => {
            run(dir, &["add", "-A"])?;
            Ok("Staged all changes".to_string())
        }
        Op::Commit(msg) => {
            let msg = msg.trim();
            if msg.is_empty() {
                return Err("Commit message is empty".to_string());
            }
            run(dir, &["add", "-A"])?;
            let out = run(dir, &["commit", "-m", msg])?;
            Ok(out.lines().next().unwrap_or("Committed").to_string())
        }
        Op::Discard => {
            // Revert tracked changes (staged + unstaged) to HEAD. Untracked files
            // are intentionally left alone so a stray new file is never destroyed.
            run(dir, &["reset", "--hard", "HEAD"])?;
            Ok("Discarded tracked changes".to_string())
        }
        Op::Fetch => {
            let out = run(dir, &["fetch", "--all"])?;
            Ok(if out.is_empty() {
                "Fetched".to_string()
            } else {
                out
            })
        }
        Op::Pull => {
            let out = run(dir, &["pull"])?;
            Ok(out.lines().last().unwrap_or("Pulled").to_string())
        }
        Op::Push => {
            let out = run(dir, &["push"])?;
            Ok(if out.is_empty() {
                "Pushed".to_string()
            } else {
                out.lines().last().unwrap_or("Pushed").to_string()
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_branch_with_ahead_behind() {
        let s = parse_status("## main...origin/main [ahead 2, behind 1]\n M src/a.rs\n?? new.txt");
        assert_eq!(s.branch, "main");
        assert_eq!(s.ahead, 2);
        assert_eq!(s.behind, 1);
        assert_eq!(s.files.len(), 2);
        assert_eq!(s.files[0].code, " M");
        assert_eq!(s.files[0].path, "src/a.rs");
        assert_eq!(s.files[1].code, "??");
        assert!(s.is_dirty());
    }

    #[test]
    fn parses_clean_branch_no_upstream() {
        let s = parse_status("## develop");
        assert_eq!(s.branch, "develop");
        assert_eq!(s.ahead, 0);
        assert_eq!(s.behind, 0);
        assert!(!s.is_dirty());
    }

    #[test]
    fn parses_detached_head() {
        let s = parse_status("## HEAD (no branch)\n M f.rs");
        assert_eq!(s.branch, "HEAD");
        assert_eq!(s.files.len(), 1);
    }

    #[test]
    fn parses_only_ahead() {
        let s = parse_status("## feature...origin/feature [ahead 3]");
        assert_eq!(s.ahead, 3);
        assert_eq!(s.behind, 0);
    }

    #[test]
    fn parses_unborn_branch() {
        // Fresh repo with no commits yet.
        let s = parse_status("## No commits yet on master\nA  a.txt");
        assert_eq!(s.branch, "master");
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.files[0].code, "A ");
    }
}

#[cfg(test)]
mod cov_tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// A temp directory removed on drop.
    struct TmpDir {
        dir: PathBuf,
    }
    impl Drop for TmpDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.dir);
        }
    }
    fn tmp() -> TmpDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("bru_git_test_{}_{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        TmpDir { dir }
    }

    /// Run a raw `git` command directly (NOT via the production `run` helper) so
    /// test setup never depends on the code under test. Returns false if the
    /// command could not be launched or exited non-zero.
    fn git_ok(dir: &Path, args: &[&str]) -> bool {
        let mut cmd = Command::new("git");
        cmd.args(args).current_dir(dir);
        hide_console(&mut cmd);
        match cmd.output() {
            Ok(out) => out.status.success(),
            Err(_) => false,
        }
    }

    /// Initialise a repo with deterministic identity + default branch. Returns
    /// `None` (and the caller should skip) when `git` is unavailable.
    fn init_repo(dir: &Path) -> Option<()> {
        if !git_ok(dir, &["init"]) {
            return None;
        }
        // Local identity so `run_op`'s bare `git commit` works in CI sandboxes.
        git_ok(dir, &["config", "user.email", "test@example.com"]);
        git_ok(dir, &["config", "user.name", "Test"]);
        // Name the branch deterministically regardless of the host's init default.
        git_ok(dir, &["checkout", "-B", "main"]);
        Some(())
    }

    // ---- parse_status / field branches not covered by the existing `tests` mod ----

    #[test]
    fn parse_status_empty_input_is_default() {
        let s = parse_status("");
        assert_eq!(s.branch, "");
        assert_eq!(s.ahead, 0);
        assert_eq!(s.behind, 0);
        assert!(!s.is_dirty());
        assert!(s.files.is_empty());
    }

    #[test]
    fn parse_status_only_behind() {
        let s = parse_status("## main...origin/main [behind 5]");
        assert_eq!(s.branch, "main");
        assert_eq!(s.ahead, 0);
        assert_eq!(s.behind, 5);
    }

    #[test]
    fn parse_status_skips_short_lines() {
        // A line shorter than 3 chars is neither a branch header nor a file entry.
        let s = parse_status("## main\nX\n M ok.rs");
        assert_eq!(s.branch, "main");
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.files[0].path, "ok.rs");
    }

    #[test]
    fn parse_status_branch_without_upstream_or_bracket() {
        // No `...upstream` and no `[ahead/behind]`: the find('[') branch is skipped.
        let s = parse_status("## release-1.2");
        assert_eq!(s.branch, "release-1.2");
        assert_eq!(s.ahead, 0);
        assert_eq!(s.behind, 0);
    }

    #[test]
    fn parse_status_renamed_entry_keeps_arrow_path() {
        // A staged rename is reported with `old -> new`; path is everything after
        // the 3-char prefix.
        let s = parse_status("## main\nR  old.rs -> new.rs");
        assert_eq!(s.files.len(), 1);
        assert_eq!(s.files[0].code, "R ");
        assert_eq!(s.files[0].path, "old.rs -> new.rs");
    }

    #[test]
    fn field_absent_key_is_zero() {
        assert!(field("[ahead 4]", "behind ") == 0);
        assert!(field("[ahead 4]", "ahead ") == 4);
    }

    #[test]
    fn field_non_numeric_is_zero() {
        // Key present but not followed by digits -> parse fails -> 0.
        assert!(field("[ahead , behind 2]", "ahead ") == 0);
        assert!(field("[ahead , behind 2]", "behind ") == 2);
    }

    // ---- Status::is_dirty ----

    #[test]
    fn status_is_dirty_reflects_files() {
        let mut s = Status::default();
        assert!(!s.is_dirty());
        s.files.push(FileEntry {
            code: "??".to_string(),
            path: "x".to_string(),
        });
        assert!(s.is_dirty());
    }

    // ---- Op::label for every variant ----

    #[test]
    fn op_label_covers_all_variants() {
        assert_eq!(Op::StageAll.label(), "Staging");
        assert_eq!(Op::Commit("m".to_string()).label(), "Committing");
        assert_eq!(Op::Discard.label(), "Discarding");
        assert_eq!(Op::Fetch.label(), "Fetching");
        assert_eq!(Op::Pull.label(), "Pulling");
        assert_eq!(Op::Push.label(), "Pushing");
    }

    // ---- run_op error paths that do NOT need a live repo ----

    #[test]
    fn run_op_commit_empty_message_errors() {
        let t = tmp();
        let err = run_op(&t.dir, &Op::Commit("   ".to_string())).unwrap_err();
        assert_eq!(err, "Commit message is empty");
    }

    // ---- is_repo on a plain (non-git) directory ----

    #[test]
    fn is_repo_false_for_plain_dir() {
        let t = tmp();
        // Whether git is installed or not, a bare temp dir is not a work tree.
        assert!(!is_repo(&t.dir));
    }

    // ---- Live-git integration: skip cleanly when git is unavailable ----

    #[test]
    fn is_repo_true_after_init() {
        let t = tmp();
        if init_repo(&t.dir).is_none() {
            return; // git not installed; skip.
        }
        assert!(is_repo(&t.dir));
    }

    #[test]
    fn status_unborn_repo_has_no_commits() {
        let t = tmp();
        if init_repo(&t.dir).is_none() {
            return;
        }
        // Fresh repo, no commits, no files: clean working tree.
        let s = status(&t.dir).expect("status on fresh repo");
        assert!(!s.is_dirty());
        assert!(s.files.is_empty());
    }

    #[test]
    fn status_reports_untracked_then_staged() {
        let t = tmp();
        if init_repo(&t.dir).is_none() {
            return;
        }
        fs::write(t.dir.join("a.txt"), "hello").unwrap();
        let s = status(&t.dir).expect("status with untracked file");
        assert!(s.is_dirty());
        assert!(s.files.iter().any(|f| f.path == "a.txt" && f.code == "??"));

        // StageAll moves it to a staged add.
        let msg = run_op(&t.dir, &Op::StageAll).expect("stage all");
        assert_eq!(msg, "Staged all changes");
        let s = status(&t.dir).expect("status after stage");
        assert!(s
            .files
            .iter()
            .any(|f| f.path == "a.txt" && f.code.starts_with('A')));
    }

    #[test]
    fn run_op_commit_then_clean_tree() {
        let t = tmp();
        if init_repo(&t.dir).is_none() {
            return;
        }
        fs::write(t.dir.join("file.txt"), "v1").unwrap();
        // Commit stages everything first, then commits; result is the first line.
        let line =
            run_op(&t.dir, &Op::Commit("initial commit".to_string())).expect("commit succeeds");
        assert!(!line.is_empty(), "commit produced no summary line");
        // After committing, the working tree is clean.
        let s = status(&t.dir).expect("status after commit");
        assert!(
            !s.is_dirty(),
            "tree should be clean after commit: {:?}",
            s.files
        );
        // Branch name should be the deterministic one we created.
        assert_eq!(s.branch, "main");
    }

    #[test]
    fn run_op_discard_reverts_tracked_changes() {
        let t = tmp();
        if init_repo(&t.dir).is_none() {
            return;
        }
        let f = t.dir.join("tracked.txt");
        fs::write(&f, "original").unwrap();
        run_op(&t.dir, &Op::Commit("add tracked".to_string())).expect("first commit");
        // Modify the tracked file, then discard.
        fs::write(&f, "modified").unwrap();
        let s = status(&t.dir).expect("status dirty");
        assert!(s.is_dirty());
        let msg = run_op(&t.dir, &Op::Discard).expect("discard");
        assert_eq!(msg, "Discarded tracked changes");
        assert_eq!(fs::read_to_string(&f).unwrap(), "original");
        let s = status(&t.dir).expect("status clean after discard");
        assert!(!s.is_dirty());
    }

    #[test]
    fn run_op_commit_with_nothing_staged_errors() {
        let t = tmp();
        if init_repo(&t.dir).is_none() {
            return;
        }
        // Make one commit so the repo is born, then commit again with no changes:
        // `git commit` exits non-zero -> run_op surfaces an Err.
        fs::write(t.dir.join("seed.txt"), "s").unwrap();
        run_op(&t.dir, &Op::Commit("seed".to_string())).expect("seed commit");
        let res = run_op(&t.dir, &Op::Commit("nothing to do".to_string()));
        assert!(res.is_err(), "empty commit should error, got {res:?}");
    }

    #[test]
    fn status_branch_and_ahead_behind_roundtrip_via_real_git() {
        let t = tmp();
        if init_repo(&t.dir).is_none() {
            return;
        }
        fs::write(t.dir.join("x.txt"), "x").unwrap();
        run_op(&t.dir, &Op::Commit("c1".to_string())).expect("commit");
        // No upstream configured, so ahead/behind stay 0 but branch is parsed.
        let s = status(&t.dir).expect("status");
        assert_eq!(s.branch, "main");
        assert_eq!(s.ahead, 0);
        assert_eq!(s.behind, 0);
    }
}
