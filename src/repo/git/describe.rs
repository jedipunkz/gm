//! What the details pane says about one working copy: its branch, its
//! remote, and its newest commits.

use super::{git_in, git_out, remotes};
use crate::repo::Worktree;

/// Status is what git says about a working copy. Every field is best-effort:
/// a repository git cannot read still has to be listed and jumped to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub remote: String,
    pub branch: String,
    pub commits: Vec<Commit>,     // newest first
    pub dirty: usize,             // changed files
    pub worktrees: Vec<Worktree>, // the other checkouts, without the main one
}

/// RECENT_COMMITS is how many commits describe collects. Three answers "is
/// this the repository I mean?" without turning the details pane into a log;
/// the pane draws fewer when it is short of height.
const RECENT_COMMITS: &str = "3";

/// describe collects the status of one working copy. It still shells out, so
/// callers keep it off any hot path and off rows the cursor only passed over.
pub fn describe(dir: &str) -> Status {
    let (names, origin) = remotes(dir);
    let dirty = match git_in(dir, &["status", "--porcelain"]) {
        Ok(out) if !out.is_empty() => out.split('\n').count(),
        _ => 0,
    };
    Status {
        branch: git_out(dir, &["rev-parse", "--abbrev-ref", "HEAD"]),
        remote: origin,
        commits: commits(dir, &names),
        worktrees: crate::repo::other_worktrees_of(dir),
        dirty,
    }
}

/// Commit is one line of the log, split so the details pane can colour its
/// parts the way `git log --oneline --decorate` does.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Commit {
    pub hash: String,
    pub refs: Vec<Ref>, // the decorations, in the order git printed them
    pub subject: String,
}

/// RefKind says how a decoration reads. git gives each kind its own colour,
/// and so does the finder.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum RefKind {
    #[default]
    Local, // a branch in this repository
    Head,   // HEAD itself
    Remote, // a remote-tracking branch
    Tag,    // a tag
}

/// Ref is one decoration, named as git prints it ("main", "origin/main",
/// "tag: v1.0").
#[derive(Debug, Clone, PartialEq)]
pub struct Ref {
    pub name: String,
    pub kind: RefKind,
}

/// The fields of one log line are separated by a NUL, which cannot appear in
/// a hash, a ref name or a subject. %x00 is git's own escape for it, written
/// in the --format argument; a real NUL there could not be passed to exec.
const COMMIT_FORMAT: &str = "--format=%h%x00%D%x00%s";

/// commits reads the newest commits and their decorations. The remote names
/// come from the caller: without them "origin/main" and a local branch called
/// "release/main" look alike, both being a name with a slash in it.
fn commits(dir: &str, remotes: &[String]) -> Vec<Commit> {
    match git_in(dir, &["log", "-n", RECENT_COMMITS, COMMIT_FORMAT]) {
        Ok(out) => parse_commits(&out, remotes),
        Err(_) => Vec::new(),
    }
}

/// parse_commits reads the lines commits asked git for. A line git could not
/// format is dropped rather than shown half-parsed.
pub fn parse_commits(out: &str, remotes: &[String]) -> Vec<Commit> {
    if out.is_empty() {
        return Vec::new();
    }
    out.split('\n')
        .filter_map(|line| {
            let f: Vec<&str> = line.splitn(3, '\0').collect();
            (f.len() == 3).then(|| Commit {
                hash: f[0].to_string(),
                refs: parse_refs(f[1], remotes),
                subject: f[2].to_string(),
            })
        })
        .collect()
}

/// parse_refs reads git's %D decoration list: comma-separated names, where the
/// checked-out branch appears as "HEAD -> name" and a tag as "tag: name".
fn parse_refs(d: &str, remotes: &[String]) -> Vec<Ref> {
    let mut refs = Vec::new();
    for part in d.trim().split(',') {
        let mut part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((head, branch)) = part.split_once(" -> ") {
            refs.push(Ref {
                name: head.trim().to_string(),
                kind: RefKind::Head,
            });
            part = branch.trim();
        }
        refs.push(Ref {
            name: part.to_string(),
            kind: ref_kind(part, remotes),
        });
    }
    refs
}

fn ref_kind(name: &str, remotes: &[String]) -> RefKind {
    if name == "HEAD" {
        return RefKind::Head;
    }
    if name.starts_with("tag: ") {
        return RefKind::Tag;
    }
    if remotes
        .iter()
        .any(|r| !r.is_empty() && name.starts_with(&format!("{r}/")))
    {
        return RefKind::Remote;
    }
    RefKind::Local
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_commits_reads_decorations() {
        let out = [
            "b1b7b91\0HEAD -> feat/migrate-scan, origin/feat/migrate-scan\0refactor: name the flag -r",
            "06f966e\0\0feat: add gm migrate -r",
            "d7f3a4c\0tag: v1.2.0, origin/main, main, release/main\0Merge pull request #24",
        ]
        .join("\n");
        let got = parse_commits(&out, &["origin".to_string()]);
        assert_eq!(got.len(), 3);
        assert_eq!(
            (got[0].hash.as_str(), got[0].subject.as_str()),
            ("b1b7b91", "refactor: name the flag -r")
        );
        // "HEAD -> branch" is two decorations, coloured differently.
        let r = |name: &str, kind| Ref {
            name: name.into(),
            kind,
        };
        assert_eq!(
            got[0].refs,
            vec![
                r("HEAD", RefKind::Head),
                r("feat/migrate-scan", RefKind::Local),
                r("origin/feat/migrate-scan", RefKind::Remote)
            ]
        );
        assert!(got[1].refs.is_empty());
        // A local branch whose name has a slash in it is not a remote one:
        // only the configured remotes make it remote.
        assert_eq!(
            got[2].refs,
            vec![
                r("tag: v1.2.0", RefKind::Tag),
                r("origin/main", RefKind::Remote),
                r("main", RefKind::Local),
                r("release/main", RefKind::Local)
            ]
        );
        assert!(parse_commits("", &[]).is_empty());
        // A line git could not format is dropped, not turned into a bad row.
        assert!(parse_commits("no separators here", &[]).is_empty());
    }
}
