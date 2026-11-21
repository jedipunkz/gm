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

    fn remove_one(&mut self, r: &Repo, dry_run: bool, yes: bool) -> Result<()> {
        let dirty = usize::from(repo::is_dirty(&r.path()).unwrap_or(false));
        let worktrees = repo::worktrees(&r.path()).unwrap_or_default();
        let plan = repo::removal_plan(r, dirty, &worktrees, repo::changed_files);
        if plan.dirty > 0 {
            writeln!(self.err, "warning: {} has uncommitted changes", r.rel)?;
        }
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
        if dry_run {
            writeln!(self.err, "would remove {}", r.path())?;
            return Ok(());
        }
        if !yes && !self.confirm(&format!("remove {}?", r.path())) {
            writeln!(self.err, "skipped")?;
            return Ok(());
        }
        let left = repo::delete(r)?;
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
