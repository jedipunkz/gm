//! gm remove takes a repository, its worktrees and the work in them out of
//! the tree, asking first.

use crate::Result;
use crate::repo::{self, Repo};

use super::{App, REMOVE_FLAGS, missing_usage, parse};

impl App<'_> {
    pub(super) fn remove(&mut self, args: &[String]) -> Result<()> {
        let p = parse(self, "gm remove", REMOVE_FLAGS, args)?;
        if p.args.is_empty() {
            return Err(missing_usage("remove").into());
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
        // discovered afterwards. A checkout outside gm's worktree directory
        // stays where it is, and this is the only place that says where.
        let (outside, going): (Vec<_>, Vec<_>) = plan
            .worktrees
            .iter()
            .partition(|impact| repo::outside_worktrees_dir(r, &impact.worktree.path));
        if !going.is_empty() {
            writeln!(self.err, "warning: {}", worktree_warning(r, &going))?;
        }
        if !outside.is_empty() {
            let wts: Vec<_> = outside.iter().map(|i| i.worktree.clone()).collect();
            writeln!(
                self.err,
                "warning: {} not under {} will stay: {}",
                crate::plural(wts.len(), "worktree", "worktrees"),
                repo::worktrees_dir(r),
                repo::worktree_labels_in(&repo::worktrees_dir(r), &wts, |_| 0)
            )?;
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

/// worktree_warning words the checkouts delete takes, each with the work
/// that goes with it.
fn worktree_warning(r: &Repo, going: &[&repo::WorktreeImpact]) -> String {
    let worktrees: Vec<_> = going.iter().map(|impact| impact.worktree.clone()).collect();
    let labels = repo::worktree_labels_in(&repo::worktrees_dir(r), &worktrees, |path| {
        going
            .iter()
            .find(|impact| impact.worktree.path == path)
            .map_or(0, |impact| impact.changed)
    });
    format!(
        "{} will go too: {labels}",
        crate::plural(worktrees.len(), "worktree", "worktrees")
    )
}
