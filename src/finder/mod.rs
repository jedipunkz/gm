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
/// action to one of them would shadow quitting or moving. Only plain Ctrl
/// chords can collide: the finder's own keys carry no other modifier.
const RESERVED: [(char, &str); 3] = [('c', "quit"), ('n', "move down"), ('p', "move up")];

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
    label: String, // how the new row reads, if one was made
    path: String,
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
    Dirty(HashMap<String, bool>),   // the result of a scan for uncommitted work
}

/// RemoteBranches carries what the remotes of one repository have that the
/// last fetch did not bring.
pub struct RemoteBranches {
    path: String,
    branches: Vec<Branch>,
    err: Option<Error>,
}

/// Prs carries gh's answer about one repository.
pub struct Prs {
    path: String, // the repository it was asked about
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
type DirtyScan = Arc<dyn Fn(&[String]) -> HashMap<String, bool> + Send + Sync>;

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
    /// dirty holds the answer for every repository once a scan has run; None
    /// until one has. dirty_only is the filter itself.
    dirty: Option<HashMap<String, bool>>,
    dirty_only: bool,
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
    dirty_of: DirtyScan,
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
            dirty: None,
            dirty_only: false,
            note: String::new(),
            busy: String::new(),
            busy_since: Instant::now(),
            spin: Spinner::default(),
            worktrees_of: Arc::new(repo::worktrees),
            branches_of: Arc::new(repo::branches),
            remote_branches_of: Arc::new(repo::remote_branches),
            prs_of: Arc::new(repo::pull_requests),
            dirty_of: Arc::new(repo::dirty_map),
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
                self.spin.tag += 1;
                self.spin.frame += 1;
                vec![Cmd::After(SPINNER_INTERVAL, Msg::Spin(self.spin.tag))]
            }
            Msg::Done(d) => self.done(d),
            Msg::RemoteBranches(r) => self.add_remote_branches(r),
            Msg::Prs(p) => self.show_prs(p),
            Msg::Dirty(d) => {
                self.dirty = Some(d);
                self.note.clear();
                self.busy.clear();
                self.filter();
                self.cursor = self.view.len().saturating_sub(1);
                self.load_status().into_iter().collect()
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
        self.busy.clear();
        if let Some(e) = d.err {
            self.note = e.0;
            return vec![];
        }
        match d.kind {
            Change::Remove | Change::RemoveWorktree => {
                self.drop_path(&d.path);
                self.note = format!("removed {}", info::tildify(&d.path));
            }
            Change::Create | Change::AddWorktree => {
                self.add(&d.label, &d.path);
                self.note = format!("created {}", info::tildify(&d.path));
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
                if self.dirty_only {
                    self.dirty_only = false;
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
    fn start_busy(&mut self, what: &str) -> Cmd {
        self.busy = what.to_string();
        self.busy_since = Instant::now();
        self.note.clear();
        Cmd::After(Duration::ZERO, Msg::Spin(self.spin.tag))
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
        if self.dirty_only {
            // The filter has to be visible, or an empty list reads as a bug.
            const LABEL: &str = "dirty only";
            const SEP: &str = "  ·  ";
            let mut spans = vec![Span::styled(LABEL, st.dirty), Span::styled(SEP, st.help)];
            spans.extend(
                self.hints(width.saturating_sub(LABEL.len() + SEP.len()))
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
        } else if self.dirty_only {
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
    pub fn render(&self, buf: &mut Buffer) {
        let area = *buf.area();
        let (w, h) = (
            self.w.min(area.width) as usize,
            self.h.min(area.height) as usize,
        );
        let rows = h.saturating_sub(4).max(3); // the bordered input box, plus the hint line under it
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

mod terminal {
    //! The terminal the finder owns while it is up: /dev/tty in raw mode and
    //! the alternate screen, put back however the finder ends.

    use super::*;
    use ratatui::backend::CrosstermBackend;
    use ratatui::crossterm::event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind,
        KeyModifiers, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
        PushKeyboardEnhancementFlags,
    };
    use ratatui::crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
    use ratatui::crossterm::{cursor, execute, terminal};

    pub struct Term {
        term: ratatui::Terminal<CrosstermBackend<Box<dyn Write>>>,
    }

    /// tty is the terminal to draw on: /dev/tty, so $(gm) can capture stdout,
    /// or stderr when there is no controlling terminal.
    fn tty() -> Box<dyn Write> {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
        {
            Ok(f) => Box::new(f),
            Err(_) => Box::new(std::io::stderr()),
        }
    }

    impl Term {
        pub fn open() -> Result<Term> {
            terminal::enable_raw_mode()?;
            let mut out = tty();
            // The Kitty keyboard protocol is what lets Ctrl-Shift chords
            // through. It is asked for without asking whether the terminal
            // speaks it: the question is answered on stdout, which belongs to
            // the shell binding, and a terminal that does not ignores it.
            execute!(
                out,
                EnterAlternateScreen,
                EnableBracketedPaste,
                cursor::Hide,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )?;
            let term = ratatui::Terminal::new(CrosstermBackend::new(out))?;
            Ok(Term { term })
        }

        pub fn size(&self) -> Result<(u16, u16)> {
            let s = self.term.size()?;
            Ok((s.width, s.height))
        }

        pub fn draw(&mut self, m: &Model) -> Result<()> {
            self.term.draw(|f| m.render(f.buffer_mut()))?;
            Ok(())
        }

        /// poll waits up to wait for something from the terminal.
        pub fn poll(&mut self, wait: Duration) -> Result<Option<Msg>> {
            if !event::poll(wait)? {
                return Ok(None);
            }
            Ok(match event::read()? {
                Event::Key(k) if k.kind != KeyEventKind::Release => {
                    key_of(k.code, k.modifiers).map(Msg::Key)
                }
                Event::Paste(s) => Some(Msg::Paste(s)),
                Event::Resize(w, h) => {
                    self.term.autoresize()?;
                    Some(Msg::Resize(w, h))
                }
                _ => None,
            })
        }
    }

    impl Drop for Term {
        fn drop(&mut self) {
            let out = self.term.backend_mut();
            let _ = execute!(
                out,
                PopKeyboardEnhancementFlags,
                DisableBracketedPaste,
                LeaveAlternateScreen,
                cursor::Show
            );
            let _ = terminal::disable_raw_mode();
        }
    }

    /// key_of spells a key press the way Bubble Tea did, modifiers in the
    /// order ctrl, alt, shift.
    pub fn key_of(code: KeyCode, m: KeyModifiers) -> Option<Key> {
        let (ctrl, alt) = (
            m.contains(KeyModifiers::CONTROL),
            m.contains(KeyModifiers::ALT),
        );
        let mut shift = m.contains(KeyModifiers::SHIFT);
        let base = match code {
            KeyCode::Char(c) if !ctrl && !alt => return Some(Key::char(c)),
            KeyCode::Char(c) => {
                shift |= c.is_uppercase();
                c.to_lowercase().to_string()
            }
            KeyCode::Enter => "enter".into(),
            KeyCode::Esc => "esc".into(),
            KeyCode::Tab => "tab".into(),
            KeyCode::BackTab => {
                shift = true;
                "tab".into()
            }
            KeyCode::Backspace => "backspace".into(),
            KeyCode::Delete => "delete".into(),
            KeyCode::Up => "up".into(),
            KeyCode::Down => "down".into(),
            KeyCode::Left => "left".into(),
            KeyCode::Right => "right".into(),
            KeyCode::Home => "home".into(),
            KeyCode::End => "end".into(),
            KeyCode::PageUp => "pgup".into(),
            KeyCode::PageDown => "pgdown".into(),
            KeyCode::F(n) => format!("f{n}"),
            _ => return None,
        };
        let mut name = String::new();
        if ctrl {
            name.push_str("ctrl+");
        }
        if alt {
            name.push_str("alt+");
        }
        if shift {
            name.push_str("shift+");
        }
        name.push_str(&base);
        Some(Key { name, text: None })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn keys_are_spelled_the_bubble_tea_way() {
            let k = |c, m| key_of(c, m).unwrap().name;
            assert_eq!(
                k(
                    KeyCode::Char('b'),
                    KeyModifiers::CONTROL | KeyModifiers::ALT
                ),
                "ctrl+alt+b"
            );
            assert_eq!(k(KeyCode::Char('B'), KeyModifiers::CONTROL), "ctrl+shift+b");
            assert_eq!(k(KeyCode::Char('w'), KeyModifiers::CONTROL), "ctrl+w");
            assert_eq!(k(KeyCode::Esc, KeyModifiers::NONE), "esc");
            assert_eq!(
                key_of(KeyCode::Char('Y'), KeyModifiers::SHIFT),
                Some(Key::char('Y'))
            );
        }
    }
}

#[cfg(test)]
mod tests;
