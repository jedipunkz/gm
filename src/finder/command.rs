use super::info::tildify;
use super::overlay::{Change, Overlay, Pending};
use super::worktree::Mode;
use super::{Action, Cmd, Model, Outcome};
use crate::plural;
use crate::repo;

/// A slash command is typed into the same box as the filter. Only a slash at
/// the very start of an empty query begins one — a repository path is full of
/// slashes, and every one of those has to keep filtering.
const COMMAND_PREFIX: &str = "/";

/// COMMAND_SEP ends a query so a command can follow it — "gm;/remove" finds a
/// repository and acts on it in one go. No repository path holds one.
const COMMAND_SEP: &str = ";";

/// Command is one slash command. Adding another is one entry here; the help
/// popup and the completion both read this table.
pub struct Command {
    pub name: &'static str,
    pub arg: &'static str, // what the argument is called in the help, empty when it takes none
    pub what: &'static str,
    run: fn(&mut Model, &str) -> Vec<Cmd>,
}

impl Command {
    /// label is how the command is written in the help popup.
    pub fn label(&self) -> String {
        if self.arg.is_empty() {
            self.name.to_string()
        } else {
            format!("{} {}", self.name, self.arg)
        }
    }
}

pub const COMMANDS: &[Command] = &[
    Command {
        name: "/help",
        arg: "",
        what: "show this list",
        run: |m, _| {
            m.over = Overlay::Help;
            vec![]
        },
    },
    Command {
        name: "/create",
        arg: "<repo>|<branch>",
        what: "create a repository, or a worktree",
        run: |m, arg| {
            if m.mode != Mode::Repos {
                return m.confirm_worktree(arg);
            }
            let u = match repo::normalize_url(arg, false) {
                Ok(u) => u,
                Err(e) => {
                    m.note = e.0;
                    return vec![];
                }
            };
            let dst = m.tree.path_for(&repo::rel_path_of(&u));
            m.confirm(Pending {
                kind: Change::Create,
                arg: arg.into(),
                title: "create repository".into(),
                detail: vec![tildify(&dst), format!("origin {u}")],
                ..Default::default()
            })
        },
    },
    Command {
        name: "/get",
        arg: "<repo>",
        what: "clone a repository, then go there",
        run: |m, arg| m.leave_with(Action::Get, arg),
    },
    Command {
        name: "/remove",
        arg: "",
        what: "remove the selected repository or worktree",
        run: |m, _| {
            if matches!(m.mode, Mode::Branches | Mode::Prs) {
                m.note = "/remove applies to repositories and worktrees".into();
                return vec![];
            }
            let Some(it) = m.current().cloned() else {
                m.note = "nothing is selected".into();
                return vec![];
            };
            // The status of the selected row is already loaded, so the warning
            // costs nothing and is the one thing worth knowing before saying yes.
            let dirty = m.status.get(&it.path).map_or(0, |s| s.dirty);
            let mut detail = vec![tildify(&it.path)];
            if dirty > 0 {
                detail.push(format!("{dirty} uncommitted changes will be lost"));
            }

            if m.mode == Mode::Worktrees {
                if it.path == m.repo_at {
                    m.note = "that is the repository itself, not a worktree of it".into();
                    return vec![];
                }
                return m.confirm(Pending {
                    kind: Change::RemoveWorktree,
                    dir: it.path.clone(),
                    title: format!("remove worktree {}", it.label),
                    detail,
                    force: dirty > 0,
                    ..Default::default()
                });
            }
            // The worktrees go with it, so the question has to say so.
            if let Ok(wts) = (m.worktrees_of)(&it.path) {
                // The main worktree is the repository itself. git prints the
                // resolved path, so a mismatch here is one entry too many rather
                // than a wrong answer.
                let labels: Vec<String> = wts
                    .iter()
                    .filter(|w| !repo::same_path(&w.path, &it.path))
                    .map(|w| w.label())
                    .collect();
                if !labels.is_empty() {
                    detail.push(format!(
                        "{} will go too: {}",
                        plural(labels.len(), "worktree", "worktrees"),
                        labels.join(", ")
                    ));
                }
            }
            m.confirm(Pending {
                kind: Change::Remove,
                arg: it.path.clone(),
                title: "remove repository".into(),
                detail,
                ..Default::default()
            })
        },
    },
    Command {
        name: "/worktrees",
        arg: "",
        what: "list the worktrees of the selected repository",
        run: |m, _| {
            if m.mode == Mode::Worktrees {
                vec![]
            } else {
                m.switch_to(Mode::Worktrees)
            }
        },
    },
    Command {
        name: "/branches",
        arg: "",
        what: "list the branches of the selected repository",
        run: |m, _| {
            if m.mode == Mode::Branches {
                vec![]
            } else {
                m.switch_to(Mode::Branches)
            }
        },
    },
    Command {
        name: "/prs",
        arg: "",
        what: "list the open pull requests of the repository",
        run: |m, _| {
            if m.mode == Mode::Prs {
                vec![]
            } else {
                m.switch_to(Mode::Prs)
            }
        },
    },
    Command {
        name: "/remote",
        arg: "",
        what: "open the selected repository's remote in a browser",
        run: |m, _| m.open_remote().into_iter().collect(),
    },
    Command {
        name: "/dirty",
        arg: "",
        what: "show only repositories with uncommitted work",
        run: |m, _| {
            if m.mode != Mode::Repos {
                m.note = "/dirty applies to the repository list".into();
                return vec![];
            }
            if m.dirty_only {
                m.dirty_only = false;
                m.filter();
                m.cursor = m.view.len().saturating_sub(1);
                return vec![];
            }
            m.dirty_only = true;
            if m.dirty.is_none() {
                // The answer needs a git call per repository, so it is asked for
                // the first time someone wants it, not at startup.
                let busy = m.start_busy("checking every repository for uncommitted work…");
                return vec![busy, m.scan_dirty()];
            }
            m.filter();
            m.cursor = m.view.len().saturating_sub(1);
            vec![]
        },
    },
];

pub fn command_names() -> Vec<String> {
    let mut names: Vec<String> = COMMANDS.iter().map(|c| c.name.to_string()).collect();
    names.sort();
    names
}

/// split_input tells the query in the box from a command typed with it: a
/// command either is the whole input, or follows the query after COMMAND_SEP.
pub fn split_input(s: &str) -> Option<(String, String)> {
    if s.starts_with(COMMAND_PREFIX) {
        return Some((String::new(), s.to_string()));
    }
    s.split_once(COMMAND_SEP)
        .map(|(q, c)| (q.to_string(), c.to_string()))
}

/// is_command reports whether the input is being typed as a command rather
/// than only as a filter.
pub fn is_command(s: &str) -> bool {
    split_input(s).is_some()
}

/// completions offers the command names as the whole box would read with
/// them, the query in front of the separator included.
pub fn completions(s: &str) -> Vec<String> {
    let names = command_names();
    match split_input(s) {
        Some((q, _)) if !q.is_empty() => names
            .into_iter()
            .map(|n| format!("{q}{COMMAND_SEP}{n}"))
            .collect(),
        _ => names,
    }
}

impl Model {
    /// leave_with closes the finder and hands the work to gm, which runs it on
    /// the terminal the user can see: a clone's progress, a password prompt and
    /// a confirmation all belong there, not inside an alternate screen.
    fn leave_with(&mut self, action: Action, arg: &str) -> Vec<Cmd> {
        self.result = Outcome {
            action,
            arg: arg.to_string(),
        };
        vec![Cmd::Quit]
    }

    /// run_command executes what the user typed. An unknown command, or one
    /// missing its argument, leaves a note under the prompt rather than doing
    /// something surprising.
    pub(super) fn run_command(&mut self, typed: &str) -> Vec<Cmd> {
        let typed = typed.trim();
        let (name, arg) = typed.split_once(' ').unwrap_or((typed, ""));
        let arg = arg.trim().to_string();
        let Some(c) = COMMANDS.iter().find(|c| c.name == name) else {
            self.note = format!("unknown command {name} — /help lists them");
            return vec![];
        };
        if !c.arg.is_empty() && arg.is_empty() {
            self.note = format!("{name} needs an argument: {}", c.label());
            return vec![];
        }
        self.input.set_value("");
        self.note.clear();
        self.filter();
        (c.run)(self, &arg)
    }
}
