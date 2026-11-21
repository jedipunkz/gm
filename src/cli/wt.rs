use crate::repo;
use crate::{Result, err, paths};

use super::{App, WT_VERBS, missing_usage, parse};

impl App<'_> {
    pub(super) fn wt(&mut self, args: &[String]) -> Result<()> {
        match args.first().map(String::as_str) {
            Some("create" | "new") => self.wt_create(&args[1..]),
            Some("remove" | "rm") => self.wt_remove(&args[1..]),
            Some("expire") => self.wt_expire(&args[1..]),
            _ => Err(missing_usage("wt").into()),
        }
    }

    pub(super) fn wt_create(&mut self, args: &[String]) -> Result<()> {
        let verb = WT_VERBS
            .iter()
            .find(|verb| verb.name == "create")
            .expect("wt create has a verb definition");
        let p = parse(self, "gm wt create", verb.flags, args)?;
        let [query, branch] = p.args.as_slice() else {
            return Err(missing_usage("wt").into());
        };
        let r = self.tree.resolve(query)?;
        let dir = self.tree.worktree_dir(&r, branch);
        if paths::exists(&dir) {
            return Err(err!("{dir} already exists"));
        }
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

    fn wt_remove(&mut self, args: &[String]) -> Result<()> {
        let verb = WT_VERBS
            .iter()
            .find(|verb| verb.name == "remove")
            .expect("wt remove has a verb definition");
        let p = parse(self, "gm wt remove", verb.flags, args)?;
        let [query, branch] = p.args.as_slice() else {
            return Err(missing_usage("wt").into());
        };
        let r = self.tree.resolve(query)?;
        let dir = self.tree.worktree_dir(&r, branch);

        let Some(w) = repo::other_worktrees(&r)
            .into_iter()
            .find(|w| repo::same_path(&w.path, &dir))
        else {
            return Err(err!("{} has no worktree for {branch}", r.rel));
        };
        let also = match w.branch.as_str() {
            "" => String::new(),
            b => format!(" and branch {b} if merged"),
        };
        let dirty = repo::is_dirty(&dir).unwrap_or(false);
        let plan = repo::worktree_removal_plan(w.clone(), usize::from(dirty));
        if plan.changed > 0 {
            writeln!(self.err, "warning: {dir} has uncommitted changes")?;
        }
        if p.on("dry-run") {
            writeln!(self.err, "would remove {dir}{also}")?;
            return Ok(());
        }
        if !p.on("y") && !self.confirm(&format!("remove {dir}{also}?")) {
            writeln!(self.err, "skipped")?;
            return Ok(());
        }
        let gone = repo::remove_worktree_and_branch(&r, &dir, &w.branch, plan.force)?;
        writeln!(self.err, "removed  {dir}")?;
        if !w.branch.is_empty() {
            let what = if gone {
                "deleted "
            } else {
                "kept     (not merged)"
            };
            writeln!(self.err, "{what} branch {}", w.branch)?;
        }
        Ok(())
    }

    fn wt_expire(&mut self, args: &[String]) -> Result<()> {
        let verb = WT_VERBS
            .iter()
            .find(|verb| verb.name == "expire")
            .expect("wt expire has a verb definition");
        let p = parse(self, "gm wt expire", verb.flags, args)?;
        let [query, age] = p.args.as_slice() else {
            return Err(missing_usage("wt").into());
        };
        let days = repo::parse_days(age)?;
        let r = self.tree.resolve(query)?;
        let plan = repo::expiry_plan(
            &r.path(),
            &repo::worktrees(&r.path())?,
            days,
            repo::now(),
            repo::changed_files,
        );
        for impact in &plan.keep {
            writeln!(
                self.err,
                "keep     {} (uncommitted changes)",
                impact.worktree.path
            )?;
        }
        if plan.remove.is_empty() {
            writeln!(self.err, "no worktree of {} idle for {days} days", r.rel)?;
            return Ok(());
        }
        let verb = if p.on("dry-run") {
            "would remove"
        } else {
            "remove"
        };
        for impact in &plan.remove {
            writeln!(self.err, "{verb} {}", impact.worktree.path)?;
        }
        if p.on("dry-run") {
            return Ok(());
        }
        let what = crate::plural(plan.remove.len(), "worktree", "worktrees");
        if !p.on("y") && !self.confirm(&format!("remove {what} and their merged branches?")) {
            writeln!(self.err, "skipped")?;
            return Ok(());
        }
        let clean: Vec<_> = plan
            .remove
            .iter()
            .map(|impact| impact.worktree.clone())
            .collect();
        let done = repo::expire(&r, &clean);
        for f in &done.failed {
            writeln!(self.err, "failed   {f}")?;
        }
        writeln!(
            self.err,
            "removed  {}, {}",
            crate::plural(done.removed.len(), "worktree", "worktrees"),
            crate::plural(done.branches, "branch", "branches")
        )?;
        Ok(())
    }
}
