//! Expiry: the worktrees whose HEAD has not moved for a given age, and
//! taking them away with their merged branches.

use crate::repo::{Repo, Worktree, remove_worktree_and_branch, same_path};
use crate::{Result, err};

/// parse_days reads the age /expire and gm wt expire take, "30d", as days.
pub fn parse_days(s: &str) -> Result<i64> {
    match s.strip_suffix('d').and_then(|n| n.parse::<i64>().ok()) {
        Some(n) if n > 0 => Ok(n),
        _ => Err(err!("{s:?} is not an age: write it in days, like 30d")),
    }
}

/// expired picks the worktrees of the repository at dir whose HEAD has not
/// moved for days: the reflog says when it last did. One without a reflog has
/// no age and stays, and so does the main worktree, which is the repository.
pub fn expired(dir: &str, wts: &[Worktree], days: i64, now: i64) -> Vec<Worktree> {
    let cutoff = now - days * 86_400;
    wts.iter()
        .filter(|w| !w.bare && w.used_at > 0 && w.used_at < cutoff && !same_path(&w.path, dir))
        .cloned()
        .collect()
}

/// Expiry is what expire did: the worktrees it removed, how many of their
/// branches went with them, and a line for each worktree git refused.
#[derive(Debug, Default)]
pub struct Expiry {
    pub removed: Vec<String>,
    pub branches: usize,
    pub failed: Vec<String>,
}

/// expire removes the worktrees in wts and each one's merged branch. The
/// worktree is not forced either, so git keeps one with changes in it.
pub fn expire(r: &Repo, wts: &[Worktree]) -> Expiry {
    let mut done = Expiry::default();
    for w in wts {
        match remove_worktree_and_branch(r, &w.path, &w.branch, false) {
            Err(e) => done.failed.push(format!("{}: {e}", w.label())),
            Ok(gone) => {
                done.removed.push(w.path.clone());
                done.branches += usize::from(gone);
            }
        }
    }
    done
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;
    use crate::repo::{Tree, WORKTREE_ROOT, add_worktree, branch_exists, now, worktrees};
    use crate::testutil::{TempDir, exists, git_repo, write};

    #[test]
    fn parse_days_takes_days_only() {
        assert_eq!(parse_days("30d"), Ok(30));
        for bad in ["0d", "30", "d", "-1d", "1w"] {
            assert!(parse_days(bad).is_err(), "{bad:?}");
        }
    }

    // Against real git: the age comes from the worktree's own reflog, the
    // main worktree never expires, and expire takes a clean worktree with its
    // merged branch while git keeps the one with work in it.
    #[test]
    fn expire_removes_idle_worktrees_and_keeps_work() {
        let base = TempDir::new();
        let r = Repo {
            root: base.path(),
            rel: "github.com/acme/alpha".into(),
        };
        git_repo(&r.path());
        let tree = Tree {
            roots: vec![base.path()],
        };
        let root = paths::join(&r.root, WORKTREE_ROOT);
        let (idle, busy) = (tree.worktree_dir(&r, "idle"), tree.worktree_dir(&r, "busy"));
        add_worktree(&root, &r.path(), &idle, "idle").unwrap();
        add_worktree(&root, &r.path(), &busy, "busy").unwrap();
        write(&paths::join(&busy, "work.txt"), "unsaved");

        let wts = worktrees(&r.path()).unwrap();
        assert!(wts[1..].iter().all(|w| w.used_at > 0), "{wts:?}");
        let now = now();
        assert!(expired(&r.path(), &wts, 30, now).is_empty());
        let old = expired(&r.path(), &wts, 30, now + 31 * 86_400);
        assert_eq!(old.len(), 2, "the main worktree must not expire: {old:?}");

        let done = expire(&r, &old);
        assert_eq!(done.removed.len(), 1, "{done:?}");
        assert!(!exists(&idle) && !branch_exists(&r.path(), "idle"));
        assert_eq!(done.branches, 1);
        assert!(done.failed[0].starts_with("busy: "), "{done:?}");
        assert!(exists(&paths::join(&busy, "work.txt")));
    }
}
