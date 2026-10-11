//! gm remove takes a repository, its worktrees and the work in them out of
//! the tree, asking first.

use crate::Result;
use crate::repo::{self, Repo};

use super::{App, Flag, parse};

impl App<'_> {
    pub(super) fn remove(&mut self, args: &[String]) -> Result<()> {
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
        let p = parse(self, "gm remove", &flags, args)?;
        if p.args.is_empty() {
            return Err("usage: gm remove [--dry-run] [-y] <repo>...".into());
        }
        for q in &p.args {
            let r = self.tree.resolve(q)?;
            self.remove_one(&r, p.on("dry-run"), p.on("y"))?;
        }
        Ok(())
    }

    /// remove_one deletes one repository, warning first when there is work in
    /// it and asking before anything is lost.
    fn remove_one(&mut self, r: &Repo, dry_run: bool, yes: bool) -> Result<()> {
        if repo::is_dirty(&r.path()).unwrap_or(false) {
            writeln!(self.err, "warning: {} has uncommitted changes", r.rel)?;
        }
        // They go with it, so they have to be said before the question, not
        // discovered afterwards.
        let wts = repo::other_worktrees(r);
        if !wts.is_empty() {
            writeln!(self.err, "warning: {}", worktree_warning(r, &wts))?;
        }
        let strays = repo::strays(r, &wts);
        if !strays.is_empty() {
            writeln!(
                self.err,
                "warning: also removed, not a worktree: {}",
                strays.join(", ")
            )?;
        }
        // The warnings are what a dry run is for, so it stops only after them.
        if dry_run {
            writeln!(self.err, "would remove {}", r.path())?;
            return Ok(());
        }
        if !yes && !self.confirm(&format!("remove {}?", r.path())) {
            writeln!(self.err, "skipped")?;
            return Ok(());
        }
        let left = repo::delete(r)?;
        // A checkout that git refused and the tree does not own stays where
        // it is, and this is where that is said.
        for p in &left {
            writeln!(
                self.err,
                "warning: left in place, not under {}: {p}",
                repo::worktrees_dir(r)
            )?;
        }
        writeln!(self.err, "removed  {}", r.path())?;
        Ok(())
    }
}

/// worktree_warning names what will be taken along with a repository, and the
/// work in each of them: they are removed with --force, so git will not stop
/// for it. A checkout outside gm's worktree directory is named by its path,
/// which only the warning says.
fn worktree_warning(r: &Repo, wts: &[repo::Worktree]) -> String {
    let what = if wts.len() == 1 {
        "worktree"
    } else {
        "worktrees"
    };
    format!(
        "{} {what} will be removed too: {}",
        wts.len(),
        repo::worktree_labels_in(&repo::worktrees_dir(r), wts, repo::changed_files)
    )
}
