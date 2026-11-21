use super::*;

fn detail_issue(numbers_and_bodies: &[(u64, &str)]) -> Result<Issue> {
    numbers_and_bodies
        .last()
        .map(|(n, body)| Issue {
            number: *n,
            title: "Add login".into(),
            state: "OPEN".into(),
            author: "alice".into(),
            labels: vec!["bug".into(), "ui".into()],
            assignees: vec![],
            url: format!("https://github.com/acme/alpha/issues/{n}"),
            body: (*body).into(),
            comments: vec![repo::Comment {
                author: "bob".into(),
                body: "Reproduced here.".into(),
            }],
        })
        .ok_or("no such issue".into())
}

fn issue_model(root: &TempDir) -> (Model, Vec<Repo>) {
    let rs = repos(
        &root.path(),
        &["github.com/acme/bravo", "github.com/acme/alpha"],
    );
    let mut m = new_model(&rs, "");
    m.keys.issue = parse_chord("ctrl-e", "ctrl-e").unwrap();
    (m.w, m.h) = (100, 30);
    set_query(&mut m, "alpha");
    let (list, at) = (
        vec![issue(7, "Add login"), issue(9, "Crash on start")],
        rs[1].path(),
    );
    m.backend.issues_of = Arc::new(move |dir| {
        assert_eq!(dir, at);
        Ok(list.clone())
    });
    (m, rs)
}

fn open_issue_list(m: &mut Model) {
    let cmds = m.update(key("ctrl+e"));
    assert!(!cmds.is_empty(), "the key did not ask gh");
    assert!(m.busy.contains("asking GitHub"), "{:?}", m.busy);
    feed(m, cmds);
}

fn open_issue_panel(m: &mut Model, detail: Issue) {
    m.backend.issue_of = Arc::new(move |_, _| Ok(detail.clone()));
    let cmds = m.update(key("enter"));
    assert!(m.busy.contains("viewing #7"), "{:?}", m.busy);
    feed(m, cmds);
    assert!(
        m.over == Overlay::Detail,
        "the panel did not open: {:?}",
        m.note
    );
}

#[test]
fn issue_mode() {
    let root = TempDir::new();
    let (mut m, rs) = issue_model(&root);
    open_issue_list(&mut m);
    assert_eq!(m.mode, Mode::Issues, "{:?}", m.note);
    assert_eq!(rows(&m).join("|"), "#9 Crash on start|#7 Add login");

    let hints = line_text(&m.help_line(140));
    assert!(
        hints.contains("enter view") && hints.contains("esc repos"),
        "{hints:?}"
    );
    let pane = lines_text(&m.info_lines(40));
    for s in [rs[1].rel.as_str(), "alice", "#7 Add login", "bug"] {
        assert!(pane.contains(s), "{s:?}:\n{pane}");
    }

    open_issue_panel(
        &mut m,
        detail_issue(&[(
            7,
            "The form hangs. As does everything else for a long time after.",
        )])
        .unwrap(),
    );
    let panel = text(&screen(&m));
    for s in ["Add login", "The form hangs.", "Reproduced here."] {
        assert!(panel.contains(s), "{s:?}:\n{panel}");
    }
    assert!(panel.contains('╭'), "{panel}");
    assert!(panel.contains("─ bob "), "{panel}");
    assert!(panel.contains('╯'), "{panel}");
    let rail = panel
        .lines()
        .find(|l| l.contains("Reproduced here."))
        .expect("the comment's row is not drawn");
    assert!(rail.trim_start().starts_with("│ "), "{rail:?}");

    m.update(key("q"));
    assert!(m.over == Overlay::None && m.detail.is_none());
    assert_eq!(m.mode, Mode::Issues);
    m.update(key("ctrl+e"));
    assert_eq!(m.mode, Mode::Repos);
}

#[test]
fn issue_row_keeps_its_number_and_the_pane_its_body() {
    let root = TempDir::new();
    let (mut m, _) = issue_model(&root);
    let mut long = issue(7, "Add login to every page of the site at once");
    long.body = "# Steps\n\nopen the form\nwait".into();
    m.backend.issues_of = Arc::new(move |_| Ok(vec![long.clone()]));
    open_issue_list(&mut m);

    let row = line_text(&m.render_row(0, false, 16));
    assert_eq!(row, "  #7 Add login …");

    let pane = lines_text(&m.info_lines(40));
    for s in ["body", "Steps", "open the form\nwait"] {
        assert!(pane.contains(s), "{s:?}:\n{pane}");
    }
    for s in ["path", "git", "loading…"] {
        assert!(!pane.lines().any(|l| l == s), "{s:?}:\n{pane}");
    }
}

#[test]
fn issue_list_qualifiers() {
    let root = TempDir::new();
    let (mut m, _) = issue_model(&root);
    let mk = |n, title, labels: &[&str], who: &[&str]| Issue {
        labels: labels.iter().map(|s| s.to_string()).collect(),
        assignees: who.iter().map(|s| s.to_string()).collect(),
        author: if n == 2 { "carol" } else { "alice" }.into(),
        ..issue(n, title)
    };
    let list = vec![
        mk(1, "Leak", &["security"], &["jedipunkz"]),
        mk(2, "Typo", &["docs"], &[]),
        mk(3, "Bump", &["foo", "security"], &["bob"]),
    ];
    m.backend.issues_of = Arc::new(move |_| Ok(list.clone()));
    open_issue_list(&mut m);

    for (q, want) in [
        ("label:security,docs", "#3 Bump|#2 Typo|#1 Leak"),
        ("label:security label:foo", "#3 Bump"),
        ("label:SECURITY assignee:jedipunkz", "#1 Leak"),
        ("assignees:bob,jedipunkz leak", "#1 Leak"),
        ("author:carol", "#2 Typo"),
        ("author:Carol,alice label:docs", "#2 Typo"),
        ("label:secur", ""),
        ("label:", "#3 Bump|#2 Typo|#1 Leak"),
    ] {
        set_query(&mut m, q);
        assert_eq!(rows(&m).join("|"), want, "{q:?}");
    }

    set_query(&mut m, "label:foo");
    let pane = lines_text(&m.info_lines(40));
    assert!(pane.contains("assignees\nbob"), "{pane}");
}

#[test]
fn issue_panel_scrolls() {
    let root = TempDir::new();
    let (mut m, _) = issue_model(&root);
    open_issue_list(&mut m);
    let body: String = (0..40).map(|_| "word ").collect();
    m.h = 16;
    open_issue_panel(&mut m, detail_issue(&[(7, &body)]).unwrap());

    m.update(key("up"));
    assert_eq!(
        m.detail.as_ref().unwrap().scroll,
        0,
        "scrolled past the top"
    );
    m.update(ch('j'));
    assert!(m.detail.as_ref().unwrap().scroll > 0, "j did not scroll");
    m.update(ch('k'));
    m.update(key("pgdown"));
    m.update(key("pgdown"));
    let at = m.detail.as_ref().unwrap().scroll;
    m.update(key("pgdown"));
    assert_eq!(
        m.detail.as_ref().unwrap().scroll,
        at,
        "scrolled past the bottom"
    );

    m.update(ch(' '));
    assert_ne!(m.detail.as_ref().unwrap().scroll, 0, "space did not page");
    let at = m.detail.as_ref().unwrap().scroll;
    m.update(ch('b'));
    assert_ne!(m.detail.as_ref().unwrap().scroll, at, "b did not page back");
    m.detail.as_mut().unwrap().scroll = 0;
    m.update(key("shift+ "));
    m.update(key("shift+ "));
    assert_eq!(
        m.detail.as_ref().unwrap().scroll,
        0,
        "shift+space paged past the top"
    );
}

#[test]
fn issue_panel_markdown() {
    let root = TempDir::new();
    let (mut m, _) = issue_model(&root);
    m.h = 44;
    open_issue_list(&mut m);
    let body = "# The form\n\nfirst line\n\n- one\n- two\n\n`code`\n\n```\nfn main() {}\n```";
    open_issue_panel(&mut m, detail_issue(&[(7, body)]).unwrap());
    let panel = text(&screen(&m));
    for s in ["The form", "first line", "• one", "fn main() {}"] {
        assert!(panel.contains(s), "{s:?}:\n{panel}");
    }
    let window = panel.lines().collect::<Vec<_>>();
    let first = window.iter().position(|l| l.contains("first line"));
    let last = window.iter().position(|l| l.contains("• one"));
    assert!(first.is_some() && last.is_some(), "{panel}");

    let bottom = row_text(&screen(&m), m.h - 1);
    assert!(bottom.contains("ctrl+y"), "{bottom:?}");
}

#[test]
fn detail_lines_are_cached_until_note_or_width_changes() {
    let root = TempDir::new();
    let (mut m, _) = issue_model(&root);
    m.h = 44;
    open_issue_list(&mut m);
    open_issue_panel(&mut m, detail_issue(&[(7, "# title\n\nbody")]).unwrap());

    crate::finder::detail::DETAIL_LINE_BUILDS.with(|count| count.set(0));
    screen(&m);
    screen(&m);
    assert_eq!(
        crate::finder::detail::DETAIL_LINE_BUILDS.with(std::cell::Cell::get),
        1
    );

    m.note = "copied".into();
    screen(&m);
    assert_eq!(
        crate::finder::detail::DETAIL_LINE_BUILDS.with(std::cell::Cell::get),
        2
    );

    m.w += 1;
    screen(&m);
    assert_eq!(
        crate::finder::detail::DETAIL_LINE_BUILDS.with(std::cell::Cell::get),
        3
    );
}

#[test]
fn issue_panel_browser_and_copy() {
    let root = TempDir::new();
    let (mut m, _) = issue_model(&root);
    open_issue_list(&mut m);

    m.update(key("ctrl+b"));
    let empty = m.input.value();
    assert!(empty.is_empty() && m.over == Overlay::None, "{:?}", m.note);
    m.update(key("ctrl+y"));
    let empty = m.input.value();
    assert!(empty.is_empty(), "{empty:?}");

    let opened = Arc::new(Mutex::new(Vec::new()));
    let seen = opened.clone();
    m.backend.open_url = Arc::new(move |u| opened.lock().unwrap().push(u.to_string()));
    let copied = Arc::new(Mutex::new(Vec::new()));
    let had = copied.clone();
    m.backend.copy_url = Arc::new(move |u| {
        copied.lock().unwrap().push(u.to_string());
        Ok(())
    });

    open_issue_panel(&mut m, detail_issue(&[(7, "The form hangs.")]).unwrap());
    let answer_of = answer(m.update(key("ctrl+b")));
    assert!(answer_of.is_none(), "the browser task answered again");
    assert_eq!(
        seen.lock().unwrap().last().map(String::as_str),
        Some("https://github.com/acme/alpha/issues/7"),
        "ctrl-b opened the wrong page or none"
    );
    m.update(key("ctrl+y"));
    assert_eq!(
        had.lock().unwrap().last().map(String::as_str),
        Some("https://github.com/acme/alpha/issues/7"),
        "ctrl-y copied nothing"
    );
    assert!(m.note.contains("issue URL copied"), "{:?}", m.note);

    let panel = text(&screen(&m));
    assert!(panel.contains("issue URL copied"), "{panel}");
    m.update(key("q"));
    assert_eq!(m.note, "");
}

#[test]
fn issue_mode_leaves_the_list_alone_on_error() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    m.keys.issue = parse_chord("ctrl-e", "ctrl-e").unwrap();
    m.backend.issues_of = Arc::new(|_| Err("not logged in".into()));
    let cmds = m.update(key("ctrl+e"));
    feed(&mut m, cmds);
    assert!(
        m.mode == Mode::Repos && m.note.contains("not logged in"),
        "{:?}",
        m.note
    );

    m.backend.issues_of = Arc::new(|_| Ok(vec![]));
    let cmds = m.update(key("ctrl+e"));
    feed(&mut m, cmds);
    assert!(
        m.mode == Mode::Repos && m.note.contains("no open issues"),
        "{:?}",
        m.note
    );
}
