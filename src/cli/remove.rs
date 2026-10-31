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
        let dirty = usize::from(repo::is_dirty(&r.path()).unwrap_or(false));
        let worktrees = repo::worktrees(&r.path()).unwrap_or_default();
        let plan = repo::removal_plan(r, dirty, &worktrees, repo::changed_files);
        if plan.dirty > 0 {
            writeln!(self.err, "warning: {} has uncommitted changes", r.rel)?;
        }
        // They go with it, so they have to be said before the question, not
        // discovered afterwards.
        if !plan.worktrees.is_empty() {
            writeln!(self.err, "warning: {}", worktree_warning(r, &plan))?;
        }
        if !plan.strays.is_empty() {
            writeln!(
                self.err,
                "warning: also removed, not a worktree: {}",
                plan.strays.join(", ")
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

/// worktree_warning words the impacts the repository layer found. A checkout
/// outside gm's worktree directory is named by its path, which only this
/// warning says.
fn worktree_warning(r: &Repo, plan: &repo::RemovalPlan) -> String {
    let worktrees: Vec<_> = plan
        .worktrees
        .iter()
        .map(|impact| impact.worktree.clone())
        .collect();
    let labels = repo::worktree_labels_in(&repo::worktrees_dir(r), &worktrees, |path| {
        plan.worktrees
            .iter()
            .find(|impact| impact.worktree.path == path)
            .map_or(0, |impact| impact.changed)
    });
    let what = if worktrees.len() == 1 {
        "worktree"
    } else {
        "worktrees"
    };
    format!("{} {what} will go too: {labels}", worktrees.len())
}
