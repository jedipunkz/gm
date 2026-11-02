//! The finder driven the way the terminal loop drives it: messages in,
//! commands out, and the screen read back from a buffer.

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::overlay::Change;
use super::theme::{ROW_LIFT, blend, rgb};
use super::*;
use crate::config::parse_chord;
use crate::paths;
use crate::repo::{Branch, Commit, Issue, PullRequest, Ref, RefKind, Worktree};
use crate::testutil::{TempDir, exists, git, git_repo, mkdir};

fn test_keys() -> Keys {
    Keys {
        worktree: parse_chord("", DEFAULT_WORKTREE_KEY).unwrap(),
        branch: parse_chord("", DEFAULT_BRANCH_KEY).unwrap(),
        pr: parse_chord("", DEFAULT_PR_KEY).unwrap(),
        issue: parse_chord("", DEFAULT_ISSUE_KEY).unwrap(),
        remote: parse_chord("", DEFAULT_REMOTE_KEY).unwrap(),
    }
}

/// new_model builds a finder over repos with an empty visit log, unless
/// bumped is set, in which case that repository is the favourite.
fn new_model(repos: &[Repo], bumped: &str) -> Model {
    let mut hist = History::open(""); // held in memory, never written
    if !bumped.is_empty() {
        // The log prunes repositories that are gone, so it has to exist.
        mkdir(bumped);
        hist.bump(bumped).unwrap();
    }
    let root = repos.first().map(|r| r.root.clone()).unwrap_or_default();
    Model::new(
        &Tree { roots: vec![root] },
        repos,
        &hist,
        &theme(),
        test_keys(),
    )
}

fn theme() -> Theme {
    lookup_theme("").unwrap()
}

fn repos(root: &str, rels: &[&str]) -> Vec<Repo> {
    rels.iter()
        .map(|rel| Repo {
            root: root.into(),
            rel: rel.to_string(),
        })
        .collect()
}

fn key(name: &str) -> Msg {
    Msg::Key(Key::named(name))
}

fn ch(c: char) -> Msg {
    Msg::Key(Key::char(c))
}

fn typed(m: &mut Model, s: &str) {
    for c in s.chars() {
        m.update(ch(c));
    }
}

fn set_query(m: &mut Model, q: &str) {
    m.input.set_value(q);
    m.filter();
}

/// run_slash types a command and presses Enter, returning what the finder did.
fn run_slash(m: &mut Model, name: &str) -> Vec<Cmd> {
    m.input.set_value(name);
    m.update(key("enter"))
}

/// ask_remove runs /remove and hands the finder git's account of what would
/// be lost, which the question waits for before y does anything.
fn ask_remove(m: &mut Model) {
    let cmds = run_slash(m, "/remove");
    feed(m, cmds);
}

fn is_quit(cmds: &[Cmd]) -> bool {
    cmds.iter().any(|c| matches!(c, Cmd::Quit))
}

// A repository answering again while the dirty filter is on folds away only
// when it has nothing left to show: the scan followed every worktree, so the
// row keeping its work in is not dropped.
#[test]
fn repo_state_folds_a_clean_answer_away() {
    let root = TempDir::new();
    let (mut m, rs) = dirty_model(&root);
    let cmds = run_slash(&mut m, "/dirty");
    feed(&mut m, cmds);

    // Charlie answers once more and it is clean; its row folds away.
    let gone = rs[2].path();
    m.update(Msg::RepoState(gone.clone(), repo::State::default()));
    assert!(
        !rows(&m).contains(&"github.com/acme/charlie".to_string()),
        "{:?}",
        rows(&m)
    );

    // Alpha answers again and it still holds work; its row stays.
    let alpha = rs[0].path();
    m.update(Msg::RepoState(
        alpha.clone(),
        repo::State {
            dirty: 1,
            ..Default::default()
        },
    ));
    assert!(
        rows(&m).contains(&"github.com/acme/alpha".to_string()),
        "{:?}",
        rows(&m)
    );
}

// Text pasted from the terminal belongs to the box the finder shows.
#[test]
fn paste_fills_the_query() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    let mut m = new_model(&rs, "");
    m.update(Msg::Paste("alb".into()));
    assert_eq!(m.input.value(), "alb", "the paste went nowhere");
}

// The command-line paths either branch has to answer: /create with a bad
// reference notes what was wrong, /remove refuses where it does not apply
// and with nothing selected, and a mode command already there is nothing to
// do.
#[test]
fn slash_commands_answer_their_own_edges() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    crate::testutil::git_repo(&rs[0].path());
    let mut m = new_model(&rs, "");

    run_slash(&mut m, "/create --ssh");
    assert_eq!(m.note, "/create --ssh needs a repository", "{}", m.note);
    let cmds = run_slash(&mut m, "/create https://ho\u{7}st/x");
    feed(&mut m, cmds);
    assert!(m.note.contains("invalid character"), "{}", m.note);

    // Branch mode: /remove applies to repositories and worktrees.
    let cmds = run_slash(&mut m, "/branches");
    feed(&mut m, cmds);
    assert_eq!(m.mode, Mode::Branches, "{:?}", m.note);
    run_slash(&mut m, "/remove");
    assert_eq!(
        m.note, "/remove applies to repositories and worktrees",
        "{}",
        m.note
    );
    m.mode = Mode::Issues;
    run_slash(&mut m, "/remove");
    assert_eq!(
        m.note, "/remove applies to repositories and worktrees",
        "{}",
        m.note
    );

    // Repos mode with nothing at all to select: the note says so. filter()
    // during the run rebuilds the view, so the only things to look at have
    // to be gone.
    m.mode = Mode::Repos;
    m.all.clear();
    run_slash(&mut m, "/remove");
    assert_eq!(m.note, "nothing is selected", "{}", m.note);
    // The mode commands, asked for the mode they are already in, have
    // nothing to do.
    m.mode = Mode::Worktrees;
    assert!(run_slash(&mut m, "/worktrees").is_empty());
    m.mode = Mode::Branches;
    assert!(run_slash(&mut m, "/branches").is_empty());
    m.mode = Mode::Prs;
    assert!(run_slash(&mut m, "/prs").is_empty());
    m.mode = Mode::Issues;
    assert!(run_slash(&mut m, "/issues").is_empty());
}

/// answer runs the commands the way the loop would and returns the message
/// that carries the work's result. The spinner's ticks and the details pane's
/// probes, which only redraw, are skipped.
fn answer(cmds: Vec<Cmd>) -> Option<Msg> {
    cmds.into_iter().find_map(|c| match c {
        Cmd::Task(t) => t(),
        _ => None,
    })
}

/// feed hands the answer to the commands back to the model.
fn feed(m: &mut Model, cmds: Vec<Cmd>) -> Vec<Cmd> {
    match answer(cmds) {
        Some(msg) => m.update(msg),
        None => vec![],
    }
}

fn rows(m: &Model) -> Vec<String> {
    m.view.iter().map(|&i| m.all[i].label.clone()).collect()
}

fn label(m: &Model) -> String {
    m.current().map(|it| it.label.clone()).unwrap_or_default()
}

fn screen(m: &Model) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, m.w, m.h));
    m.render(&mut buf);
    buf
}

fn row_text(buf: &Buffer, y: u16) -> String {
    (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol())
        .collect::<String>()
        .trim_end()
        .to_string()
}

fn text(buf: &Buffer) -> String {
    (0..buf.area.height)
        .map(|y| row_text(buf, y))
        .collect::<Vec<_>>()
        .join("\n")
}

fn view_text(m: &Model) -> String {
    text(&screen(m))
}

fn line_text(l: &Line) -> String {
    l.spans.iter().map(|s| s.content.as_ref()).collect()
}

fn lines_text(ls: &[Line]) -> String {
    ls.iter()
        .map(|l| line_text(l).trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// painted reports whether any cell of row y (every row when None) wears the
/// foreground colour hex.
fn painted(buf: &Buffer, y: Option<u16>, hex: &str) -> bool {
    let ys: Vec<u16> = match y {
        Some(y) => vec![y],
        None => (0..buf.area.height).collect(),
    };
    ys.iter()
        .any(|&y| (0..buf.area.width).any(|x| buf[(x, y)].fg == rgb(hex)))
}

fn line_painted(l: &Line, hex: &str) -> bool {
    l.spans.iter().any(|s| s.style.fg == Some(rgb(hex)))
}

/// dirty_model is a finder over three repositories, two of which have
/// uncommitted work and two of which have unpushed commits, with the scan
/// stubbed so no git runs.
fn dirty_model(root: &TempDir) -> (Model, Vec<Repo>) {
    let rs = repos(
        &root.path(),
        &[
            "github.com/acme/alpha",
            "github.com/acme/bravo",
            "github.com/acme/charlie",
        ],
    );
    let mut m = new_model(&rs, "");
    (m.w, m.h) = (90, 14);
    let paths: Vec<String> = rs.iter().map(Repo::path).collect();
    m.backend.state_scan_of = Arc::new(move |_| {
        [
            (
                paths[0].clone(),
                repo::State {
                    dirty: 1,
                    ..Default::default()
                },
            ),
            (
                paths[1].clone(),
                repo::State {
                    ahead: 1,
                    ..Default::default()
                },
            ),
            (
                paths[2].clone(),
                repo::State {
                    dirty: 1,
                    ahead: 1,
                    ..Default::default()
                },
            ),
        ]
        .into_iter()
        .collect()
    });
    (m, rs)
}

/// open_worktrees asks for the worktree list and hands the finder git's
/// answer.
fn open_worktrees(m: &mut Model) {
    let cmds = m.open_worktrees();
    feed(m, cmds);
}

fn only_main(m: &mut Model) {
    m.backend.worktrees_of = Arc::new(|dir| {
        Ok(vec![Worktree {
            path: dir.into(),
            branch: "main".into(),
            ..Default::default()
        }])
    });
}

/// branch_model is a finder over one repository whose branches are stubbed:
/// main is checked out in the repository itself, feat/login lives only on
/// origin, and fix/timeout is local with no worktree.
fn branch_model(root: &TempDir) -> (Model, Repo) {
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    let mut m = new_model(std::slice::from_ref(&r), "");
    (m.w, m.h) = (90, 14);
    only_main(&mut m);
    let want = r.path();
    let b = |name: &str, remote: &str, unfetched| Branch {
        name: name.into(),
        remote: remote.into(),
        unfetched,
    };
    m.backend.branches_of = Arc::new(move |dir| {
        assert_eq!(dir, want);
        // Newest first, the way git sorts them.
        Ok(vec![
            b("main", "", false),
            b("fix/timeout", "", false),
            b("feat/login", "origin/feat/login", false),
        ])
    });
    // The remote has one branch the last fetch did not bring, and repeats the
    // ones it did.
    m.backend.remote_branches_of = Arc::new(move |_| {
        (
            vec![
                b("feat/login", "origin/feat/login", true),
                b("main", "origin/main", true),
                b("feat/new", "origin/feat/new", true),
            ],
            None,
        )
    });
    (m, r)
}

/// open_branches presses the branch key and hands the finder git's answer,
/// returning what it asked next: the remotes, for what the last fetch did not
/// bring.
fn open_branches(m: &mut Model) -> Vec<Cmd> {
    let cmds = m.update(key("ctrl+l"));
    assert_eq!(m.mode, Mode::Repos, "the list changed before git answered");
    feed(m, cmds)
}

fn pull_request(
    number: u64,
    title: &str,
    branch: &str,
    draft: bool,
    fork: bool,
    owner: &str,
) -> PullRequest {
    PullRequest {
        number,
        title: title.into(),
        branch: branch.into(),
        draft,
        fork,
        author: owner.into(),
        head_owner: owner.into(),
        state: "OPEN".into(),
        url: format!("https://github.com/acme/alpha/pull/{number}"),
        ..Default::default()
    }
}

/// pr_model is a finder over two repositories; alpha has three open pull
/// requests, one of them already checked out, and gh is stubbed.
fn pr_model(root: &TempDir) -> (Model, Vec<Repo>) {
    let rs = repos(
        &root.path(),
        &["github.com/acme/bravo", "github.com/acme/alpha"],
    );
    let mut m = new_model(&rs, "");
    (m.w, m.h) = (100, 16);
    set_query(&mut m, "alpha");
    m.backend.worktrees_of = Arc::new(|dir| {
        let w = |p: &str, b: &str| Worktree {
            path: p.into(),
            branch: b.into(),
            ..Default::default()
        };
        Ok(vec![w(dir, "main"), w("/tmp/wt/alpha-login", "feat/login")])
    });
    let want = rs[1].path();
    m.backend.prs_of = Arc::new(move |dir| {
        assert_eq!(dir, want);
        // Newest first, the way gh lists them.
        Ok(vec![
            pull_request(9, "Fix typo", "main", false, true, "bob"),
            pull_request(8, "Try a new parser", "exp/parser", true, false, "acme"),
            pull_request(7, "Add login", "feat/login", false, false, "acme"),
        ])
    });
    m.backend.pr_of = Arc::new(|_, n| Ok(pr_in_full(n)));
    (m, rs)
}

/// pr_in_full is what gh pr view answers for number: the listing's fields,
/// a body and a comment.
fn pr_in_full(number: u64) -> PullRequest {
    PullRequest {
        body: "Adds the **login** form.".into(),
        labels: vec!["security".into()],
        comments: vec![repo::Comment {
            author: "carol".into(),
            body: "Looks good.".into(),
        }],
        ..pull_request(number, "Add login", "feat/login", false, false, "acme")
    }
}

/// open_pr_list presses the key and hands the finder gh's answer.
fn open_pr_list(m: &mut Model) {
    let cmds = m.update(key("ctrl+j"));
    assert!(!cmds.is_empty(), "the key did not ask gh");
    assert!(m.busy.contains("asking GitHub"), "{:?}", m.busy);
    feed(m, cmds);
}

fn issue(number: u64, title: &str) -> Issue {
    Issue {
        number,
        title: title.into(),
        state: "OPEN".into(),
        author: "alice".into(),
        labels: vec!["bug".into()],
        url: format!("https://github.com/acme/alpha/issues/{number}"),
        ..Default::default()
    }
}

mod branch;
mod commands;
mod details;
mod issue_list;
mod list;
mod panels;
mod pr;
mod view;
mod worktree;
