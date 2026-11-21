mod backend;
mod branch;
mod browse;
mod clipboard;
mod command;
mod detail;
mod info;
mod input;
mod issues;
mod list;
mod md;
mod overlay;
mod pr;
pub mod score;
mod terminal;
mod theme;
mod worktree;

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget};

pub use theme::{Theme, lookup_theme};

use crate::config::{self, Chord};
use crate::repo::{self, History, Repo, Status, Tree};
use crate::{Result, err};
use backend::Backend;
use branch::{Branches, RemoteBranches};
use command::{completions, is_command, split_input};
use info::{Seg, wrap_segs};
use input::{Input, SPINNER_INTERVAL, Spinner};
use issues::{IssueDetail, Issues};
use list::Item;
use overlay::{ConfirmDetail, Done, ExpireDetail, RemoveDetail};
use overlay::{Overlay, Pending};
use pr::{PrDetail, Prs};
use theme::Styles;
use worktree::Worktrees;
use worktree::{Mode, Stash};

pub const DEFAULT_WORKTREE_KEY: &str = "ctrl-w";
pub const DEFAULT_BRANCH_KEY: &str = "ctrl-l";
pub const DEFAULT_PR_KEY: &str = "ctrl-j";
pub const DEFAULT_ISSUE_KEY: &str = "ctrl-[";
pub const DEFAULT_REMOTE_KEY: &str = "ctrl-alt-b";

const RESERVED: [(char, &str); 4] = [
    ('c', "quit"),
    ('n', "move down"),
    ('p', "move up"),
    ('g', "back out"),
];

#[derive(Debug, Clone, Default)]
pub struct Keys {
    pub worktree: Chord,
    pub branch: Chord,
    pub pr: Chord,
    pub issue: Chord,
    pub remote: Chord,
}

impl Keys {
    pub fn from_config(cfg: &config::Config) -> Result<Keys> {
        Ok(Keys {
            worktree: config::parse_chord(&cfg.worktree_key, DEFAULT_WORKTREE_KEY)?,
            branch: config::parse_chord(&cfg.branch_key, DEFAULT_BRANCH_KEY)?,
            pr: config::parse_chord(&cfg.pr_key, DEFAULT_PR_KEY)?,
            issue: config::parse_chord(&cfg.issue_key, DEFAULT_ISSUE_KEY)?,
            remote: config::parse_chord(&cfg.remote_key, DEFAULT_REMOTE_KEY)?,
        })
    }

    fn check(&self) -> Result<()> {
        let named = [
            ("worktree_key", &self.worktree),
            ("branch_key", &self.branch),
            ("pr_key", &self.pr),
            ("issue_key", &self.issue),
            ("remote_key", &self.remote),
        ];
        for (i, (name, chord)) in named.iter().enumerate() {
            for (other, o) in &named[i + 1..] {
                if chord.key() == o.key() {
                    return Err(err!("{name} and {other} are both {}", chord.display));
                }
            }
            // crossterm reads the byte as Enter or Tab, so the binding would
            // never fire; with Alt or Shift too, outside the Kitty protocol.
            // Ctrl-J is the exception: in raw mode it arrives as Ctrl-J.
            if let Some((_, key)) = config::DISGUISED
                .iter()
                .find(|(c, _)| *c == chord.letter && *c != 'j')
            {
                return Err(err!(
                    "{name} cannot be {}: a terminal sends it as {key}",
                    chord.display
                ));
            }
            if !chord.plain() {
                continue;
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

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Action {
    #[default]
    None,
    Jump,
    Get,
}

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

pub enum Msg {
    Key(Key),
    Paste(String),
    Resize(u16, u16),
    Probe(String),
    Status(String, Status),
    Spin(u64),
    Done(Done),
    Worktrees(Worktrees),
    Branches(Branches),
    RemoteBranches(RemoteBranches),
    Prs(Prs),
    Issues(Issues),
    IssueDetail(IssueDetail),
    PrDetail(PrDetail),
    ConfirmDetail(ConfirmDetail),
    RemoveDetail(RemoveDetail),
    ExpireDetail(ExpireDetail),
    RepoStates(u64, HashMap<String, repo::State>),
    RepoState(String, repo::State),
}

pub type Task = Box<dyn FnOnce() -> Option<Msg> + Send>;

pub enum Cmd {
    Quit,
    Task(Task),
    After(Duration, Box<Msg>),
}

const STATUS_DELAY: Duration = Duration::from_millis(100);

#[derive(Clone)]
pub struct Model {
    all: Vec<Item>,
    view: Vec<usize>,
    view_stale: bool,
    matched: HashMap<usize, Vec<usize>>,
    cursor: usize,
    input: Input,
    status: HashMap<String, Status>,
    probing: HashSet<String>,
    st: Styles,
    w: u16,
    h: u16,
    result: Outcome,

    mode: Mode,
    origin: String,
    repo_at: String,
    saved: Option<Stash>,
    keys: Keys,
    tree: Tree,
    query: String,
    over: Overlay,
    ask: Pending,
    detail: Option<issues::Detail>,
    repo_states: Option<HashMap<String, repo::State>>,
    scanning_states: bool,
    dirty_only: bool,
    unpushed_only: bool,
    note: String,
    busy: String,
    busy_since: Instant,
    spin: Spinner,

    backend: Backend,
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
            detail: None,
            repo_states: None,
            scanning_states: false,
            dirty_only: false,
            unpushed_only: false,
            note: String::new(),
            busy: String::new(),
            busy_since: Instant::now(),
            spin: Spinner::default(),
            backend: Backend::default(),
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

    fn load_status(&self) -> Option<Cmd> {
        let p = self.current()?.path.clone();
        if p.is_empty() || self.status.contains_key(&p) {
            return None;
        }
        Some(Cmd::After(STATUS_DELAY, Msg::Probe(p).into()))
    }

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
                if self.busy.is_empty() || tag != self.spin.tag {
                    return vec![];
                }
                self.spin.frame += 1;
                vec![Cmd::After(SPINNER_INTERVAL, Msg::Spin(tag).into())]
            }
            Msg::Done(d) => self.done(d),
            Msg::Worktrees(w) => self.show_worktrees(w),
            Msg::Branches(b) => self.show_branches(b),
            Msg::RemoteBranches(r) => self.add_remote_branches(r),
            Msg::Prs(p) => self.show_prs(p),
            Msg::Issues(i) => self.show_issues(i),
            Msg::IssueDetail(d) => self.show_issue_detail(d),
            Msg::PrDetail(d) => self.show_pr_detail(d),
            Msg::ConfirmDetail(d) => self.fill_confirm_detail(d),
            Msg::RemoveDetail(d) => self.fill_detail(d),
            Msg::ExpireDetail(d) => self.fill_expire_detail(d),
            Msg::RepoStates(tag, states) => {
                self.repo_states.get_or_insert_default().extend(states);
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
            Msg::RepoState(path, state) => {
                self.repo_states.get_or_insert_default().insert(path, state);
                if self.dirty_only || self.unpushed_only {
                    if self.mode == Mode::Repos {
                        self.filter();
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

    fn key(&mut self, k: Key) -> Vec<Cmd> {
        if self.over != Overlay::None {
            return self.panel_key(&k);
        }

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
        // The default issue chord is ctrl-[, the byte terminals send instead
        // of ESC. It is answered to from the fixed esc branch below instead,
        // because a binding that answered to "ctrl+[" would never fire: no
        // terminal sends that name.
        if name == self.keys.issue.key() && self.keys.issue.letter != '[' {
            return self.switch_to(Mode::Issues);
        }
        if name == self.keys.remote.key() {
            return self.open_remote().into_iter().collect();
        }
        match name {
            "ctrl+c" => return vec![Cmd::Quit],
            "esc" | "ctrl+g" => {
                if self.mode != Mode::Repos {
                    self.restore();
                    return self.load_status().into_iter().collect();
                }
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
                if self.keys.issue.letter == '[' {
                    return self.switch_to(Mode::Issues);
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
                    Mode::Prs => return self.view_pr(),
                    Mode::Issues => return self.view_issue(),
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

    fn after_edit(&mut self, before: &str) -> Vec<Cmd> {
        let now = self.input.value();
        if now != before {
            self.input.set_suggestions(completions(&now));
            self.filter();
        }
        self.load_status().into_iter().collect()
    }

    fn open_remote(&self) -> Option<Cmd> {
        let it = self.current()?;
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
        let open = self.backend.open_url.clone();
        Some(Cmd::Task(Box::new(move || {
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

    fn start_busy(&mut self, what: &str) -> (u64, Cmd) {
        self.spin.tag = self.spin.tag.wrapping_add(1);
        self.spin.frame = 0;
        self.busy = what.to_string();
        self.busy_since = Instant::now();
        self.note.clear();
        let tag = self.spin.tag;
        (tag, Cmd::After(Duration::ZERO, Msg::Spin(tag).into()))
    }

    fn finish_busy(&mut self, tag: u64) {
        if !self.busy.is_empty() && tag == self.spin.tag {
            self.busy.clear();
        }
    }

    fn help_line(&self, width: usize) -> Line<'static> {
        let first = |segs: Vec<Seg>, w: usize| {
            wrap_segs(&segs, w, "")
                .into_iter()
                .next()
                .unwrap_or_default()
        };
        let st = &self.st;
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
        let (wt, br, pr, iss, rm) = (
            self.keys.worktree.short(),
            self.keys.branch.short(),
            self.keys.pr.short(),
            self.keys.issue.short(),
            self.keys.remote.short(),
        );
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
                (rm, "browse"),
                (br, "branches"),
                (pr, "prs"),
                (iss, "issues"),
            ],
            Mode::Worktrees => vec![
                mv,
                ("enter".into(), "jump"),
                (format!("{wt}/g/esc"), "repos"),
                (rm, "browse"),
                (br, "branches"),
                (pr, "prs"),
            ],
            Mode::Branches => vec![
                mv,
                ("enter".into(), "check out"),
                (format!("{br}/g/esc"), "repos"),
                (rm, "browse"),
                (wt, "worktrees"),
                (pr, "prs"),
            ],
            Mode::Prs => vec![
                mv,
                ("enter".into(), "view"),
                (format!("{pr}/g/esc"), "repos"),
                (rm, "browse"),
                (wt, "worktrees"),
                (br, "branches"),
            ],
            Mode::Issues => vec![
                mv,
                ("enter".into(), "view"),
                ("esc".into(), "repos"),
                (rm, "browse"),
                (pr, "prs"),
                (wt, "worktrees"),
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

    fn page_rows(&self) -> usize {
        self.h.saturating_sub(4).max(3) as usize
    }

    pub fn render(&self, buf: &mut Buffer) {
        let area = *buf.area();
        let w = self.w.min(area.width) as usize;
        let rows = self.page_rows();
        let (list_w, info_w) = if w >= 66 {
            (w - w * 2 / 5 - 3, w * 2 / 5)
        } else {
            (w, 0)
        };

        let start = self.view.len().saturating_sub(rows).min(self.cursor);
        let end = (start + rows).min(self.view.len());

        let mut lines: Vec<Line> = Vec::with_capacity(rows);
        for _ in 0..rows - (end - start) {
            lines.push(Line::from(" ".repeat(list_w)));
        }
        for i in start..end {
            lines.push(self.render_row(i, i == self.cursor, list_w));
        }

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

        let box_w = (w as u16).saturating_sub(2);
        let mode_w = 12.min(box_w);
        let y = area.y + rows as u16;
        let box_area = Rect::new(area.x, y, box_w - mode_w, 3).intersection(area);
        let mode_area = Rect::new(area.x + box_w - mode_w, y, mode_w, 3).intersection(area);
        let mode = match self.mode {
            Mode::Repos => "repository",
            Mode::Worktrees => "worktree",
            Mode::Branches => "branch",
            Mode::Prs => "pr",
            Mode::Issues => "issue",
        };
        Paragraph::new(Line::styled(mode, self.st.prompt))
            .alignment(ratatui::layout::Alignment::Center)
            .block(self.bordered(0))
            .render(mode_area, buf);
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
            Overlay::Detail => self.render_detail(buf),
            Overlay::None => {}
        }
    }

    fn bordered(&self, pad: u16) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(self.st.border)
            .padding(ratatui::widgets::Padding::horizontal(pad))
    }

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
    let mut redraw = true;
    loop {
        for cmd in cmds.drain(..) {
            match cmd {
                Cmd::Quit => return Ok(m.result),
                Cmd::After(d, msg) => timers.push((Instant::now() + d, *msg)),
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
        if redraw {
            term.draw(&m)?;
            redraw = false;
        }

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

        redraw |= !pending.is_empty();
        for msg in pending.drain(..) {
            cmds.extend(m.update(msg));
        }
    }
}

#[cfg(test)]
mod tests;
