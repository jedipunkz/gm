//! The commands that write to the tree: get clones, create starts an empty
//! repository, migrate moves one in, and wt checks worktrees out beside it.

use std::process::{Command, Stdio};

use crate::repo;
use crate::{Result, err, paths, plural};

use super::{App, Flag, parse};

impl App<'_> {
    pub(super) fn get(&mut self, args: &[String]) -> Result<()> {
        let flags = [
            Flag {
                names: &["u", "update"],
                value: None,
                usage: "update the repository if it is already cloned",
            },
            Flag {
                names: &["p"],
                value: None,
                usage: "clone via SSH",
            },
            Flag {
                names: &["shallow"],
                value: None,
                usage: "do a shallow clone",
            },
            Flag {
                names: &["s", "silent"],
                value: None,
                usage: "clone quietly",
            },
            Flag {
                names: &["l", "look"],
                value: None,
                usage: "open a shell in the repository afterwards",
            },
            Flag {
                names: &["no-recursive"],
                value: None,
                usage: "do not clone submodules",
            },
            Flag {
                names: &["b", "branch"],
                value: Some("branch"),
                usage: "clone a single branch",
            },
        ];
        let p = parse(self, "gm get", &flags, args)?;
        if p.args.is_empty() {
            return Err("usage: gm get [options] <url>|<user>/<repo>|<repo>".into());
        }

        let mut last = String::new();
        for reference in &p.args {
            let u = repo::normalize_url(reference, p.on("p"))?;
            let rel = repo::rel_path_of(&u);
            // One repository under two roots is a real setup (a work laptop
            // and a private one, say), so cloning or updating "the" one needs
            // a name that says which. The finder learned this in #120; the
            // CLI answers the same way.
            let existing = self.tree.existing_paths(&rel);
            let dst = match existing.as_slice() {
                [] => self.tree.path_for(&rel),
                [one] => one.clone(),
                many => {
                    let mut msg = format!("{reference:?} exists under {} roots:", many.len());
                    for path in many {
                        msg.push_str(&format!("\n  {path}"));
                    }
                    return Err(msg.into());
                }
            };
            last = dst.clone();

            if !existing.is_empty() {
                if !p.on("u") {
                    writeln!(self.err, "exists   {dst}")?;
                    self.bump(&dst);
                    continue;
                }
                writeln!(self.err, "update   {dst}")?;
                repo::git(&["-C", &dst, "remote", "update", "--prune"])?;
                self.bump(&dst);
                continue;
            }
            std::fs::create_dir_all(paths::dir(&dst))?;

            let url = u.to_string();
            let branch = p.value("b");
            let mut clone = vec!["clone"];
            if p.on("shallow") {
                clone.extend(["--depth", "1"]);
            }
            if !branch.is_empty() {
                clone.extend(["--branch", &branch, "--single-branch"]);
            }
            if !p.on("no-recursive") {
                clone.push("--recursive");
            }
            if p.on("s") {
                clone.push("--quiet");
            }
            clone.extend([url.as_str(), dst.as_str()]);

            writeln!(self.err, "clone    {url} -> {dst}")?;
            if let Err(e) = repo::git(&clone) {
                repo::prune_empty_parents(self.tree.primary(), paths::dir(&dst));
                return Err(e);
            }
            self.bump(&dst);
        }

        if p.on("l") && !last.is_empty() {
            return look_in(&last);
        }
        Ok(())
    }

    pub(super) fn create(&mut self, args: &[String]) -> Result<()> {
        let flags = [Flag {
            names: &["p"],
            value: None,
            usage: "set the origin remote to its SSH URL",
        }];
        let p = parse(self, "gm create", &flags, args)?;
        let [reference] = p.args.as_slice() else {
            return Err("usage: gm create [-p] <repo>|<user>/<repo>|<host>/<user>/<repo>".into());
        };
        let r = self.tree.create(reference, p.on("p"))?;
        self.bump(&r.path());
        writeln!(self.out, "{}", r.path())?;
        Ok(())
    }

    /// wt is the command line half of the finder's worktree mode. The finder
    /// covers the interactive case better — the repository is the row under
    /// the cursor rather than something to name — so this exists for scripts:
    /// a bootstrap that checks out the branches you always want.
    ///
    /// The verbs are the finder's, so there is one set of names to remember:
    /// create and remove, spelled out, no abbreviations.
    pub(super) fn wt(&mut self, args: &[String]) -> Result<()> {
        match args.first().map(String::as_str) {
            Some("create") => self.wt_create(&args[1..]),
            Some("remove") => self.wt_remove(&args[1..]),
            _ => Err(WT_USAGE.into()),
        }
    }

    /// wt_create checks a branch out beside the repository and prints where,
    /// so a script can cd into it the way it can with gm create.
    fn wt_create(&mut self, args: &[String]) -> Result<()> {
        let p = parse(self, "gm wt create", &[], args)?;
        let [query, branch] = p.args.as_slice() else {
            return Err(WT_USAGE.into());
        };
        let r = self.tree.resolve(query)?;
        let dir = self.tree.worktree_dir(&r, branch);
        if paths::exists(&dir) {
            return Err(err!("{dir} already exists"));
        }
        // add_worktree starts the branch when it is new and checks it out when
        // it is not, which is the one thing worth saying before the path.
        let start = if repo::branch_exists(&r.path(), branch) {
            "existing branch"
        } else {
            "new branch"
        };
        repo::add_worktree(
            &paths::join(&r.root, repo::WORKTREE_ROOT),
            &r.path(),
            &dir,
            branch,
        )?;
        writeln!(self.err, "created  {} ({start})", r.rel)?;
        self.bump(&dir);
        writeln!(self.out, "{dir}")?;
        Ok(())
    }

    /// wt_remove takes one worktree away. It is destructive, so it says what is
    /// in the way first and asks, the way gm remove does.
    fn wt_remove(&mut self, args: &[String]) -> Result<()> {
        let flags = [
            Flag {
                names: &["dry-run"],
                value: None,
                usage: "show what would be removed",
            },
            Flag {
                names: &["y"],
                value: None,
                usage: "skip the confirmation prompt",
            },
        ];
        let p = parse(self, "gm wt remove", &flags, args)?;
        let [query, branch] = p.args.as_slice() else {
            return Err(WT_USAGE.into());
        };
        let r = self.tree.resolve(query)?;
        let dir = self.tree.worktree_dir(&r, branch);

        // Asking git which worktrees there are answers two questions at once:
        // is this one of them, and is it the main one — which is the
        // repository itself and must never be removed this way.
        if !repo::other_worktrees(&r)
            .iter()
            .any(|w| repo::same_path(&w.path, &dir))
        {
            return Err(err!("{} has no worktree for {branch}", r.rel));
        }
        if p.on("dry-run") {
            writeln!(self.err, "would remove {dir}")?;
            return Ok(());
        }
        // Work in the worktree is lost with it, so it is said before the
        // question, not discovered afterwards.
        let dirty = repo::is_dirty(&dir).unwrap_or(false);
        if dirty {
            writeln!(self.err, "warning: {dir} has uncommitted changes")?;
        }
        if !p.on("y") && !self.confirm(&format!("remove {dir}?")) {
            writeln!(self.err, "skipped")?;
            return Ok(());
        }
        repo::remove_worktree_and_prune(&r, &dir, dirty)?;
        writeln!(self.err, "removed  {dir}")?;
        Ok(())
    }

    pub(super) fn migrate(&mut self, args: &[String]) -> Result<()> {
        let flags = [
            Flag {
                names: &["dry-run"],
                value: None,
                usage: "show what would move",
            },
            Flag {
                names: &["y"],
                value: None,
                usage: "skip the confirmation prompt",
            },
            Flag {
                names: &["r", "recursive"],
                value: None,
                usage: "search the arguments for repositories instead of moving them",
            },
        ];
        let p = parse(self, "gm migrate", &flags, args)?;
        if p.args.is_empty() {
            return Err("usage: gm migrate [--dry-run] [-y] [-r] <directory>...".into());
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
        let dst = self.tree.path_for(&repo::rel_path_of(&u));
        if src == dst {
            return Plan::InPlace;
        }
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

const WT_USAGE: &str = "usage: gm wt <create|remove> [-y] <repo> <branch>";

/// look_in drops the user into a shell inside the repository just cloned.
fn look_in(dir: &str) -> Result<()> {
    let sh = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".into());
    let status = Command::new(sh)
        .current_dir(dir)
        .env("GM_LOOK", dir)
        .status()?;
    if !status.success() {
        return Err(repo::exit_error(&status));
    }
    Ok(())
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
