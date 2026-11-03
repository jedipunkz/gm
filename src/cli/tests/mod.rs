//! The CLI's tests: one run per behavior, both streams read back. They live
//! apart from the code the way the finder's do.

use super::*;
use ratatui::text::Span;

use crate::paths;
use crate::repo::Repo;
use crate::testutil::{TempDir, exists, git, git_repo, mkdir, write};

/// Run is one command run against a tree, with both streams captured.
struct Run {
    out: String,
    err: String,
    res: Result<()>,
}

fn run_in(tree: &Tree, state: &str, cfg: Config, f: impl FnOnce(&mut App) -> Result<()>) -> Run {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let res = {
        let mut a = App {
            cfg,
            tree: tree.clone(),
            state: state.into(),
            out: &mut out,
            err: &mut err,
        };
        f(&mut a)
    };
    Run {
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
        res,
    }
}

fn tree(root: &str) -> Tree {
    Tree {
        roots: vec![root.to_string()],
    }
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

mod act;
mod get;
mod list;
mod migrate;
mod remove;
mod spawn_budget;
mod usage;
mod wt;
