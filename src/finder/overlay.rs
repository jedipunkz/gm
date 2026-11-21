use ratatui::text::{Line, Span};

use super::command::COMMANDS;
use super::info::{self, Seg, tildify, wrap_segs};
use super::list::Item;
use super::worktree::Mode;
use super::{Action, Cmd, Key, Model, Msg, Outcome};
use crate::repo::{self, Branch, Worktree};
use crate::{Error, err, paths, plural};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Overlay {
    #[default]
    None,
    Help,
    Confirm,
    Detail,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Change {
    #[default]
    None,
    Create,
    Remove,
    AddWorktree,
    RemoveWorktree,
    CheckOut,
    CheckOutPr,
    Expire,
}

#[derive(Debug, Clone, Default)]
pub struct Pending {
    pub kind: Change,
    pub mode: Mode,
    pub repo_at: String,
    pub arg: String,
    pub dir: String,
    pub from: String,
    pub fetch: bool,
    pub title: String,
    pub detail: Vec<String>,
    pub force: bool,
    pub wait: Option<u64>,
    pub wts: Vec<Worktree>,
    pub ssh: bool,
}

#[derive(Debug, Clone)]
pub struct Done {
    pub(super) kind: Change,
    pub(super) mode: Mode,
    pub(super) repo_at: String,
    pub(super) label: String,
    pub(super) path: String,
    pub(super) busy_tag: u64,
    pub(super) err: Option<Error>,
    pub(super) gone: Vec<String>,
}

pub struct ConfirmDetail {
    pub(super) busy_tag: u64,
    pub(super) result: crate::Result<Vec<String>>,
}

pub struct RemoveDetail {
    pub(super) busy_tag: u64,
    pub(super) detail: Vec<String>,
    pub(super) force: bool,
}

pub struct ExpireDetail {
    pub(super) busy_tag: u64,
    pub(super) days: i64,
    pub(super) result: crate::Result<ExpireScan>,
}

pub struct ExpireScan {
    pub(super) worktrees: Vec<Worktree>,
    pub(super) detail: Vec<String>,
    pub(super) kept: Option<String>,
}

fn under_root(tree: &repo::Tree, path: &str) -> Result<repo::Repo, Error> {
    tree.at(path)
        .ok_or_else(|| err!("{path} is not under any root"))
}

impl Model {
    pub(super) fn perform(&self, a: Pending, busy_tag: u64) -> Cmd {
        let (tree, gh) = (self.tree.clone(), self.backend.gh.clone());
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
                    gone: vec![],
                }))
            };
            match kind {
                Change::Remove => done(under_root(&tree, &a.arg).and_then(|r| {
                    repo::delete(&r).map(|left| {
                        let mut label = r.rel.clone();
                        if !left.is_empty() {
                            label += &format!("; kept {}", left.join(", "));
                        }
                        (r.path(), label)
                    })
                })),
                Change::Create => done(tree.create(&a.arg, a.ssh).map(|r| (r.path(), r.rel))),
                Change::AddWorktree | Change::CheckOut => done((|| {
                    if a.fetch {
                        let b = Branch {
                            name: a.arg.clone(),
                            remote: a.from.clone(),
                            unfetched: true,
                        };
                        repo::fetch_branch(&a.repo_at, &b)?;
                    }
                    let r = under_root(&tree, &a.repo_at)?;
                    let root = paths::join(&r.root, repo::WORKTREE_ROOT);
                    repo::add_worktree_from(&root, &a.repo_at, &a.dir, &a.arg, &a.from)?;
                    Ok((a.dir.clone(), a.arg.clone()))
                })()),
                Change::CheckOutPr => done((|| {
                    let n = a
                        .arg
                        .parse()
                        .map_err(|_| err!("{:?} is not a pull request", a.arg))?;
                    let r = under_root(&tree, &a.repo_at)?;
                    let root = paths::join(&r.root, repo::WORKTREE_ROOT);
                    repo::check_out_pull_request_in(&root, &gh, &a.repo_at, &a.dir, n)?;
                    Ok((a.dir.clone(), a.arg.clone()))
                })()),
                Change::RemoveWorktree => done(
                    under_root(&tree, &a.repo_at)
                        .and_then(|r| repo::remove_worktree_and_branch(&r, &a.dir, &a.arg, a.force))
                        .map(|gone| {
                            let said = match (a.arg.as_str(), gone) {
                                ("", _) => String::new(),
                                (b, true) => format!(", deleted branch {b}"),
                                (b, false) => format!(", kept branch {b}: not merged"),
                            };
                            (a.dir.clone(), said)
                        }),
                ),
                Change::Expire => {
                    let gone = match under_root(&tree, &a.repo_at) {
                        Ok(r) => repo::expire(&r, &a.wts),
                        Err(e) => return done(Err(e)),
                    };
                    let mut said = format!(
                        "removed {}, {}",
                        plural(gone.removed.len(), "worktree", "worktrees"),
                        plural(gone.branches, "branch", "branches")
                    );
                    if !gone.failed.is_empty() {
                        said += &format!("; kept {}", gone.failed.join("; "));
                    }
                    let Some(Msg::Done(d)) = done(Ok((String::new(), said))) else {
                        return None;
                    };
                    Some(Msg::Done(Done {
                        gone: gone.removed,
                        ..d
                    }))
                }
                Change::None => None,
            }
        }))
    }

    pub(super) fn done(&mut self, d: Done) -> Vec<Cmd> {
        self.finish_busy(d.busy_tag);
        if let Some(e) = d.err {
            self.note = e.0;
            return vec![];
        }
        let status_path = if d.repo_at.is_empty() {
            match d.kind {
                Change::Create | Change::Remove => d.path.as_str(),
                _ => "",
            }
        } else {
            d.repo_at.as_str()
        };
        if !status_path.is_empty() {
            self.status.remove(status_path);
            self.probing.remove(status_path);
        }
        let rescan = (!status_path.is_empty()
            && d.kind != Change::Remove
            && (self.repo_states.is_some() || self.scanning_states))
            .then(|| self.rescan_repo_state(status_path));

        match d.kind {
            Change::Remove | Change::RemoveWorktree | Change::Create | Change::AddWorktree => {
                let changed = |all: &mut Vec<Item>| match d.kind {
                    Change::Remove | Change::RemoveWorktree => {
                        let before = all.len();
                        all.retain(|it| it.path != d.path);
                        all.len() != before
                    }
                    Change::Create => {
                        let it = Item {
                            label: d.label.clone(),
                            path: d.path.clone(),
                            ..Default::default()
                        };
                        let mut at = 0;
                        while at < all.len()
                            && (all[at].score < it.score
                                || (all[at].score == it.score && all[at].label <= it.label))
                        {
                            at += 1;
                        }
                        all.insert(at, it);
                        true
                    }
                    Change::AddWorktree => {
                        let existing = match d.mode {
                            Mode::Branches => all.iter_mut().find(|it| {
                                it.branch.name == d.label || it.branch.label() == d.label
                            }),
                            Mode::Prs => all
                                .iter_mut()
                                .find(|it| it.pr.checkout() == d.label || it.pr.branch == d.label),
                            _ => all.iter_mut().find(|it| it.label == d.label),
                        };
                        if let Some(it) = existing {
                            it.path = d.path.clone();
                        } else {
                            let at = if d.mode == Mode::Worktrees {
                                0
                            } else {
                                all.len()
                            };
                            all.insert(
                                at,
                                Item {
                                    label: d.label.clone(),
                                    path: d.path.clone(),
                                    branch: crate::repo::Branch {
                                        name: d.label.clone(),
                                        ..Default::default()
                                    },
                                    ..Default::default()
                                },
                            );
                        }
                        true
                    }
                    _ => false,
                };
                let active =
                    self.mode == d.mode && (d.mode == Mode::Repos || self.repo_at == d.repo_at);
                if active {
                    if changed(&mut self.all) {
                        self.view_stale = true;
                        self.filter();
                        if !matches!(d.kind, Change::Remove | Change::RemoveWorktree)
                            && let Some(i) =
                                self.view.iter().position(|&x| self.all[x].path == d.path)
                        {
                            self.cursor = i;
                        }
                    }
                } else if d.mode == Mode::Repos
                    && self.mode != Mode::Repos
                    && let Some(saved) = self.saved.as_mut()
                {
                    saved.update(changed);
                }
                self.note = match d.kind {
                    Change::Remove => format!("removed {}{}", info::tildify(&d.path), d.label),
                    Change::RemoveWorktree => {
                        format!("removed {}{}", info::tildify(&d.path), d.label)
                    }
                    Change::Create | Change::AddWorktree => {
                        format!("created {}", info::tildify(&d.path))
                    }
                    _ => unreachable!(),
                };
            }
            Change::Expire => {
                if self.mode == d.mode && self.repo_at == d.repo_at {
                    self.all.retain(|it| !d.gone.contains(&it.path));
                    self.view_stale = true;
                    self.filter();
                }
                self.note = d.label;
            }
            Change::CheckOut | Change::CheckOutPr => {
                self.result = Outcome {
                    action: Action::Jump,
                    arg: d.path,
                };
                return vec![Cmd::Quit];
            }
            Change::None => {}
        }
        rescan.into_iter().chain(self.load_status()).collect()
    }

    pub(super) fn confirm(&mut self, mut p: Pending) -> Vec<Cmd> {
        p.mode = self.mode;
        p.repo_at = self.repo_at.clone();
        self.over = Overlay::Confirm;
        self.ask = p;
        self.note.clear();
        vec![]
    }

    pub(super) fn fill_detail(&mut self, d: RemoveDetail) -> Vec<Cmd> {
        self.finish_busy(d.busy_tag);
        if self.over == Overlay::Confirm && self.ask.wait == Some(d.busy_tag) {
            self.ask.detail = d.detail;
            self.ask.force = d.force;
            self.ask.wait = None;
        }
        vec![]
    }

    pub(super) fn fill_confirm_detail(&mut self, d: ConfirmDetail) -> Vec<Cmd> {
        self.finish_busy(d.busy_tag);
        if self.over != Overlay::Confirm || self.ask.wait != Some(d.busy_tag) {
            return vec![];
        }
        match d.result {
            Ok(detail) => {
                self.ask.detail = detail;
                self.ask.wait = None;
            }
            Err(e) => {
                self.over = Overlay::None;
                self.ask = Pending::default();
                self.note = e.0;
            }
        }
        vec![]
    }

    pub(super) fn fill_expire_detail(&mut self, d: ExpireDetail) -> Vec<Cmd> {
        self.finish_busy(d.busy_tag);
        if self.over != Overlay::Confirm || self.ask.wait != Some(d.busy_tag) {
            return vec![];
        }
        let scan = match d.result {
            Ok(scan) => scan,
            Err(e) => {
                self.over = Overlay::None;
                self.ask = Pending::default();
                self.note = e.0;
                return vec![];
            }
        };
        if scan.worktrees.is_empty() {
            self.over = Overlay::None;
            self.ask = Pending::default();
            self.note = format!("no worktree idle for {} days", d.days);
            if let Some(kept) = scan.kept {
                self.note += &format!("; {kept}");
            }
            return vec![];
        }
        self.ask.title = format!(
            "remove {}",
            plural(scan.worktrees.len(), "worktree", "worktrees")
        );
        self.ask.wts = scan.worktrees;
        self.ask.detail = scan.detail;
        self.ask.wait = None;
        vec![]
    }

    fn box_width(&self) -> usize {
        (self.w as usize).saturating_sub(10).clamp(20, 76)
    }

    pub(super) fn confirm_box(&self) -> Vec<Line<'static>> {
        let st = &self.st;
        let w = self.box_width().min(56);
        let mut rows = vec![Line::from(Span::styled(self.ask.title.clone(), st.label))];
        for d in &self.ask.detail {
            rows.extend(wrap_segs(&[Seg::new(d, st.subject)], w, "  "));
        }
        rows.push(Line::default());
        let mut keys = match self.ask.wait {
            Some(_) => vec![],
            None => vec![
                Span::styled("y", st.ref_local),
                Span::styled(" do it", st.subject),
                Span::styled("   ", st.dim),
            ],
        };
        keys.push(Span::styled("n", st.ref_local));
        keys.push(Span::styled(" cancel", st.subject));
        rows.push(Line::from(keys));
        rows
    }

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

    pub(super) fn confirm_worktree(&mut self, branch: &str) -> Vec<Cmd> {
        let dir = match self.worktree_for(branch) {
            Ok(dir) => dir,
            Err(why) => {
                self.note = why;
                return vec![];
            }
        };
        let repo_at = self.repo_at.clone();
        let branch = branch.to_string();
        let check_dir = dir.clone();
        let (busy_tag, busy) = self.start_busy("checking branch…");
        let mut cmds = self.confirm(Pending {
            kind: Change::AddWorktree,
            arg: branch.clone(),
            detail: vec![format!("{branch}, checking branch…"), tildify(&dir)],
            dir,
            title: "create worktree".into(),
            wait: Some(busy_tag),
            ..Default::default()
        });
        cmds.push(busy);
        cmds.push(Cmd::Task(Box::new(move || {
            let start = if repo::branch_exists(&repo_at, &branch) {
                "existing branch"
            } else {
                "new branch"
            };
            Some(Msg::ConfirmDetail(ConfirmDetail {
                busy_tag,
                result: Ok(vec![format!("{branch}, {start}"), tildify(&check_dir)]),
            }))
        })));
        cmds
    }

    pub(super) fn confirm_expire(&mut self, age: &str) -> Vec<Cmd> {
        if self.mode != Mode::Worktrees {
            self.note = "/expire applies to the worktree list".into();
            return vec![];
        }
        let days = match repo::parse_days(age) {
            Ok(d) => d,
            Err(e) => {
                self.note = e.0;
                return vec![];
            }
        };
        let repo_at = self.repo_at.clone();
        let (worktrees_of, changed) = (
            self.backend.worktrees_of.clone(),
            self.backend.changed_of.clone(),
        );
        let (busy_tag, busy) = self.start_busy("checking for uncommitted work…");
        let mut cmds = self.confirm(Pending {
            kind: Change::Expire,
            title: "remove worktrees".into(),
            detail: vec!["checking for uncommitted work…".into()],
            wait: Some(busy_tag),
            ..Default::default()
        });
        cmds.push(busy);
        cmds.push(Cmd::Task(Box::new(move || {
            let result = worktrees_of(&repo_at).map(|wts| {
                let plan = repo::expiry_plan(&repo_at, &wts, days, repo::now(), |p| changed(p));
                let dirty: Vec<_> = plan
                    .keep
                    .iter()
                    .map(|impact| impact.worktree.clone())
                    .collect();
                let kept = (!plan.keep.is_empty()).then(|| {
                    format!(
                        "kept, uncommitted changes: {}",
                        repo::worktree_labels(&dirty, |p| {
                            plan.keep
                                .iter()
                                .find(|impact| impact.worktree.path == p)
                                .map_or(0, |impact| impact.changed)
                        })
                    )
                });
                let names = plan
                    .remove
                    .iter()
                    .map(|impact| impact.worktree.label())
                    .collect::<Vec<_>>();
                let mut detail = vec![format!("idle {days}+ days: {}", names.join(", "))];
                detail.extend(kept.clone());
                detail.push("their branches go too where git branch -d allows".into());
                ExpireScan {
                    worktrees: plan
                        .remove
                        .into_iter()
                        .map(|impact| impact.worktree)
                        .collect(),
                    detail,
                    kept,
                }
            });
            Some(Msg::ExpireDetail(ExpireDetail {
                busy_tag,
                days,
                result,
            }))
        })));
        cmds
    }

    pub(super) fn panel_key(&mut self, k: &Key) -> Vec<Cmd> {
        let name = k.name.as_str();
        match self.over {
            Overlay::Help if matches!(name, "q" | "esc" | "ctrl+c" | "enter") => {
                self.over = Overlay::None
            }
            Overlay::Detail => {
                let is_pr = matches!(
                    self.detail.as_ref().map(|d| &d.subject),
                    Some(super::detail::Subject::Pr(_))
                );
                match name {
                    "enter" if is_pr => return self.check_out_shown_pr(),
                    "q" | "esc" | "ctrl+c" | "enter" | "ctrl+g" => self.close_detail(),
                    "up" | "k" => self.scroll_detail(false, false),
                    "down" | "j" => self.scroll_detail(false, true),
                    // Space pages forward, shift+space back — but only a
                    // terminal speaking the Kitty keyboard protocol reports
                    // the shift, so b pages back the way less's does and
                    // PgUp too, whatever the terminal is.
                    " " => self.scroll_detail(true, true),
                    "shift+ " => self.scroll_detail(true, false),
                    "b" => self.scroll_detail(true, false),
                    "pgup" => self.scroll_detail(true, false),
                    "pgdown" => self.scroll_detail(true, true),
                    "ctrl+b" => return self.open_detail_in_browser(),
                    "ctrl+y" => return self.copy_detail_url(),
                    _ => {}
                }
            }
            Overlay::Confirm if matches!(name, "y" | "Y") && self.ask.wait.is_none() => {
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
                    Change::Expire => Some(format!(
                        "removing {}…",
                        plural(a.wts.len(), "worktree", "worktrees")
                    )),
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
