use std::collections::HashMap;

use super::info::tildify;
use super::issues::{Detail, Subject};
use super::list::Item;
use super::overlay::{Change, Overlay, Pending};
use super::worktree::Mode;
use super::{Action, Cmd, Model, Msg, Outcome, PrDetail, Prs};
use crate::repo::{self, PullRequest, Tree, Worktree};

impl Model {
    /// open_prs asks gh for the selected repository's open pull requests, off
    /// the UI thread: it goes to GitHub, and the finder stays usable while it
    /// does.
    pub(super) fn open_prs(&mut self) -> Vec<Cmd> {
        let Some(it) = self.current().cloned().filter(|_| self.mode == Mode::Repos) else {
            return vec![];
        };
        let (busy_tag, busy) = self.start_busy(&format!("asking GitHub about {}…", it.label));
        let (prs_of, worktrees_of) = (self.prs_of.clone(), self.worktrees_of.clone());
        let (tree, path) = (self.tree.clone(), it.path);
        vec![
            busy,
            Cmd::Task(Box::new(move || {
                // Where each one is checked out is git's to say, so it is
                // asked here too, not once the answer is on the UI thread.
                let prs = prs_of(&path).map(|prs| {
                    let wts = if prs.is_empty() {
                        vec![]
                    } else {
                        worktrees_of(&path).unwrap_or_default()
                    };
                    with_checkouts(&tree, &path, wts, prs)
                });
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

        // gh lists the newest first; reversed, it lands at the bottom next to
        // the cursor.
        let items = prs
            .iter()
            .rev()
            .map(|(p, at)| Item {
                label: p.label(),
                path: at.clone(),
                pr: p.clone(),
                ..Default::default()
            })
            .collect();
        self.replace_list(Mode::Prs, &it, items);
        // A full page exactly means the next page cost nothing to hide:
        // #42 not on the list is not "no such pull request".
        if prs.len() >= repo::PR_LIMIT {
            self.note = format!("showing the newest {} open pull requests", repo::PR_LIMIT);
        }
        self.load_status().into_iter().collect()
    }

    /// view_pr is Enter in the pull request list: gh is asked for the one
    /// pull request in full, off the UI thread, and the panel opens once it
    /// is back. Checking it out is the panel's enter, not the list's.
    pub(super) fn view_pr(&mut self) -> Vec<Cmd> {
        let Some(it) = self.current().cloned() else {
            return vec![];
        };
        let n = it.pr.number;
        let (busy_tag, busy) = self.start_busy(&format!("viewing #{n}…"));
        let (pr_of, path) = (self.pr_of.clone(), self.repo_at.clone());
        vec![
            busy,
            Cmd::Task(Box::new(move || {
                let pr = pr_of(&path, n);
                Some(Msg::PrDetail(PrDetail {
                    path,
                    busy_tag,
                    number: n,
                    pr,
                }))
            })),
        ]
    }

    /// show_pr_detail puts the panel up, if the answer is still about the
    /// pull request list on screen. When gh fails the panel still opens, on
    /// what the listing carried, with gh's reason where the title goes:
    /// checking out is behind the panel's enter, and a failed read of the
    /// comments must not take that away.
    pub(super) fn show_pr_detail(&mut self, msg: PrDetail) -> Vec<Cmd> {
        self.finish_busy(msg.busy_tag);
        if self.mode != Mode::Prs || msg.path != self.repo_at {
            return vec![];
        }
        let (pr, note) = match msg.pr {
            Ok(pr) => (pr, String::new()),
            Err(e) => match self.pr_row(msg.number) {
                Some(it) => (it.pr.clone(), e.0),
                None => {
                    self.note = e.0;
                    return vec![];
                }
            },
        };
        self.detail = Some(Detail {
            subject: Subject::Pr(pr),
            scroll: 0,
        });
        self.over = Overlay::Detail;
        self.note = note;
        vec![]
    }

    /// pr_row is the pull request list's row for number. The panel goes by
    /// number, not by the cursor: the cursor can move while gh is answering.
    pub(super) fn pr_row(&self, number: u64) -> Option<&Item> {
        if self.mode != Mode::Prs {
            return None;
        }
        self.all.iter().find(|it| it.pr.number == number)
    }

    /// check_out_shown_pr is Enter in a pull request's detail panel: the
    /// panel comes down and the pull request it showed is checked out.
    pub(super) fn check_out_shown_pr(&mut self) -> Vec<Cmd> {
        let Some(n) = self.detail.as_ref().map(|d| d.subject.number()) else {
            return vec![];
        };
        let Some(it) = self.pr_row(n).cloned() else {
            return vec![];
        };
        self.close_detail();
        self.check_out_pr(it)
    }

    /// check_out_pr goes to the pull request's worktree, having gh make it
    /// first when there is none.
    fn check_out_pr(&mut self, it: Item) -> Vec<Cmd> {
        if !it.path.is_empty() {
            self.result = Outcome {
                action: Action::Jump,
                arg: it.path,
            };
            return vec![Cmd::Quit];
        }
        let dir = match self.worktree_at(&it.pr.checkout(), &it.pr.worktree_name()) {
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

/// with_checkouts pairs each of the pull requests of the repository at path
/// with the worktree it is already checked out in, or "" when there is none.
fn with_checkouts(
    tree: &Tree,
    path: &str,
    wts: Vec<Worktree>,
    prs: Vec<PullRequest>,
) -> Vec<(PullRequest, String)> {
    // A pull request is already checked out when its branch is, or when
    // its worktree is where gm files it. A fork's branch is matched by
    // place only: its main is not the repository's main.
    let (mut by_branch, mut by_path) = (HashMap::new(), Vec::new());
    for w in wts {
        by_path.push(w.path.clone());
        if !w.branch.is_empty() {
            by_branch.insert(w.branch, w.path);
        }
    }
    let r = tree.at(path);
    let at = |p: &PullRequest| -> String {
        if let Some(path) = by_branch.get(&p.branch).filter(|_| !p.fork) {
            return path.clone();
        }
        if let Some(r) = r.as_ref().filter(|_| repo::valid_branch(&p.checkout())) {
            let dir = tree.worktree_dir(r, &p.worktree_name());
            // git prints the resolved path, the tree the configured one:
            // under a symlinked root they differ as text.
            if by_path.iter().any(|w| repo::same_path(w, &dir)) {
                return dir;
            }
        }
        String::new()
    };

    prs.into_iter()
        .map(|p| {
            let dir = at(&p);
            (p, dir)
        })
        .collect()
}
