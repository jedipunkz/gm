//! The work a repository holds that nothing else does: changed files and
//! commits not pushed.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{git_in, remotes};
use crate::Result;

/// is_dirty reports whether the working copy has uncommitted changes.
pub fn is_dirty(dir: &str) -> Result<bool> {
    Ok(!git_in(dir, &["status", "--porcelain"])?.is_empty())
}

/// changed_files counts what a removal of dir would lose: the files git status
/// reports. It asks git now, never a cache, because it is only asked right
/// before something is deleted. A checkout git cannot answer for counts as
/// none.
pub fn changed_files(dir: &str) -> usize {
    status_of(dir).dirty
}

/// State is what a repository holds that nothing else does: work not
/// committed, and commits not pushed. It comes out of one `git status
/// --porcelain -b`, which reads the refs already on disk and never fetches, so
/// the answer is as fresh as the last time something did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub branch: String,  // as git names it, "HEAD" when detached
    pub upstream: bool,  // the branch tracks something
    pub ahead: usize,    // commits here the upstream does not have
    pub behind: usize,   // and the other way round
    pub dirty: usize,    // changed files
    pub unpushed: usize, // commits on branches no remote has, 0 without remotes
}

impl State {
    /// unfinished reports whether this is work someone walked away from. Being
    /// behind is the remote's news rather than the user's, so it alone does
    /// not make a repository worth listing.
    pub fn unfinished(&self) -> bool {
        self.dirty > 0 || self.has_unpushed()
    }

    /// has_unpushed is what the finder's /unpushed and gm status --unpushed
    /// both keep. Being ahead counts even when another remote-tracking ref has
    /// the commits: the row says "N ahead", so the filter has to agree.
    pub fn has_unpushed(&self) -> bool {
        self.unpushed > 0 || self.ahead > 0
    }
}

/// STATUS_WORKERS bounds the Git processes in a bulk status scan. More
/// workers than this do not help once they compete for the same disk.
const STATUS_WORKERS: usize = 8;

/// status_of reads one repository's state.
pub fn status_of(dir: &str) -> State {
    let mut s = git_in(dir, &["status", "--porcelain", "-b"])
        .map(|out| parse_status(&out))
        .unwrap_or_default();
    // The commits on branches no remote has are unpushed work, as much as
    // being ahead of the upstream is — more, since they name every branch,
    // not just the checked-out one. Only a repository with remotes counts:
    // one without would list every commit it ever made.
    if !remotes(dir).0.is_empty() {
        s.unpushed = unpushed_commits(dir);
    }
    s
}

/// unpushed_commits counts the commits reachable from local branches and from
/// no remote-tracking ref. Never a fetch: the answer is as fresh as the last
/// fetch was.
fn unpushed_commits(dir: &str) -> usize {
    git_in(
        dir,
        &["rev-list", "--count", "--branches", "--not", "--remotes"],
    )
    .ok()
    .and_then(|n| n.trim().parse().ok())
    .unwrap_or(0)
}

/// status_with_worktrees is status_of with the work in the repository's other
/// checkouts folded in, because a worktree is where work in progress lives:
/// their changed files and their commits ahead count as the repository's.
/// unpushed already covers every branch, and branch stays the main checkout's.
pub fn status_with_worktrees(dir: &str) -> State {
    let mut s = status_of(dir);
    for w in crate::repo::other_live_worktrees_of(dir) {
        let t = git_in(&w.path, &["status", "--porcelain", "-b"])
            .map(|out| parse_status(&out))
            .unwrap_or_default();
        s.dirty += t.dirty;
        s.ahead += t.ahead;
    }
    s
}

/// status_map reads every path with of, a few at a time: git is the slow part
/// and the disk is shared, so more workers than this buys nothing.
pub fn status_map(paths: &[String], of: fn(&str) -> State) -> HashMap<String, State> {
    let out = Mutex::new(HashMap::with_capacity(paths.len()));
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..STATUS_WORKERS.min(paths.len()) {
            s.spawn(|| {
                while let Some(p) = paths.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let st = of(p);
                    out.lock().unwrap().insert(p.clone(), st);
                }
            });
        }
    });
    out.into_inner().unwrap()
}

/// parse_status reads `git status --porcelain -b`: a "## " header naming the
/// branch and how far it has drifted, then a line per changed file.
pub fn parse_status(out: &str) -> State {
    let mut s = State::default();
    for (i, l) in out.split('\n').enumerate() {
        if i == 0
            && let Some(header) = l.strip_prefix("## ")
        {
            s.parse_branch(header);
            continue;
        }
        if !l.is_empty() {
            s.dirty += 1;
        }
    }
    s
}

impl State {
    /// parse_branch reads the header, which git writes as one of:
    ///
    ///   main...origin/main [ahead 1, behind 2]
    ///   main...origin/main
    ///   main
    ///   HEAD (no branch)
    ///   No commits yet on main
    fn parse_branch(&mut self, mut l: &str) {
        if let (Some(i), true) = (l.rfind(" ["), l.ends_with(']')) {
            for part in l[i + 2..l.len() - 1].split(", ") {
                // "gone" has no number and means the upstream was deleted.
                let Some((kind, num)) = part.split_once(' ') else {
                    continue;
                };
                let Ok(n) = num.parse() else { continue };
                match kind {
                    "ahead" => self.ahead = n,
                    "behind" => self.behind = n,
                    _ => {}
                }
            }
            l = &l[..i];
        }
        let l = l.strip_prefix("No commits yet on ").unwrap_or(l);
        if let Some((b, up)) = l.split_once("...") {
            self.branch = b.to_string();
            self.upstream = !up.is_empty();
            return;
        }
        self.branch = l.strip_suffix(" (no branch)").unwrap_or(l).to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;
    use crate::repo::{Repo, Tree, WORKTREE_ROOT, add_worktree};
    use crate::testutil::{TempDir, git_repo, mkdir, write};

    // The bug where /unpushed kept a repository whose commits were ahead of
    // the upstream but on another remote, and gm status --unpushed did not.
    #[test]
    fn ahead_alone_is_unpushed() {
        let s = super::State {
            ahead: 1,
            ..Default::default()
        };
        assert!(s.has_unpushed() && s.unfinished());
        assert!(!super::State::default().has_unpushed());
    }

    #[test]
    fn parse_status_reads_every_header() {
        let st = |branch: &str, upstream, ahead, behind, dirty| State {
            branch: branch.into(),
            upstream,
            ahead,
            behind,
            dirty,
            unpushed: 0,
        };
        for (name, out, want) in [
            (
                "tracking and drifted",
                "## main...origin/main [ahead 1, behind 2]",
                st("main", true, 1, 2, 0),
            ),
            (
                "ahead only",
                "## main...origin/main [ahead 3]",
                st("main", true, 3, 0, 0),
            ),
            (
                "in sync",
                "## main...origin/main",
                st("main", true, 0, 0, 0),
            ),
            ("no upstream", "## main", st("main", false, 0, 0, 0)),
            (
                "detached",
                "## HEAD (no branch)",
                st("HEAD", false, 0, 0, 0),
            ),
            (
                "empty repository",
                "## No commits yet on main",
                st("main", false, 0, 0, 0),
            ),
            // The upstream branch was deleted: there is no count to read, and
            // the branch still tracks something as far as the config is concerned.
            (
                "gone upstream",
                "## main...origin/main [gone]",
                st("main", true, 0, 0, 0),
            ),
            (
                "changes counted",
                "## main...origin/main\n M a.go\n?? b.go\nD  c.go",
                st("main", true, 0, 0, 3),
            ),
        ] {
            assert_eq!(parse_status(out), want, "{name}");
        }
        // Being behind is the remote's news, not work anyone left behind.
        assert!(!st("", true, 0, 4, 0).unfinished());
        assert!(st("", true, 1, 0, 0).unfinished());
    }

    #[test]
    fn status_map_counts_untracked_files() {
        let dir = TempDir::new();
        let clean = dir.join("clean");
        let dirty = dir.join("dirty");
        for p in [&clean, &dirty] {
            mkdir(p);
            crate::testutil::git(p, &["init", "-q"]);
        }
        write(&paths::join(&dirty, "scratch.txt"), "wip\n");
        // Not a repository at all; git cannot answer, which counts as clean.
        let plain = dir.join("plain");
        mkdir(&plain);

        let got = status_map(&[clean.clone(), dirty.clone(), plain.clone()], status_of);
        assert_eq!(got.len(), 3, "{got:?}");
        assert!(got[&dirty].dirty > 0);
        assert_eq!(got[&clean].dirty, 0);
        assert_eq!(got[&plain].dirty, 0);
        assert!(status_map(&[], status_of).is_empty());
    }

    // The bug where /dirty hid a repository whose only work was in one of its
    // worktrees.
    #[test]
    fn status_with_worktrees_counts_the_work_in_them() {
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
        add_worktree(&base.join(WORKTREE_ROOT), &r.path(), &login, "feat/login").unwrap();
        assert_eq!(status_with_worktrees(&r.path()).dirty, 0);

        write(&paths::join(&login, "wip.txt"), "wip\n");
        assert_eq!(status_of(&r.path()).dirty, 0);
        assert_eq!(status_with_worktrees(&r.path()).dirty, 1);
    }
}
