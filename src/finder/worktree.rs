use std::collections::HashMap;

use super::list::Item;
use super::{Cmd, Model};

/// Mode says which list is on screen.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Mode {
    #[default]
    Repos,
    Worktrees,
    Branches,
    Prs,
}

/// Stash is the repository list put aside while another list is up, so Esc
/// can put it back exactly as it was.
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
    pub(super) fn update(&mut self, change: impl FnOnce(&mut Vec<Item>) -> bool) {
        if change(&mut self.all) {
            self.stale = true;
        }
    }
}

impl Model {
    /// open_worktrees replaces the repository list with the checkouts of the
    /// selected repository. A repository git cannot answer for is left alone:
    /// the list simply does not change.
    pub(super) fn open_worktrees(&mut self) -> Vec<Cmd> {
        let Some(it) = self.current().cloned().filter(|_| self.mode == Mode::Repos) else {
            return vec![];
        };
        let wts = match (self.worktrees_of)(&it.path) {
            Ok(wts) if !wts.is_empty() => wts,
            _ => return vec![],
        };
        // Reversed, so git's first worktree — the main one — lands at the
        // bottom next to the cursor, the way the best match does in the main
        // list.
        let items = wts.iter().rev().map(|w| Item {
            label: w.label(),
            path: w.path.clone(),
            ..Default::default()
        });
        self.replace_list(Mode::Worktrees, &it, items.collect());
        self.load_status().into_iter().collect()
    }

    /// replace_list puts a list that belongs to the repository row it in place
    /// of the repository list, which is kept aside for restore.
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
        // A different list entirely: the held selection means nothing in it.
        self.view_stale = true;
        self.filter();
    }

    /// switch_to answers the worktree, branch and pull request keys. Each
    /// toggles its own list, and goes from another one straight to its own:
    /// all of them belong to the repository the repository list has selected.
    pub(super) fn switch_to(&mut self, mode: Mode) -> Vec<Cmd> {
        if self.mode == mode {
            self.restore();
            return self.load_status().into_iter().collect();
        }
        self.restore();
        match mode {
            Mode::Branches => self.open_branches(),
            Mode::Prs => self.open_prs(),
            _ => self.open_worktrees(),
        }
    }

    /// restore puts the repository list back, query and cursor included.
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
