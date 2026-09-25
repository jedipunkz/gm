//! The finder driven the way the terminal loop drives it: messages in,
//! commands out, and the screen read back from a buffer.

use std::sync::{Arc, Mutex};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;

use super::theme::{ROW_LIFT, blend, rgb};
use super::*;
use crate::config::parse_chord;
use crate::paths;
use crate::repo::{Commit, Ref, RefKind};
use crate::testutil::{TempDir, exists, git, git_repo, mkdir};

fn test_keys() -> Keys {
    Keys {
        worktree: parse_chord("", DEFAULT_WORKTREE_KEY).unwrap(),
        branch: parse_chord("", DEFAULT_BRANCH_KEY).unwrap(),
        pr: parse_chord("", DEFAULT_PR_KEY).unwrap(),
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

fn is_quit(cmds: &[Cmd]) -> bool {
    cmds.iter().any(|c| matches!(c, Cmd::Quit))
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
/// uncommitted work, with the scan stubbed so no git runs.
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
    m.dirty_of = Arc::new(move |_| {
        [
            (paths[0].clone(), true),
            (paths[1].clone(), false),
            (paths[2].clone(), true),
        ]
        .into_iter()
        .collect()
    });
    (m, rs)
}

fn only_main(m: &mut Model) {
    m.worktrees_of = Arc::new(|dir| {
        Ok(vec![Worktree {
            path: dir.into(),
            branch: "main".into(),
            ..Default::default()
        }])
    });
}

// ---- the view -----------------------------------------------------------

// The finder's layout: no header, a bordered prompt, a highlighted selection,
// and the query's characters picked out inside it.
#[test]
fn view_chrome() {
    let root = TempDir::new();
    let mut m = new_model(
        &repos(
            &root.path(),
            &["github.com/acme/alpha", "github.com/acme/bravo"],
        ),
        "",
    );
    (m.w, m.h) = (100, 10);
    set_query(&mut m, "brav");
    let buf = screen(&m);
    let out = text(&buf);
    let th = theme();
    for (what, s) in [
        ("rounded input box", "╭"),
        ("input box bottom", "╰"),
        ("selection marker", "▸"),
        ("divider", "│ "),
    ] {
        assert!(out.contains(s), "the view is missing the {what}:\n{out}");
    }
    assert!(painted(&buf, None, th.border), "the theme's border grey");
    assert!(painted(&buf, None, th.orange), "the matched characters");
    let bg =
        (0..buf.area.height).any(|y| (0..buf.area.width).any(|x| buf[(x, y)].bg == rgb(th.bg_hi)));
    assert!(bg, "the selected row's background");
    assert!(
        !out.lines().next().unwrap().contains("1/2"),
        "the count header should be gone"
    );
}

// A placeholder reads as something the user already typed.
#[test]
fn prompt_starts_empty() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    (m.w, m.h) = (80, 10);
    let out = view_text(&m);
    let prompt = out
        .lines()
        .find(|l| l.contains('❯'))
        .expect("the view has no prompt line");
    let rest = prompt.split_once('❯').unwrap().1;
    assert_eq!(
        rest.trim_matches(|c| c == '│' || c == ' '),
        "",
        "the prompt starts with something"
    );
}

// The chords that would leave the finder impossible to quit or move around in.
#[test]
fn reserved_keys_are_refused() {
    let keys = test_keys();
    for name in ["ctrl-c", "ctrl-n", "ctrl-p"] {
        let c = parse_chord(name, DEFAULT_WORKTREE_KEY).unwrap();
        for set in [
            |k: &mut Keys, c: Chord| k.worktree = c,
            |k: &mut Keys, c: Chord| k.remote = c,
            |k: &mut Keys, c: Chord| k.branch = c,
            |k: &mut Keys, c: Chord| k.pr = c,
        ] {
            let mut k = keys.clone();
            set(&mut k, c.clone());
            assert!(k.check().is_err(), "{name} was accepted");
        }
    }
    // Two actions cannot answer to the same chord.
    let (wt, br, pr, rm) = (
        keys.worktree.clone(),
        keys.branch.clone(),
        keys.pr.clone(),
        keys.remote.clone(),
    );
    for k in [
        Keys {
            worktree: wt.clone(),
            branch: br.clone(),
            pr: pr.clone(),
            remote: wt.clone(),
        },
        Keys {
            worktree: wt.clone(),
            branch: wt.clone(),
            pr: pr.clone(),
            remote: rm.clone(),
        },
        Keys {
            worktree: wt.clone(),
            branch: rm.clone(),
            pr: pr.clone(),
            remote: rm.clone(),
        },
        Keys {
            worktree: wt.clone(),
            branch: br.clone(),
            pr: br.clone(),
            remote: rm.clone(),
        },
    ] {
        assert!(k.check().is_err(), "the same chord twice: {k:?}");
    }
    assert!(keys.check().is_ok());
}

// The two places the configured chord has to reach: the key that opens the
// list, and the hint line that names it.
#[test]
fn worktree_key_is_configurable() {
    let root = TempDir::new();
    let mut keys = test_keys();
    keys.worktree = parse_chord("ctrl-t", DEFAULT_WORKTREE_KEY).unwrap();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    let mut m = Model::new(
        &Tree {
            roots: vec![root.path()],
        },
        &rs,
        &History::open(""),
        &theme(),
        keys,
    );
    (m.w, m.h) = (90, 12);
    only_main(&mut m);

    assert!(line_text(&m.help_line(90)).contains("ctrl-t worktrees"));
    // The old default must no longer do anything.
    let mut old = m.clone();
    old.update(key("ctrl+w"));
    assert_eq!(
        old.mode,
        Mode::Repos,
        "ctrl-w still opened the worktree list"
    );

    m.update(key("ctrl+t"));
    assert_eq!(m.mode, Mode::Worktrees);
    assert!(line_text(&m.help_line(90)).contains("ctrl-t/g/esc repos"));
    // And it toggles back off.
    m.update(key("ctrl+t"));
    assert_eq!(m.mode, Mode::Repos);
}

#[test]
fn remote_key_in_hints() {
    let root = TempDir::new();
    let m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    assert!(line_text(&m.help_line(120)).contains("ctrl-alt-b remote"));
}

// The hints under the prompt name the keys for the list that is up, the key
// names carry their own colour, and a narrow window drops hints instead of
// wrapping.
#[test]
fn help_line() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    (m.w, m.h) = (90, 12);

    let buf = screen(&m);
    let last = buf.area.height - 1;
    let plain = row_text(&buf, last);
    for want in [
        "↑↓ ctrl-p/n move",
        "enter jump",
        "ctrl-w worktrees",
        "esc quit",
    ] {
        assert!(
            plain.contains(want),
            "the hint line is missing {want:?}: {plain:?}"
        );
    }
    assert!(
        painted(&buf, Some(last), theme().blue),
        "the key names are not coloured"
    );

    // In the worktree list the same keys go back instead of out.
    let mut wm = m.clone();
    only_main(&mut wm);
    wm.open_worktrees();
    let buf = screen(&wm);
    assert!(row_text(&buf, buf.area.height - 1).contains("ctrl-w/g/esc repos"));

    // Too narrow for everything: the tail is dropped, nothing wraps.
    let narrow = line_text(&m.help_line(24));
    assert!(
        !narrow.contains("esc"),
        "a hint that does not fit was drawn anyway: {narrow:?}"
    );
    assert!(
        narrow.contains("↑↓ ctrl-p/n move"),
        "the first hint was dropped: {narrow:?}"
    );
}

// The remote key hands the browser an https URL built from the selected
// repository's origin, and does nothing at all when there is no origin.
#[test]
fn open_remote() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    let mut m = new_model(&rs, "");
    let opened = Arc::new(Mutex::new(Vec::<String>::new()));
    let sink = opened.clone();
    m.open_url = Arc::new(move |u| sink.lock().unwrap().push(u.to_string()));
    m.status.insert(
        rs[0].path(),
        Status {
            remote: "git@github.com:acme/alpha.git".into(),
            ..Default::default()
        },
    );

    let cmds = m.update(key("ctrl+alt+b"));
    assert!(!cmds.is_empty(), "the remote key produced no command");
    answer(cmds);
    assert_eq!(
        *opened.lock().unwrap(),
        vec!["https://github.com/acme/alpha".to_string()]
    );
    // The finder stays where it was: this is a side action, not navigation.
    assert_eq!((m.mode, m.result.action), (Mode::Repos, Action::None));

    // A repository whose remote git cannot supply opens nothing.
    opened.lock().unwrap().clear();
    m.status.insert(rs[0].path(), Status::default());
    answer(m.update(key("ctrl+alt+b")));
    assert!(opened.lock().unwrap().is_empty());
}

// Cloning needs the network, a progress bar and sometimes a passphrase, so it
// is the one action that still happens outside.
#[test]
fn get_leaves_the_finder() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    let cmds = run_slash(&mut m, "/get github.com/acme/charlie");
    assert!(is_quit(&cmds));
    assert_eq!(
        m.result,
        Outcome {
            action: Action::Get,
            arg: "github.com/acme/charlie".into()
        }
    );
}

// The way out of /dirty: one key, and only once there is nothing left to
// undo does Esc quit.
#[test]
fn esc_clears_the_filter() {
    let root = TempDir::new();
    let (mut m, rs) = dirty_model(&root);
    let cmds = run_slash(&mut m, "/dirty");
    feed(&mut m, cmds);
    assert_eq!(m.view.len(), 2, "the filter is not on: {:?}", rows(&m));
    assert!(line_text(&m.help_line(90)).contains("esc show all"));

    let cmds = m.update(key("esc"));
    assert!(!is_quit(&cmds), "Esc quit instead of clearing the filter");
    assert!(
        m.view.len() == rs.len() && !m.dirty_only,
        "Esc left the filter on: {:?}",
        rows(&m)
    );
    assert_eq!(m.cursor, m.view.len() - 1);
    assert!(line_text(&m.help_line(90)).contains("esc quit"));

    // With nothing left to undo, Esc quits as it always did.
    assert!(is_quit(&m.update(key("esc"))));
}

// The worktree list is backed out of before the filter is.
#[test]
fn esc_leaves_the_worktree_list_first() {
    let root = TempDir::new();
    let (mut m, _) = dirty_model(&root);
    only_main(&mut m);
    let cmds = run_slash(&mut m, "/dirty");
    feed(&mut m, cmds);
    m.open_worktrees();
    assert_eq!(m.mode, Mode::Worktrees);
    m.update(key("esc"));
    assert_eq!(m.mode, Mode::Repos);
    assert!(
        m.dirty_only,
        "Esc cleared the filter on the way out of the worktree list"
    );
}

// An empty list has no selection, and everything that reads one has to cope.
#[test]
fn removing_the_last_repository_leaves_a_usable_finder() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    mkdir(&paths::join(&rs[0].path(), ".git"));
    let mut m = new_model(&rs, "");
    (m.w, m.h) = (80, 12);

    run_slash(&mut m, "/remove");
    let cmds = m.update(ch('y'));
    feed(&mut m, cmds);
    assert!(m.view.is_empty(), "the list still has {:?}", rows(&m));
    assert!(m.current().is_none());
    // None of these may panic on an empty list.
    assert!(view_text(&m).contains("no match"));
    for k in ["up", "down", "enter"] {
        let mut after = m.clone();
        after.update(key(k));
        assert_eq!(
            after.result.arg, "",
            "{k} chose something out of an empty list"
        );
    }
    // And a command that needs a selection says so.
    run_slash(&mut m, "/remove");
    assert!(
        m.over == Overlay::None && m.note.contains("nothing is selected"),
        "{:?}",
        m.note
    );
}

// A row the cursor swept past is never described, so holding an arrow key
// through a tree of repositories starts no git processes at all.
#[test]
fn probe_waits_for_the_cursor_to_settle() {
    let mut m = new_model(
        &repos("/r", &["github.com/acme/alpha", "github.com/acme/beta"]),
        "",
    );
    let here = m.current().unwrap().path.clone();

    // A probe for a row that is no longer selected does nothing.
    assert!(m.probe("/r/github.com/acme/gone").is_none());
    assert!(m.probing.is_empty());

    // One for the selected row asks git, and only once while the answer is
    // still on its way.
    assert!(m.probe(&here).is_some());
    assert!(m.probing.contains(&here));
    assert!(m.probe(&here).is_none(), "probed the same row twice");

    // And once the answer is in, it is not asked for again.
    m.update(Msg::Status(
        here.clone(),
        Status {
            branch: "main".into(),
            ..Default::default()
        },
    ));
    assert!(!m.probing.contains(&here));
    assert!(m.load_status().is_none());
}

// ---- the list -----------------------------------------------------------

// The core contract: the cursor rests on the most-used repository, and it is
// the last row drawn.
#[test]
fn best_at_bottom() {
    let root = TempDir::new();
    let rs = repos(
        &root.path(),
        &[
            "github.com/acme/alpha",
            "github.com/acme/bravo",
            "github.com/other/charlie",
        ],
    );
    let mut m = new_model(&rs, &rs[1].path()); // bravo is the favourite
    assert_eq!(m.view.len(), 3);
    assert_eq!(m.cursor, 2);
    assert_eq!(label(&m), "github.com/acme/bravo");

    set_query(&mut m, "charlie");
    assert_eq!(rows(&m), vec!["github.com/other/charlie"]);
    assert_eq!(label(&m), "github.com/other/charlie");
}

// Scattered matches across the shared "github.com/user/" prefix must not
// outrank a literal hit in the repository name.
#[test]
fn ranking_prefers_the_repository_name() {
    let root = TempDir::new();
    let rs = repos(
        &root.path(),
        &[
            "github.com/jedipunkz/detect-minecraft-versions",
            "github.com/jedipunkz/miniecs",
            "github.com/jedipunkz/spacex-ipo-checker",
        ],
    );
    let mut m = new_model(&rs, "");
    set_query(&mut m, "miniec");
    assert_eq!(label(&m), "github.com/jedipunkz/miniecs");
    // The highlight sits on the literal "miniec", not scattered across the
    // host and user segments.
    let at = label(&m).find("miniec").unwrap();
    assert_eq!(
        m.matched[&m.view[m.cursor]],
        (at..at + 6).collect::<Vec<_>>()
    );
}

// The unselected row on screen carries the lifted colour, not the comment
// colour the labels still use.
#[test]
fn unselected_rows_are_lifted() {
    let root = TempDir::new();
    let mut m = new_model(
        &repos(
            &root.path(),
            &["github.com/acme/alpha", "github.com/acme/bravo"],
        ),
        "",
    );
    (m.w, m.h) = (80, 10);
    let buf = screen(&m);
    let y = (0..buf.area.height)
        .find(|&y| row_text(&buf, y).contains("acme/alpha"))
        .expect("alpha is not on screen");
    let th = theme();
    assert!(painted(&buf, Some(y), &blend(th.comment, th.fg, ROW_LIFT)));
}

// The rows do not move while a command is typed, so neither does the cursor.
#[test]
fn cursor_survives_a_command() {
    let root = TempDir::new();
    let mut m = new_model(
        &repos(
            &root.path(),
            &[
                "github.com/acme/alpha",
                "github.com/acme/bravo",
                "github.com/acme/charlie",
            ],
        ),
        "",
    );
    m.update(key("up"));
    assert_eq!(m.cursor, m.view.len() - 2);
    let want = label(&m);

    for c in "/he".chars() {
        m.update(ch(c));
        assert_eq!(label(&m), want, "after typing {c:?} the selection moved");
    }
    // Erasing it back to an empty query must not move it either.
    for _ in 0..3 {
        m.update(key("backspace"));
    }
    assert_eq!(label(&m), want);

    // A real query still puts the cursor on the best match.
    m.update(ch('a'));
    assert_eq!(m.cursor, m.view.len() - 1);
}

// Filter down to a repository, clear the box, and run a command on what is
// still selected. Clearing must not throw the selection back to the best
// match.
#[test]
fn find_then_act_on_it() {
    let root = TempDir::new();
    let rs = repos(
        &root.path(),
        &[
            "github.com/jedipunkz/agx",
            "github.com/jedipunkz/gm",
            "github.com/jedipunkz/miniecs",
        ],
    );
    let mut m = new_model(&rs, "");
    typed(&mut m, "agx");
    assert_eq!(label(&m), "github.com/jedipunkz/agx");
    assert!(line_text(&m.help_line(90)).contains("esc clear"));

    // Esc clears the query rather than quitting, and holds the selection.
    let cmds = m.update(key("esc"));
    assert!(!is_quit(&cmds));
    assert_eq!(m.input.value(), "");
    assert_eq!(m.view.len(), rs.len());
    assert_eq!(label(&m), "github.com/jedipunkz/agx");

    // And the command asks about it.
    run_slash(&mut m, "/remove");
    assert!(
        m.over == Overlay::Confirm && m.ask.arg == rs[0].path(),
        "{:?}",
        m.ask
    );

    // With the box empty, Esc quits as it always did.
    assert!(is_quit(&new_model(&rs, "").update(key("esc"))));
}

// /dirty end to end: the scan runs off the UI thread, the list keeps every
// row until the answer lands, and running it again shows everything.
#[test]
fn dirty_filter() {
    let root = TempDir::new();
    let (mut m, rs) = dirty_model(&root);
    let cmds = run_slash(&mut m, "/dirty");
    assert!(!cmds.is_empty(), "/dirty started no scan");
    // Nothing is hidden while the answer is still coming.
    assert_eq!(m.view.len(), rs.len());
    assert!(line_text(&m.help_line(90)).contains("checking"));

    feed(&mut m, cmds);
    assert_eq!(
        rows(&m),
        vec!["github.com/acme/alpha", "github.com/acme/charlie"]
    );
    assert_eq!(m.cursor, m.view.len() - 1);
    assert!(line_text(&m.help_line(90)).contains("dirty only"));

    // A query narrows what is left, rather than bringing the clean ones back.
    set_query(&mut m, "charlie");
    assert_eq!(rows(&m), vec!["github.com/acme/charlie"]);
    set_query(&mut m, "bravo");
    assert!(
        rows(&m).is_empty(),
        "a clean repository came back through the query"
    );

    // Running it again shows everything, and does not scan twice.
    set_query(&mut m, "");
    let cmds = run_slash(&mut m, "/dirty");
    assert!(
        cmds.is_empty(),
        "/dirty scanned again instead of reusing the answer"
    );
    assert_eq!(m.view.len(), rs.len());
    assert!(!line_text(&m.help_line(90)).contains("dirty only"));
}

// It declines in the worktree list rather than filtering something the answer
// does not describe.
#[test]
fn dirty_filter_is_for_repositories() {
    let root = TempDir::new();
    let (mut m, _) = dirty_model(&root);
    only_main(&mut m);
    m.open_worktrees();
    let cmds = run_slash(&mut m, "/dirty");
    assert!(cmds.is_empty() && !m.dirty_only);
    assert!(m.note.contains("repository list"), "{:?}", m.note);
}

// ---- the slash commands -------------------------------------------------

// Repository paths are full of slashes, and only a leading one means a
// command.
#[test]
fn slash_only_starts_a_command_at_the_start() {
    let root = TempDir::new();
    let mut m = new_model(
        &repos(
            &root.path(),
            &["github.com/acme/alpha", "github.com/other/bravo"],
        ),
        "",
    );
    set_query(&mut m, "acme/alpha");
    assert_eq!(rows(&m), vec!["github.com/acme/alpha"]);
    set_query(&mut m, "/help");
    assert_eq!(m.view.len(), 2, "a command filtered the list");
    assert!(command::is_command("/help") && !command::is_command("acme/alpha"));
}

#[test]
fn unknown_command() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    let cmds = run_slash(&mut m, "/nope");
    assert!(!is_quit(&cmds) && m.result.action == Action::None);
    let note = line_text(&m.help_line(90));
    assert!(note.contains("/nope") && note.contains("/help"), "{note:?}");
    // The next keystroke clears it.
    set_query(&mut m, "/n");
    assert_eq!(m.note, "");
}

// The box fills the rest of a command in, and Tab accepts what it offered.
#[test]
fn command_completion() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    typed(&mut m, "/wo");
    let shown: String = m
        .input
        .view("❯ ", m.st.prompt, m.st.text, m.st.suggestion)
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(shown.contains("/worktrees"), "{shown:?}");
    m.update(key("tab"));
    assert_eq!(m.input.value(), "/worktrees");
}

// What to create or clone is not guessed.
#[test]
fn actions_need_their_argument() {
    let root = TempDir::new();
    let m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    for typed in ["/create", "/get", "/create   "] {
        let mut next = m.clone();
        let cmds = run_slash(&mut next, typed);
        assert!(
            !is_quit(&cmds) && next.result.action == Action::None,
            "{typed:?} acted"
        );
        assert!(next.note.contains("<repo>"), "{typed:?}: {:?}", next.note);
    }
}

// Find a repository, then act on it without emptying the box first: a
// semicolon ends the query.
#[test]
fn command_after_a_query() {
    let root = TempDir::new();
    let rs = repos(
        &root.path(),
        &[
            "github.com/acme/alpha",
            "github.com/acme/bravo",
            "github.com/other/charlie",
        ],
    );
    let mut m = new_model(&rs, "");
    for (i, c) in "bravo;/remov".chars().enumerate() {
        m.update(ch(c));
        // The query keeps filtering while the command is typed after it.
        if i >= 4 {
            assert_eq!(
                label(&m),
                "github.com/acme/bravo",
                "after {:?}",
                m.input.value()
            );
        }
    }
    let shown: String = m
        .input
        .view("", m.st.prompt, m.st.text, m.st.suggestion)
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(shown.contains("bravo;/remove"), "{shown:?}");
    m.update(key("tab"));
    assert_eq!(m.input.value(), "bravo;/remove");
    m.update(key("enter"));
    assert!(
        m.over == Overlay::Confirm && m.ask.arg == rs[1].path(),
        "{:?}",
        m.ask
    );
    for (s, want) in [
        ("bravo;", true),
        ("bravo;/help", true),
        ("acme/alpha", false),
    ] {
        assert_eq!(command::is_command(s), want, "{s}");
    }
}

// ---- the panels ---------------------------------------------------------

// Enter on /help opens the command list, it names every command, it swallows
// keys while it is up, and q or Esc closes it.
#[test]
fn help_popup() {
    let root = TempDir::new();
    let mut m = new_model(
        &repos(
            &root.path(),
            &["github.com/acme/alpha", "github.com/acme/bravo"],
        ),
        "",
    );
    (m.w, m.h) = (90, 20);
    run_slash(&mut m, "/help");
    assert_eq!(m.over, Overlay::Help);
    assert_eq!(m.result.action, Action::None);
    assert_eq!(m.input.value(), "", "the command was left in the box");

    let view = view_text(&m);
    for c in command::COMMANDS {
        assert!(
            view.contains(c.name) && view.contains(c.what),
            "the popup does not describe {}:\n{view}",
            c.name
        );
    }
    assert!(view.contains("esc"));

    // Moving is impossible while it is up.
    let mut moved = m.clone();
    moved.update(key("up"));
    assert!(moved.cursor == m.cursor && moved.over == Overlay::Help);

    for k in [ch('q'), key("esc")] {
        let mut closed = m.clone();
        let cmds = closed.update(k);
        assert_eq!(closed.over, Overlay::None);
        assert!(!is_quit(&cmds));
    }
}

#[test]
fn help_shows_arguments() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    (m.w, m.h) = (90, 24);
    m.over = Overlay::Help;
    let view = view_text(&m);
    for want in ["/create <repo>", "/get <repo>", "/remove"] {
        assert!(view.contains(want), "{want:?}:\n{view}");
    }
}

// The question names what will be lost, y does it, the row goes, and the
// finder says so without leaving.
#[test]
fn confirm_remove() {
    let root = TempDir::new();
    let rs = repos(
        &root.path(),
        &["github.com/acme/alpha", "github.com/acme/bravo"],
    );
    for r in &rs {
        mkdir(&paths::join(&r.path(), ".git"));
    }
    let mut m = new_model(&rs, "");
    (m.w, m.h) = (90, 16);
    m.status.insert(
        rs[1].path(),
        Status {
            dirty: 3,
            ..Default::default()
        },
    );

    run_slash(&mut m, "/remove"); // bravo, the bottom row
    assert_eq!(m.over, Overlay::Confirm);
    assert_eq!(m.ask.arg, rs[1].path());
    // Only fold-proof words are looked for on screen: where a long path breaks
    // depends on the width of the temporary directory.
    let view = view_text(&m);
    for want in [
        "remove",
        "3 uncommitted changes",
        "y do it",
        "n cancel",
        "❯",
    ] {
        assert!(view.contains(want), "{want:?}:\n{view}");
    }

    let cmds = m.update(ch('y'));
    assert!(!cmds.is_empty() && m.over == Overlay::None);
    let after = feed(&mut m, cmds);
    assert!(!is_quit(&after) && m.result.action == Action::None);
    assert!(!exists(&rs[1].path()));
    assert_eq!(rows(&m), vec!["github.com/acme/alpha"]);
    assert!(m.note.contains("removed"));
}

#[test]
fn confirm_cancel() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    mkdir(&paths::join(&rs[0].path(), ".git"));
    let m = new_model(&rs, "");
    for k in [ch('n'), key("esc")] {
        let mut asked = m.clone();
        run_slash(&mut asked, "/remove");
        let cmds = asked.update(k);
        assert!(cmds.is_empty());
        assert!(asked.over == Overlay::None && asked.ask.kind == Change::None);
        assert!(exists(&paths::join(&rs[0].path(), ".git")));
        assert_eq!(asked.note, "cancelled");
    }
}

// The repository is made where the reference says, the row is added and
// selected, all without leaving the finder.
#[test]
fn confirm_create() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    (m.w, m.h) = (90, 16);
    run_slash(&mut m, "/create acme/bravo");
    assert_eq!(m.over, Overlay::Confirm);
    assert_eq!(m.ask.arg, "acme/bravo");
    let view = view_text(&m);
    for want in ["create", "origin https://github.com/acme/bravo"] {
        assert!(view.contains(want), "{want:?}:\n{view}");
    }

    let cmds = m.update(ch('y'));
    feed(&mut m, cmds);
    let dst = root.join("github.com/acme/bravo");
    assert!(exists(&paths::join(&dst, ".git")));
    assert_eq!(
        repo::git_in(&dst, &["remote", "get-url", "origin"]).unwrap(),
        "https://github.com/acme/bravo"
    );
    assert_eq!(m.current().unwrap().path, dst);
    assert!(m.note.contains("created"));
}

// The worktrees go with the repository, so the question names them before
// anyone says yes; the repository is not one of its own worktrees.
#[test]
fn remove_says_the_worktrees_go_too() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    (m.w, m.h) = (90, 18);
    m.worktrees_of = Arc::new(|dir| {
        let w = |p: &str, b: &str| Worktree {
            path: p.into(),
            branch: b.into(),
            ..Default::default()
        };
        Ok(vec![
            w(dir, "main"),
            w("/tmp/wt/login", "feat/login"),
            w("/tmp/wt/timeout", "fix/timeout"),
        ])
    });
    run_slash(&mut m, "/remove");
    assert_eq!(m.over, Overlay::Confirm, "{:?}", m.note);
    let view = view_text(&m);
    for want in ["2 worktrees will go too", "feat/login", "fix/timeout"] {
        assert!(view.contains(want), "{want:?}:\n{view}");
    }
    assert!(!view.contains("3 worktrees"));
}

#[test]
fn remove_says_nothing_without_worktrees() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    (m.w, m.h) = (90, 18);
    only_main(&mut m);
    run_slash(&mut m, "/remove");
    assert!(!view_text(&m).contains("will go too"));
}

// ---- the details pane ---------------------------------------------------

// A label sits on its own line above its value, and a value longer than the
// pane is folded, never cut.
#[test]
fn info_pane_stacks_and_wraps() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    let mut m = new_model(&rs, "");
    m.status.insert(
        rs[0].path(),
        Status {
            remote: "https://github.com/acme/alpha".into(),
            branch: "main".into(),
            commits: vec![Commit {
                hash: "abc1234".into(),
                subject: "a subject long enough to need two lines in the pane".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    let w = 30;
    let joined = lines_text(&m.info_lines(w));
    for label in [
        "repository",
        "path",
        "remote",
        "branch",
        "status",
        "visits",
        "last commit",
    ] {
        assert!(
            joined.contains(&format!("\n{label}\n")) || joined.starts_with(&format!("{label}\n")),
            "{label:?} is not on a line of its own:\n{joined}"
        );
    }
    for l in joined.lines() {
        assert!(l.chars().count() <= w, "{l:?} is wider than {w}");
    }
    assert!(joined.contains("https://github.com/acme"));
    // A commit folds too, with its continuations indented.
    assert!(
        joined.contains("abc1234") && joined.contains(&format!("\n{}", info::COMMIT_INDENT)),
        "{joined}"
    );
    // Nothing is spaced apart: a blank line costs a row of the commits.
    assert!(!joined.contains("\n\n"), "{joined}");
}

// The hash, HEAD, a local branch, a remote-tracking branch and a tag each get
// their own colour, the way `git log --oneline --decorate` does.
#[test]
fn commit_line_colours() {
    let root = TempDir::new();
    let m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    let th = theme();
    let r = |name: &str, kind| Ref {
        name: name.into(),
        kind,
    };
    let lines = m.commit_lines(
        &Commit {
            hash: "b1b7b91".into(),
            refs: vec![
                r("HEAD", RefKind::Head),
                r("feat/x", RefKind::Local),
                r("origin/feat/x", RefKind::Remote),
                r("tag: v1.0", RefKind::Tag),
            ],
            subject: "refactor: name the flag".into(),
        },
        120,
    );
    assert_eq!(lines.len(), 1, "a line that fits should not fold");
    for (what, hex) in [
        ("hash", blend(th.blue, th.comment, 0.35)),
        ("HEAD", th.cyan.to_string()),
        ("local branch", th.blue.to_string()),
        ("remote branch", blend(th.blue, th.comment, 0.5)),
        ("subject", blend(th.comment, th.fg, ROW_LIFT)),
    ] {
        assert!(
            line_painted(&lines[0], &hex),
            "the {what} is not painted {hex}"
        );
    }
    assert_eq!(
        line_text(&lines[0]),
        "b1b7b91 (HEAD, feat/x, origin/feat/x, tag: v1.0) refactor: name the flag"
    );
}

// A long commit folds instead of being cut: no text is lost, no line is wider
// than the pane, and the continuations are indented.
#[test]
fn commit_line_wraps() {
    let root = TempDir::new();
    let m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    let c = Commit {
        hash: "b1b7b91".into(),
        refs: vec![Ref {
            name: "origin/a-long-branch-name".into(),
            kind: RefKind::Remote,
        }],
        subject: "a subject far too long for the pane to hold in one line".into(),
    };
    let whole = "b1b7b91 (origin/a-long-branch-name) a subject far too long for the pane to hold in one line";
    let squash = |s: &str| s.replace(' ', "");
    for w in [80, 40, 20, 8, 3] {
        let lines = m.commit_lines(&c, w);
        let mut words = String::new();
        for (i, l) in lines.iter().enumerate() {
            let plain = line_text(l);
            assert!(plain.chars().count() <= w, "width {w}, line {i}: {plain:?}");
            if i > 0 {
                assert!(
                    plain.starts_with(info::COMMIT_INDENT),
                    "width {w}, line {i} is not indented"
                );
            }
            words.push_str(plain.strip_prefix(info::COMMIT_INDENT).unwrap_or(&plain));
        }
        // Folding must not lose or duplicate anything.
        assert_eq!(squash(&words), squash(whole), "width {w}");
    }
    // The fold keeps the colours.
    let lines = m.commit_lines(&c, 24);
    assert!(lines.len() >= 2);
    let th = theme();
    assert!(line_painted(&lines[0], &blend(th.blue, th.comment, 0.35)));
}

// The field names are the quiet half of the pane: the comment colour in bold,
// against values that keep the theme's own colours.
#[test]
fn pane_labels_stand_out() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    let mut m = new_model(&rs, "");
    m.status.insert(
        rs[0].path(),
        Status {
            remote: "https://github.com/acme/alpha".into(),
            branch: "main".into(),
            ..Default::default()
        },
    );
    let lines = m.info_lines(40);
    let label = lines
        .iter()
        .find(|l| line_text(l) == "path")
        .expect("no path label");
    let th = theme();
    assert!(line_painted(label, th.comment));
    assert!(
        label
            .spans
            .iter()
            .all(|s| s.style.add_modifier.contains(Modifier::BOLD)),
        "the label is not bold"
    );
    for hex in [th.fg, th.green, th.cyan, th.magenta] {
        assert!(
            !line_painted(label, hex),
            "the label carries the value colour {hex}"
        );
    }
}

// Worktrees in the pane are how a row says whether the worktree key is worth
// pressing on it; a repository without any says nothing at all.
#[test]
fn worktrees_in_the_details_pane() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    let mut m = new_model(&rs, "");
    let path = rs[0].path();
    let pane = |m: &Model| lines_text(&m.info_lines(40));

    m.status.insert(
        path.clone(),
        Status {
            branch: "main".into(),
            ..Default::default()
        },
    );
    assert!(!pane(&m).contains("worktrees"));

    let now = repo::now();
    let wt = |name: &str, ago: i64| Worktree {
        path: format!("{}/.worktrees/{name}", root.path()),
        branch: name.into(),
        committed_at: now - ago,
        ..Default::default()
    };
    let detached = Worktree {
        path: format!("{}/.worktrees/b", root.path()),
        head: "abc1234def".into(),
        ..Default::default()
    };
    m.status.insert(
        path.clone(),
        Status {
            branch: "main".into(),
            worktrees: vec![wt("feat/login", 3600), detached],
            ..Default::default()
        },
    );
    let got = pane(&m);
    assert!(got.contains("\nlast 2 worktrees\n"), "{got}");
    // A detached checkout has no branch to name it, so it goes by its hash.
    assert!(
        got.contains("feat/login") && got.contains("abc1234"),
        "{got}"
    );
    assert!(got.contains("1h ago"), "{got}");

    // Newest first, and only three.
    m.status.insert(
        path,
        Status {
            branch: "main".into(),
            worktrees: vec![
                wt("stale", 30 * 86400),
                wt("newest", 60),
                wt("mid", 2 * 86400),
                wt("second", 3600),
            ],
            ..Default::default()
        },
    );
    let got = pane(&m);
    assert!(got.contains("\nlast 3 of 4 worktrees\n"), "{got}");
    assert!(!got.contains("stale"), "{got}");
    assert!(
        got.find("newest").unwrap() < got.find("second").unwrap(),
        "{got}"
    );
}

// ---- the worktree list --------------------------------------------------

// The list replaces the repositories, the main worktree rests under the
// cursor, typing filters it, Enter yields that worktree's path, and Esc puts
// the repository list back untouched.
#[test]
fn worktree_mode() {
    let root = TempDir::new();
    let rs = repos(
        &root.path(),
        &["github.com/acme/alpha", "github.com/acme/bravo"],
    );
    let mut m = new_model(&rs, "");
    set_query(&mut m, "bravo");
    let want = rs[1].path();
    m.worktrees_of = Arc::new(move |dir| {
        assert_eq!(dir, want, "asked for the wrong repository's worktrees");
        let w = |p: &str, b: &str| Worktree {
            path: p.into(),
            branch: b.into(),
            ..Default::default()
        };
        Ok(vec![
            w(dir, "main"),
            w("/tmp/wt/bravo-login", "feat/login"),
            w("/tmp/wt/bravo-fix", "fix/timeout"),
        ])
    });

    m.open_worktrees();
    assert_eq!(m.mode, Mode::Worktrees);
    assert_eq!(m.view.len(), 3);
    assert_eq!(label(&m), "main");
    assert_eq!(
        m.input.value(),
        "",
        "the repository query leaked into the worktree list"
    );

    set_query(&mut m, "login");
    assert_eq!(label(&m), "feat/login");
    assert_eq!(m.current().unwrap().path, "/tmp/wt/bravo-login");

    m.restore();
    assert_eq!(m.mode, Mode::Repos);
    assert_eq!(m.input.value(), "bravo");
    assert_eq!(label(&m), "github.com/acme/bravo");
}

// Ctrl-W toggles it off, Ctrl-G closes it, and Esc does too. Only Esc quits
// gm, and only from the repository list.
#[test]
fn worktree_mode_keys_go_back() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    for k in ["ctrl+w", "ctrl+g", "esc"] {
        let mut m = new_model(&rs, "");
        only_main(&mut m);
        m.open_worktrees();
        assert_eq!(m.mode, Mode::Worktrees);
        let cmds = m.update(key(k));
        assert_eq!(m.mode, Mode::Repos, "{k}");
        assert!(
            !is_quit(&cmds),
            "{k} quit gm instead of closing the worktree list"
        );
    }
    assert!(is_quit(&new_model(&rs, "").update(key("esc"))));
    assert!(!is_quit(&new_model(&rs, "").update(key("ctrl+g"))));
}

// When git cannot answer, the finder stays usable rather than emptying itself.
#[test]
fn worktree_mode_leaves_the_list_alone_on_error() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    m.worktrees_of = Arc::new(|_| Err("not a git repository".into()));
    m.open_worktrees();
    assert!(m.mode == Mode::Repos && m.view.len() == 1);
}

// Against real git: the branch is checked out where gm says it belongs, the row
// appears, and removing it takes both away.
#[test]
fn worktree_create_and_remove() {
    let root = TempDir::new();
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    let mut m = new_model(std::slice::from_ref(&r), "");
    (m.w, m.h) = (90, 18);
    m.open_worktrees(); // the real repo::worktrees, not a stub
    assert!(m.mode == Mode::Worktrees && m.view.len() == 1);

    run_slash(&mut m, "/create feat/login");
    assert_eq!(m.over, Overlay::Confirm, "{:?}", m.note);
    assert!(m.ask.dir.contains(repo::WORKTREE_ROOT), "{:?}", m.ask.dir);
    let view = view_text(&m);
    for want in ["create worktree", "feat/login, new branch"] {
        assert!(view.contains(want), "{want:?}:\n{view}");
    }

    let cmds = m.update(ch('y'));
    feed(&mut m, cmds);
    let dir = root.join(&format!(
        "{}/github.com/acme/alpha/feat/login",
        repo::WORKTREE_ROOT
    ));
    assert!(
        exists(&paths::join(&dir, ".git")),
        "no worktree at {dir}: {:?}",
        m.note
    );
    assert!(repo::branch_exists(&r.path(), "feat/login"));
    assert_eq!(rows(&m).len(), 2);
    assert_eq!(m.current().unwrap().path, dir);
    // A worktree must not show up as a repository.
    assert_eq!(repo::find_repos(&root.path()), vec![r.path()]);

    // Now take it away.
    run_slash(&mut m, "/remove");
    assert_eq!(m.over, Overlay::Confirm, "{:?}", m.note);
    let cmds = m.update(ch('y'));
    feed(&mut m, cmds);
    assert!(!exists(&dir));
    assert_eq!(rows(&m).len(), 1);
    assert!(m.note.contains("removed"), "{:?}", m.note);
}

#[test]
fn worktree_create_refuses_a_duplicate() {
    let root = TempDir::new();
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    mkdir(&root.join(&format!(
        "{}/github.com/acme/alpha/feat/login",
        repo::WORKTREE_ROOT
    )));
    let mut m = new_model(std::slice::from_ref(&r), "");
    m.open_worktrees();
    run_slash(&mut m, "/create feat/login");
    assert_eq!(m.over, Overlay::None);
    assert!(m.note.contains("already exists"), "{:?}", m.note);
}

// A name that is not a branch name is refused under the prompt, before any
// directory is made.
#[test]
fn worktree_create_refuses_a_bogus_branch() {
    let root = TempDir::new();
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    let mut m = new_model(std::slice::from_ref(&r), "");
    m.open_worktrees();
    let cmds = run_slash(&mut m, "/create ../../escaped");
    assert!(cmds.is_empty() && m.over == Overlay::None);
    assert!(m.note.contains("not a branch name"), "{:?}", m.note);
    assert!(!exists(&root.join(repo::WORKTREE_ROOT)));
}

// The top checkout is the repository itself, and git will not remove it
// either.
#[test]
fn remove_refuses_the_main_worktree() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    only_main(&mut m);
    m.open_worktrees();
    let cmds = run_slash(&mut m, "/remove");
    assert!(!is_quit(&cmds) && m.over == Overlay::None);
    assert!(m.note.contains("repository itself"), "{:?}", m.note);
}

// ---- the branch list ----------------------------------------------------

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
    m.branches_of = Arc::new(move |dir| {
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
    m.remote_branches_of = Arc::new(move |_| {
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

// The newest branch rests under the cursor, a remote-only branch reads as the
// remote's, Enter on a branch that is already checked out goes there, and the
// key toggles the list off.
#[test]
fn branch_mode() {
    let root = TempDir::new();
    let (mut m, r) = branch_model(&root);
    let cmds = m.update(key("ctrl+l"));
    assert_eq!(m.mode, Mode::Branches);
    assert_eq!(rows(&m).join(" "), "origin/feat/login fix/timeout main");
    feed(&mut m, cmds);
    let hints = line_text(&m.help_line(90));
    assert!(
        hints.contains("enter check out") && hints.contains("ctrl-l/g/esc repos"),
        "{hints:?}"
    );
    assert!(lines_text(&m.info_lines(40)).contains("github.com/acme/alpha"));

    let mut jumped = m.clone();
    let cmds = jumped.update(key("enter"));
    assert!(is_quit(&cmds));
    assert_eq!(
        jumped.result,
        Outcome {
            action: Action::Jump,
            arg: r.path()
        }
    );

    let mut back = m.clone();
    back.update(key("ctrl+l"));
    assert_eq!(back.mode, Mode::Repos);
    let mut other = m.clone();
    other.update(key("ctrl+w"));
    assert_eq!(other.mode, Mode::Worktrees);
    let cmds = m.update(key("esc"));
    assert!(m.mode == Mode::Repos && !is_quit(&cmds));
}

#[test]
fn branches_command() {
    let root = TempDir::new();
    let (mut m, _) = branch_model(&root);
    run_slash(&mut m, "/branches");
    assert_eq!(m.mode, Mode::Branches);
}

// A worktree is made for a branch that has none, against real git, and the
// finder is left for it once git is done.
#[test]
fn branch_check_out() {
    let root = TempDir::new();
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    git(&r.path(), &["branch", "fix/timeout"]);

    let mut m = new_model(std::slice::from_ref(&r), "");
    m.update(key("ctrl+l"));
    typed(&mut m, "timeout");
    assert_eq!(label(&m), "fix/timeout");
    let cmds = m.update(key("enter"));
    let msg = answer(cmds).expect("Enter did not start making the worktree");
    assert!(matches!(msg, Msg::Done(_)));
    let cmds = m.update(msg);
    let dir = root.join(&format!(
        "{}/github.com/acme/alpha/fix/timeout",
        repo::WORKTREE_ROOT
    ));
    assert!(exists(&paths::join(&dir, ".git")), "no worktree at {dir}");
    assert!(is_quit(&cmds), "the finder stayed open: {:?}", m.note);
    assert_eq!(
        m.result,
        Outcome {
            action: Action::Jump,
            arg: dir
        }
    );
}

// What only the remote has goes at the top of the list, once, without moving
// the selection.
#[test]
fn remote_branches_join_the_list() {
    let root = TempDir::new();
    let (mut m, _) = branch_model(&root);
    let cmds = m.update(key("ctrl+l"));
    assert!(m.busy.contains("asking the remotes"), "{:?}", m.busy);
    let before = label(&m);
    let Some(Msg::RemoteBranches(msg)) = answer(cmds) else {
        panic!("no answer from the remotes")
    };
    let again = RemoteBranches {
        path: msg.path.clone(),
        branches: msg.branches.clone(),
        err: None,
    };

    m.update(Msg::RemoteBranches(msg));
    assert_eq!(m.busy, "");
    let want = "origin/feat/new origin/feat/login fix/timeout main";
    assert_eq!(rows(&m).join(" "), want);
    assert_eq!(label(&m), before);

    // The same answer again adds nothing.
    m.update(Msg::RemoteBranches(again));
    assert_eq!(rows(&m).join(" "), want);

    set_query(&mut m, "new");
    assert!(lines_text(&m.info_lines(60)).contains("not fetched yet"));
}

// The local list stays when a remote cannot be reached, and the remote is
// named under the prompt; an answer for a list that has since closed is
// dropped.
#[test]
fn remote_branches_say_why_not() {
    let root = TempDir::new();
    let (mut m, _) = branch_model(&root);
    m.remote_branches_of = Arc::new(|_| {
        (
            vec![],
            Some("origin: Could not read from remote repository.".into()),
        )
    });
    let cmds = m.update(key("ctrl+l"));
    feed(&mut m, cmds);
    assert!(
        m.view.len() == 3 && m.note.contains("origin: Could not read"),
        "{:?} {:?}",
        rows(&m),
        m.note
    );

    let (mut m, _) = branch_model(&root);
    let cmds = m.update(key("ctrl+l"));
    m.update(key("ctrl+l"));
    feed(&mut m, cmds);
    assert!(m.mode == Mode::Repos && m.view.len() == 1);
}

// A branch the clone has never seen is fetched and checked out, against real
// git.
#[test]
fn unfetched_branch_check_out() {
    let root = TempDir::new();
    let tmp = TempDir::new();
    let upstream = tmp.join("upstream");
    git_repo(&upstream);
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    mkdir(&paths::dir(&r.path()));
    git(&tmp.path(), &["clone", "-q", &upstream, &r.path()]);
    git(&upstream, &["branch", "feat/new"]);

    let mut m = new_model(std::slice::from_ref(&r), "");
    let cmds = m.update(key("ctrl+l"));
    feed(&mut m, cmds);
    typed(&mut m, "new");
    assert_eq!(label(&m), "origin/feat/new", "{:?}", rows(&m));
    let cmds = m.update(key("enter"));
    assert!(m.busy.contains("fetching origin/feat/new"), "{:?}", m.busy);
    let cmds = feed(&mut m, cmds);
    let dir = root.join(&format!(
        "{}/github.com/acme/alpha/feat/new",
        repo::WORKTREE_ROOT
    ));
    assert!(
        exists(&paths::join(&dir, ".git")),
        "no worktree at {dir}: {:?}",
        m.note
    );
    assert!(is_quit(&cmds) && m.result.arg == dir);
}

// ---- the pull request list ----------------------------------------------

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
    m.worktrees_of = Arc::new(|dir| {
        let w = |p: &str, b: &str| Worktree {
            path: p.into(),
            branch: b.into(),
            ..Default::default()
        };
        Ok(vec![w(dir, "main"), w("/tmp/wt/alpha-login", "feat/login")])
    });
    let want = rs[1].path();
    m.prs_of = Arc::new(move |dir| {
        assert_eq!(dir, want);
        // Newest first, the way gh lists them.
        Ok(vec![
            pull_request(9, "Fix typo", "main", false, true, "bob"),
            pull_request(8, "Try a new parser", "exp/parser", true, false, "acme"),
            pull_request(7, "Add login", "feat/login", false, false, "acme"),
        ])
    });
    (m, rs)
}

/// open_pr_list presses the key and hands the finder gh's answer.
fn open_pr_list(m: &mut Model) {
    let cmds = m.update(key("ctrl+j"));
    assert!(!cmds.is_empty(), "the key did not ask gh");
    assert!(m.busy.contains("asking GitHub"), "{:?}", m.busy);
    feed(m, cmds);
}

// Newest at the bottom, drafts marked, a checked-out pull request goes to its
// worktree, and a fork's main is not mistaken for the repository's own.
#[test]
fn pr_mode() {
    let root = TempDir::new();
    let (mut m, rs) = pr_model(&root);
    open_pr_list(&mut m);
    assert_eq!(m.mode, Mode::Prs, "{:?}", m.note);
    assert_eq!(
        rows(&m).join("|"),
        "#7 Add login|#8 [draft] Try a new parser|#9 Fix typo"
    );
    let hints = line_text(&m.help_line(100));
    assert!(
        hints.contains("enter check out") && hints.contains("ctrl-j/g/esc repos"),
        "{hints:?}"
    );
    let pane = lines_text(&m.info_lines(40));
    for s in [rs[1].rel.as_str(), "bob:main", "none yet"] {
        assert!(pane.contains(s), "{s:?}:\n{pane}");
    }
    // The fork's main is the selected row, and has no worktree of its own.
    assert_eq!(m.current().unwrap().path, "");

    let mut login = m.clone();
    set_query(&mut login, "login");
    let cmds = login.update(key("enter"));
    assert!(is_quit(&cmds));
    assert_eq!(
        login.result,
        Outcome {
            action: Action::Jump,
            arg: "/tmp/wt/alpha-login".into()
        }
    );

    m.update(key("ctrl+j"));
    assert_eq!(m.mode, Mode::Repos);
    assert_eq!(label(&m), rs[1].rel);
}

#[test]
fn prs_command() {
    let root = TempDir::new();
    let (mut m, _) = pr_model(&root);
    let cmds = run_slash(&mut m, "/prs");
    assert!(!cmds.is_empty(), "/prs did not ask gh");
    feed(&mut m, cmds);
    assert_eq!(m.mode, Mode::Prs);
}

// gh is slow, and an answer about a repository the cursor has left must not
// replace the list.
#[test]
fn pr_mode_drops_a_stale_answer() {
    let root = TempDir::new();
    let (mut m, _) = pr_model(&root);
    let cmds = m.update(key("ctrl+j"));
    set_query(&mut m, "bravo");
    feed(&mut m, cmds);
    assert_eq!(m.mode, Mode::Repos);
}

// The repository list stays when gh fails or has nothing, and the reason is
// under the prompt.
#[test]
fn pr_mode_says_why_not() {
    let root = TempDir::new();
    for (res, note) in [
        (
            Err("gh is not installed: pull requests come from the GitHub CLI".into()),
            "gh is not installed",
        ),
        (Ok(vec![]), "no open pull requests"),
    ] {
        let (mut m, _) = pr_model(&root);
        let res: crate::Result<Vec<PullRequest>> = res;
        m.prs_of = Arc::new(move |_| res.clone());
        open_pr_list(&mut m);
        assert!(
            m.mode == Mode::Repos && m.note.contains(note),
            "{:?}",
            m.note
        );
    }
}

// A stand-in gh makes the worktree, and the finder is left for it.
#[test]
fn pr_check_out() {
    use std::os::unix::fs::PermissionsExt;
    let root = TempDir::new();
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    let bin = TempDir::new();
    // gh pr checkout <n> --worktree <dir>
    let gh = bin.join("gh");
    std::fs::write(
        &gh,
        "#!/bin/sh\nexec git worktree add -q -b \"pr-$3\" \"$5\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();

    let mut m = new_model(std::slice::from_ref(&r), "");
    m.gh = gh;
    m.prs_of = Arc::new(|_| {
        Ok(vec![pull_request(
            7,
            "Add login",
            "feat/login",
            false,
            false,
            "acme",
        )])
    });
    open_pr_list(&mut m);
    let cmds = m.update(key("enter"));
    let msg =
        answer(cmds).unwrap_or_else(|| panic!("Enter did not start the checkout: {:?}", m.note));
    assert!(matches!(msg, Msg::Done(_)));
    let cmds = m.update(msg);
    let dir = root.join(&format!(
        "{}/github.com/acme/alpha/feat/login",
        repo::WORKTREE_ROOT
    ));
    assert!(
        exists(&paths::join(&dir, ".git")),
        "no worktree at {dir}: {:?}",
        m.note
    );
    assert!(is_quit(&cmds));
    assert_eq!(
        m.result,
        Outcome {
            action: Action::Jump,
            arg: dir
        }
    );
}

// What gm waits on stays under the prompt, with a spinner, until the answer
// arrives, and the spinner stops once it has.
#[test]
fn busy_spinner() {
    let root = TempDir::new();
    let (mut m, _) = pr_model(&root);
    let cmds = m.update(key("ctrl+j"));
    let line = line_text(&m.help_line(100));
    assert!(
        line.contains("asking GitHub about github.com/acme/alpha… 0s"),
        "{line:?}"
    );
    assert!(line.starts_with(input::SPINNER_FRAMES[0]), "{line:?}");

    // A keystroke answers nothing, so the wait stays on screen.
    let mut typing = m.clone();
    typing.update(ch('x'));
    assert_ne!(typing.busy, "");

    // The spinner keeps ticking while busy.
    let mut ticking = m.clone();
    assert!(!ticking.update(Msg::Spin(ticking.spin.tag)).is_empty());

    feed(&mut m, cmds);
    assert_eq!(m.busy, "");
    assert!(
        m.update(Msg::Spin(m.spin.tag)).is_empty(),
        "the spinner kept ticking with nothing running"
    );
}

// A terminal too small for the layout clips it; nothing may panic drawing it,
// with or without a panel up.
#[test]
fn tiny_terminals_do_not_panic() {
    let root = TempDir::new();
    let mut m = new_model(
        &repos(
            &root.path(),
            &["github.com/acme/alpha", "github.com/acme/bravo"],
        ),
        "",
    );
    for over in [Overlay::None, Overlay::Help] {
        m.over = over;
        for w in 0..30 {
            for h in 0..12 {
                (m.w, m.h) = (w, h);
                screen(&m);
            }
        }
    }
}
