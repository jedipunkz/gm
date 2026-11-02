//! Branches: which names git accepts, and which branches a worktree can
//! be made from.

use std::collections::HashSet;
use std::process::Stdio;

use super::{git_command, git_in, remotes};
use crate::Result;

/// valid_branch checks git's documented branch-name rules without starting a
/// process. gm asks before it builds a path out of one: a name like "../.."
/// would otherwise make a directory outside the tree before git ever saw it.
pub fn valid_branch(name: &str) -> bool {
    if name.is_empty()
        || name.starts_with('-')
        || name.starts_with('/')
        || name.ends_with('/')
        || name.contains("//")
        || name.contains("..")
        || name.contains("@{")
    {
        return false;
    }
    name.split('/').all(|part| {
        !part.is_empty()
            && !part.starts_with('.')
            && !part.ends_with('.')
            && !part.ends_with(".lock")
            && !part
                .bytes()
                .any(|b| b <= b' ' || b == 0x7f || b"~^:?*[\\".contains(&b))
    })
}

/// branch_exists reports whether the repository already has this branch,
/// which decides whether a worktree starts one or checks one out.
pub fn branch_exists(dir: &str, branch: &str) -> bool {
    git_command()
        .args([
            "-C",
            dir,
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Branch is one branch a worktree can be made from. remote is set when only a
/// remote has it, "origin/feature", and is where the local branch will start.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Branch {
    pub name: String,
    pub remote: String,
    /// unfetched is a branch the remote has that the last fetch did not
    /// bring: it is known by name only, and has to be fetched to be used.
    pub unfetched: bool,
}

impl Branch {
    /// label is how the branch reads in a list: by where it lives when that
    /// is only a remote.
    pub fn label(&self) -> String {
        if self.remote.is_empty() {
            self.name.clone()
        } else {
            self.remote.clone()
        }
    }
}

/// branches lists the local branches, and the remote ones with no local branch
/// of the same name, newest commit first. The remote ones are what the last
/// fetch left: opening a list is not the moment to go to the network.
pub fn branches(dir: &str) -> Result<Vec<Branch>> {
    let (names, _) = remotes(dir);
    let out = git_in(
        dir,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes",
        ],
    )?;
    Ok(parse_branches(&out, &names))
}

/// parse_branches reads full ref names. The remote names come from the caller,
/// for the reason commits needs them: "origin/main" is a remote's branch only
/// because origin is a remote.
pub fn parse_branches(out: &str, remotes: &[String]) -> Vec<Branch> {
    let lines: Vec<&str> = out.split('\n').collect();
    let local: HashSet<&str> = lines
        .iter()
        .filter_map(|l| l.strip_prefix("refs/heads/"))
        .collect();
    let prefixes: Vec<String> = remotes.iter().map(|r| format!("{r}/")).collect();
    let mut list = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for l in &lines {
        if let Some(name) = l.strip_prefix("refs/heads/") {
            list.push(Branch {
                name: name.to_string(),
                ..Default::default()
            });
            continue;
        }
        let Some(short) = l.strip_prefix("refs/remotes/") else {
            continue;
        };
        for prefix in &prefixes {
            let Some(name) = short.strip_prefix(prefix) else {
                continue;
            };
            // HEAD is a pointer at the remote's default branch, not a branch.
            if name == "HEAD" || local.contains(name) || !seen.insert(name) {
                continue;
            }
            list.push(Branch {
                name: name.to_string(),
                remote: short.to_string(),
                unfetched: false,
            });
        }
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{Repo, Tree, WORKTREE_ROOT, add_worktree};
    use crate::testutil::{TempDir, git_repo};

    #[test]
    fn parse_branches_keeps_one_row_per_branch() {
        let out = [
            "refs/heads/main",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            "refs/remotes/origin/feat/login",
            "refs/heads/release/main",
            "refs/remotes/upstream/feat/login",
            "refs/remotes/upstream/fix",
        ]
        .join("\n");
        let b = |name: &str, remote: &str| Branch {
            name: name.into(),
            remote: remote.into(),
            unfetched: false,
        };
        assert_eq!(
            parse_branches(&out, &["origin".into(), "upstream".into()]),
            vec![
                b("main", ""),
                b("feat/login", "origin/feat/login"),
                b("release/main", ""),
                b("fix", "upstream/fix")
            ]
        );
    }

    #[test]
    fn valid_branch_refuses_what_git_would() {
        for name in [
            "main",
            "feat/login",
            "release-1.2",
            "équipe/été",
            "a.b",
            "",
            "-force",
            "@",
            "../../escaped",
            "a..b",
            "feat login",
            "~x",
            "x:y",
            "a//b",
            "/main",
            "main/",
            ".hidden",
            "branch.lock",
            "a@{b",
            "line\nfeed",
        ] {
            let git_accepts = git_command()
                .args(["check-ref-format", "--branch", name])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success();
            assert_eq!(valid_branch(name), git_accepts, "{name:?}");
        }

        // And nothing is created on the way to finding out.
        let base = TempDir::new();
        let main = base.join("github.com/acme/alpha");
        git_repo(&main);
        let tree = Tree {
            roots: vec![base.path()],
        };
        let r = Repo {
            root: base.path(),
            rel: "github.com/acme/alpha".into(),
        };
        assert!(
            add_worktree(
                &base.path(),
                &main,
                &tree.worktree_dir(&r, "../../escaped"),
                "../../escaped"
            )
            .is_err()
        );
        assert!(!crate::testutil::exists(&base.join(WORKTREE_ROOT)));
    }
}
