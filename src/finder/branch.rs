use std::collections::{HashMap, HashSet};

use super::info::tildify;
use super::list::Item;
use super::overlay::{Change, Pending};
use super::worktree::Mode;
use super::{Action, Cmd, Model, Msg, Outcome, RemoteBranches};
use crate::repo;

impl Model {
    /// open_branches replaces the repository list with the branches of the
    /// selected repository, local ones and those only a remote has. A branch
    /// that is checked out somewhere carries that worktree's path, so Enter
    /// can simply go there.
    ///
    /// The list is what git already knows, so it is up at once; the remotes
    /// are asked in the background for what the last fetch did not bring.
    pub(super) fn open_branches(&mut self) -> Vec<Cmd> {
        let Some(it) = self.current().cloned().filter(|_| self.mode == Mode::Repos) else {
            return vec![];
        };
        let bs = match (self.branches_of)(&it.path) {
            Ok(bs) if !bs.is_empty() => bs,
            _ => return vec![],
        };
        let at: HashMap<String, String> = (self.worktrees_of)(&it.path)
            .unwrap_or_default()
            .into_iter()
            .filter(|w| !w.branch.is_empty())
            .map(|w| (w.branch, w.path))
            .collect();

        // Reversed, so the branch with the newest commit lands at the bottom
        // next to the cursor.
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
        let busy = self.start_busy(&format!(
            "asking the remotes of {} for their branches…",
            it.label
        ));
        let (remote_of, path) = (self.remote_branches_of.clone(), it.path.clone());
        let mut cmds: Vec<Cmd> = self.load_status().into_iter().collect();
        cmds.push(busy);
        cmds.push(Cmd::Task(Box::new(move || {
            let (branches, err) = remote_of(&path);
            Some(Msg::RemoteBranches(RemoteBranches {
                path,
                branches,
                err,
            }))
        })));
        cmds
    }

    /// add_remote_branches puts the remotes' answer into the branch list it
    /// was asked for, at the top: nothing says how recent they are. The
    /// selection stays on the row it was on.
    pub(super) fn add_remote_branches(&mut self, msg: RemoteBranches) -> Vec<Cmd> {
        self.busy.clear();
        if self.mode != Mode::Branches || self.repo_at != msg.path {
            return vec![];
        }
        // Skip what the list already has: a local branch hides its remote
        // namesake, and a second answer for the same list adds nothing.
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

    /// check_out is Enter in the branch list: go to the branch's worktree,
    /// making it first when there is none. Making one is not asked about — the
    /// branch is what Enter was pressed on, and nothing is lost if it was the
    /// wrong one.
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
        let busy = self.start_busy(&what);
        let b = it.branch;
        vec![
            busy,
            self.perform(Pending {
                kind: Change::CheckOut,
                mode: self.mode,
                repo_at: self.repo_at.clone(),
                arg: b.name,
                from: b.remote,
                fetch: b.unfetched,
                dir,
                ..Default::default()
            }),
        ]
    }

    /// worktree_for is where a new worktree filed under name goes in the
    /// repository the list belongs to, or why one cannot go there. The name is
    /// checked before a path is built from it: a branch name comes from
    /// whoever pushed it.
    pub(super) fn worktree_for(&self, name: &str) -> Result<String, String> {
        if !repo::valid_branch(name) {
            return Err(format!("{name:?} is not a branch name"));
        }
        let Some(r) = self.tree.at(&self.repo_at) else {
            return Err(format!("{} is not under any root", self.repo_at));
        };
        let dir = self.tree.worktree_dir(&r, name);
        if crate::paths::exists(&dir) {
            return Err(format!("{} already exists", tildify(&dir)));
        }
        Ok(dir)
    }
}
