use std::collections::{HashMap, HashSet};

use super::info::tildify;
use super::list::Item;
use super::overlay::{Change, Pending};
use super::worktree::Mode;
use super::{Action, Cmd, Model, Msg, Outcome};
use crate::Error;
use crate::repo;
use crate::repo::Branch;

pub struct Branches {
    pub(super) path: String,
    pub(super) busy_tag: u64,
    pub(super) branches: crate::Result<Vec<Branch>>,
    pub(super) at: HashMap<String, String>,
}

pub struct RemoteBranches {
    pub(super) path: String,
    pub(super) branches: Vec<Branch>,
    pub(super) busy_tag: u64,
    pub(super) err: Option<Error>,
}

impl Model {
    pub(super) fn open_branches(&mut self) -> Vec<Cmd> {
        let Some(it) = self.current().cloned().filter(|_| self.mode == Mode::Repos) else {
            return vec![];
        };
        let (busy_tag, busy) = self.start_busy(&format!("listing the branches of {}…", it.label));
        let (branches_of, worktrees_of) = (
            self.backend.branches_of.clone(),
            self.backend.worktrees_of.clone(),
        );
        let path = it.path;
        vec![
            busy,
            Cmd::Task(Box::new(move || {
                let branches = branches_of(&path);
                let at = match &branches {
                    Ok(bs) if !bs.is_empty() => worktrees_of(&path)
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|w| !w.branch.is_empty())
                        .map(|w| (w.branch, w.path))
                        .collect(),
                    _ => HashMap::new(),
                };
                Some(Msg::Branches(Branches {
                    path,
                    busy_tag,
                    branches,
                    at,
                }))
            })),
        ]
    }

    pub(super) fn show_branches(&mut self, msg: Branches) -> Vec<Cmd> {
        self.finish_busy(msg.busy_tag);
        let Some(it) = self
            .current()
            .cloned()
            .filter(|it| self.mode == Mode::Repos && it.path == msg.path)
        else {
            return vec![];
        };
        let bs = match msg.branches {
            Ok(bs) if !bs.is_empty() => bs,
            Err(e) => {
                self.note = e.0;
                return vec![];
            }
            Ok(_) => {
                self.note = format!("no branches in {}", it.label);
                return vec![];
            }
        };
        let at = msg.at;

        let items = bs
            .iter()
            .rev()
            .map(|b| Item {
                label: b.label(),
                path: at.get(&b.name).cloned().unwrap_or_default(),
                branch: b.clone(),
                ..Default::default()
            })
            .collect();
        self.replace_list(Mode::Branches, &it, items);
        let (busy_tag, busy) = self.start_busy(&format!(
            "asking the remotes of {} for their branches…",
            it.label
        ));
        let (remote_of, path) = (self.backend.remote_branches_of.clone(), it.path.clone());
        let mut cmds: Vec<Cmd> = self.load_status().into_iter().collect();
        cmds.push(busy);
        cmds.push(Cmd::Task(Box::new(move || {
            let (branches, err) = remote_of(&path);
            Some(Msg::RemoteBranches(RemoteBranches {
                path,
                branches,
                busy_tag,
                err,
            }))
        })));
        cmds
    }

    pub(super) fn add_remote_branches(&mut self, msg: RemoteBranches) -> Vec<Cmd> {
        self.finish_busy(msg.busy_tag);
        if self.mode != Mode::Branches || self.repo_at != msg.path {
            return vec![];
        }
        let mut known: HashSet<String> = self
            .all
            .iter()
            .map(|it| {
                if it.branch.remote.is_empty() {
                    it.branch.name.clone()
                } else {
                    it.branch.remote.clone()
                }
            })
            .collect();
        let mut added = Vec::new();
        for b in msg.branches {
            if known.contains(&b.remote) || known.contains(&b.name) {
                continue;
            }
            known.insert(b.remote.clone());
            added.push(Item {
                label: b.label(),
                branch: b,
                ..Default::default()
            });
        }
        if !added.is_empty() {
            let keep = self
                .current()
                .map(|it| it.label.clone())
                .unwrap_or_default();
            added.append(&mut self.all);
            self.all = added;
            self.view_stale = true;
            self.filter();
            if let Some(i) = self
                .view
                .iter()
                .rposition(|&idx| self.all[idx].label == keep)
            {
                self.cursor = i;
            }
        }
        if let Some(e) = msg.err {
            self.note = e.0;
        }
        self.load_status().into_iter().collect()
    }

    pub(super) fn check_out(&mut self) -> Vec<Cmd> {
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
        let dir = match self.worktree_for(&it.branch.name) {
            Ok(dir) => dir,
            Err(why) => {
                self.note = why;
                return vec![];
            }
        };
        let what = if it.branch.unfetched {
            format!(
                "fetching {} and checking it out at {}…",
                it.branch.remote,
                tildify(&dir)
            )
        } else {
            format!("checking {} out at {}…", it.branch.name, tildify(&dir))
        };
        let (busy_tag, busy) = self.start_busy(&what);
        let b = it.branch;
        vec![
            busy,
            self.perform(
                Pending {
                    kind: Change::CheckOut,
                    mode: self.mode,
                    repo_at: self.repo_at.clone(),
                    arg: b.name,
                    from: b.remote,
                    fetch: b.unfetched,
                    dir,
                    ..Default::default()
                },
                busy_tag,
            ),
        ]
    }

    pub(super) fn worktree_for(&self, name: &str) -> Result<String, String> {
        self.worktree_at(name, name)
    }

    pub(super) fn worktree_at(&self, name: &str, dir_name: &str) -> Result<String, String> {
        if !repo::valid_branch(name) {
            return Err(format!("{name:?} is not a branch name"));
        }
        let Some(r) = self.tree.at(&self.repo_at) else {
            return Err(format!("{} is not under any root", self.repo_at));
        };
        let dir = self.tree.worktree_dir(&r, dir_name);
        if crate::paths::exists(&dir) {
            return Err(format!("{} already exists", tildify(&dir)));
        }
        Ok(dir)
    }
}
