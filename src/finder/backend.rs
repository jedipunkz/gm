//! The calls the finder makes to git, gh and the terminal, held as fields so
//! the tests can stand in for each one.

use std::collections::HashMap;
use std::sync::Arc;

use super::{browse, clipboard};
use crate::repo::{self, Branch, PullRequest, Worktree};
use crate::{Error, Result};

/// Seam is a git or GitHub call the model makes through a field, so the tests
/// can replace it; every real run uses the repo function of the same name.
type Seam<T> = Arc<dyn Fn(&str) -> T + Send + Sync>;
type StateScan = Arc<dyn Fn(&[String]) -> HashMap<String, repo::State> + Send + Sync>;
/// IssueOf asks gh for one issue's full text, the repository dir and the
/// number in hand; the tests stand in for it.
type IssueOf = Arc<dyn Fn(&str, u64) -> Result<crate::repo::Issue> + Send + Sync>;
/// PrOf asks gh for one pull request's full text, the same way.
type PrOf = Arc<dyn Fn(&str, u64) -> Result<PullRequest> + Send + Sync>;
/// CopyOf puts a string on the clipboard the terminal remembers.
type CopyOf = Arc<dyn Fn(&str) -> std::io::Result<()> + Send + Sync>;

/// Backend is every call the model makes through a field. Default is the real
/// one; a test replaces the field it needs to answer for itself.
#[derive(Clone)]
pub(super) struct Backend {
    pub(super) worktrees_of: Seam<Result<Vec<Worktree>>>,
    pub(super) branches_of: Seam<Result<Vec<Branch>>>,
    pub(super) remote_branches_of: Seam<(Vec<Branch>, Option<Error>)>,
    pub(super) prs_of: Seam<Result<Vec<PullRequest>>>,
    /// pr_of reads one pull request in full, for its detail panel.
    pub(super) pr_of: PrOf,
    /// issues_of lists a repository's open issues, issue_of reads one in
    /// full; both gh's to answer, so the tests can stand in for them.
    pub(super) issues_of: Seam<Result<Vec<crate::repo::Issue>>>,
    pub(super) issue_of: IssueOf,
    pub(super) state_scan_of: StateScan,
    pub(super) changed_of: Seam<usize>,
    pub(super) open_url: Arc<dyn Fn(&str) + Send + Sync>,
    /// copy_url puts a string on the clipboard the terminal remembers, by
    /// OSC52, so a test can watch for it instead of a tty.
    pub(super) copy_url: CopyOf,
    pub(super) gh: String, // the program that checks pull requests out
}

impl Default for Backend {
    fn default() -> Backend {
        Backend {
            worktrees_of: Arc::new(repo::worktrees),
            branches_of: Arc::new(repo::branches),
            remote_branches_of: Arc::new(repo::remote_branches),
            prs_of: Arc::new(repo::pull_requests),
            pr_of: Arc::new(repo::pull_request),
            issues_of: Arc::new(repo::issues),
            issue_of: Arc::new(repo::issue),
            state_scan_of: Arc::new(|ps: &[String]| {
                repo::status_map(ps, repo::status_with_worktrees)
            }),
            changed_of: Arc::new(repo::changed_files),
            open_url: Arc::new(|u| {
                let _ = browse::open_url(u);
            }),
            copy_url: Arc::new(clipboard::copy),
            gh: "gh".into(),
        }
    }
}
