use std::ffi::OsStr;
use std::process::Command;
use std::time::Duration;

use super::deadline::{FETCH_TIMEOUT, run_deadline};
use super::git::{failure, quiet_result};
use super::issue::{Comment, read_comments};
use crate::{Error, Result, err, paths};

/// PullRequest is an open pull request, as much of it as a list row, the
/// details pane and the detail panel need. comments stay empty on a listing:
/// they are what the detail panel is for. The body comes with the listing,
/// for the pane beside the list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub branch: String, // headRefName
    pub draft: bool,
    pub fork: bool,         // isCrossRepository
    pub author: String,     // author.login
    pub head_owner: String, // headRepositoryOwner.login
    pub state: String,      // OPEN, CLOSED, MERGED
    pub url: String,
    pub body: String,
    pub comments: Vec<Comment>,
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

    /// worktree_name is where under the repository's worktree directory the
    /// pull request is checked out. A fork's goes under FORK_DIR, apart from
    /// the repository's own branches: bob:main is not a local bob/main, and
    /// bob:fix must not land inside a local bob's worktree.
    pub fn worktree_name(&self) -> String {
        if self.fork && !self.head_owner.is_empty() {
            return format!("{FORK_DIR}/{}", self.checkout());
        }
        self.branch.clone()
    }
}

/// FORK_DIR holds the checkouts of fork pull requests. No branch name can
/// have a component starting with a dot, so none can be filed here.
pub const FORK_DIR: &str = ".forks";

/// PR_LIMIT is how many open pull requests gh is asked for, newest first. A
/// repository with more than this has the rest invisible: whoever shows the
/// answer — and gets exactly a full page back — says so.
pub(crate) const PR_LIMIT: usize = 100;

/// gh talks to the network the way git's remote calls do, so it gets the same
/// kind of deadline: a listing moves nothing but names, and a checkout
/// downloads objects like a fetch. A blackholed network or a stalled auth
/// helper is hung, not waiting, and the deadline turns it into an error.
pub(crate) const GH_LIST_TIMEOUT: Duration = Duration::from_secs(30);
const GH_CHECKOUT_TIMEOUT: Duration = FETCH_TIMEOUT;
/// The detail view moves nothing but words, so it waits as long as the
/// listing does.
const PR_VIEW_TIMEOUT: Duration = GH_LIST_TIMEOUT;

/// PR_LIST_FIELDS is what the listing asks gh for; the detail view asks for
/// the same and the comments, so both go through the same reader.
const PR_LIST_FIELDS: &str =
    "number,title,headRefName,isDraft,isCrossRepository,author,headRepositoryOwner,state,url,body";

pub(crate) fn gh_command(program: &OsStr) -> Command {
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
    let limit = PR_LIMIT.to_string();
    cmd.args(["pr", "list", "--state", "open", "--limit", &limit, "--json"])
        .arg(PR_LIST_FIELDS)
        .current_dir(dir)
        .env("GH_PROMPT_DISABLED", "1");
    let out = run_deadline(&mut cmd, timeout, "gh pr list", gh_missing)?;
    if !out.status.success() {
        return Err(failure(&out.stderr, &out.status));
    }
    parse_pull_requests(&out.stdout)
}

/// pull_request asks gh for one pull request in full: its body and its
/// comments.
pub fn pull_request(dir: &str, number: u64) -> Result<PullRequest> {
    pull_request_with(OsStr::new("gh"), PR_VIEW_TIMEOUT, dir, number)
}

fn pull_request_with(gh: &OsStr, timeout: Duration, dir: &str, number: u64) -> Result<PullRequest> {
    let mut cmd = gh_command(gh);
    let n = number.to_string();
    cmd.args(["pr", "view", &n, "--json"])
        .arg(format!("{PR_LIST_FIELDS},comments"))
        .current_dir(dir)
        .env("GH_PROMPT_DISABLED", "1");
    let out = run_deadline(&mut cmd, timeout, "gh pr view", gh_missing)?;
    if !out.status.success() {
        return Err(failure(&out.stderr, &out.status));
    }
    let v: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| err!("gh pr view: {e}"))?;
    if !v.is_object() {
        return Err(err!("gh pr view: not a pull request"));
    }
    let pr = read_pull_request(&v);
    // The panel's enter checks out the pull request it shows, so an answer
    // about another one is a defect, not something to show.
    if pr.number != number {
        return Err(err!("gh pr view: asked for #{n}, got #{}", pr.number));
    }
    Ok(pr)
}

pub fn parse_pull_requests(out: &[u8]) -> Result<Vec<PullRequest>> {
    let v: serde_json::Value = serde_json::from_slice(out).map_err(|e| err!("gh pr list: {e}"))?;
    let list = v.as_array().ok_or_else(|| err!("gh pr list: not a list"))?;
    Ok(list.iter().map(read_pull_request).collect())
}

/// read_pull_request takes one pull request out of gh's JSON, whichever
/// command it answered.
fn read_pull_request(p: &serde_json::Value) -> PullRequest {
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
        state: s("state"),
        url: s("url"),
        body: s("body"),
        comments: read_comments(p),
    }
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
    quiet_result(out)
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
pub(crate) fn gh_missing(e: std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::NotFound {
        return "gh is not installed: pull requests come from the GitHub CLI".into();
    }
    e.into()
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
             "author":{"login":"alice"},"headRepositoryOwner":{"login":"acme"},"state":"OPEN",
             "url":"https://github.com/acme/alpha/pull/7","body":"adds the form"},
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
        assert_eq!(prs[0].worktree_name(), "feat/login");
        assert_eq!(prs[1].worktree_name(), ".forks/bob/main");
        assert_eq!(prs[1].author, "bob");
        assert_eq!(prs[0].url, "https://github.com/acme/alpha/pull/7");
        assert_eq!(
            (prs[0].state.as_str(), prs[0].body.as_str()),
            ("OPEN", "adds the form")
        );
        assert!(prs[0].comments.is_empty(), "a listing has no comments");
    }

    // gh pr view is asked for the listing's fields and the comments, and its
    // answer is read the way the listing's is.
    #[test]
    fn pull_request_reads_gh_pr_view() {
        let tmp = TempDir::new();
        let gh = script(
            &tmp.path(),
            r#"#!/bin/sh
echo "$@" > "$0.args"
echo '{"number":7,"title":"Add login","headRefName":"feat/login","state":"OPEN",
 "url":"https://github.com/acme/alpha/pull/7","body":"adds the form",
 "comments":[{"author":{"login":"carol"},"body":"looks good"}]}'
"#,
        );
        let repo = tmp.join("repo");
        crate::testutil::mkdir(&repo);
        let pr = pull_request_with(OsStr::new(&gh), Duration::from_secs(10), &repo, 7).unwrap();
        assert_eq!((pr.number, pr.branch.as_str()), (7, "feat/login"));
        assert_eq!(pr.body, "adds the form");
        assert_eq!(pr.comments.len(), 1);
        assert_eq!(pr.comments[0].author, "carol");
        let args = std::fs::read_to_string(format!("{gh}.args")).unwrap();
        assert_eq!(
            args.trim(),
            format!("pr view 7 --json {PR_LIST_FIELDS},comments")
        );

        // An answer about another pull request is refused: the panel's enter
        // would check out the wrong one.
        let e = pull_request_with(OsStr::new(&gh), Duration::from_secs(10), &repo, 8).unwrap_err();
        assert!(e.0.contains("asked for #8, got #7"), "{e}");

        let gh = script(&tmp.path(), "#!/bin/sh\necho '[]'\n");
        let e = pull_request_with(OsStr::new(&gh), Duration::from_secs(10), &repo, 7).unwrap_err();
        assert!(e.0.contains("not a pull request"), "{e}");
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

    // gh answering with a failure says what gh said, in the same shape the
    // rest of gm's errors take.
    #[test]
    fn pull_requests_say_what_gh_said() {
        let tmp = TempDir::new();
        let gh = script(&tmp.path(), "#!/bin/sh\necho 'not logged in' >&2\nexit 1\n");
        let repo = tmp.join("repo");
        git_repo(&repo);
        let e = pull_requests_with(OsStr::new(&gh), Duration::from_secs(10), &repo).unwrap_err();
        assert!(e.0.contains("not logged in"), "{e}");
        let e = pull_request_with(OsStr::new(&gh), Duration::from_secs(10), &repo, 7).unwrap_err();
        assert!(e.0.contains("not logged in"), "{e}");

        // A gh that is not there at all is named as the thing to install.
        let e = gh_missing(std::io::Error::new(std::io::ErrorKind::NotFound, "no gh"));
        assert!(
            e.0.contains("gh is not installed"),
            "the missing program was not named:\n{e}"
        );
    }
}
