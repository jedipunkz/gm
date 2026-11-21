use std::process::Command;

use crate::repo;
use crate::{Result, paths, plural};

use super::{App, GET_FLAGS, missing_usage, parse};

impl App<'_> {
    fn advance(&mut self, dst: &str, submodules: bool) -> Result<()> {
        let branch = repo::git_in(dst, &["branch", "--show-current"])?;
        if branch.is_empty() {
            return Ok(()); // detached head: nothing to name or move
        }
        let upstream = repo::git_in(
            dst,
            &["rev-parse", "--abbrev-ref", &format!("{branch}@{{u}}")],
        )
        .ok()
        .filter(|u| !u.is_empty());
        let Some(upstream) = upstream else {
            return Ok(());
        };
        if repo::is_dirty(dst)? {
            writeln!(self.err, "skip     {dst}: dirty")?;
            return Ok(());
        }
        let old = repo::git_in(dst, &["rev-parse", "HEAD"])?;
        if let Err(e) = repo::git_quiet(&["-C", dst, "merge", "--ff-only", &upstream]) {
            writeln!(self.err, "skip     {dst}: diverged ({e})")?;
            return Ok(());
        }
        let new = repo::git_in(dst, &["rev-parse", "HEAD"])?;
        if new != old {
            let n = repo::git_in(dst, &["rev-list", "--count", &format!("{old}..{new}")])?;
            writeln!(
                self.err,
                "ff       {dst}: {}",
                plural(n.trim().parse().unwrap_or(0), "commit", "commits")
            )?;
        }
        if submodules && std::fs::metadata(paths::join(dst, ".gitmodules")).is_ok() {
            repo::git_quiet(&["-C", dst, "submodule", "update", "--init", "--recursive"])?;
        }
        Ok(())
    }

    pub(super) fn get(&mut self, args: &[String]) -> Result<()> {
        self.get_last(args).map(drop)
    }

    pub(super) fn get_last(&mut self, args: &[String]) -> Result<String> {
        let p = parse(self, "gm get", GET_FLAGS, args)?;
        if p.args.is_empty() {
            return Err(missing_usage("get").into());
        }

        let mut last = String::new();
        for reference in &p.args {
            let u = repo::normalize_url(reference, p.on("ssh"))?;
            let rel = repo::rel_path_of(&u);
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
                self.advance(&dst, !p.on("no-recursive"))?;
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
            look_in(&last)?;
        }
        Ok(last)
    }

    pub(super) fn create(&mut self, args: &[String]) -> Result<()> {
        let p = parse(self, "gm create", super::CREATE_FLAGS, args)?;
        let [reference] = p.args.as_slice() else {
            return Err(missing_usage("create").into());
        };
        let r = self.tree.create(reference, p.on("ssh"))?;
        self.bump(&r.path());
        writeln!(self.out, "{}", r.path())?;
        Ok(())
    }
}

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
