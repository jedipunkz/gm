//! The view: the finder's layout, its keys and its hints.

use super::*;

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
    // The input box ends at the first border after the prompt; the mode box follows.
    let rest = prompt.split_once('❯').unwrap().1.split('│').next().unwrap();
    assert_eq!(
        rest.trim_matches(|c| c == '│' || c == ' '),
        "",
        "the prompt starts with something"
    );
}

// The box right of the prompt names the list being searched.
#[test]
fn mode_box_names_the_list() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    (m.w, m.h) = (80, 10);
    for (mode, want) in [
        (Mode::Repos, "│repository│"),
        (Mode::Worktrees, "│ worktree │"),
        (Mode::Branches, "│  branch  │"),
        (Mode::Prs, "│    pr    │"),
    ] {
        m.mode = mode;
        let out = view_text(&m);
        let prompt = out.lines().find(|l| l.contains('❯')).unwrap();
        assert!(prompt.ends_with(want), "{mode:?}: {prompt:?}");
    }
}

// The chords that would leave the finder impossible to quit or move around in.
#[test]
fn reserved_keys_are_refused() {
    let keys = test_keys();
    // ctrl-m and ctrl-i are Enter and Tab in a terminal.
    for name in [
        "ctrl-c",
        "ctrl-n",
        "ctrl-p",
        "ctrl-g",
        "ctrl-m",
        "ctrl-i",
        "ctrl-alt-m",
    ] {
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
    let (wt, br, pr, is, rm) = (
        keys.worktree.clone(),
        keys.branch.clone(),
        keys.pr.clone(),
        keys.issue.clone(),
        keys.remote.clone(),
    );
    for k in [
        Keys {
            worktree: wt.clone(),
            branch: br.clone(),
            pr: pr.clone(),
            issue: is.clone(),
            remote: wt.clone(),
        },
        Keys {
            worktree: wt.clone(),
            branch: wt.clone(),
            pr: pr.clone(),
            issue: is.clone(),
            remote: rm.clone(),
        },
        Keys {
            worktree: wt.clone(),
            branch: rm.clone(),
            pr: pr.clone(),
            issue: is.clone(),
            remote: rm.clone(),
        },
        Keys {
            worktree: wt.clone(),
            branch: br.clone(),
            pr: br.clone(),
            issue: is.clone(),
            remote: rm.clone(),
        },
    ] {
        assert!(k.check().is_err(), "the same chord twice: {k:?}");
    }
    assert!(keys.check().is_ok());
}

// gm.toml's keys reach the finder through Keys::from_config: a key it sets is
// that chord, a key it leaves out is the default, and one that does not parse
// is the error the user reads.
#[test]
fn keys_come_from_the_config() {
    let cfg = crate::config::Config {
        worktree_key: "ctrl-t".into(),
        ..Default::default()
    };
    let keys = Keys::from_config(&cfg).unwrap();
    assert_eq!(
        keys.worktree,
        parse_chord("ctrl-t", DEFAULT_WORKTREE_KEY).unwrap()
    );
    let defaults = test_keys();
    assert_eq!(keys.branch, defaults.branch);
    assert_eq!(keys.pr, defaults.pr);
    assert_eq!(keys.issue, defaults.issue);
    assert_eq!(keys.remote, defaults.remote);

    let bad = crate::config::Config {
        pr_key: "ctrl-".into(),
        ..Default::default()
    };
    assert!(Keys::from_config(&bad).is_err());
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
    let cmds = old.update(key("ctrl+w"));
    feed(&mut old, cmds);
    assert_eq!(
        old.mode,
        Mode::Repos,
        "ctrl-w still opened the worktree list"
    );

    let cmds = m.update(key("ctrl+t"));
    feed(&mut m, cmds);
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
    assert!(line_text(&m.help_line(120)).contains("ctrl-alt-b browse"));
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
    open_worktrees(&mut wm);
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
    m.backend.open_url = Arc::new(move |u| sink.lock().unwrap().push(u.to_string()));
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

#[test]
fn get_refuses_the_look_flag() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    for flag in ["-l", "--look", "-l=true", "--look=1"] {
        let mut m = new_model(&rs, "");
        let cmds = run_slash(&mut m, &format!("/get {flag} acme/x"));

        assert!(cmds.is_empty(), "{flag} left the finder");
        assert_eq!(m.over, Overlay::None);
        assert_eq!(m.result.action, Action::None);
        assert_eq!(
            m.note,
            "-l does not apply inside the finder: /get already goes there"
        );
    }
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

    // With nothing left to undo, Esc quits as it always did — with an issue
    // key that is not the default, whose ctrl-[ takes esc for the issue
    // list (esc_opens_the_issue_list below names what that does instead).
    let mut esc_quits = {
        let mut m = m.clone();
        m.keys.issue = parse_chord("", "ctrl-e").unwrap();
        m
    };
    assert!(is_quit(&esc_quits.update(key("esc"))));
}

// The default issue chord is ctrl-[, and esc is the key it arrives as: at
// the repository list, with nothing to undo, it opens the issue list
// instead of quitting — ctrl+c is left quit's key.
#[test]
fn esc_opens_the_issue_list() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    let mut m = new_model(&rs, "");
    m.backend.issues_of = Arc::new(|_| Ok(vec![issue(7, "Add login")]));
    let cmds = m.update(key("esc"));
    assert!(!is_quit(&cmds), "esc quit instead of opening the issues");
    feed(&mut m, cmds);
    assert_eq!(m.mode, Mode::Issues, "{:?}", m.note);
    assert_eq!(rows(&m), ["#7 Add login"]);

    // And the same key takes the list back down.
    m.update(key("esc"));
    assert_eq!(m.mode, Mode::Repos);

    // Deeper lists keep esc's own back-outs: from the issue list it goes to
    // the repositories, which the toggle above is, and a query still clears
    // rather than opening anything.
    let mut m = new_model(&rs, "");
    set_query(&mut m, "alpha");
    assert!(
        !is_quit(&m.update(key("esc"))),
        "esc quit with a query to clear"
    );
    assert_eq!(m.input.value(), "");
}

// The worktree list is backed out of before the filter is.
#[test]
fn esc_leaves_the_worktree_list_first() {
    let root = TempDir::new();
    let (mut m, _) = dirty_model(&root);
    only_main(&mut m);
    let cmds = run_slash(&mut m, "/dirty");
    feed(&mut m, cmds);
    open_worktrees(&mut m);
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

    ask_remove(&mut m);
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
