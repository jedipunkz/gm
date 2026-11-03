//! GitHub, through gh: the pull requests and issues of a repository, and the
//! one way gm starts gh.

mod issue;
mod pr;

pub(crate) use issue::ISSUE_LIMIT;
pub use issue::{Comment, Issue, issue, issues, parse_issues};
pub(crate) use pr::PR_LIMIT;
pub use pr::{
    FORK_DIR, PullRequest, check_out_pull_request, check_out_pull_request_in, parse_pull_requests,
    pull_request, pull_requests,
};

use std::ffi::OsStr;
use std::process::Command;

use crate::Error;

pub(crate) fn gh_command(program: &OsStr) -> Command {
    let command = Command::new(program);
    #[cfg(test)]
    let command = {
        let mut command = command;
        crate::repo::git::isolate_git_config(&mut command);
        command
    };
    command
}

/// gh_missing turns a gh that could not be started into the one line worth
/// reading.
pub(crate) fn gh_missing(e: std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::NotFound {
        return "gh is not installed: pull requests come from the GitHub CLI".into();
    }
    e.into()
}
