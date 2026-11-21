use std::collections::HashMap;

use super::detail::{Detail, Subject};
use super::info::tildify;
use super::list::Item;
use super::overlay::{Change, Overlay, Pending};
use super::worktree::Mode;
use super::{Action, Cmd, Model, Msg, Outcome};
use crate::repo::{self, PullRequest, Tree, Worktree};

pub(super) fn pr_branch(p: &PullRequest) -> String {
    if p.fork {
        format!("{}:{}", p.head_owner, p.branch)
    } else {
        p.branch.clone()
    }
}

pub struct Prs {
    pub(super) path: String,
    pub(super) busy_tag: u64,
    pub(super) prs: crate::Result<Vec<(PullRequest, String)>>,
}

pub struct PrDetail {
    pub(super) path: String,
    pub(super) busy_tag: u64,
    pub(super) number: u64,
    pub(super) pr: crate::Result<PullRequest>,
}

impl Model {
    pub(super) fn open_prs(&mut self) -> Vec<Cmd> {
        let Some(it) = self.current().cloned().filter(|_| self.mode == Mode::Repos) else {
            return vec![];
        };
        let (busy_tag, busy) = self.start_busy(&format!("asking GitHub about {}…", it.label));
        let (prs_of, worktrees_of) = (
            self.backend.prs_of.clone(),
            self.backend.worktrees_of.clone(),
        );
        let (tree, path) = (self.tree.clone(), it.path);
        vec![
            busy,
            Cmd::Task(Box::new(move || {
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
        if prs.len() >= repo::PR_LIMIT {
            self.note = format!("showing the newest {} open pull requests", repo::PR_LIMIT);
        }
        self.load_status().into_iter().collect()
    }

    pub(super) fn view_pr(&mut self) -> Vec<Cmd> {
        let Some(it) = self.current().cloned() else {
            return vec![];
        };
        let n = it.pr.number;
        let (busy_tag, busy) = self.start_busy(&format!("viewing #{n}…"));
        let (pr_of, path) = (self.backend.pr_of.clone(), self.repo_at.clone());
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
            lines: std::cell::RefCell::new(None),
        });
        self.over = Overlay::Detail;
        self.note = note;
        vec![]
    }

    pub(super) fn pr_row(&self, number: u64) -> Option<&Item> {
        if self.mode != Mode::Prs {
            return None;
        }
        self.all.iter().find(|it| it.pr.number == number)
    }

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

fn with_checkouts(
    tree: &Tree,
    path: &str,
    wts: Vec<Worktree>,
    prs: Vec<PullRequest>,
) -> Vec<(PullRequest, String)> {
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
