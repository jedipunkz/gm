use std::collections::HashMap;
use std::sync::Arc;

use super::{browse, clipboard};
use crate::repo::{self, Branch, PullRequest, Worktree};
use crate::{Error, Result};

type Seam<T> = Arc<dyn Fn(&str) -> T + Send + Sync>;
type StateScan = Arc<dyn Fn(&[String]) -> HashMap<String, repo::State> + Send + Sync>;
type IssueOf = Arc<dyn Fn(&str, u64) -> Result<crate::repo::Issue> + Send + Sync>;
type PrOf = Arc<dyn Fn(&str, u64) -> Result<PullRequest> + Send + Sync>;
type CopyOf = Arc<dyn Fn(&str) -> std::io::Result<()> + Send + Sync>;

#[derive(Clone)]
pub(super) struct Backend {
    pub(super) worktrees_of: Seam<Result<Vec<Worktree>>>,
    pub(super) branches_of: Seam<Result<Vec<Branch>>>,
    pub(super) remote_branches_of: Seam<(Vec<Branch>, Option<Error>)>,
    pub(super) prs_of: Seam<Result<Vec<PullRequest>>>,
    pub(super) pr_of: PrOf,
    pub(super) issues_of: Seam<Result<Vec<crate::repo::Issue>>>,
    pub(super) issue_of: IssueOf,
    pub(super) state_scan_of: StateScan,
    pub(super) changed_of: Seam<usize>,
    pub(super) open_url: Arc<dyn Fn(&str) + Send + Sync>,
    pub(super) copy_url: CopyOf,
    pub(super) gh: String,
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
