//! Plans what a confirmed removal would take, without choosing how to ask.

use super::{Repo, Worktree, expired, same_path, strays};

/// WorktreeImpact is a checkout and the count of changed files it would lose.
#[derive(Debug, Clone, PartialEq)]
pub struct WorktreeImpact {
    pub worktree: Worktree,
    pub changed: usize,
}

/// RemovalPlan is the work a repository removal would take with it.
#[derive(Debug, Clone, PartialEq)]
pub struct RemovalPlan {
    pub dirty: usize,
    pub worktrees: Vec<WorktreeImpact>,
    pub strays: Vec<String>,
}

/// removal_plan excludes the main checkout, counts the work in each linked
/// checkout once, and names everything under gm's worktree directory that
/// delete would take without git knowing about it. The caller supplies the
/// repository's dirty signal because CLI and finder intentionally use
/// different status policies.
pub fn removal_plan(
    r: &Repo,
    dirty: usize,
    worktrees: &[Worktree],
    mut changed: impl FnMut(&str) -> usize,
) -> RemovalPlan {
    let others: Vec<_> = worktrees
        .iter()
        .filter(|w| !same_path(&w.path, &r.path()))
        .cloned()
        .collect();
    let worktrees: Vec<_> = others
        .iter()
        .map(|worktree| WorktreeImpact {
            worktree: worktree.clone(),
            changed: changed(&worktree.path),
        })
        .collect();
    RemovalPlan {
        dirty,
        strays: strays(r, &others),
        worktrees,
    }
}

/// WorktreeRemovalPlan is one checkout and the exact signal that decides
/// whether git must be told to force its removal.
#[derive(Debug, Clone, PartialEq)]
pub struct WorktreeRemovalPlan {
    pub worktree: Worktree,
    pub changed: usize,
    pub force: bool,
}

/// worktree_removal_plan turns the caller's existing dirty signal into the
/// force decision. The caller keeps its own status/error policy.
pub fn worktree_removal_plan(worktree: Worktree, changed: usize) -> WorktreeRemovalPlan {
    WorktreeRemovalPlan {
        worktree,
        changed,
        force: changed > 0,
    }
}

/// ExpiryPlan divides aged worktrees into those safe to remove and those
/// kept for uncommitted work.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExpiryPlan {
    pub remove: Vec<WorktreeImpact>,
    pub keep: Vec<WorktreeImpact>,
}

/// expiry_plan finds worktrees past the requested age and checks each one for
/// changes exactly once. Main, bare, unknown-age, recent, and dirty worktrees
/// are never added to the removal list.
pub fn expiry_plan(
    dir: &str,
    worktrees: &[Worktree],
    days: i64,
    now: i64,
    mut changed: impl FnMut(&str) -> usize,
) -> ExpiryPlan {
    let mut plan = ExpiryPlan::default();
    for worktree in expired(dir, worktrees, days, now) {
        let impact = WorktreeImpact {
            changed: changed(&worktree.path),
            worktree,
        };
        if impact.changed == 0 {
            plan.remove.push(impact);
        } else {
            plan.keep.push(impact);
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;
    use crate::repo::{self, Repo, Tree, Worktree};
    use crate::testutil::{TempDir, git_repo, mkdir, write};

    #[test]
    fn repository_removal_plan_names_dirty_worktrees_and_strays() {
        let root = TempDir::new();
        let r = Repo {
            root: root.path(),
            rel: "github.com/acme/alpha".into(),
        };
        git_repo(&r.path());
        let tree = Tree {
            roots: vec![root.path()],
        };
        let login = tree.worktree_dir(&r, "feat/login");
        repo::add_worktree(&root.path(), &r.path(), &login, "feat/login").unwrap();
        write(&paths::join(&login, "wip.txt"), "wip\n");
        mkdir(&tree.worktree_dir(&r, "scratch"));

        let mut scanned = Vec::new();
        let plan = removal_plan(&r, 0, &repo::worktrees(&r.path()).unwrap(), |path| {
            scanned.push(path.to_string());
            repo::changed_files(path)
        });

        assert_eq!(plan.dirty, 0);
        assert_eq!(plan.worktrees.len(), 1);
        assert_eq!(plan.worktrees[0].worktree.branch, "feat/login");
        assert_eq!(plan.worktrees[0].changed, 1);
        assert_eq!(plan.strays, ["scratch"]);
        assert_eq!(scanned, [plan.worktrees[0].worktree.path.clone()]);
    }

    #[test]
    fn worktree_removal_plan_keeps_the_supplied_force_signal() {
        let worktree = Worktree {
            path: "/worktree/login".into(),
            branch: "feat/login".into(),
            ..Default::default()
        };

        let plan = worktree_removal_plan(worktree.clone(), 3);

        assert_eq!(plan.worktree, worktree);
        assert_eq!(plan.changed, 3);
        assert!(plan.force);
        let clean = worktree_removal_plan(worktree, 0);
        assert!(!clean.force);
    }

    #[test]
    fn expiry_plan_removes_only_idle_clean_worktrees() {
        let root = TempDir::new();
        let r = Repo {
            root: root.path(),
            rel: "github.com/acme/alpha".into(),
        };
        git_repo(&r.path());
        let tree = Tree {
            roots: vec![root.path()],
        };
        let wt_root = paths::join(&r.root, repo::WORKTREE_ROOT);
        for branch in ["old/clean", "old/dirty", "busy"] {
            repo::add_worktree(&wt_root, &r.path(), &tree.worktree_dir(&r, branch), branch)
                .unwrap();
        }
        write(
            &paths::join(&tree.worktree_dir(&r, "old/dirty"), "wip.txt"),
            "wip\n",
        );
        let mut worktrees = repo::worktrees(&r.path()).unwrap();
        for worktree in &mut worktrees {
            worktree.used_at = if worktree.branch.starts_with("old/") {
                1
            } else {
                199_999
            };
        }

        let mut scanned = Vec::new();
        let plan = expiry_plan(&r.path(), &worktrees, 1, 200_000, |path| {
            scanned.push(path.to_string());
            repo::changed_files(path)
        });

        assert_eq!(
            plan.remove
                .iter()
                .map(|impact| impact.worktree.branch.as_str())
                .collect::<Vec<_>>(),
            ["old/clean"]
        );
        assert_eq!(
            plan.keep
                .iter()
                .map(|impact| impact.worktree.branch.as_str())
                .collect::<Vec<_>>(),
            ["old/dirty"]
        );
        assert_eq!(plan.keep[0].changed, 1);
        assert_eq!(
            scanned,
            [
                plan.remove[0].worktree.path.clone(),
                plan.keep[0].worktree.path.clone()
            ]
        );
    }
}
