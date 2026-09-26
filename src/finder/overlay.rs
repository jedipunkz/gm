use ratatui::text::{Line, Span};

use super::command::COMMANDS;
use super::info::{Seg, tildify, wrap_segs};
use super::worktree::Mode;
use super::{Cmd, Done, Key, Model, Msg};
use crate::repo::{self, Branch};
use crate::{Error, err, paths};

/// Overlay says which panel is drawn over the list.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Overlay {
    #[default]
    None,
    Help,
    Confirm,
}

/// Change is what a confirmation will carry out. It is not an Action: none of
/// these leave the finder, they happen under it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Change {
    #[default]
    None,
    Create,
    Remove,
    AddWorktree,
    RemoveWorktree,
    CheckOut,   // a worktree for a branch in the branch list, then go there
    CheckOutPr, // a worktree for a pull request, made by gh, then go there
}

/// Pending is the change a confirmation is waiting on. Nothing has happened
/// yet when one is on screen.
#[derive(Debug, Clone, Default)]
pub struct Pending {
    pub kind: Change,
    pub mode: Mode,          // the list the change belongs to
    pub repo_at: String,     // the repository that list belongs to
    pub arg: String,         // the path to remove, the reference to create, the branch to check out
    pub dir: String,         // where a worktree will go, or which one goes away
    pub from: String,        // the remote branch a new branch starts at, "origin/feature"
    pub fetch: bool,         // from has to be fetched first: only the remote has it
    pub title: String,       // "remove", "create worktree"
    pub detail: Vec<String>, // what it will do, a line each
    pub force: bool,         // there is work in it and the user has been told
}

impl Model {
    /// perform carries out a confirmed change, off the UI thread.
    pub(super) fn perform(&self, a: Pending, busy_tag: u64) -> Cmd {
        let (tree, gh) = (self.tree.clone(), self.gh.clone());
        Cmd::Task(Box::new(move || {
            let kind = a.kind;
            let done = |res: Result<(String, String), Error>| {
                let (path, label, err) = match res {
                    Ok((path, label)) => (path, label, None),
                    Err(e) => (String::new(), String::new(), Some(e)),
                };
                Some(Msg::Done(Done {
                    kind,
                    mode: a.mode,
                    repo_at: a.repo_at.clone(),
                    label,
                    path,
                    busy_tag,
                    err,
                }))
            };
            match kind {
                Change::Remove => done(match tree.at(&a.arg) {
                    None => Err(err!("{} is not under any root", a.arg)),
                    Some(r) => repo::delete(&r).map(|_| (r.path(), r.rel)),
                }),
                Change::Create => done(tree.create(&a.arg, false).map(|r| (r.path(), r.rel))),
                Change::AddWorktree | Change::CheckOut => done((|| {
                    if a.fetch {
                        let b = Branch {
                            name: a.arg.clone(),
                            remote: a.from.clone(),
                            unfetched: true,
                        };
                        repo::fetch_branch(&a.repo_at, &b)?;
                    }
                    let r = tree
                        .at(&a.repo_at)
                        .ok_or_else(|| err!("{} is not under any root", a.repo_at))?;
                    let root = paths::join(&r.root, repo::WORKTREE_ROOT);
                    repo::add_worktree_from(&root, &a.repo_at, &a.dir, &a.arg, &a.from)?;
                    Ok((a.dir.clone(), a.arg.clone()))
                })()),
                Change::CheckOutPr => done((|| {
                    let n = a
                        .arg
                        .parse()
                        .map_err(|_| err!("{:?} is not a pull request", a.arg))?;
                    let r = tree
                        .at(&a.repo_at)
                        .ok_or_else(|| err!("{} is not under any root", a.repo_at))?;
                    let root = paths::join(&r.root, repo::WORKTREE_ROOT);
                    repo::check_out_pull_request_in(&root, &gh, &a.repo_at, &a.dir, n)?;
                    Ok((a.dir.clone(), a.arg.clone()))
                })()),
                Change::RemoveWorktree => done(
                    match tree.at(&a.repo_at) {
                        None => Err(err!("{} is not under any root", a.repo_at)),
                        Some(r) => repo::remove_worktree_and_prune(&r, &a.dir, a.force),
                    }
                    .map(|_| (a.dir.clone(), String::new())),
                ),
                Change::None => None,
            }
        }))
    }

    /// confirm puts the question on screen. Nothing happens until it is
    /// answered.
    pub(super) fn confirm(&mut self, mut p: Pending) -> Vec<Cmd> {
        p.mode = self.mode;
        p.repo_at = self.repo_at.clone();
        self.over = Overlay::Confirm;
        self.ask = p;
        self.note.clear();
        vec![]
    }

    /// box_width is how wide a panel may be: narrow enough to leave the list
    /// visible around it, and never wider than the window.
    fn box_width(&self) -> usize {
        (self.w as usize).saturating_sub(10).clamp(20, 76)
    }

    /// confirm_box asks before anything changes, and says exactly what will.
    /// It is narrower than the help panel: a question should not blot out the
    /// list it is asking about.
    pub(super) fn confirm_box(&self) -> Vec<Line<'static>> {
        let st = &self.st;
        let w = self.box_width().min(56);
        let mut rows = vec![Line::from(Span::styled(self.ask.title.clone(), st.label))];
        for d in &self.ask.detail {
            rows.extend(wrap_segs(&[Seg::new(d, st.subject)], w, "  "));
        }
        rows.push(Line::default());
        rows.push(Line::from(vec![
            Span::styled("y", st.ref_local),
            Span::styled(" do it", st.subject),
            Span::styled("   ", st.dim),
            Span::styled("n", st.ref_local),
            Span::styled(" cancel", st.subject),
        ]));
        rows
    }

    /// help_box draws the command list as a panel.
    pub(super) fn help_box(&self) -> Vec<Line<'static>> {
        let st = &self.st;
        let mut rows = vec![
            Line::from(Span::styled("commands", st.label)),
            Line::default(),
        ];
        let names = COMMANDS
            .iter()
            .map(|c| c.label().chars().count())
            .max()
            .unwrap_or(0);
        // The descriptions fold under themselves rather than push the panel
        // wider than the window.
        let w = self.box_width().saturating_sub(names + 2).max(20);
        for c in COMMANDS {
            let label = c.label();
            let pad = " ".repeat(names - label.chars().count());
            let mut lines = wrap_segs(&[Seg::new(c.what, st.subject)], w, "").into_iter();
            let mut first = vec![
                Span::styled(label, st.ref_local),
                Span::raw(format!("{pad}  ")),
            ];
            first.extend(lines.next().unwrap_or_default().spans);
            rows.push(Line::from(first));
            for extra in lines {
                let mut spans = vec![Span::raw(" ".repeat(names + 2))];
                spans.extend(extra.spans);
                rows.push(Line::from(spans));
            }
        }
        rows.push(Line::default());
        rows.push(Line::from(Span::styled("q or esc closes this", st.dim)));
        rows
    }

    /// confirm_worktree asks about checking a branch out beside the
    /// repository. The path is gm's to decide, so the branch name is all it
    /// needs.
    pub(super) fn confirm_worktree(&mut self, branch: &str) -> Vec<Cmd> {
        let dir = match self.worktree_for(branch) {
            Ok(dir) => dir,
            Err(why) => {
                self.note = why;
                return vec![];
            }
        };
        let start = if repo::branch_exists(&self.repo_at, branch) {
            "existing branch"
        } else {
            "new branch"
        };
        self.confirm(Pending {
            kind: Change::AddWorktree,
            arg: branch.into(),
            detail: vec![format!("{branch}, {start}"), tildify(&dir)],
            dir,
            title: "create worktree".into(),
            ..Default::default()
        })
    }

    /// panel_key is the whole keyboard while a panel is up. Everything it does
    /// not answer to is swallowed, so nothing moves behind it.
    pub(super) fn panel_key(&mut self, k: &Key) -> Vec<Cmd> {
        let name = k.name.as_str();
        match self.over {
            Overlay::Help if matches!(name, "q" | "esc" | "ctrl+c" | "enter") => {
                self.over = Overlay::None
            }
            Overlay::Confirm if matches!(name, "y" | "Y") => {
                let a = std::mem::take(&mut self.ask);
                self.over = Overlay::None;
                let busy = match a.kind {
                    Change::Create => Some(format!("creating {}…", a.arg)),
                    Change::Remove => Some(format!("removing {}…", tildify(&a.arg))),
                    Change::AddWorktree => {
                        Some(format!("creating worktree at {}…", tildify(&a.dir)))
                    }
                    Change::RemoveWorktree => {
                        Some(format!("removing worktree at {}…", tildify(&a.dir)))
                    }
                    _ => None,
                };
                let mut cmds = Vec::new();
                let mut busy_tag = 0;
                if let Some(what) = busy {
                    let started = self.start_busy(&what);
                    busy_tag = started.0;
                    cmds.push(started.1);
                }
                cmds.push(self.perform(a, busy_tag));
                return cmds;
            }
            Overlay::Confirm if matches!(name, "n" | "N" | "q" | "esc" | "ctrl+c") => {
                self.over = Overlay::None;
                self.ask = Pending::default();
                self.note = "cancelled".into();
            }
            _ => {}
        }
        vec![]
    }
}
