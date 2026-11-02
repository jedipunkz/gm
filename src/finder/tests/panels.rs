//! The panels: help and the confirmations.

use super::*;

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
    for want in ["/create <repo>", "/get [flags] <repo>", "/remove"] {
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
    let bravo = rs[1].path();
    m.changed_of = Arc::new(move |p| if p == bravo { 3 } else { 0 });

    let cmds = run_slash(&mut m, "/remove"); // bravo, the bottom row
    assert_eq!(m.over, Overlay::Confirm);
    assert_eq!(m.ask.arg, rs[1].path());
    // The question is up before git has answered, and y waits for the
    // answer: nobody is asked before being told what will be lost.
    let view = view_text(&m);
    assert!(view.contains("checking for uncommitted work"), "{view}");
    assert!(!view.contains("y do it"), "{view}");
    assert!(
        m.update(ch('y')).is_empty(),
        "y was taken before git answered"
    );
    assert_eq!(m.over, Overlay::Confirm);
    feed(&mut m, cmds);
    assert_eq!(m.busy, "");
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
    assert!(m.busy.contains("remov"), "{:?}", m.busy);
    assert!(line_text(&m.help_line(90)).contains("remov"));
    let after = feed(&mut m, cmds);
    assert!(!is_quit(&after) && m.result.action == Action::None);
    assert!(!exists(&rs[1].path()));
    assert_eq!(rows(&m), vec!["github.com/acme/alpha"]);
    assert!(m.note.contains("removed"));
}

// An answer for a question that has since been cancelled and asked again
// about another row is not this question's answer.
#[test]
fn remove_drops_a_stale_answer() {
    let root = TempDir::new();
    let rs = repos(
        &root.path(),
        &["github.com/acme/alpha", "github.com/acme/bravo"],
    );
    let mut m = new_model(&rs, "");
    (m.w, m.h) = (90, 16);
    only_main(&mut m);
    let bravo = rs[1].path();
    m.changed_of = Arc::new(move |p| if p == bravo { 3 } else { 0 });
    let stale = run_slash(&mut m, "/remove"); // bravo, the bottom row
    m.update(ch('n'));
    m.update(key("up"));
    run_slash(&mut m, "/remove");
    feed(&mut m, stale);
    assert_eq!(m.ask.arg, rs[0].path());
    assert!(m.ask.wait.is_some(), "the stale answer unlocked y");
    assert!(!view_text(&m).contains("3 uncommitted"));
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
    let cmds = run_slash(&mut m, "/create acme/bravo");
    assert_eq!(m.over, Overlay::Confirm);
    assert!(m.ask.wait.is_some());
    feed(&mut m, cmds);
    assert_eq!(m.ask.arg, "acme/bravo");
    let view = view_text(&m);
    for want in ["create", "origin https://github.com/acme/bravo"] {
        assert!(view.contains(want), "{want:?}:\n{view}");
    }

    let cmds = m.update(ch('y'));
    assert!(m.busy.contains("creat"), "{:?}", m.busy);
    assert!(line_text(&m.help_line(90)).contains("creat"));
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

// The bug where /create always set an HTTPS origin: --ssh does what it does
// for gm create, and is said in the panel before anything is made.
#[test]
fn confirm_create_over_ssh() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    (m.w, m.h) = (90, 16);
    run_slash(&mut m, "/create --ssh");
    assert!(m.note.contains("needs a repository"), "{:?}", m.note);

    let cmds = run_slash(&mut m, "/create --ssh acme/bravo");
    assert_eq!(m.over, Overlay::Confirm);
    feed(&mut m, cmds);
    assert_eq!(m.ask.arg, "acme/bravo");
    let view = view_text(&m);
    assert!(
        view.contains("origin ssh://git@github.com/acme/bravo"),
        "{view}"
    );

    let cmds = m.update(ch('y'));
    feed(&mut m, cmds);
    let dst = root.join("github.com/acme/bravo");
    assert_eq!(
        repo::git_in(&dst, &["remote", "get-url", "origin"]).unwrap(),
        "ssh://git@github.com/acme/bravo"
    );
}

// /create asks every root first, so the panel never offers to make a second
// copy of a repository that lives under another one.
#[test]
fn create_refuses_a_repository_under_another_root() {
    let base = TempDir::new();
    let (r1, r2) = (base.join("r1"), base.join("r2"));
    let rs = repos(&r2, &["github.com/acme/bravo"]);
    git_repo(&rs[0].path());
    let mut m = Model::new(
        &Tree {
            roots: vec![r1, r2],
        },
        &rs,
        &History::open(""),
        &theme(),
        test_keys(),
    );
    let cmds = run_slash(&mut m, "/create acme/bravo");
    feed(&mut m, cmds);
    assert_eq!(m.over, Overlay::None);
    assert!(m.note.contains(&rs[0].path()), "{:?}", m.note);
}

// The row a fresh clone deserves is the top: the list is ascending by
// frecency, so pushing it to the end gave the least-visited repository the
// best spot until the next restart.
#[test]
fn created_repository_lands_where_its_slot_is() {
    let root = TempDir::new();
    let mut m = new_model(
        &repos(
            &root.path(),
            &["github.com/acme/bravo", "github.com/acme/charlie"],
        ),
        "",
    );
    (m.w, m.h) = (90, 16);
    let cmds = run_slash(&mut m, "/create acme/alpha");
    feed(&mut m, cmds);
    let cmds = m.update(ch('y'));
    feed(&mut m, cmds);

    assert_eq!(
        m.current().unwrap().path,
        root.join("github.com/acme/alpha"),
        "the created row is the selection"
    );
    let rs = rows(&m);
    assert_eq!(rs.first().unwrap(), "github.com/acme/alpha", "{rs:?}");
    assert_eq!(
        rs.last().unwrap(),
        "github.com/acme/charlie",
        "the best match must stay at the bottom: {rs:?}"
    );
}

// PgDown and PgUp move a screen of rows at a time, clamped by the list, and
// the status of the row they land on is loaded like the arrows do.
#[test]
fn page_keys_move_a_screen_at_a_time() {
    let root = TempDir::new();
    let (w, h) = (90, 14); // the list window is 14 - 4 = 10 rows tall
    let names: Vec<String> = (0..26).map(|i| format!("github.com/acme/{i:02}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let rs = repos(&root.path(), &refs);
    let mut m = new_model(&rs, "");
    (m.w, m.h) = (w, h);
    assert_eq!(
        m.cursor,
        m.view.len() - 1,
        "the finder starts at the bottom"
    );

    // Up: three pages tops out at the first row.
    m.update(key("pgup"));
    assert_eq!(m.cursor, m.view.len() - 1 - m.page_rows());
    m.update(key("pgup"));
    assert_eq!(m.cursor, m.view.len() - 1 - m.page_rows() * 2);
    m.update(key("pgup"));
    assert_eq!(m.cursor, 0);
    assert_eq!(m.current().unwrap().label, "github.com/acme/00");

    // Down: three pages bottoms out at the last row.
    m.update(key("pgdown"));
    assert_eq!(m.cursor, m.page_rows());
    m.update(key("pgdown"));
    assert_eq!(m.cursor, m.page_rows() * 2);
    m.update(key("pgdown"));
    assert_eq!(m.cursor, m.view.len() - 1);
    assert_eq!(m.current().unwrap().label, "github.com/acme/25");

    // An empty list: both keys are harmless.
    let mut m = new_model(&[], "");
    m.update(key("pgdown"));
    m.update(key("pgup"));
    assert!(m.current().is_none());
}

// The main worktree is the bottom row, before and after a worktree is made:
// the created one goes at the top, and it is selected.
#[test]
fn created_worktree_keeps_the_main_worktree_last() {
    let root = TempDir::new();
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    let mut m = new_model(std::slice::from_ref(&r), "");
    (m.w, m.h) = (90, 16);
    open_worktrees(&mut m);
    assert_eq!(label(&m), "main", "the main worktree starts at the bottom");

    let cmds = run_slash(&mut m, "/create feat/login");
    assert_eq!(m.over, Overlay::Confirm, "{:?}", m.note);
    assert!(m.ask.wait.is_some());
    feed(&mut m, cmds);
    let cmds = m.update(ch('y'));
    feed(&mut m, cmds);
    assert_eq!(m.mode, Mode::Worktrees, "{:?}", m.note);
    let rs = rows(&m);
    assert_eq!(rs.last().unwrap(), "main", "{rs:?}");
    assert_eq!(rs.first().unwrap(), "feat/login", "{rs:?}");
    assert!(m.current().unwrap().path.contains("feat/login"));
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
    ask_remove(&mut m);
    assert_eq!(m.over, Overlay::Confirm, "{:?}", m.note);
    let view = view_text(&m);
    for want in ["2 worktrees will go too", "feat/login", "fix/timeout"] {
        assert!(view.contains(want), "{want:?}:\n{view}");
    }
    assert!(!view.contains("3 worktrees"));
}

// The worktrees go with --force, so the work in each is named before anyone
// says yes.
#[test]
fn remove_names_the_work_in_the_worktrees() {
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
    m.changed_of = Arc::new(|p| if p == "/tmp/wt/login" { 2 } else { 0 });
    ask_remove(&mut m);
    let view = view_text(&m);
    // /tmp/wt is outside gm's worktree directory for this repository, so each
    // path is named too: nothing else says where the checkouts are. The panel
    // wraps between words, so the assertions stop where a line can break.
    assert!(
        view.contains("feat/login at /tmp/wt/login") && view.contains("changed)"),
        "{view}"
    );
    assert!(
        view.contains("fix/timeout at /tmp/wt/timeout") && !view.contains("fix/timeout ("),
        "{view}"
    );
    // The repository itself is clean here, so nothing claims otherwise.
    assert!(!view.contains("uncommitted"), "{view}");
}

// "gm;/remove" finds a repository and removes it in one keystroke, long
// before the details pane has asked git about it, and a cached answer is as
// old as the moment it was taken. Neither may hide work that would be lost.
#[test]
fn remove_asks_git_rather_than_the_cache() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    let mut m = new_model(&rs, "");
    (m.w, m.h) = (90, 16);
    only_main(&mut m);
    m.changed_of = Arc::new(|_| 4);

    // Not loaded yet.
    let mut fresh = m.clone();
    ask_remove(&mut fresh);
    assert!(fresh.status.is_empty());
    assert!(view_text(&fresh).contains("4 uncommitted changes"));

    // Loaded, but from before the files were changed.
    m.status.insert(rs[0].path(), Status::default());
    ask_remove(&mut m);
    assert!(view_text(&m).contains("4 uncommitted changes"));
}

// In the worktree list the answer also decides --force: without it git
// refuses, and the question the user answered was the wrong one.
#[test]
fn remove_worktree_forces_what_it_warned_about() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    let mut m = new_model(&rs, "");
    m.worktrees_of = Arc::new(|dir| {
        let w = |p: &str, b: &str| Worktree {
            path: p.into(),
            branch: b.into(),
            ..Default::default()
        };
        Ok(vec![w(dir, "main"), w("/tmp/wt/login", "feat/login")])
    });
    m.changed_of = Arc::new(|p| if p == "/tmp/wt/login" { 1 } else { 0 });
    let cmds = m.update(key("ctrl+w"));
    feed(&mut m, cmds);
    assert_eq!(m.mode, Mode::Worktrees);
    m.cursor = m
        .view
        .iter()
        .position(|&i| m.all[i].path == "/tmp/wt/login")
        .unwrap();
    ask_remove(&mut m);
    assert_eq!(m.over, Overlay::Confirm, "{:?}", m.note);
    assert!(m.ask.force);
    assert!(view_text(&m).contains("1 uncommitted changes"));
}

#[test]
fn removing_worktree_prunes_empty_host_directory() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    let r = &rs[0];
    git_repo(&r.path());
    let tree = Tree {
        roots: vec![root.path()],
    };
    let dir = tree.worktree_dir(r, "feat/login");
    repo::add_worktree(&root.path(), &r.path(), &dir, "feat/login").unwrap();

    let mut m = new_model(&rs, "");
    m.repo_at = r.path();
    let msg = answer(vec![m.perform(
        Pending {
            kind: Change::RemoveWorktree,
            mode: Mode::Worktrees,
            repo_at: r.path(),
            dir,
            ..Default::default()
        },
        0,
    )])
    .unwrap();

    assert!(matches!(msg, Msg::Done(Done { err: None, .. })));
    assert!(!exists(&root.join(".worktrees/github.com")));
}

#[test]
fn remove_says_nothing_without_worktrees() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    (m.w, m.h) = (90, 18);
    only_main(&mut m);
    ask_remove(&mut m);
    assert!(!view_text(&m).contains("will go too"));
}
