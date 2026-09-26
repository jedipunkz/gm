use std::collections::{HashMap, HashSet};

use super::info::tildify;
use super::list::Item;
use super::overlay::{Change, Pending};
use super::worktree::Mode;
use super::{Action, Cmd, Model, Msg, Outcome, Prs};
use crate::repo::{self, PullRequest};

impl Model {
    /// open_prs asks gh for the selected repository's open pull requests, off
    /// the UI thread: it goes to GitHub, and the finder stays usable while it
    /// does.
    pub(super) fn open_prs(&mut self) -> Vec<Cmd> {
        let Some(it) = self.current().cloned().filter(|_| self.mode == Mode::Repos) else {
            return vec![];
        };
        let (busy_tag, busy) = self.start_busy(&format!("asking GitHub about {}…", it.label));
        let (prs_of, path) = (self.prs_of.clone(), it.path);
        vec![
            busy,
            Cmd::Task(Box::new(move || {
                let prs = prs_of(&path);
                Some(Msg::Prs(Prs {
                    path,
                    busy_tag,
                    prs,
                }))
            })),
        ]
    }

    /// show_prs puts the answer on screen, if the repository it is about is
    /// still the one selected in the repository list; otherwise the user has
    /// moved on.
    pub(super) fn show_prs(&mut self, msg: Prs) -> Vec<Cmd> {
        self.finish_busy(msg.busy_tag);
        let Some(it) = self
            .current()
            .cloned()
            .filter(|it| self.mode == Mode::Repos && it.path == msg.path)
        else {
            return vec![];
        };
        let prs = match msg.prs {
            Err(e) => {
                self.note = e.0;
                return vec![];
            }
            Ok(prs) if prs.is_empty() => {
                self.note = format!("no open pull requests in {}", it.label);
                return vec![];
            }
            Ok(prs) => prs,
        };

        // A pull request is already checked out when its branch is, or when
        // its worktree is where gm files it. A fork's branch is matched by
        // place only: its main is not the repository's main.
        let (mut by_branch, mut by_path) = (HashMap::new(), HashSet::new());
        for w in (self.worktrees_of)(&it.path).unwrap_or_default() {
            by_path.insert(w.path.clone());
            if !w.branch.is_empty() {
                by_branch.insert(w.branch, w.path);
            }
        }
        let r = self.tree.at(&it.path);
        let at = |p: &PullRequest| -> String {
            if let Some(path) = by_branch.get(&p.branch).filter(|_| !p.fork) {
                return path.clone();
            }
            if let Some(r) = r.as_ref().filter(|_| repo::valid_branch(&p.checkout())) {
                let dir = self.tree.worktree_dir(r, &p.checkout());
                if by_path.contains(&dir) {
                    return dir;
                }
            }
            String::new()
        };

        // gh lists the newest first; reversed, it lands at the bottom next to
        // the cursor.
        let items = prs
            .iter()
            .rev()
            .map(|p| Item {
                label: p.label(),
                path: at(p),
                pr: p.clone(),
                ..Default::default()
            })
            .collect();
        self.replace_list(Mode::Prs, &it, items);
        self.load_status().into_iter().collect()
    }

    /// check_out_pr is Enter in the pull request list: go to the pull
    /// request's worktree, having gh make it first when there is none.
    pub(super) fn check_out_pr(&mut self) -> Vec<Cmd> {
        let Some(it) = self.current().cloned() else {
            return vec![];
        };
        if !it.path.is_empty() {
            self.result = Outcome {
                action: Action::Jump,
                arg: it.path,
            };
            return vec![Cmd::Quit];
        }
        let dir = match self.worktree_for(&it.pr.checkout()) {
            Ok(dir) => dir,
            Err(why) => {
                self.note = why;
                return vec![];
            }
        };
        let (busy_tag, busy) = self.start_busy(&format!(
            "checking #{} out at {}…",
            it.pr.number,
            tildify(&dir)
        ));
        vec![
            busy,
            self.perform(
                Pending {
                    kind: Change::CheckOutPr,
                    mode: self.mode,
                    repo_at: self.repo_at.clone(),
                    arg: it.pr.number.to_string(),
                    dir,
                    ..Default::default()
                },
                busy_tag,
            ),
        ]
    }
}
