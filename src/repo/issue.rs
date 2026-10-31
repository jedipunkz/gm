use std::ffi::OsStr;
use std::time::Duration;

use super::deadline::run_deadline;
use super::git::failure;
use super::pr::{GH_LIST_TIMEOUT, gh_command, gh_missing};
use crate::{Result, err};

/// Issue is a GitHub issue, as much of it as a list row and the detail panel
/// need. comments stay empty on a listing: they are what the detail panel is
/// for. The body comes with the listing, for the pane beside the list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub state: String, // OPEN, CLOSED...
    pub author: String,
    pub labels: Vec<String>,
    pub url: String,
    pub body: String,
    pub comments: Vec<Comment>,
}

impl Issue {
    /// label is how the issue reads in a list: number first, so typing it
    /// finds the row.
    pub fn label(&self) -> String {
        format!("#{} {}", self.number, self.title)
    }
}

/// Comment is one comment on an issue, as the detail panel shows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Comment {
    pub author: String,
    pub body: String,
}

/// ISSUE_LIMIT is how many open issues gh is asked for, newest first. A
/// repository with more than this has the rest invisible: whoever shows the
/// answer — and gets exactly a full page back — says so.
pub(crate) const ISSUE_LIMIT: usize = 100;

/// The detail view moves nothing but words, so it waits as long as the
/// listing does.
const ISSUE_VIEW_TIMEOUT: Duration = GH_LIST_TIMEOUT;

/// issues asks gh for the repository's open issues. gh talks to GitHub, so
/// this is slow next to git and fails without a network, gh or a login; the
/// error says which. issue below shares every field's name but `comments`, so
/// both go through the same reader.
pub fn issues(dir: &str) -> Result<Vec<Issue>> {
    issues_with(OsStr::new("gh"), GH_LIST_TIMEOUT, dir)
}

fn issues_with(gh: &OsStr, timeout: Duration, dir: &str) -> Result<Vec<Issue>> {
    let mut cmd = gh_command(gh);
    let limit = ISSUE_LIMIT.to_string();
    cmd.args([
        "issue", "list", "--state", "open", "--limit", &limit, "--json",
    ])
    .arg("number,title,state,author,labels,url,body")
    .current_dir(dir)
    .env("GH_PROMPT_DISABLED", "1");
    let out = run_deadline(&mut cmd, timeout, "gh issue list", gh_missing)?;
    if !out.status.success() {
        return Err(failure(&out.stderr, &out.status));
    }
    parse_issues(&out.stdout)
}

/// issue asks gh for one issue in full: its body and its comments.
pub fn issue(dir: &str, number: u64) -> Result<Issue> {
    issue_with(OsStr::new("gh"), ISSUE_VIEW_TIMEOUT, dir, number)
}

fn issue_with(gh: &OsStr, timeout: Duration, dir: &str, number: u64) -> Result<Issue> {
    let mut cmd = gh_command(gh);
    let n = number.to_string();
    cmd.args(["issue", "view", &n, "--json"])
        .arg("number,title,state,author,labels,url,body,comments")
        .current_dir(dir)
        .env("GH_PROMPT_DISABLED", "1");
    let out = run_deadline(&mut cmd, timeout, "gh issue view", gh_missing)?;
    if !out.status.success() {
        return Err(failure(&out.stderr, &out.status));
    }
    let issue = parse_one(&out.stdout)?;
    if issue.url.is_empty() {
        return Err(err!("gh issue view: no url for #{n}"));
    }
    Ok(issue)
}

pub fn parse_issues(out: &[u8]) -> Result<Vec<Issue>> {
    let v: serde_json::Value =
        serde_json::from_slice(out).map_err(|e| err!("gh issue list: {e}"))?;
    let list = v
        .as_array()
        .ok_or_else(|| err!("gh issue list: not a list"))?;
    Ok(list.iter().map(read_issue).collect())
}

fn parse_one(out: &[u8]) -> Result<Issue> {
    let v: serde_json::Value =
        serde_json::from_slice(out).map_err(|e| err!("gh issue view: {e}"))?;
    if !v.is_object() {
        return Err(err!("gh issue view: not an issue"));
    }
    Ok(read_issue(&v))
}

/// read_issue takes one issue out of gh's JSON, whichever command it answered.
fn read_issue(p: &serde_json::Value) -> Issue {
    let s = |k: &str| p.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
    let login = p
        .pointer("/author/login")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let labels = p
        .get("labels")
        .and_then(|v| v.as_array())
        .map(|ls| {
            ls.iter()
                .map(|l| {
                    l.get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                })
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let comments = p
        .get("comments")
        .and_then(|v| v.as_array())
        .map(|cs| {
            cs.iter()
                .map(|c| Comment {
                    author: c
                        .pointer("/author/login")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                    body: c
                        .get("body")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    Issue {
        number: p.get("number").and_then(|v| v.as_u64()).unwrap_or(0),
        title: s("title"),
        state: s("state"),
        author: login,
        labels,
        url: s("url"),
        body: s("body"),
        comments,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    // gh's JSON is read, labels and comments included, from either command's
    // answer.
    #[test]
    fn parse_issues_reads_gh() {
        let out = br#"[
            {"number":7,"title":"Add login","state":"OPEN","url":"https://github.com/acme/alpha/issues/7",
             "author":{"login":"alice"},"labels":[{"name":"bug"}]},
            {"number":9,"title":"Fix typo","state":"OPEN","url":"https://github.com/acme/alpha/issues/9",
             "author":{"login":"bob"},"labels":[]}
        ]"#;
        let issues = parse_issues(out).unwrap();
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].label(), "#7 Add login");
        assert_eq!(issues[0].author, "alice");
        assert_eq!(issues[0].labels, vec!["bug"]);
        assert_eq!(issues[0].url, "https://github.com/acme/alpha/issues/7");
        assert!(issues[0].comments.is_empty(), "a listing has no comments");

        let out = br#"{"number":7,"title":"Add login","state":"OPEN","url":"https://github.com/acme/alpha/issues/7",
            "author":{"login":"bob"},"labels":[],"body":"steps: none","comments":[
            {"author":{"login":"carol"},"body":"reproduced here"}]}"#;
        let one = parse_one(out).unwrap();
        assert_eq!(one.body, "steps: none");
        assert_eq!(one.comments.len(), 1);
        assert_eq!(one.comments[0].author, "carol");

        // A URL is part of the answer, and parse reads it; the refusal for a
        // missing one is the detail view's, tested below.

        let e = parse_one(b"[]").unwrap_err();
        assert!(e.0.contains("not an issue"), "{e}");
    }

    use std::ffi::OsStr;
    use std::os::unix::fs::PermissionsExt;

    // gh answering with a failure says what gh said, in the same shape the
    // rest of gm's errors take; both commands run through the same reader and
    // the same stderr forwarding, so one stand-in script answers for both.
    #[test]
    fn issues_say_what_gh_said() {
        let tmp = TempDir::new();
        let gh = tmp.join("gh");
        std::fs::write(&gh, "#!/bin/sh\necho 'not logged in' >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
        let repo = tmp.join("repo");
        crate::testutil::mkdir(&repo);
        let gh = OsStr::new(&gh);
        let e = issues_with(gh, Duration::from_secs(2), &repo).unwrap_err();
        assert!(e.0.contains("not logged in"), "{e}");
        let e = issue_with(gh, Duration::from_secs(2), &repo, 7).unwrap_err();
        assert!(e.0.contains("not logged in"), "{e}");

        // A gh that is not there at all is named as the thing to install.
        let e = gh_missing(std::io::Error::new(std::io::ErrorKind::NotFound, "no gh"));
        assert!(
            e.0.contains("gh is not installed"),
            "the missing program was not named:\n{e}"
        );
    }

    // An issue without a URL cannot be opened in a browser or copied, and gh
    // is asked again rather than trusted to carry it over: the detail view
    // names the defect instead of showing it.
    #[test]
    fn an_issue_without_a_url_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = TempDir::new();
        let gh = tmp.join("gh");
        std::fs::write(
            &gh,
            "#!/bin/sh\necho '{\"number\":7,\"title\":\"Add login\"}'\n",
        )
        .unwrap();
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
        let repo = tmp.join("repo");
        crate::testutil::mkdir(&repo);
        let e = issue_with(OsStr::new(&gh), Duration::from_secs(2), &repo, 7).unwrap_err();
        assert!(e.0.contains("no url for #7"), "{e}");
    }
}
