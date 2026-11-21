use std::collections::HashMap;

use super::list::Item;
use super::{Cmd, Model, Msg};
use crate::repo::Worktree;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Mode {
    #[default]
    Repos,
    Worktrees,
    Branches,
    Prs,
    Issues,
}

#[derive(Debug, Clone)]
pub struct Stash {
    all: Vec<Item>,
    view: Vec<usize>,
    matched: HashMap<usize, Vec<usize>>,
    cursor: usize,
    query: String,
    stale: bool,
}

impl Stash {
    pub(super) fn mark_stale(&mut self) {
        self.stale = true;
    }

    pub(super) fn update(&mut self, change: impl FnOnce(&mut Vec<Item>) -> bool) {
        if change(&mut self.all) {
            self.stale = true;
        }
    }
}

pub struct Worktrees {
    pub(super) path: String,
    pub(super) busy_tag: u64,
    pub(super) wts: crate::Result<Vec<Worktree>>,
}

impl Model {
    pub(super) fn open_worktrees(&mut self) -> Vec<Cmd> {
        let Some(it) = self.current().cloned().filter(|_| self.mode == Mode::Repos) else {
            return vec![];
        };
        let (busy_tag, busy) = self.start_busy(&format!("listing the worktrees of {}…", it.label));
        let (worktrees_of, path) = (self.backend.worktrees_of.clone(), it.path);
        vec![
            busy,
            Cmd::Task(Box::new(move || {
                let wts = worktrees_of(&path);
                Some(Msg::Worktrees(Worktrees {
                    path,
                    busy_tag,
                    wts,
                }))
            })),
        ]
    }

    pub(super) fn show_worktrees(&mut self, msg: Worktrees) -> Vec<Cmd> {
        self.finish_busy(msg.busy_tag);
        let Some(it) = self
            .current()
            .cloned()
            .filter(|it| self.mode == Mode::Repos && it.path == msg.path)
        else {
            return vec![];
        };
        let wts = match msg.wts {
            Ok(wts) if !wts.is_empty() => wts,
            Err(e) => {
                self.note = e.0;
                return vec![];
            }
            Ok(_) => {
                self.note = format!("no worktrees in {}", it.label);
                return vec![];
            }
        };
        let items = wts.iter().rev().map(|w| Item {
            label: w.label(),
            path: w.path.clone(),
            branch: crate::repo::Branch {
                name: w.branch.clone(),
                ..Default::default()
            },
            ..Default::default()
        });
        self.replace_list(Mode::Worktrees, &it, items.collect());
        self.load_status().into_iter().collect()
    }

    pub(super) fn replace_list(&mut self, mode: Mode, it: &Item, items: Vec<Item>) {
        self.saved = Some(Stash {
            all: std::mem::replace(&mut self.all, items),
            view: std::mem::take(&mut self.view),
            matched: std::mem::take(&mut self.matched),
            cursor: self.cursor,
            query: self.input.value(),
            stale: false,
        });
        self.origin = it.label.clone();
        self.repo_at = it.path.clone();
        self.mode = mode;
        self.input.set_value("");
        self.view_stale = true;
        self.filter();
    }

    pub(super) fn switch_to(&mut self, mode: Mode) -> Vec<Cmd> {
        if self.mode == mode {
            self.restore();
            return self.load_status().into_iter().collect();
        }
        self.restore();
        match mode {
            Mode::Branches => self.open_branches(),
            Mode::Prs => self.open_prs(),
            Mode::Issues => self.open_issues(),
            _ => self.open_worktrees(),
        }
    }

    pub(super) fn restore(&mut self) {
        let Some(s) = self.saved.take() else { return };
        let stale = s.stale;
        (self.all, self.view, self.matched, self.cursor) = (s.all, s.view, s.matched, s.cursor);
        self.input.set_value(&s.query);
        self.mode = Mode::Repos;
        self.origin.clear();
        self.repo_at.clear();
        if stale {
            let note = std::mem::take(&mut self.note);
            self.view_stale = true;
            self.filter();
            self.note = note;
        }
    }
}
