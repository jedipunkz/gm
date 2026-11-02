//! gm migrate: moving a repository cloned elsewhere into the tree.

use std::process::{Command, Stdio};

use crate::repo;
use crate::{Result, err, paths, plural};

use super::{App, MIGRATE_FLAGS, missing_usage, parse};

impl App<'_> {
    pub(super) fn migrate(&mut self, args: &[String]) -> Result<()> {
        let p = parse(self, "gm migrate", MIGRATE_FLAGS, args)?;
        if p.args.is_empty() {
            return Err(missing_usage("migrate").into());
        }
        let recursive = p.on("r");

        for src in self.migrate_sources(&p.args, recursive)? {
            let dst = match self.migrate_plan(&src) {
                Plan::InPlace => {
                    writeln!(self.err, "in place {src}")?;
                    continue;
                }
                // A directory named on the command line is the user's claim
                // that it should move; a directory the search turned up is
                // only a candidate, so it is skipped rather than fatal.
                Plan::Problem(why) if !recursive => return Err(err!("{src} {why}")),
                Plan::Problem(why) => {
                    writeln!(self.err, "skip     {src}: {why}")?;
                    continue;
                }
                Plan::Move(dst) => dst,
            };
            // Each checkout's .git file names the repository's path, so they
            // have to be asked about before it moves and pointed at the new
            // place after. One whose directory is already gone has nothing
            // to point.
            let wts: Vec<repo::Worktree> = repo::other_worktrees_of(&src)
                .into_iter()
                .filter(|w| paths::exists(&w.path))
                .collect();
            let along = |wts: &[repo::Worktree]| {
                let labels: Vec<String> = wts.iter().map(repo::Worktree::label).collect();
                format!(
                    "{}: {}",
                    plural(wts.len(), "worktree", "worktrees"),
                    labels.join(", ")
                )
            };
            if p.on("dry-run") {
                writeln!(self.err, "would move {src} -> {dst}")?;
                if !wts.is_empty() {
                    writeln!(self.err, "  and repair its {}", along(&wts))?;
                }
                continue;
            }
            if !wts.is_empty() {
                writeln!(self.err, "its {} will be repaired", along(&wts))?;
            }
            if !p.on("y") && !self.confirm(&format!("move {src} -> {dst}?")) {
                writeln!(self.err, "skipped")?;
                continue;
            }
            // Worked out before the move: src has to exist to be resolved.
            let dirs: Vec<String> = wts
                .iter()
                .map(|w| moved_along(&w.path, &src, &dst))
                .collect();
            move_dir(&src, &dst)?;
            self.bump(&dst);
            writeln!(self.err, "moved    {src} -> {dst}")?;
            if let Err(e) = repo::repair_worktrees(&dst, &dirs) {
                return Err(err!(
                    "moved {src} -> {dst}, but its worktrees still point at the old place ({e}); \
                     run `git -C {dst} worktree repair <worktree>...`"
                ));
            }
            if !wts.is_empty() {
                writeln!(self.err, "repaired {}", along(&wts))?;
            }
            writeln!(self.out, "{dst}")?;
        }
        Ok(())
    }

    /// migrate_sources turns the arguments into the directories to consider.
    /// Each one is a repository, or with -r a directory to search;
    /// repositories already under a root are dropped, since the whole point is
    /// to move the ones that are not.
    fn migrate_sources(&self, args: &[String], recursive: bool) -> Result<Vec<String>> {
        let mut srcs = Vec::new();
        for arg in args {
            let dir = paths::abs(arg)?;
            if !recursive {
                srcs.push(dir);
                continue;
            }
            srcs.extend(
                repo::find_repos(&dir)
                    .into_iter()
                    .filter(|p| !self.tree.contains(p)),
            );
        }
        Ok(srcs)
    }

    /// migrate_plan decides where a repository belongs, without touching
    /// anything.
    fn migrate_plan(&self, src: &str) -> Plan {
        if !repo::is_repo(src) {
            return Plan::Problem("is not a repository".into());
        }
        // A .git file means a worktree or submodule; moving it breaks the link.
        if std::fs::metadata(paths::join(src, ".git")).is_ok_and(|m| !m.is_dir()) {
            return Plan::Problem(
                "is a worktree or submodule and cannot be moved on its own".into(),
            );
        }
        let remote = match repo::git_in(src, &["remote", "get-url", "origin"]) {
            Ok(r) if !r.is_empty() => r,
            _ => return Plan::Problem("has no origin remote".into()),
        };
        let Ok(u) = repo::normalize_url(&remote, false) else {
            return Plan::Problem(format!("has an origin gm cannot read ({remote})"));
        };
        // Any root can hold the repository, not just the primary one: under
        // another root it is in place already, and moving a stray clone of
        // it in would make a second copy. same_path lets a root reached
        // through a symlink still count as the root.
        let rel = repo::rel_path_of(&u);
        let existing = self.tree.existing_paths(&rel);
        if existing.iter().any(|p| repo::same_path(p, src)) {
            return Plan::InPlace;
        }
        if let Some(other) = existing.first() {
            return Plan::Problem(format!("is already cloned at {other}"));
        }
        let dst = self.tree.path_for(&rel);
        if paths::exists(&dst) {
            return Plan::Problem(format!("would move to {dst}, which already exists"));
        }
        Plan::Move(dst)
    }
}

/// Plan is where one repository would move, or why it would not.
enum Plan {
    Move(String),    // where it goes
    InPlace,         // already where gm would put it
    Problem(String), // why it cannot move, phrased to follow the path
}

/// moved_along is where a checkout will be once its repository has moved from
/// src to dst: one kept inside the repository's directory goes with it. git
/// reports the resolved path, so src is compared resolved as well, which is
/// why this has to run before the move.
fn moved_along(path: &str, src: &str, dst: &str) -> String {
    let resolved = std::fs::canonicalize(src)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    [src, resolved.as_str()]
        .iter()
        .filter(|s| !s.is_empty())
        .find_map(|s| path.strip_prefix(&format!("{}/", s.trim_end_matches('/'))))
        .map_or_else(|| path.to_string(), |rest| paths::join(dst, rest))
}

/// move_dir renames src to dst, falling back to mv across filesystems.
fn move_dir(src: &str, dst: &str) -> Result<()> {
    std::fs::create_dir_all(paths::dir(dst))?;
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    // Rename fails across filesystems; mv copies and unlinks instead.
    let status = Command::new("mv")
        .args([src, dst])
        .stdout(Stdio::from(std::io::stderr()))
        .stderr(Stdio::from(std::io::stderr()))
        .status()?;
    if !status.success() {
        return Err(err!("moving {src} to {dst}: {}", repo::exit_error(&status)));
    }
    Ok(())
}
