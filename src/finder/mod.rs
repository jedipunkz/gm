//! The interactive repository picker: a fuzzy filter over the repositories,
//! ranked by match quality and then by how often they are used.
//!
//! The best match sits at the BOTTOM of the list, next to the prompt and the
//! cursor's resting place, so the most likely repository needs zero
//! keystrokes.
//!
//! It is built the way the Go version's Bubble Tea model was: update takes one
//! message and returns the commands it wants run, and render draws the state.
//! Work that talks to git or GitHub runs on a thread and comes back as a
//! message, so the list never freezes.

mod branch;
mod browse;
mod command;
mod info;
mod input;
mod list;
mod overlay;
mod pr;
mod score;
mod terminal;
mod theme;
mod worktree;

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget};

pub use theme::{Theme, lookup_theme};

use crate::config::Chord;
use crate::repo::{self, Branch, History, PullRequest, Repo, Status, Tree, Worktree};
use crate::{Error, Result, err};
use command::{completions, is_command, split_input};
use info::{Seg, wrap_segs};
use input::{Input, SPINNER_INTERVAL, Spinner};
use list::Item;
use overlay::{Change, Overlay, Pending};
use theme::Styles;
use worktree::{Mode, Stash};

/// The finder keys gm.toml can move: DEFAULT_WORKTREE_KEY opens the worktree
/// list, DEFAULT_BRANCH_KEY the branch list, DEFAULT_PR_KEY the pull request
/// list, and DEFAULT_REMOTE_KEY the selected repository's remote.
pub const DEFAULT_WORKTREE_KEY: &str = "ctrl-w";
pub const DEFAULT_BRANCH_KEY: &str = "ctrl-l";
pub const DEFAULT_PR_KEY: &str = "ctrl-j";
pub const DEFAULT_REMOTE_KEY: &str = "ctrl-alt-b";

/// RESERVED are the Ctrl chords the finder already answers to; binding an
/// action to one of them would shadow quitting, moving or backing out. Only
/// plain Ctrl chords can collide: the finder's own keys carry no other
/// modifier.
const RESERVED: [(char, &str); 4] = [
    ('c', "quit"),
    ('n', "move down"),
    ('p', "move up"),
    ('g', "back out"),
];

/// Keys are the finder's configurable chords.
#[derive(Debug, Clone, Default)]
pub struct Keys {
    pub worktree: Chord, // open and close the worktree list
    pub branch: Chord,   // open and close the branch list
    pub pr: Chord,       // open and close the pull request list
    pub remote: Chord,   // open the selected repository's remote
}

impl Keys {
    /// check refuses a binding that would shadow one of the finder's fixed
    /// keys, or that two actions would answer to at once.
    fn check(&self) -> Result<()> {
        let named = [
            ("worktree_key", &self.worktree),
            ("branch_key", &self.branch),
            ("pr_key", &self.pr),
            ("remote_key", &self.remote),
        ];
        for (i, (name, chord)) in named.iter().enumerate() {
            for (other, o) in &named[i + 1..] {
                if chord.key() == o.key() {
                    return Err(err!("{name} and {other} are both {}", chord.display));
                }
            }
            if !chord.plain() {
                continue; // Alt or Shift can never collide with the fixed keys
            }
            if let Some((_, what)) = RESERVED.iter().find(|(c, _)| *c == chord.letter) {
                return Err(err!(
                    "{name} cannot be {}: the finder uses it to {what}",
                    chord.display
                ));
            }
        }
        Ok(())
    }
}

/// Action is the work the finder could not finish itself. Making and removing
/// repositories and worktrees happen under it, behind a confirmation; what is
/// left is going somewhere, and cloning, which wants a terminal of its own for
/// its progress and its passwords.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Action {
    #[default]
    None, // the user quit
    Jump, // go to arg, a repository or worktree path
    Get,  // clone arg, a repository reference
}

/// Outcome is what the finder leaves behind for gm to carry out once the
/// alternate screen is gone.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Outcome {
    pub action: Action,
    pub arg: String,
}

/// Key is one key press, spelled the way Bubble Tea spelled it: "ctrl+w",
/// "ctrl+alt+b", "esc", "enter", "a". text is the character it types, for a
/// key that types one.
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub name: String,
    pub text: Option<char>,
}

impl Key {
    #[cfg(test)]
    pub fn named(name: &str) -> Key {
        Key {
            name: name.to_string(),
            text: None,
        }
    }

    pub fn char(c: char) -> Key {
        Key {
            name: c.to_string(),
            text: Some(c),
        }
    }
}

/// Done is the outcome of a confirmed change, back on the UI thread.
#[derive(Debug, Clone)]
pub struct Done {
    kind: Change,
    mode: Mode,
    repo_at: String,
    label: String, // how the new row reads, if one was made
    path: String,
    busy_tag: u64,
    err: Option<Error>,
}

/// Msg is everything update answers to.
pub enum Msg {
    Key(Key),
    Paste(String),
    Resize(u16, u16),
    /// Probe says the cursor has rested on a path for STATUS_DELAY. It is
    /// dropped if the selection moved on in the meantime, which is what keeps
    /// a held arrow key from asking git about every row it swept past.
    Probe(String),
    Status(String, Status),         // one repository's git status
    Spin(u64),                      // the spinner's next frame, with its tag
    Done(Done),                     // a confirmed change has been carried out
    RemoteBranches(RemoteBranches), // what the remotes have that was not fetched
    Prs(Prs),                       // gh's answer about one repository
    RepoStates(u64, HashMap<String, repo::State>), // the result of a bulk repository status scan
}

/// RemoteBranches carries what the remotes of one repository have that the
/// last fetch did not bring.
pub struct RemoteBranches {
    path: String,
    branches: Vec<Branch>,
    busy_tag: u64,
    err: Option<Error>,
}

/// Prs carries gh's answer about one repository.
pub struct Prs {
    path: String, // the repository it was asked about
    busy_tag: u64,
    prs: Result<Vec<PullRequest>>,
}

/// Task is work off the UI thread; the message it returns is fed back in.
pub type Task = Box<dyn FnOnce() -> Option<Msg> + Send>;

/// Cmd is what update asks the loop to do.
pub enum Cmd {
    Quit,
    Task(Task),
    After(Duration, Msg),
}

/// STATUS_DELAY is how long a row has to stay selected before git is asked
/// about it. Key repeat is faster than this, so scrolling through a tree costs
/// nothing and the row the eye stops on is described right away.
const STATUS_DELAY: Duration = Duration::from_millis(100);

/// Seam is a git or GitHub call the model makes through a field, so the tests
/// can replace it; every real run uses the repo function of the same name.
type Seam<T> = Arc<dyn Fn(&str) -> T + Send + Sync>;
type StateScan = Arc<dyn Fn(&[String]) -> HashMap<String, repo::State> + Send + Sync>;

#[derive(Clone)]
pub struct Model {
    all: Vec<Item>,                      // ascending by frecency: the best is last
    view: Vec<usize>,                    // indices into all, same convention
    view_stale: bool,                    // view does not describe all; filter starts afresh
    matched: HashMap<usize, Vec<usize>>, // item index -> matched char positions
    cursor: usize,                       // index into view
    input: Input,
    status: HashMap<String, Status>,
    probing: HashSet<String>, // paths git is being asked about right now
    st: Styles,
    w: u16,
    h: u16,
    result: Outcome,

    mode: Mode,
    origin: String,  // in the worktree or branch list, the repository it belongs to
    repo_at: String, // ...and where it is on disk
    saved: Option<Stash>,
    keys: Keys,
    tree: Tree,
    query: String, // the query the view was built from; a command is not one
    over: Overlay, // the panel drawn over the list, if any
    ask: Pending,  // what a confirmation is waiting on
    /// repo_states holds the bulk status scan once one of the repository
    /// filters needs it. Both filters share the same result.
    repo_states: Option<HashMap<String, repo::State>>,
    scanning_states: bool,
    dirty_only: bool,
    unpushed_only: bool,
    note: String, // a one-line answer under the prompt, cleared on the next keystroke
    /// busy says what gm is waiting on — git or GitHub, off the UI thread —
    /// and stays under the prompt, with a spinner and the time taken so far,
    /// until the answer arrives. Empty when nothing is running.
    busy: String,
    busy_since: Instant,
    spin: Spinner,

    worktrees_of: Seam<Result<Vec<Worktree>>>,
    branches_of: Seam<Result<Vec<Branch>>>,
    remote_branches_of: Seam<(Vec<Branch>, Option<Error>)>,
    prs_of: Seam<Result<Vec<PullRequest>>>,
    state_scan_of: StateScan,
    changed_of: Seam<usize>,
    open_url: Arc<dyn Fn(&str) + Send + Sync>,
    gh: String, // the program that checks pull requests out
}

impl Model {
    pub fn new(tree: &Tree, repos: &[Repo], hist: &History, theme: &Theme, keys: Keys) -> Model {
        let now = repo::now();
        let mut items: Vec<Item> = repos
            .iter()
            .map(|r| {
                let v = hist.visit(&r.path());
                Item {
                    label: r.rel.clone(),
                    path: r.path(),
                    score: v.score(now),
                    seen: v,
                    ..Default::default()
                }
            })
            .collect();
        items.sort_by(|a, b| {
            a.score
                .total_cmp(&b.score)
                .then_with(|| a.label.cmp(&b.label))
        });

        let mut input = Input::default();
        // Completing the slash commands as they are typed: the box fills in
        // the rest of the name, and Tab accepts it.
        input.set_suggestions(command::command_names());

        let mut m = Model {
            all: items,
            view: Vec::new(),
            view_stale: true,
            matched: HashMap::new(),
            cursor: 0,
            input,
            status: HashMap::new(),
            probing: HashSet::new(),
            st: theme.styles(),
            w: 80,
            h: 24,
            result: Outcome::default(),
            mode: Mode::Repos,
            origin: String::new(),
            repo_at: String::new(),
            saved: None,
            keys,
            tree: tree.clone(),
            query: String::new(),
            over: Overlay::None,
            ask: Pending::default(),
            repo_states: None,
            scanning_states: false,
            dirty_only: false,
            unpushed_only: false,
            note: String::new(),
            busy: String::new(),
            busy_since: Instant::now(),
            spin: Spinner::default(),
            worktrees_of: Arc::new(repo::worktrees),
            branches_of: Arc::new(repo::branches),
            remote_branches_of: Arc::new(repo::remote_branches),
            prs_of: Arc::new(repo::pull_requests),
            state_scan_of: Arc::new(repo::status_map),
            changed_of: Arc::new(repo::changed_files),
            open_url: Arc::new(|u| {
                let _ = browse::open_url(u);
            }),
            gh: "gh".into(),
        };
        m.filter();
        m
    }

    pub fn init(&self) -> Vec<Cmd> {
        self.load_status().into_iter().collect()
    }

    fn current(&self) -> Option<&Item> {
        self.view.get(self.cursor).map(|&i| &self.all[i])
    }

    /// load_status arms the delay for the selected repository. No git runs
    /// yet: the probe that follows checks the cursor is still here first.
    fn load_status(&self) -> Option<Cmd> {
        let p = self.current()?.path.clone();
        if p.is_empty() || self.status.contains_key(&p) {
            return None; // a branch with no worktree yet has nothing to ask git about
        }
        Some(Cmd::After(STATUS_DELAY, Msg::Probe(p)))
    }

    /// probe asks git about path, off the UI thread, unless the cursor has
    /// since moved elsewhere or the answer is already on its way.
    fn probe(&mut self, path: &str) -> Option<Cmd> {
        let here = self.current()?;
        if here.path != path || self.probing.contains(path) || self.status.contains_key(path) {
            return None;
        }
        self.probing.insert(path.to_string());
        let path = path.to_string();
        Some(Cmd::Task(Box::new(move || {
            let status = repo::describe(&path);
            Some(Msg::Status(path, status))
        })))
    }

    pub fn update(&mut self, msg: Msg) -> Vec<Cmd> {
        match msg {
            Msg::Resize(w, h) => {
                (self.w, self.h) = (w, h);
                vec![]
            }
            Msg::Probe(p) => self.probe(&p).into_iter().collect(),
            Msg::Status(p, s) => {
                self.probing.remove(&p);
                self.status.insert(p, s);
                vec![]
            }
            Msg::Spin(tag) => {
                // Ticks stop once nothing is running: the chain ends here. A
                // tick from a chain that has been replaced is dropped too.
                if self.busy.is_empty() || tag != self.spin.tag {
                    return vec![];
                }
                self.spin.frame += 1;
                vec![Cmd::After(SPINNER_INTERVAL, Msg::Spin(tag))]
            }
            Msg::Done(d) => self.done(d),
            Msg::RemoteBranches(r) => self.add_remote_branches(r),
            Msg::Prs(p) => self.show_prs(p),
            Msg::RepoStates(tag, states) => {
                self.repo_states = Some(states);
                self.scanning_states = false;
                self.note.clear();
                self.finish_busy(tag);
                if self.dirty_only || self.unpushed_only {
                    if self.mode == Mode::Repos {
                        self.filter();
                        self.cursor = self.view.len().saturating_sub(1);
                        return self.load_status().into_iter().collect();
                    }
                    if let Some(saved) = self.saved.as_mut() {
                        saved.mark_stale();
                    }
                }
                vec![]
            }
            Msg::Paste(text) => {
                if self.over != Overlay::None {
                    return vec![];
                }
                let before = self.input.value();
                self.input.paste(&text);
                self.after_edit(&before)
            }
            Msg::Key(k) => self.key(k),
        }
    }

    fn done(&mut self, d: Done) -> Vec<Cmd> {
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

        match d.kind {
            Change::Remove | Change::RemoveWorktree | Change::Create | Change::AddWorktree => {
                let changed = |all: &mut Vec<Item>| match d.kind {
                    Change::Remove | Change::RemoveWorktree => {
                        let before = all.len();
                        all.retain(|it| it.path != d.path);
                        all.len() != before
                    }
                    Change::Create => {
                        // The repository list is ascending by frecency, best
                        // last, and a fresh clone has no visits — so its row
                        // belongs at the top, not at the end a push would
                        // give it, which is the best spot in the list.
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
                            // In the worktree list the main worktree belongs
                            // to the bottom row, and the rest read newest
                            // first, so a new worktree goes at the top. The
                            // branch and pull request lists keep the row a
                            // branch was: those are updated in place above.
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
                        // The created row is the selection, wherever its
                        // ordering put it; standing on the bottom row would
                        // be right only when it was pushed to the end.
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
                    Change::Remove | Change::RemoveWorktree => {
                        format!("removed {}", info::tildify(&d.path))
                    }
                    Change::Create | Change::AddWorktree => {
                        format!("created {}", info::tildify(&d.path))
                    }
                    _ => unreachable!(),
                };
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
        self.load_status().into_iter().collect()
    }

    fn key(&mut self, k: Key) -> Vec<Cmd> {
        // A panel is modal: it answers to its own keys and swallows
        // everything else, so nothing moves behind it.
        if self.over != Overlay::None {
            return self.panel_key(&k);
        }

        // The configurable keys first: they are not fixed names.
        let name = k.name.as_str();
        if name == self.keys.worktree.key() {
            return self.switch_to(Mode::Worktrees);
        }
        if name == self.keys.branch.key() {
            return self.switch_to(Mode::Branches);
        }
        if name == self.keys.pr.key() {
            return self.switch_to(Mode::Prs);
        }
        if name == self.keys.remote.key() {
            return self.open_remote().into_iter().collect();
        }
        match name {
            "ctrl+c" => return vec![Cmd::Quit],
            "esc" | "ctrl+g" => {
                // These back out of whatever is narrowing the list before they
                // quit gm: the worktree list first, then a filter. From the
                // repository list with nothing to undo, Ctrl-G does nothing —
                // it is the key that opened gm in the first place.
                if self.mode != Mode::Repos {
                    self.restore();
                    return self.load_status().into_iter().collect();
                }
                // Clearing the query holds the selection, so this is also how
                // you get from a repository you found to a command that acts
                // on it.
                if !self.input.value().is_empty() {
                    self.input.set_value("");
                    self.filter();
                    return self.load_status().into_iter().collect();
                }
                if self.dirty_only || self.unpushed_only {
                    self.dirty_only = false;
                    self.unpushed_only = false;
                    self.filter();
                    self.cursor = self.view.len().saturating_sub(1);
                    return self.load_status().into_iter().collect();
                }
                return if name == "esc" {
                    vec![Cmd::Quit]
                } else {
                    vec![]
                };
            }
            "enter" => {
                if let Some((_, typed)) = split_input(&self.input.value()) {
                    return self.run_command(&typed);
                }
                match self.mode {
                    Mode::Branches => return self.check_out(),
                    Mode::Prs => return self.check_out_pr(),
                    _ => {}
                }
                if let Some(it) = self.current() {
                    self.result = Outcome {
                        action: Action::Jump,
                        arg: it.path.clone(),
                    };
                }
                return vec![Cmd::Quit];
            }
            "down" | "ctrl+n" => {
                if self.cursor + 1 < self.view.len() {
                    self.cursor += 1;
                }
                return self.load_status().into_iter().collect();
            }
            "up" | "ctrl+p" => {
                self.cursor = self.cursor.saturating_sub(1);
                return self.load_status().into_iter().collect();
            }
            "pgdown" | "pgup" => {
                // One screen at a time: the window move() renders with is the
                // size a page is measured in.
                let rows = self.page_rows();
                if name == "pgdown" {
                    self.cursor = (self.cursor + rows).min(self.view.len().saturating_sub(1));
                } else {
                    self.cursor = self.cursor.saturating_sub(rows);
                }
                return self.load_status().into_iter().collect();
            }
            _ => {}
        }

        let before = self.input.value();
        self.input.update(&k);
        self.after_edit(&before)
    }

    /// after_edit refilters when the box changed, and describes whatever the
    /// cursor ended up on.
    fn after_edit(&mut self, before: &str) -> Vec<Cmd> {
        let now = self.input.value();
        if now != before {
            self.input.set_suggestions(completions(&now));
            self.filter();
        }
        self.load_status().into_iter().collect()
    }

    /// open_remote hands the selected repository's remote to a browser. A
    /// repository without one, or one whose remote is not a URL, does nothing
    /// visible: the finder is showing a list, not reporting on git.
    fn open_remote(&self) -> Option<Cmd> {
        let it = self.current()?;
        // A branch with no worktree: the repository has the remote.
        let path = if it.path.is_empty() {
            self.repo_at.clone()
        } else {
            it.path.clone()
        };
        let known = self
            .status
            .get(&path)
            .map(|s| s.remote.clone())
            .unwrap_or_default();
        let open = self.open_url.clone();
        Some(Cmd::Task(Box::new(move || {
            // Not fetched yet for this row; ask git now rather than making the
            // key do nothing on the first press.
            let remote = if known.is_empty() && !path.is_empty() {
                repo::origin_url(&path)
            } else {
                known
            };
            if let Ok(url) = repo::browse_url(&remote) {
                open(&url);
            }
            None
        })))
    }

    /// start_busy puts what gm now waits on under the prompt and starts the
    /// spinner that shows it has not hung. The returned command is the first
    /// tick.
    fn start_busy(&mut self, what: &str) -> (u64, Cmd) {
        self.spin.tag = self.spin.tag.wrapping_add(1);
        self.spin.frame = 0;
        self.busy = what.to_string();
        self.busy_since = Instant::now();
        self.note.clear();
        let tag = self.spin.tag;
        (tag, Cmd::After(Duration::ZERO, Msg::Spin(tag)))
    }

    fn finish_busy(&mut self, tag: u64) {
        if !self.busy.is_empty() && tag == self.spin.tag {
            self.busy.clear();
        }
    }

    /// help_line draws the key hints, dropping the ones that do not fit rather
    /// than wrapping onto a second line.
    fn help_line(&self, width: usize) -> Line<'static> {
        let first = |segs: Vec<Seg>, w: usize| {
            wrap_segs(&segs, w, "")
                .into_iter()
                .next()
                .unwrap_or_default()
        };
        let st = &self.st;
        // An answer to something the user just typed displaces the hints: it
        // is about to be cleared by their next keystroke anyway.
        if !self.note.is_empty() {
            return first(vec![Seg::new(&self.note, st.dirty)], width);
        }
        if !self.busy.is_empty() {
            let elapsed = format!("{} {}s", self.busy, self.busy_since.elapsed().as_secs());
            let mut spans = vec![Span::styled(self.spin.view(), st.help_key), Span::raw(" ")];
            spans.extend(first(vec![Seg::new(&elapsed, st.help)], width.saturating_sub(2)).spans);
            return Line::from(spans);
        }
        if is_command(&self.input.value()) {
            return Line::from(vec![
                Span::styled("enter runs the command  ·  tab completes it  ·  ", st.help),
                Span::styled("/help", st.help_key),
                Span::styled(" lists them", st.help),
            ]);
        }
        if self.dirty_only || self.unpushed_only {
            // The filter has to be visible, or an empty list reads as a bug.
            let label = match (self.dirty_only, self.unpushed_only) {
                (true, true) => "dirty + unpushed",
                (true, false) => "dirty only",
                (false, true) => "unpushed only",
                (false, false) => unreachable!(),
            };
            const SEP: &str = "  ·  ";
            let mut spans = vec![Span::styled(label, st.dirty), Span::styled(SEP, st.help)];
            spans.extend(
                self.hints(width.saturating_sub(label.len() + SEP.len()))
                    .spans,
            );
            return Line::from(spans);
        }
        self.hints(width)
    }

    fn hints(&self, width: usize) -> Line<'static> {
        let (wt, br, pr, rm) = (
            self.keys.worktree.short(),
            self.keys.branch.short(),
            self.keys.pr.short(),
            self.keys.remote.short(),
        );
        // Esc undoes one layer of narrowing at a time, so it has to say which.
        let esc = if !self.input.value().is_empty() {
            "clear"
        } else if self.dirty_only || self.unpushed_only {
            "show all"
        } else {
            "quit"
        };
        let mv = ("↑↓ ctrl-p/n".to_string(), "move");
        let hints: Vec<(String, &str)> = match self.mode {
            Mode::Repos => vec![
                mv,
                ("enter".into(), "jump"),
                (wt, "worktrees"),
                ("esc".into(), esc),
                (rm, "remote"),
                (br, "branches"),
                (pr, "prs"),
            ],
            Mode::Worktrees => vec![
                mv,
                ("enter".into(), "jump"),
                (format!("{wt}/g/esc"), "repos"),
                (rm, "remote"),
                (br, "branches"),
                (pr, "prs"),
            ],
            Mode::Branches => vec![
                mv,
                ("enter".into(), "check out"),
                (format!("{br}/g/esc"), "repos"),
                (rm, "remote"),
                (wt, "worktrees"),
                (pr, "prs"),
            ],
            Mode::Prs => vec![
                mv,
                ("enter".into(), "check out"),
                (format!("{pr}/g/esc"), "repos"),
                (rm, "remote"),
                (wt, "worktrees"),
                (br, "branches"),
            ],
        };

        const SEP: &str = "  ·  ";
        let width_of = |s: &str| Span::raw(s).width();
        let mut spans = Vec::new();
        let mut used = 0;
        for (i, (key, what)) in hints.into_iter().enumerate() {
            let lead = if i > 0 { SEP } else { "" };
            let w = width_of(lead) + width_of(&key) + 1 + width_of(what);
            if used + w > width {
                break;
            }
            used += w;
            spans.push(Span::styled(lead, self.st.help));
            spans.push(Span::styled(key, self.st.help_key));
            spans.push(Span::styled(format!(" {what}"), self.st.help));
        }
        Line::from(spans)
    }

    /// render draws the whole screen: the list hanging from the prompt on the
    /// left, the details pane on the right, the prompt, the hints, and any
    /// panel over the lot.
    /// page_rows is how many rows of the list one turn of `pgup`/`pgdown`
    /// moves, the height of the window render hangs from the prompt.
    fn page_rows(&self) -> usize {
        self.h.saturating_sub(4).max(3) as usize // the bordered input box, plus the hint line under it
    }

    pub fn render(&self, buf: &mut Buffer) {
        let area = *buf.area();
        let w = self.w.min(area.width) as usize;
        let rows = self.page_rows();
        // The list gets the left 3/5: it is what gets scanned.
        let (list_w, info_w) = if w >= 66 {
            (w - w * 2 / 5 - 3, w * 2 / 5)
        } else {
            (w, 0)
        };

        // A window over the view, anchored so the cursor stays visible.
        let start = self.view.len().saturating_sub(rows).min(self.cursor);
        let end = (start + rows).min(self.view.len());

        let mut lines: Vec<Line> = Vec::with_capacity(rows);
        for _ in 0..rows - (end - start) {
            lines.push(Line::from(" ".repeat(list_w))); // the list hangs from the bottom
        }
        for i in start..end {
            lines.push(self.render_row(i, i == self.cursor, list_w));
        }

        // The info pane is top-aligned: it reads top-down, unlike the list,
        // which hangs from the prompt. A pane taller than the window loses its
        // tail rather than its name and path.
        let mut info = if info_w > 0 {
            self.info_lines(info_w)
        } else {
            Vec::new()
        };
        info.resize(lines.len(), Line::default());

        let set = |buf: &mut Buffer, y: usize, line: &Line| {
            if y < area.height as usize {
                buf.set_line(area.x, area.y + y as u16, line, w as u16);
            }
        };
        for (y, (mut line, pane)) in lines.into_iter().zip(info).enumerate() {
            if info_w > 0 {
                line.spans.push(Span::styled(" │ ", self.st.divider));
                line.spans.extend(pane.spans);
            }
            set(buf, y, &line);
        }

        // The prompt box, two columns short of the window, as the Go finder
        // drew it.
        let box_area = Rect::new(
            area.x,
            area.y + rows as u16,
            (w as u16).saturating_sub(2),
            3,
        )
        .intersection(area);
        let prompt =
            Line::from(
                self.input
                    .view("❯ ", self.st.prompt, self.st.text, self.st.suggestion),
            );
        Paragraph::new(prompt)
            .block(self.bordered(0))
            .render(box_area, buf);

        let mut help = self.help_line(w.saturating_sub(1));
        help.spans.insert(0, Span::raw(" "));
        set(buf, rows + 3, &help);

        match self.over {
            Overlay::Help => self.overlay(buf, self.help_box()),
            Overlay::Confirm => self.overlay(buf, self.confirm_box()),
            Overlay::None => {}
        }
    }

    /// bordered is the rounded box every panel and the prompt wear.
    fn bordered(&self, pad: u16) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(self.st.border)
            .padding(ratatui::widgets::Padding::horizontal(pad))
    }

    /// overlay draws a panel over the list, centred, leaving what is behind it
    /// visible around the edges.
    fn overlay(&self, buf: &mut Buffer, content: Vec<Line<'static>>) {
        let area = *buf.area();
        let inner_w = content.iter().map(Line::width).max().unwrap_or(0) as u16;
        let (bw, bh) = (inner_w + 4, content.len() as u16 + 2);
        let x = self.w.saturating_sub(bw) / 2;
        let y = self.h.saturating_sub(bh) / 2;
        let r = Rect::new(area.x + x, area.y + y, bw, bh).intersection(area);
        Clear.render(r, buf);
        Paragraph::new(content)
            .block(self.bordered(1))
            .render(r, buf);
    }
}

/// run draws the finder and returns what the user asked for. It draws on the
/// terminal itself, never on stdout: stdout carries the chosen path back to
/// the shell binding.
pub fn run(
    tree: &Tree,
    repos: &[Repo],
    hist: &History,
    theme: &Theme,
    keys: Keys,
) -> Result<Outcome> {
    keys.check()?;
    let mut m = Model::new(tree, repos, hist, theme, keys);
    let mut term = terminal::Term::open()?;
    let (w, h) = term.size()?;
    m.update(Msg::Resize(w, h));

    let (tx, rx) = mpsc::channel::<Msg>();
    let mut timers: Vec<(Instant, Msg)> = Vec::new();
    let mut pending: Vec<Msg> = Vec::new();
    let mut cmds = m.init();
    loop {
        // Carry out what the last round asked for.
        for cmd in cmds.drain(..) {
            match cmd {
                Cmd::Quit => return Ok(m.result),
                Cmd::After(d, msg) => timers.push((Instant::now() + d, msg)),
                Cmd::Task(task) => {
                    let tx = tx.clone();
                    std::thread::spawn(move || {
                        if let Some(msg) = task() {
                            let _ = tx.send(msg);
                        }
                    });
                }
            }
        }
        term.draw(&m)?;

        // Wait for a key, a finished task, or the next timer, whichever is
        // first. A task's answer is picked up within a frame.
        let next_timer = timers.iter().map(|(at, _)| *at).min();
        let wait = next_timer
            .map(|at| at.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_millis(50))
            .min(Duration::from_millis(50));
        if let Some(msg) = term.poll(wait)? {
            pending.push(msg);
        }
        pending.extend(rx.try_iter());
        let now = Instant::now();
        let (due, later): (Vec<_>, Vec<_>) = timers.into_iter().partition(|(at, _)| *at <= now);
        timers = later;
        pending.extend(due.into_iter().map(|(_, msg)| msg));

        for msg in pending.drain(..) {
            cmds.extend(m.update(msg));
        }
    }
}

#[cfg(test)]
mod tests;
