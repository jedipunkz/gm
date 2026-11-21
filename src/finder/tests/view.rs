use super::*;

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
    let rest = prompt.split_once('❯').unwrap().1.split('│').next().unwrap();
    assert_eq!(
        rest.trim_matches(|c| c == '│' || c == ' '),
        "",
        "the prompt starts with something"
    );
}

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
    m.update(key("ctrl+t"));
    assert_eq!(m.mode, Mode::Repos);
}

#[test]
fn remote_key_in_hints() {
    let root = TempDir::new();
    let m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    assert!(line_text(&m.help_line(120)).contains("ctrl-alt-b browse"));
}

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

    let mut wm = m.clone();
    only_main(&mut wm);
    open_worktrees(&mut wm);
    let buf = screen(&wm);
    assert!(row_text(&buf, buf.area.height - 1).contains("ctrl-w/g/esc repos"));

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
    assert_eq!((m.mode, m.result.action), (Mode::Repos, Action::None));

    opened.lock().unwrap().clear();
    m.status.insert(rs[0].path(), Status::default());
    answer(m.update(key("ctrl+alt+b")));
    assert!(opened.lock().unwrap().is_empty());
}

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

    let mut esc_quits = {
        let mut m = m.clone();
        m.keys.issue = parse_chord("", "ctrl-e").unwrap();
        m
    };
    assert!(is_quit(&esc_quits.update(key("esc"))));
}

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

    m.update(key("esc"));
    assert_eq!(m.mode, Mode::Repos);

    let mut m = new_model(&rs, "");
    set_query(&mut m, "alpha");
    assert!(
        !is_quit(&m.update(key("esc"))),
        "esc quit with a query to clear"
    );
    assert_eq!(m.input.value(), "");
}

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
    assert!(view_text(&m).contains("no match"));
    for k in ["up", "down", "enter"] {
        let mut after = m.clone();
        after.update(key(k));
        assert_eq!(
            after.result.arg, "",
            "{k} chose something out of an empty list"
        );
    }
    run_slash(&mut m, "/remove");
    assert!(
        m.over == Overlay::None && m.note.contains("nothing is selected"),
        "{:?}",
        m.note
    );
}

#[test]
fn probe_waits_for_the_cursor_to_settle() {
    let mut m = new_model(
        &repos("/r", &["github.com/acme/alpha", "github.com/acme/beta"]),
        "",
    );
    let here = m.current().unwrap().path.clone();

    assert!(m.probe("/r/github.com/acme/gone").is_none());
    assert!(m.probing.is_empty());

    assert!(m.probe(&here).is_some());
    assert!(m.probing.contains(&here));
    assert!(m.probe(&here).is_none(), "probed the same row twice");

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
