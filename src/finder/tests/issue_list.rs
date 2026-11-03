//! The issue list and its detail panel.

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

/// issue_model is a finder over two repositories; alpha has two open issues
/// and gh is stubbed, for the listing and for one issue in full.
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
    m.issues_of = Arc::new(move |dir| {
        assert_eq!(dir, at);
        Ok(list.clone())
    });
    (m, rs)
}

/// open_issue_list presses the issue key and hands the finder gh's answer.
fn open_issue_list(m: &mut Model) {
    let cmds = m.update(key("ctrl+e"));
    assert!(!cmds.is_empty(), "the key did not ask gh");
    assert!(m.busy.contains("asking GitHub"), "{:?}", m.busy);
    feed(m, cmds);
}

/// open_issue_panel presses enter in the issue list and feeds gh's view
/// answer, leaving the panel up.
fn open_issue_panel(m: &mut Model, detail: Issue) {
    m.issue_of = Arc::new(move |_, _| Ok(detail.clone()));
    let cmds = m.update(key("enter"));
    assert!(m.busy.contains("viewing #7"), "{:?}", m.busy);
    feed(m, cmds);
    assert!(
        m.over == Overlay::Detail,
        "the panel did not open: {:?}",
        m.note
    );
}

// The listing is newest at the bottom, enter opens the panel over it, and q
// closes the panel back onto the list — which the issue key closes as well.
#[test]
fn issue_mode() {
    let root = TempDir::new();
    let (mut m, rs) = issue_model(&root);
    open_issue_list(&mut m);
    assert_eq!(m.mode, Mode::Issues, "{:?}", m.note);
    assert_eq!(rows(&m).join("|"), "#9 Crash on start|#7 Add login");

    // The hints and the pane say what an issue is.
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
    // Each Comment is its own frame, its author riding the top rail.
    assert!(panel.contains('╭'), "{panel}");
    assert!(panel.contains("─ bob "), "{panel}");
    assert!(panel.contains('╯'), "{panel}");
    let rail = panel
        .lines()
        .find(|l| l.contains("Reproduced here."))
        .expect("the comment's row is not drawn");
    assert!(rail.trim_start().starts_with("│ "), "{rail:?}");

    // q closes the panel back onto the list.
    m.update(key("q"));
    assert!(m.over == Overlay::None && m.detail.is_none());
    assert_eq!(m.mode, Mode::Issues);
    // The same key closes the list as well.
    m.update(key("ctrl+e"));
    assert_eq!(m.mode, Mode::Repos);
}

// An issue row too long for the list keeps its number and cuts the title's
// tail, and the pane shows the body the listing carried, as Markdown.
#[test]
fn issue_row_keeps_its_number_and_the_pane_its_body() {
    let root = TempDir::new();
    let (mut m, _) = issue_model(&root);
    let mut long = issue(7, "Add login to every page of the site at once");
    long.body = "# Steps\n\nopen the form\nwait".into();
    m.issues_of = Arc::new(move |_| Ok(vec![long.clone()]));
    open_issue_list(&mut m);

    let row = line_text(&m.render_row(0, false, 16));
    assert_eq!(row, "  #7 Add login …");

    let pane = lines_text(&m.info_lines(40));
    for s in ["body", "Steps", "open the form\nwait"] {
        assert!(pane.contains(s), "{s:?}:\n{pane}");
    }
    // An issue has no worktree: no path, no git status waiting on one.
    for s in ["path", "git", "loading…"] {
        assert!(!pane.lines().any(|l| l == s), "{s:?}:\n{pane}");
    }
}

// label:, assignee: and author: narrow the issue list the way GitHub's search does:
// commas are any-of, repeated qualifiers are all-of, case does not matter,
// and what is left of the query is matched as ever.
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
    m.issues_of = Arc::new(move |_| Ok(list.clone()));
    open_issue_list(&mut m);

    for (q, want) in [
        ("label:security,docs", "#3 Bump|#2 Typo|#1 Leak"),
        ("label:security label:foo", "#3 Bump"),
        ("label:SECURITY assignee:jedipunkz", "#1 Leak"),
        ("assignees:bob,jedipunkz leak", "#1 Leak"),
        ("author:carol", "#2 Typo"),
        ("author:Carol,alice label:docs", "#2 Typo"),
        ("label:secur", ""),
        // A qualifier with no value yet narrows nothing.
        ("label:", "#3 Bump|#2 Typo|#1 Leak"),
    ] {
        set_query(&mut m, q);
        assert_eq!(rows(&m).join("|"), want, "{q:?}");
    }

    // The pane names the assignees, so what assignee: matched is on screen.
    set_query(&mut m, "label:foo");
    let pane = lines_text(&m.info_lines(40));
    assert!(pane.contains("assignees\nbob"), "{pane}");
}

// The scroll keys walk the panel's window: never past the top, and never
// past the bottom, where the hint line already is.
#[test]
fn issue_panel_scrolls() {
    let root = TempDir::new();
    let (mut m, _) = issue_model(&root);
    open_issue_list(&mut m);
    let body: String = (0..40).map(|_| "word ").collect();
    // The pane is small, so the fold really has somewhere past the window
    // to go.
    m.h = 16;
    open_issue_panel(&mut m, detail_issue(&[(7, &body)]).unwrap());

    m.update(key("up"));
    assert_eq!(
        m.detail.as_ref().unwrap().scroll,
        0,
        "scrolled past the top"
    );
    // j and k scroll the window, the way vim moves: the panel has no query
    // to type, so the letters are free there.
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

    // Space pages forward, b pages back (shift+space too, where a terminal
    // reports the shift; less's b is the one every terminal sends), clamped
    // the same.
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

// The body and the comments go out as the Markdown they are typed in: rows
// take their own line, headings and lists take their shapes.
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
    // The rows stayed rows: a newline is not a character it folded away.
    let window = panel.lines().collect::<Vec<_>>();
    let first = window.iter().position(|l| l.contains("first line"));
    let last = window.iter().position(|l| l.contains("• one"));
    assert!(first.is_some() && last.is_some(), "{panel}");

    // The hints line sits on the screen's own bottom row, with no frame
    // under it, naming the chords.
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
    screen(&m); // repeated idle loop draw
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

// ctrl-b opens the issue in a browser on a thread, and ctrl-y copies its URL
// and says so; both are the detail panel's, and answer where its text was.
#[test]
fn issue_panel_browser_and_copy() {
    let root = TempDir::new();
    let (mut m, _) = issue_model(&root);
    open_issue_list(&mut m);

    // Nothing is open yet, so the chords belong to the box: ctrl+b is a
    // cursor move, and both are no-ops on an empty one.
    m.update(key("ctrl+b"));
    let empty = m.input.value();
    assert!(empty.is_empty() && m.over == Overlay::None, "{:?}", m.note);
    m.update(key("ctrl+y"));
    let empty = m.input.value();
    assert!(empty.is_empty(), "{empty:?}");

    let opened = Arc::new(Mutex::new(Vec::new()));
    let seen = opened.clone();
    m.open_url = Arc::new(move |u| opened.lock().unwrap().push(u.to_string()));
    let copied = Arc::new(Mutex::new(Vec::new()));
    let had = copied.clone();
    m.copy_url = Arc::new(move |u| {
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

    // The copy's answer is where the panel's title was, and closing takes
    // the note with it.
    let panel = text(&screen(&m));
    assert!(panel.contains("issue URL copied"), "{panel}");
    m.update(key("q"));
    assert_eq!(m.note, "");
}

// gh failing or answering nothing leaves the repository list with a reason,
// the way the pull request list does.
#[test]
fn issue_mode_leaves_the_list_alone_on_error() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    m.keys.issue = parse_chord("ctrl-e", "ctrl-e").unwrap();
    m.issues_of = Arc::new(|_| Err("not logged in".into()));
    let cmds = m.update(key("ctrl+e"));
    feed(&mut m, cmds);
    assert!(
        m.mode == Mode::Repos && m.note.contains("not logged in"),
        "{:?}",
        m.note
    );

    m.issues_of = Arc::new(|_| Ok(vec![]));
    let cmds = m.update(key("ctrl+e"));
    feed(&mut m, cmds);
    assert!(
        m.mode == Mode::Repos && m.note.contains("no open issues"),
        "{:?}",
        m.note
    );
}
