//! Strays: what sits in a repository's worktree directory that git does
//! not know as a checkout.

use crate::repo::{Repo, Worktree, worktrees_dir};

/// strays are what delete would take from r's worktree directory besides the
/// checkouts in wts: a checkout git has lost track of, or files put there by
/// hand. Symlinks are named as entries and never followed, since delete takes
/// the link itself, not its target. They are named relative to that directory.
pub fn strays(r: &Repo, wts: &[Worktree]) -> Vec<String> {
    // Git prints /private/var where the tree has /var; resolve its paths and
    // the root once, then compare every entry lexically without following it.
    let Ok(root) = std::fs::canonicalize(worktrees_dir(r)) else {
        return vec![];
    };
    let wts: Vec<_> = wts
        .iter()
        .filter_map(|w| std::fs::canonicalize(&w.path).ok())
        .filter(|w| w.starts_with(&root))
        .collect();
    let mut out = vec![];
    walk_strays(&root, "", &wts, &mut out);
    out
}

fn walk_strays(
    dir: &std::path::Path,
    rel: &str,
    wts: &[std::path::PathBuf],
    out: &mut Vec<String>,
) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<String> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for n in names {
        let p = dir.join(&n);
        let label = if rel.is_empty() {
            n
        } else {
            format!("{rel}/{n}")
        };
        let Ok(metadata) = std::fs::symlink_metadata(&p) else {
            out.push(label);
            continue;
        };
        if metadata.file_type().is_symlink() {
            out.push(label);
        } else if wts.contains(&p) {
            continue;
        } else if metadata.is_dir() && wts.iter().any(|w| w.starts_with(&p)) {
            // A branch directory like feat/ holds checkouts, and may hold
            // something else beside them.
            walk_strays(&p, &label, wts, out);
        } else {
            out.push(label);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;
    use crate::repo::{Tree, add_worktree, other_worktrees};
    use crate::testutil::{TempDir, git_repo, mkdir, write};

    // The bug where gm remove deleted a directory under .worktrees that was
    // not one of git's checkouts without naming it first.
    #[test]
    fn strays_names_what_is_not_a_worktree() {
        let base = TempDir::new();
        let r = Repo {
            root: base.path(),
            rel: "github.com/acme/alpha".into(),
        };
        git_repo(&r.path());
        let tree = Tree {
            roots: vec![base.path()],
        };
        let login = tree.worktree_dir(&r, "feat/login");
        add_worktree(&base.path(), &r.path(), &login, "feat/login").unwrap();
        assert!(strays(&r, &other_worktrees(&r)).is_empty());

        mkdir(&tree.worktree_dir(&r, "scratch"));
        write(&tree.worktree_dir(&r, "feat/notes.txt"), "x\n");
        assert_eq!(
            strays(&r, &other_worktrees(&r)),
            ["feat/notes.txt", "scratch"]
        );
    }

    #[test]
    fn strays_names_symlinks_without_following_them() {
        let base = TempDir::new();
        let r = Repo {
            root: base.path(),
            rel: "github.com/acme/alpha".into(),
        };
        git_repo(&r.path());
        let tree = Tree {
            roots: vec![base.path()],
        };
        let login = tree.worktree_dir(&r, "feat/login");
        add_worktree(&base.path(), &r.path(), &login, "feat/login").unwrap();
        let root = worktrees_dir(&r);
        let ancestor = paths::join(&root, "ancestor");
        let checkout = paths::join(&root, "checkout");
        std::os::unix::fs::symlink(base.path(), &ancestor).unwrap();
        std::os::unix::fs::symlink(&login, &checkout).unwrap();

        assert_eq!(strays(&r, &other_worktrees(&r)), ["ancestor", "checkout"]);
    }
}
