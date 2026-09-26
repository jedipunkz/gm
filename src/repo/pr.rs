use std::ffi::OsStr;
use std::process::Command;
use std::time::Duration;

use super::deadline::{FETCH_TIMEOUT, run_deadline};
use super::git::{exit_error, git_message};
use crate::{Error, Result, err, paths};

/// PullRequest is an open pull request, as much of it as a list row and the
/// details pane need.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub branch: String, // headRefName
    pub draft: bool,
    pub fork: bool,         // isCrossRepository
    pub author: String,     // author.login
    pub head_owner: String, // headRepositoryOwner.login
}

impl PullRequest {
    /// label is how the pull request reads in a list: number first, so typing
    /// it finds the row, and a draft says so before its title.
    pub fn label(&self) -> String {
        let draft = if self.draft { "[draft] " } else { "" };
        format!("#{} {draft}{}", self.number, self.title)
    }

    /// checkout is the name the pull request's worktree is filed under. A
    /// fork's branch is often called main, so it goes under its owner's name
    /// instead of next to the repository's own main.
    pub fn checkout(&self) -> String {
        if self.fork && !self.head_owner.is_empty() {
            return format!("{}/{}", self.head_owner, self.branch);
        }
        self.branch.clone()
    }
}

/// PR_LIMIT is how many open pull requests gh is asked for, newest first.
const PR_LIMIT: &str = "100";

/// gh talks to the network the way git's remote calls do, so it gets the same
/// kind of deadline: a listing moves nothing but names, and a checkout
/// downloads objects like a fetch. A blackholed network or a stalled auth
/// helper is hung, not waiting, and the deadline turns it into an error.
const GH_LIST_TIMEOUT: Duration = Duration::from_secs(30);
const GH_CHECKOUT_TIMEOUT: Duration = FETCH_TIMEOUT;

fn gh_command(program: &OsStr) -> Command {
    let command = Command::new(program);
    #[cfg(test)]
    let command = {
        let mut command = command;
        super::git::isolate_git_config(&mut command);
        command
    };
    command
}

/// pull_requests asks gh for the repository's open pull requests. gh talks to
/// GitHub, so this is slow next to git and fails without a network, gh or a
/// login; the error says which.
pub fn pull_requests(dir: &str) -> Result<Vec<PullRequest>> {
    pull_requests_with(OsStr::new("gh"), GH_LIST_TIMEOUT, dir)
}

fn pull_requests_with(gh: &OsStr, timeout: Duration, dir: &str) -> Result<Vec<PullRequest>> {
    let mut cmd = gh_command(gh);
    cmd.args([
        "pr", "list", "--state", "open", "--limit", PR_LIMIT, "--json",
    ])
    .arg("number,title,headRefName,isDraft,isCrossRepository,author,headRepositoryOwner")
    .current_dir(dir)
    .env("GH_PROMPT_DISABLED", "1");
    let out = run_deadline(&mut cmd, timeout, "gh pr list", gh_missing)?;
    if !out.status.success() {
        return Err(gh_failed(&out));
    }
    parse_pull_requests(&out.stdout)
}

pub fn parse_pull_requests(out: &[u8]) -> Result<Vec<PullRequest>> {
    let v: serde_json::Value = serde_json::from_slice(out).map_err(|e| err!("gh pr list: {e}"))?;
    let list = v.as_array().ok_or_else(|| err!("gh pr list: not a list"))?;
    Ok(list
        .iter()
        .map(|p| {
            let s = |k: &str| p.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let b = |k: &str| p.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
            let login = |k: &str| {
                p.pointer(&format!("/{k}/login"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            PullRequest {
                number: p.get("number").and_then(|v| v.as_u64()).unwrap_or(0),
                title: s("title"),
                branch: s("headRefName"),
                draft: b("isDraft"),
                fork: b("isCrossRepository"),
                author: login("author"),
                head_owner: login("headRepositoryOwner"),
            }
        })
        .collect())
}

/// check_out_pull_request has gh check the pull request out as a worktree at
/// dir. gh names the branch and, for a fork, sets up where it pushes to. gh is
/// the program to run, which a test replaces with a stand-in.
pub fn check_out_pull_request(gh: &str, repo_dir: &str, dir: &str, number: u64) -> Result<()> {
    std::fs::create_dir_all(paths::dir(dir))?;
    // Everything gh prints stays off the terminal: the finder is drawn there.
    let mut cmd = gh_command(OsStr::new(gh));
    cmd.args(["pr", "checkout", &number.to_string(), "--worktree", dir])
        .current_dir(repo_dir)
        .env("GH_PROMPT_DISABLED", "1");
    let out = run_deadline(&mut cmd, GH_CHECKOUT_TIMEOUT, "gh pr checkout", gh_missing)?;
    if out.status.success() {
        return Ok(());
    }
    let mut all = out.stdout.clone();
    all.extend_from_slice(&out.stderr);
    match git_message(&all) {
        msg if !msg.is_empty() => Err(Error(msg)),
        _ => Err(exit_error(&out.status)),
    }
}

/// check_out_pull_request_in removes the empty directories it made if gh
/// cannot create the worktree. worktree_root is left for the next checkout.
pub fn check_out_pull_request_in(
    worktree_root: &str,
    gh: &str,
    repo_dir: &str,
    dir: &str,
    number: u64,
) -> Result<()> {
    match check_out_pull_request(gh, repo_dir, dir, number) {
        Ok(()) => Ok(()),
        Err(e) => {
            super::prune_empty_parents(worktree_root, paths::dir(dir));
            Err(e)
        }
    }
}

/// gh_missing turns a gh that could not be started into the one line worth
/// reading.
fn gh_missing(e: std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::NotFound {
        return "gh is not installed: pull requests come from the GitHub CLI".into();
    }
    e.into()
}

/// gh_failed is what a gh that ran and failed said about it.
fn gh_failed(out: &std::process::Output) -> Error {
    match git_message(&out.stderr) {
        msg if !msg.is_empty() => Error(msg),
        _ => exit_error(&out.status),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::git::git_in;
    use crate::testutil::{TempDir, git_repo};
    use std::os::unix::fs::PermissionsExt;

    // gh's JSON is read, and a fork's branch is filed under its owner so a
    // fork's main does not land on the repository's own.
    #[test]
    fn parse_pull_requests_reads_gh() {
        let out = br#"[
            {"number":7,"title":"Add login","headRefName":"feat/login","isDraft":false,"isCrossRepository":false,
             "author":{"login":"alice"},"headRepositoryOwner":{"login":"acme"}},
            {"number":9,"title":"Fix typo","headRefName":"main","isDraft":true,"isCrossRepository":true,
             "author":{"login":"bob"},"headRepositoryOwner":{"login":"bob"}}
        ]"#;
        let prs = parse_pull_requests(out).unwrap();
        assert_eq!(prs.len(), 2);
        assert_eq!(
            (prs[0].label(), prs[0].checkout()),
            ("#7 Add login".into(), "feat/login".into())
        );
        assert_eq!(
            (prs[1].label(), prs[1].checkout()),
            ("#9 [draft] Fix typo".into(), "bob/main".into())
        );
        assert_eq!(prs[1].author, "bob");
    }

    fn script(dir: &str, body: &str) -> String {
        let p = paths::join(dir, "gh");
        std::fs::write(&p, body).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    // A stand-in gh records its arguments and makes the worktree the way the
    // real one would.
    #[test]
    fn check_out_pull_request_runs_gh() {
        let tmp = TempDir::new();
        let main = tmp.join("main");
        git_repo(&main);
        let bin = tmp.join("bin");
        crate::testutil::mkdir(&bin);
        let gh = script(
            &bin,
            "#!/bin/sh\necho \"$@\" > \"$0.args\"\nexec git worktree add -b feat/login \"$5\"\n",
        );

        let dir = tmp.join("wt/feat/login");
        check_out_pull_request(&gh, &main, &dir, 7).unwrap();
        let args = std::fs::read_to_string(format!("{gh}.args")).unwrap();
        assert_eq!(args.trim(), format!("pr checkout 7 --worktree {dir}"));
        assert_eq!(
            git_in(&dir, &["branch", "--show-current"]).unwrap(),
            "feat/login"
        );

        // A gh that fails says why, in its own words.
        let gh = script(
            &bin,
            "#!/bin/sh\necho 'could not find pull request' >&2\nexit 1\n",
        );
        let root = tmp.join("worktrees");
        let dir = paths::join(&root, "github.com/acme/alpha/feat/login");
        let e = check_out_pull_request_in(&root, &gh, &main, &dir, 8).unwrap_err();
        assert!(e.0.contains("could not find pull request"), "{e}");
        assert!(!crate::testutil::exists(&paths::join(&root, "github.com")));
    }

    // gh runs under a deadline like git's remote calls do: when it hangs, gm
    // gets an error instead of a spinner forever, and nothing it started
    // outlives the kill.
    #[test]
    fn pull_requests_times_out() {
        let tmp = TempDir::new();
        let marker = tmp.join("grandchild-survived");
        let gh = script(
            &tmp.path(),
            &format!("#!/bin/sh\n(sleep 3; touch '{marker}') &\nwait\n"),
        );
        let repo = tmp.join("repo");
        git_repo(&repo);

        let e = pull_requests_with(OsStr::new(&gh), Duration::from_secs(1), &repo).unwrap_err();
        assert!(
            e.0.contains("gh pr list took longer than 1s"),
            "the timeout was not named:\n{e}"
        );
        std::thread::sleep(Duration::from_millis(3_100));
        assert!(
            !std::path::Path::new(&marker).exists(),
            "grandchild outlived the timed-out gh process"
        );
    }
}
