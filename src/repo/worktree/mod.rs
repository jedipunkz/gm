//! The checkouts of a repository: listing them, dating them, and making
//! and removing the ones gm keeps under its worktree directory.

mod expire;
mod strays;

pub use expire::{Expiry, expire, expired, parse_days};
pub use strays::strays;

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::repo::{
    Repo, Tree, branch_exists, git_in, git_quiet, prune_empty_parents, same_path, valid_branch,
};
use crate::{Result, err, paths};

/// worktree_labels names checkouts for a warning, each with the work that
/// would go with it: "feat/login (3 changed), fix/timeout". changed is
/// changed_files, or what a test puts in its place.
pub fn worktree_labels(wts: &[Worktree], changed: impl Fn(&str) -> usize) -> String {
    worktree_labels_in("", wts, changed)
}

/// worktree_labels_in is worktree_labels with dir, gm's worktree directory for
/// the repository: a checkout whose path is outside it is named by its path as
/// well, because git's entry, not gm's tree, decides where it is and the
/// warning is the only place the reader learns it.
pub fn worktree_labels_in(dir: &str, wts: &[Worktree], changed: impl Fn(&str) -> usize) -> String {
    wts.iter()
        .map(|w| {
            let where_ = if dir.is_empty() || super::is_under(&w.path, dir) {
                String::new()
            } else {
                format!(" at {}", w.path)
            };
            match changed(&w.path) {
                0 => format!("{}{where_}", w.label()),
                n => format!("{}{where_} ({n} changed)", w.label()),
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Worktree is one checkout of a repository: the main one, plus whatever
/// `git worktree add` created.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Worktree {
    pub path: String,
    pub branch: String, // short name, empty when HEAD is detached
    pub head: String,   // commit hash
    pub bare: bool,
    pub committed_at: i64, // unix seconds of the branch tip; 0 when unknown
    pub used_at: i64,      // unix seconds HEAD last moved in it, from its reflog; 0 when unknown
}

impl Worktree {
    /// label names a worktree in one word: its branch, or a detached HEAD's
    /// hash.
    pub fn label(&self) -> String {
        if self.bare {
            "(bare)".into()
        } else if !self.branch.is_empty() {
            self.branch.clone()
        } else if self.head.len() >= 7 {
            self.head[..7].to_string()
        } else {
            "(detached)".into()
        }
    }
}

/// worktrees lists the checkouts of the repository at dir, the main one
/// first, which is the order git itself reports.
pub fn worktrees(dir: &str) -> Result<Vec<Worktree>> {
    let mut list = registered_worktrees(dir)?;
    let dates = branch_dates(dir, &list);
    let used = used_times(dir);
    for w in &mut list {
        w.committed_at = dates.get(&w.branch).copied().unwrap_or(0);
        w.used_at = canonicalize_worktree_path(Path::new(&w.path))
            .ok()
            .and_then(|path| used.get(&path).copied())
            .unwrap_or(0);
    }
    Ok(list)
}

/// registered_worktrees is Git's checkout list without the branch and reflog
/// metadata worktree details need. Status scans use this as their shared source
/// so discovering worktrees costs only the list command.
pub fn registered_worktrees(dir: &str) -> Result<Vec<Worktree>> {
    let out = git_in(dir, &["worktree", "list", "--porcelain"])?;
    Ok(parse_worktrees(&out))
}

/// used_times is when HEAD last moved in each linked worktree of the
/// repository at dir — its creation, a checkout, a commit — keyed by each
/// canonical worktree path. It reads only the reflog tail Git keeps beside the
/// repository, so a broken .git link still has a time without reading its full
/// history. The main worktree is not among them.
fn used_times(dir: &str) -> HashMap<PathBuf, i64> {
    let Ok(common) = git_in(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ) else {
        return HashMap::new();
    };
    let Ok(admins) = std::fs::read_dir(paths::join(&common, "worktrees")) else {
        return HashMap::new();
    };
    admins
        .flatten()
        .filter_map(|e| {
            let admin = e.path().to_string_lossy().into_owned();
            // gitdir names the worktree's .git file; worktree.useRelativePaths
            // writes it relative to this directory.
            let gitdir = std::fs::read_to_string(paths::join(&admin, "gitdir")).ok()?;
            let gitdir = match gitdir.trim() {
                p if p.starts_with('/') => p.to_string(),
                p => paths::join(&admin, p),
            };
            let at = reflog_time(&last_reflog_line(&paths::join(&admin, "logs/HEAD"))?)?;
            let path = canonicalize_worktree_path(Path::new(&paths::dir(&gitdir))).ok()?;
            Some((path, at))
        })
        .collect()
}

/// last_reflog_line reads only the tail needed to find the last reflog entry.
fn last_reflog_line(path: &str) -> Option<String> {
    const TAIL: u64 = 4096;
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.seek(SeekFrom::End(0)).ok()?;
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL))).ok()?;
    let mut bytes = Vec::with_capacity(len.min(TAIL) as usize);
    file.take(TAIL).read_to_end(&mut bytes).ok()?;
    String::from_utf8_lossy(&bytes)
        .lines()
        .rev()
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
thread_local! {
    static WORKTREE_PATH_CANONICALIZATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// canonicalize_worktree_path resolves one path used to join Git's worktree
/// list with its administrative reflog metadata.
fn canonicalize_worktree_path(path: &Path) -> std::io::Result<PathBuf> {
    #[cfg(test)]
    WORKTREE_PATH_CANONICALIZATIONS.with(|count| count.set(count.get() + 1));
    std::fs::canonicalize(path)
}

/// reflog_time reads the time off one reflog line:
/// "<old> <new> Name <email> <unix seconds> <zone>\t<message>".
fn reflog_time(line: &str) -> Option<i64> {
    let head = line.split('\t').next()?;
    head.split_whitespace().rev().nth(1)?.parse().ok()
}

/// branch_dates is when each checkout's branch was last committed to. One
/// call for all of them, so dating the checkouts costs a process and not one
/// per checkout, and it asks only for the branches that are checked out: a
/// repository with thousands of branches is no more work than one with three.
/// A detached HEAD has no branch to ask about and stays undated.
fn branch_dates(dir: &str, list: &[Worktree]) -> HashMap<String, i64> {
    let refs: Vec<String> = list
        .iter()
        .filter(|w| !w.branch.is_empty())
        .map(|w| format!("refs/heads/{}", w.branch))
        .collect();
    // Without a pattern for-each-ref lists every ref there is, which is the
    // one call this must never make.
    if refs.is_empty() {
        return HashMap::new();
    }
    // The full name, because the short one is "heads/main" when a tag is
    // also called main, and that is no branch name to look up.
    let mut args = vec!["for-each-ref", "--format=%(committerdate:unix) %(refname)"];
    args.extend(refs.iter().map(String::as_str));
    let Ok(out) = git_in(dir, &args) else {
        return HashMap::new();
    };
    out.split('\n')
        .filter_map(|line| {
            let (ts, name) = line.trim().split_once(' ')?;
            let name = name.strip_prefix("refs/heads/")?;
            Some((name.to_string(), ts.parse().ok()?))
        })
        .collect()
}

/// parse_worktrees reads `git worktree list --porcelain`: records separated by
/// a blank line, each a "worktree <path>" line followed by attributes.
pub fn parse_worktrees(out: &str) -> Vec<Worktree> {
    let mut list = Vec::new();
    let mut w = Worktree::default();
    for line in out.split('\n') {
        let line = line.trim_end_matches('\r');
        let (key, val) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "worktree" => {
                if !w.path.is_empty() {
                    list.push(std::mem::take(&mut w));
                }
                w = Worktree {
                    path: val.to_string(),
                    ..Default::default()
                };
            }
            "HEAD" => w.head = val.to_string(),
            "branch" => w.branch = val.strip_prefix("refs/heads/").unwrap_or(val).to_string(),
            "bare" => w.bare = true,
            _ => {}
        }
    }
    if !w.path.is_empty() {
        list.push(w);
    }
    list
}

/// WORKTREE_ROOT is where gm keeps the checkouts it creates:
///
///   <root>/.worktrees/<host>/<user>/<repo>/<branch>
///
/// The dot matters. A worktree has a .git file, so is_repo answers yes for
/// one, and a worktree in the open would be listed as a repository and
/// refused by gm migrate. find_repos never descends into a dotted directory,
/// so this one stays out of the way.
pub const WORKTREE_ROOT: &str = ".worktrees";

/// worktrees_dir is where gm keeps every checkout of one repository.
pub fn worktrees_dir(r: &Repo) -> String {
    paths::join(&paths::join(&r.root, WORKTREE_ROOT), &r.rel)
}

impl Tree {
    /// worktree_dir is where a branch's checkout of r belongs.
    pub fn worktree_dir(&self, r: &Repo, branch: &str) -> String {
        paths::join(&worktrees_dir(r), branch)
    }
}

/// add_worktree checks branch out at dir, starting the branch from HEAD when
/// it does not exist yet.
pub fn add_worktree(worktree_root: &str, repo_dir: &str, dir: &str, branch: &str) -> Result<()> {
    add_worktree_from(worktree_root, repo_dir, dir, branch, "")
}

/// add_worktree_from is add_worktree for a branch that may live only on a
/// remote: a new branch starts at start, "origin/feature", and tracks it. An
/// empty start is HEAD.
pub fn add_worktree_from(
    worktree_root: &str,
    repo_dir: &str,
    dir: &str,
    branch: &str,
    start: &str,
) -> Result<()> {
    if !valid_branch(branch) {
        return Err(err!("{branch:?} is not a branch name"));
    }
    std::fs::create_dir_all(paths::dir(dir))?;
    let mut args = vec!["-C", repo_dir, "worktree", "add"];
    if branch_exists(repo_dir, branch) {
        args.extend([dir, branch]);
    } else if !start.is_empty() {
        args.extend(["--track", "-b", branch, dir, start]);
    } else {
        args.extend(["-b", branch, dir]);
    }
    match git_quiet(&args) {
        Ok(()) => Ok(()),
        Err(e) => {
            super::prune_empty_parents(worktree_root, paths::dir(dir));
            Err(e)
        }
    }
}

/// repair_worktrees links the checkouts at dirs back to the repository at
/// repo_dir after the repository has moved: each checkout's .git file holds
/// the repository's old path, and git answers "not a git repository" in it
/// until this runs.
pub fn repair_worktrees(repo_dir: &str, dirs: &[String]) -> Result<()> {
    if dirs.is_empty() {
        return Ok(());
    }
    let mut args = vec!["-C", repo_dir, "worktree", "repair"];
    args.extend(dirs.iter().map(String::as_str));
    git_quiet(&args)
}

/// remove_worktree takes a checkout away. force is what the caller has already
/// confirmed: git refuses on its own when there is work in it.
pub fn remove_worktree(repo_dir: &str, dir: &str, force: bool) -> Result<()> {
    let mut args = vec!["-C", repo_dir, "worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(dir);
    git_quiet(&args)
}

/// remove_worktree_and_branch takes a checkout away, removes any empty
/// directories it leaves under the root's worktree directory, and then its
/// branch where git branch -d allows: a branch not merged stays, so force
/// never reaches the commits. It answers whether the branch went. An empty
/// branch is a detached HEAD, which has none.
pub fn remove_worktree_and_branch(r: &Repo, dir: &str, branch: &str, force: bool) -> Result<bool> {
    remove_worktree(&r.path(), dir, force)?;
    prune_empty_parents(&paths::join(&r.root, WORKTREE_ROOT), paths::dir(dir));
    Ok(!branch.is_empty() && git_quiet(&["-C", &r.path(), "branch", "-d", "--", branch]).is_ok())
}

/// other_worktrees are the checkouts of r that are not r itself. A repository
/// git cannot answer for has none, which is the right answer for a directory
/// that is about to be deleted.
pub fn other_worktrees(r: &Repo) -> Vec<Worktree> {
    other_worktrees_of(&r.path())
}

/// other_worktrees_of is the same thing for a path, which is all describe has.
pub fn other_worktrees_of(dir: &str) -> Vec<Worktree> {
    exclude_main_worktree(worktrees(dir).unwrap_or_default(), dir)
}

/// live_worktrees_of is every checkout Git registers for dir, including the
/// main one. Git's list, not a directory walk, decides whether a path is a
/// worktree; an unavailable repository has none.
pub fn live_worktrees_of(dir: &str) -> Vec<Worktree> {
    registered_worktrees(dir).unwrap_or_default()
}

/// other_live_worktrees_of is the registered list without the main checkout.
/// Status and the finder use it so both see exactly the paths Git reports.
pub fn other_live_worktrees_of(dir: &str) -> Vec<Worktree> {
    exclude_main_worktree(live_worktrees_of(dir), dir)
}

fn exclude_main_worktree(wts: Vec<Worktree>, dir: &str) -> Vec<Worktree> {
    wts.into_iter()
        .filter(|w| !same_path(&w.path, dir)) // the main worktree is the repository
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{Branch, branches};
    use crate::testutil::{TempDir, exists, git_repo, mkdir, write};

    #[test]
    fn parse_worktrees_reads_porcelain() {
        let out = "worktree /home/u/ghq/github.com/u/agx
HEAD 0123456789abcdef0123456789abcdef01234567
branch refs/heads/main

worktree /home/u/.worktrees/agx-login
HEAD 89abcdef0123456789abcdef0123456789abcdef
branch refs/heads/feat/login

worktree /home/u/.worktrees/agx-detached
HEAD fedcba9876543210fedcba9876543210fedcba98
detached
";
        let got = parse_worktrees(out);
        assert_eq!(got.len(), 3, "{got:?}");
        // The main worktree comes first, the way git reports it.
        assert_eq!(
            (got[0].path.as_str(), got[0].branch.as_str()),
            ("/home/u/ghq/github.com/u/agx", "main")
        );
        assert_eq!(
            (got[1].branch.as_str(), got[1].label()),
            ("feat/login", "feat/login".to_string())
        );
        assert_eq!(
            (got[2].branch.as_str(), got[2].label()),
            ("", "fedcba9".to_string())
        );
        assert!(parse_worktrees("").is_empty());
        assert_eq!(
            Worktree {
                path: "/x".into(),
                bare: true,
                ..Default::default()
            }
            .label(),
            "(bare)"
        );
    }

    #[test]
    fn last_reflog_line_reads_a_large_log_tail() {
        let tmp = TempDir::new();
        let path = paths::join(&tmp.path(), "logs/HEAD");
        mkdir(&paths::dir(&path));
        let mut log = "old reflog entry that is not the last one\n".repeat(256);
        let last = "0000000000000000000000000000000000000000 1111111111111111111111111111111111111111 User <u@example.com> 1234567890 +0000\tcheckout: moving from main to work\n";
        log.push_str(last);
        write(&path, &log);

        let line = last_reflog_line(&path).unwrap();
        assert_eq!(line, last.trim_end());
        assert_eq!(reflog_time(&line), Some(1_234_567_890));
    }

    #[test]
    fn worktrees_canonicalize_paths_linearly() {
        const COUNT: usize = 50;
        let tmp = TempDir::new();
        let r = Repo {
            root: tmp.path(),
            rel: "github.com/acme/alpha".into(),
        };
        git_repo(&r.path());
        let tree = Tree {
            roots: vec![tmp.path()],
        };
        let root = paths::join(&r.root, WORKTREE_ROOT);
        for i in 0..COUNT {
            let branch = format!("work-{i:02}");
            add_worktree(&root, &r.path(), &tree.worktree_dir(&r, &branch), &branch).unwrap();
        }

        WORKTREE_PATH_CANONICALIZATIONS.with(|count| count.set(0));
        let got = worktrees(&r.path()).unwrap();
        let canonicalizations = WORKTREE_PATH_CANONICALIZATIONS.with(std::cell::Cell::get);

        assert_eq!(got.len(), COUNT + 1);
        assert!(got[1..].iter().all(|w| w.used_at > 0), "{got:?}");
        assert_eq!(canonicalizations, COUNT * 2 + 1);
    }

    #[test]
    fn failed_worktree_add_prunes_its_empty_parents() {
        let base = TempDir::new();
        let main = base.join("github.com/acme/alpha");
        git_repo(&main);
        let root = base.join(WORKTREE_ROOT);
        let first = base.join("other/feat/login");
        add_worktree(&root, &main, &first, "feat/login").unwrap();

        let failed = paths::join(&root, "github.com/acme/alpha/fix/login");
        assert!(add_worktree(&root, &main, &failed, "feat/login").is_err());
        assert!(!crate::testutil::exists(&paths::join(&root, "github.com")));

        remove_worktree(&main, &first, false).unwrap();
    }

    #[test]
    fn add_worktree_from_a_remote_branch_tracks_it() {
        let tmp = TempDir::new();
        let upstream = tmp.join("upstream");
        git_repo(&upstream);
        git_in(&upstream, &["branch", "feat/login"]).unwrap();
        let clone = tmp.join("clone");
        git_quiet(&["clone", "-q", &upstream, &clone]).unwrap();

        let bs = branches(&clone).unwrap();
        assert!(
            bs.contains(&Branch {
                name: "feat/login".into(),
                remote: "origin/feat/login".into(),
                unfetched: false
            }),
            "{bs:?}"
        );
        let dir = tmp.join("wt");
        add_worktree_from(&tmp.path(), &clone, &dir, "feat/login", "origin/feat/login").unwrap();
        assert_eq!(
            git_in(
                &dir,
                &["rev-parse", "--abbrev-ref", "feat/login@{upstream}"]
            )
            .unwrap(),
            "origin/feat/login"
        );
    }

    // A tag named like the checked-out branch made git shorten the branch to
    // "heads/main", and the worktree went undated.
    #[test]
    fn worktrees_date_a_branch_with_a_tag_of_its_name() {
        let tmp = TempDir::new();
        git_repo(&tmp.path());
        git_in(&tmp.path(), &["tag", "main"]).unwrap();
        let got = worktrees(&tmp.path()).unwrap();
        assert_eq!(got[0].branch, "main");
        assert!(got[0].committed_at > 0, "{got:?}");
    }

    // A branch with commits nothing else has stays when its worktree goes,
    // however the worktree was removed.
    #[test]
    fn remove_worktree_keeps_an_unmerged_branch() {
        let base = TempDir::new();
        let r = Repo {
            root: base.path(),
            rel: "github.com/acme/alpha".into(),
        };
        git_repo(&r.path());
        let dir = Tree {
            roots: vec![base.path()],
        }
        .worktree_dir(&r, "work");
        add_worktree(
            &paths::join(&r.root, WORKTREE_ROOT),
            &r.path(),
            &dir,
            "work",
        )
        .unwrap();
        crate::testutil::git(&dir, &["commit", "-q", "--allow-empty", "-m", "only here"]);

        assert_eq!(
            remove_worktree_and_branch(&r, &dir, "work", true),
            Ok(false)
        );
        assert!(!exists(&dir) && branch_exists(&r.path(), "work"));
    }
}
