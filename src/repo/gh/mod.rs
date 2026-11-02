//! GitHub, through gh: the pull requests and issues of a repository, and the
//! one way gm starts gh.

mod issue;
mod pr;

pub use issue::*;
pub use pr::*;

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
