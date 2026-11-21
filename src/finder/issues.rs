pub(super) use super::detail::{Detail, Subject};
use super::list::Item;
use super::overlay::Overlay;
use super::worktree::Mode;
use super::{Cmd, Model, Msg};
use crate::repo;

pub struct Issues {
    pub(super) path: String,
    pub(super) busy_tag: u64,
    pub(super) issues: crate::Result<Vec<crate::repo::Issue>>,
}

pub struct IssueDetail {
    pub(super) path: String,
    pub(super) busy_tag: u64,
    pub(super) issue: crate::Result<crate::repo::Issue>,
}

impl Model {
    pub(super) fn open_issues(&mut self) -> Vec<Cmd> {
        let Some(it) = self.current().cloned().filter(|_| self.mode == Mode::Repos) else {
            return vec![];
        };
        let (busy_tag, busy) = self.start_busy(&format!("asking GitHub about {}…", it.label));
        let (issues_of, path) = (self.backend.issues_of.clone(), it.path);
        vec![
            busy,
            Cmd::Task(Box::new(move || {
                let list = issues_of(&path);
                Some(Msg::Issues(Issues {
                    path,
                    busy_tag,
                    issues: list,
                }))
            })),
        ]
    }

    pub(super) fn show_issues(&mut self, msg: Issues) -> Vec<Cmd> {
        self.finish_busy(msg.busy_tag);
        let Some(it) = self
            .current()
            .cloned()
            .filter(|it| self.mode == Mode::Repos && it.path == msg.path)
        else {
            return vec![];
        };
        let issues = match msg.issues {
            Err(e) => {
                self.note = e.0;
                return vec![];
            }
            Ok(issues) if issues.is_empty() => {
                self.note = format!("no open issues in {}", it.label);
                return vec![];
            }
            Ok(issues) => issues,
        };

        let items = issues
            .iter()
            .rev()
            .map(|i| Item {
                label: i.label(),
                issue: i.clone(),
                ..Default::default()
            })
            .collect();
        self.replace_list(Mode::Issues, &it, items);
        if issues.len() >= repo::ISSUE_LIMIT {
            self.note = format!("showing the newest {} open issues", repo::ISSUE_LIMIT);
        }
        self.load_status().into_iter().collect()
    }

    pub(super) fn view_issue(&mut self) -> Vec<Cmd> {
        let Some(it) = self.current().cloned() else {
            return vec![];
        };
        let n = it.issue.number;
        let (busy_tag, busy) = self.start_busy(&format!("viewing #{n}…"));
        let (issue_of, path) = (self.backend.issue_of.clone(), self.repo_at.clone());
        vec![
            busy,
            Cmd::Task(Box::new(move || {
                let issue = issue_of(&path, n);
                Some(Msg::IssueDetail(IssueDetail {
                    path,
                    busy_tag,
                    issue,
                }))
            })),
        ]
    }

    pub(super) fn show_issue_detail(&mut self, msg: IssueDetail) -> Vec<Cmd> {
        self.finish_busy(msg.busy_tag);
        if self.mode != Mode::Issues || msg.path != self.repo_at {
            return vec![];
        }
        match msg.issue {
            Err(e) => self.note = e.0,
            Ok(issue) => {
                self.detail = Some(Detail {
                    subject: Subject::Issue(issue),
                    scroll: 0,
                    lines: std::cell::RefCell::new(None),
                });
                self.over = Overlay::Detail;
                self.note.clear();
            }
        }
        vec![]
    }
}
