use super::info::tildify;
use super::overlay::{Change, Overlay, Pending};
use super::worktree::Mode;
use super::{Action, Cmd, Model, Msg, Outcome, RemoveDetail};
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

#[derive(Clone, Copy)]
enum RepoFilter {
    Dirty,
    Unpushed,
}

impl RepoFilter {
    fn name(self) -> &'static str {
        match self {
            Self::Dirty => "/dirty",
            Self::Unpushed => "/unpushed",
        }
    }
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
            // The flag gm create takes, spelled the same, so a user who
            // pushes over SSH does not fix the remote after every /create.
            let (ssh, arg) = match arg.strip_prefix("--ssh") {
                Some(rest) if rest.is_empty() || rest.starts_with(char::is_whitespace) => {
                    (true, rest.trim())
                }
                _ => (false, arg),
            };
            if arg.is_empty() {
                m.note = "/create --ssh needs a repository".into();
                return vec![];
            }
            let u = match repo::normalize_url(arg, ssh) {
                Ok(u) => u,
                Err(e) => {
                    m.note = e.0;
                    return vec![];
                }
            };
            let rel = repo::rel_path_of(&u);
            // Asked now, so the panel never offers to make a second copy.
            if let Err(e) = m.tree.refuse_existing(&rel) {
                m.note = e.0;
                return vec![];
            }
            let dst = m.tree.path_for(&rel);
            m.confirm(Pending {
                kind: Change::Create,
                arg: arg.into(),
                ssh,
                title: "create repository".into(),
                detail: vec![tildify(&dst), format!("origin {u}")],
                ..Default::default()
            })
        },
    },
    Command {
        name: "/get",
        arg: "[flags] <repo>",
        what: "clone a repository, then go there",
        run: |m, arg| m.leave_with(Action::Get, arg),
    },
    Command {
        name: "/remove",
        arg: "",
        what: "remove the selected repository or worktree",
        run: |m, _| {
            if matches!(m.mode, Mode::Branches | Mode::Prs | Mode::Issues) {
                m.note = "/remove applies to repositories and worktrees".into();
                return vec![];
            }
            let Some(it) = m.current().cloned() else {
                m.note = "nothing is selected".into();
                return vec![];
            };
            // git prints the resolved path for a worktree, while repo_at is
            // the path the finder walked; under a symlinked root the two
            // differ textually, so compare them the way the repository case
            // below does.
            let worktree = m.mode == Mode::Worktrees;
            if worktree && repo::same_path(&it.path, &m.repo_at) {
                m.note = "that is the repository itself, not a worktree of it".into();
                return vec![];
            }
            // git is asked now rather than the details pane's cache: the cache
            // fills only once the cursor has rested, which "gm;/remove" never
            // gives it, and it is as old as the moment it was filled. It is
            // asked off the UI thread, a git status per worktree being slow
            // enough to freeze the screen, so the question goes up at once and
            // y waits for the answer.
            let (busy_tag, busy) = m.start_busy("checking for uncommitted work…");
            let pending = Pending {
                detail: vec![tildify(&it.path), "checking for uncommitted work…".into()],
                wait: Some(busy_tag),
                ..Default::default()
            };
            let mut cmds = m.confirm(if worktree {
                Pending {
                    kind: Change::RemoveWorktree,
                    arg: it.branch.name.clone(),
                    dir: it.path.clone(),
                    title: format!("remove worktree {}", it.label),
                    ..pending
                }
            } else {
                Pending {
                    kind: Change::Remove,
                    arg: it.path.clone(),
                    title: "remove repository".into(),
                    ..pending
                }
            });
            let (changed, worktrees_of) = (m.changed_of.clone(), m.worktrees_of.clone());
            let tree = m.tree.clone();
            cmds.push(busy);
            cmds.push(Cmd::Task(Box::new(move || {
                let dirty = changed(&it.path);
                let mut detail = vec![tildify(&it.path)];
                if dirty > 0 {
                    detail.push(format!("{dirty} uncommitted changes will be lost"));
                }
                if worktree {
                    let removal = repo::worktree_removal_plan(
                        repo::Worktree {
                            path: it.path.clone(),
                            branch: it.branch.name.clone(),
                            ..Default::default()
                        },
                        dirty,
                    );
                    if !it.branch.name.is_empty() {
                        detail.push(format!(
                            "branch {} goes too if git branch -d allows",
                            it.branch.name
                        ));
                    }
                    return Some(Msg::RemoveDetail(RemoveDetail {
                        busy_tag,
                        detail,
                        force: removal.force,
                    }));
                }

                if let Ok(wts) = worktrees_of(&it.path) {
                    // The worktrees go with it, so the question has to say so.
                    // The main worktree is the repository itself. git prints
                    // the resolved path, so a mismatch here is one entry too
                    // many rather than a wrong answer.
                    if let Some(r) = tree.at(&it.path) {
                        let plan = repo::removal_plan(&r, dirty, &wts, |p| changed(p));
                        // They go with --force, so the work in each is named
                        // here or nowhere.
                        if !plan.worktrees.is_empty() {
                            // A checkout outside gm's worktree directory is
                            // named by its path, which only the question says.
                            let others: Vec<_> = plan
                                .worktrees
                                .iter()
                                .map(|impact| impact.worktree.clone())
                                .collect();
                            let labels = repo::worktree_labels_in(
                                &repo::worktrees_dir(&r),
                                &others,
                                |path| {
                                    plan.worktrees
                                        .iter()
                                        .find(|impact| impact.worktree.path == path)
                                        .map_or(0, |impact| impact.changed)
                                },
                            );
                            detail.push(format!(
                                "{} will go too: {}",
                                plural(plan.worktrees.len(), "worktree", "worktrees"),
                                labels
                            ));
                        }
                        if !plan.strays.is_empty() {
                            detail.push(format!(
                                "also removed, not a worktree: {}",
                                plan.strays.join(", ")
                            ));
                        }
                    }
                }
                Some(Msg::RemoveDetail(RemoveDetail {
                    busy_tag,
                    detail,
                    force: dirty > 0,
                }))
            })));
            cmds
        },
    },
    Command {
        name: "/expire",
        arg: "<days>d",
        what: "remove the worktrees idle that long",
        run: |m, arg| m.confirm_expire(arg),
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
        name: "/issues",
        arg: "",
        what: "list the open issues of the repository",
        run: |m, _| {
            if m.mode == Mode::Issues {
                vec![]
            } else {
                m.switch_to(Mode::Issues)
            }
        },
    },
    Command {
        name: "/browse",
        arg: "",
        what: "open the selected repository's remote in a browser",
        run: |m, _| m.open_remote().into_iter().collect(),
    },
    Command {
        name: "/dirty",
        arg: "",
        what: "show only repositories with uncommitted work",
        run: |m, _| m.toggle_repo_filter(RepoFilter::Dirty),
    },
    Command {
        name: "/unpushed",
        arg: "",
        what: "show only repositories with unpushed commits",
        run: |m, _| m.toggle_repo_filter(RepoFilter::Unpushed),
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
    fn toggle_repo_filter(&mut self, filter: RepoFilter) -> Vec<Cmd> {
        if self.mode != Mode::Repos {
            self.note = format!("{} applies to the repository list", filter.name());
            return vec![];
        }

        let enabled = match filter {
            RepoFilter::Dirty => {
                self.dirty_only = !self.dirty_only;
                self.dirty_only
            }
            RepoFilter::Unpushed => {
                self.unpushed_only = !self.unpushed_only;
                self.unpushed_only
            }
        };

        // Switching a filter on asks git again, so work done in another
        // terminal since the last scan shows up. The last answer filters the
        // list until the new one lands; with none yet, every row stays.
        let scan = if enabled && !self.scanning_states {
            self.scanning_states = true;
            let (busy_tag, busy) = self.start_busy("checking repository status…");
            vec![busy, self.scan_repo_states(busy_tag)]
        } else {
            vec![] // off, or the other filter already started the shared scan
        };
        if !enabled || self.repo_states.is_some() {
            self.filter();
            self.cursor = self.view.len().saturating_sub(1);
        }
        scan
    }

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
