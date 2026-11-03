//! The pull request list, the busy line, and the smallest terminals.

use super::*;

/// open_pr_panel presses enter in the pull request list and feeds gh's view
/// answer, leaving the panel up.
fn open_pr_panel(m: &mut Model) {
    let n = m.current().unwrap().pr.number;
    let cmds = m.update(key("enter"));
    assert!(m.busy.contains(&format!("viewing #{n}")), "{:?}", m.busy);
    assert!(!is_quit(&cmds), "enter in the list left the finder");
    feed(m, cmds);
    assert!(
        m.over == Overlay::Detail,
        "the panel did not open: {:?}",
        m.note
    );
}

// A pull request row too long for the list keeps its number and cuts the
// title's tail, the way an issue row does.
#[test]
fn pr_row_keeps_its_number() {
    let root = TempDir::new();
    let (mut m, _) = pr_model(&root);
    m.backend.prs_of = Arc::new(|_| {
        Ok(vec![pull_request(
            7,
            "Add login to every page of the site at once",
            "feat/login",
            false,
            false,
            "acme",
        )])
    });
    open_pr_list(&mut m);

    let row = line_text(&m.render_row(0, false, 16));
    assert_eq!(row, "  #7 Add login …");
}

// label:, assignee: and author: narrow the pull request list the way they do
// the issue list; a bot's author is its login as gh names it.
#[test]
fn pr_list_qualifiers() {
    let root = TempDir::new();
    let (mut m, _) = pr_model(&root);
    let mk = |n, title, author: &str, labels: &[&str], who: &[&str]| PullRequest {
        labels: labels.iter().map(|s| s.to_string()).collect(),
        assignees: who.iter().map(|s| s.to_string()).collect(),
        ..pull_request(n, title, &format!("b{n}"), false, false, author)
    };
    let list = vec![
        mk(3, "Bump openssl", "app/dependabot", &["security"], &[]),
        mk(2, "Bump serde", "app/dependabot", &["dependencies"], &[]),
        mk(1, "Fix leak", "alice", &["security"], &["jedipunkz"]),
    ];
    m.backend.prs_of = Arc::new(move |_| Ok(list.clone()));
    open_pr_list(&mut m);

    for (q, want) in [
        ("author:app/dependabot label:security", "#3 Bump openssl"),
        ("author:App/Dependabot", "#2 Bump serde|#3 Bump openssl"),
        ("label:security assignee:jedipunkz", "#1 Fix leak"),
        ("label:security,dependencies serde", "#2 Bump serde"),
        ("author:dependabot", ""),
    ] {
        set_query(&mut m, q);
        assert_eq!(rows(&m).join("|"), want, "{q:?}");
    }
}

// A full page exactly means there were at least that many open pull
// requests: the hint line says so, so a #42 that is not on the list is not
// read as "no such pull request".
#[test]
fn pr_list_says_when_it_is_cut_short() {
    let root = TempDir::new();
    let (mut m, rs) = pr_model(&root);
    // Exactly a page: everything the real page wears applies, plus the note.
    let page: Vec<PullRequest> = (1..=repo::PR_LIMIT as u64)
        .map(|n| pull_request(n, "Change something", "feat/x", false, false, "acme"))
        .collect();
    let _want = rs[1].path();
    m.backend.prs_of = Arc::new(move |_| Ok(page.clone()));

    open_pr_list(&mut m);
    assert!(m.mode == Mode::Prs && m.view.len() == repo::PR_LIMIT);
    assert!(
        m.note.contains(&format!(
            "showing the newest {} open pull requests",
            repo::PR_LIMIT
        )),
        "{:?}",
        m.note
    );
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
        hints.contains("enter view") && hints.contains("ctrl-j/g/esc repos"),
        "{hints:?}"
    );
    let pane = lines_text(&m.info_lines(40));
    for s in [rs[1].rel.as_str(), "bob:main", "none yet"] {
        assert!(pane.contains(s), "{s:?}:\n{pane}");
    }
    // The fork's main is the selected row, and has no worktree of its own.
    assert_eq!(m.current().unwrap().path, "");

    // Enter views; enter in the panel goes to the worktree.
    let mut login = m.clone();
    set_query(&mut login, "login");
    open_pr_panel(&mut login);
    let cmds = login.update(key("enter"));
    assert!(is_quit(&cmds));
    assert_eq!(login.over, Overlay::None);
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

// Which pull request is checked out where is git's answer too, so it is
// worked out in gh's task rather than once the answer is on the UI thread.
#[test]
fn pr_mode_asks_git_off_the_ui_thread() {
    let root = TempDir::new();
    let (mut m, _) = pr_model(&root);
    let msg = answer(m.update(key("ctrl+j"))).expect("no answer from gh");
    m.backend.worktrees_of = Arc::new(|_| panic!("git was asked on the UI thread"));
    m.update(msg);
    assert_eq!(m.mode, Mode::Prs, "{:?}", m.note);
    set_query(&mut m, "login");
    assert_eq!(m.current().unwrap().path, "/tmp/wt/alpha-login");
}

// A fork's checkout is found under a symlinked root, where git prints the
// resolved path, and is filed apart from the repository's own branches: a
// local bob/main is not fork pull request bob:main.
#[test]
fn pr_mode_finds_a_fork_checkout_by_place() {
    let base = TempDir::new();
    let real = base.join("real");
    mkdir(&real);
    let link = base.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let rs = repos(&link, &["github.com/acme/alpha"]);
    let wt = |name: &str| {
        paths::join(
            &real,
            &format!("{}/github.com/acme/alpha/{name}", repo::WORKTREE_ROOT),
        )
    };
    let (fork, local) = (wt(".forks/bob/main"), wt("bob/main"));
    mkdir(&fork);
    mkdir(&local);

    let mut m = new_model(&rs, "");
    (m.w, m.h) = (100, 16);
    let (f, l) = (fork.clone(), local.clone());
    m.backend.worktrees_of = Arc::new(move |dir| {
        let w = |p: &str, b: &str| Worktree {
            path: p.into(),
            branch: b.into(),
            ..Default::default()
        };
        Ok(vec![w(dir, "main"), w(&l, "bob/main"), w(&f, "bob-main")])
    });
    m.backend.prs_of = Arc::new(|_| {
        Ok(vec![pull_request(
            9, "Fix typo", "main", false, true, "bob",
        )])
    });
    open_pr_list(&mut m);
    assert_eq!(m.mode, Mode::Prs, "{:?}", m.note);
    let at = m.current().unwrap().path.clone();
    assert!(repo::same_path(&at, &fork), "{at:?}");

    // Without the fork's own checkout, the local bob/main is not offered.
    let l = local.clone();
    m.backend.worktrees_of = Arc::new(move |dir| {
        Ok(vec![
            Worktree {
                path: dir.into(),
                branch: "main".into(),
                ..Default::default()
            },
            Worktree {
                path: l.clone(),
                branch: "bob/main".into(),
                ..Default::default()
            },
        ])
    });
    m.update(key("ctrl+j"));
    open_pr_list(&mut m);
    assert_eq!(m.current().unwrap().path, "");
}

// The pane beside the list shows the labels and the description the listing
// carried, as Markdown, ahead of the checkout's fields.
#[test]
fn pr_pane_shows_the_body() {
    let root = TempDir::new();
    let (mut m, _) = pr_model(&root);
    m.backend.prs_of = Arc::new(|_| {
        let mut p = pull_request(7, "Add login", "feat/login", false, false, "acme");
        p.body = "# Why\n\nthe form\nis missing".into();
        p.labels = vec!["security".into(), "go".into()];
        Ok(vec![p])
    });
    open_pr_list(&mut m);
    let pane = lines_text(&m.info_lines(40));
    for s in [
        "labels\nsecurity, go",
        "body",
        "Why",
        "the form\nis missing",
        "https://github.com/acme/alpha/pull/7",
    ] {
        assert!(pane.contains(s), "{s:?}:\n{pane}");
    }
    let body = pane.find("body").unwrap();
    let path = pane.find("\npath\n").expect("no path field");
    assert!(body < path, "the body came after the path:\n{pane}");
}

// Enter opens the panel: the labels, description and comments are drawn,
// the hint line leads with check out, q goes back to the list, and nothing is
// checked out until the panel's own enter.
#[test]
fn pr_panel() {
    let root = TempDir::new();
    let (mut m, rs) = pr_model(&root);
    (m.w, m.h) = (100, 30);
    open_pr_list(&mut m);
    // The fork's main, which has no worktree yet.
    open_pr_panel(&mut m);
    let panel = text(&screen(&m));
    for s in [
        "Add login",
        "#9 [OPEN]",
        "login",
        "Looks good.",
        "─ carol ",
        "labels",
        "security",
        "none yet; enter makes one",
        rs[1].rel.as_str(),
    ] {
        assert!(panel.contains(s), "{s:?}:\n{panel}");
    }
    let bottom = row_text(&screen(&m), m.h - 1);
    assert!(
        bottom.trim_start().starts_with("enter check out"),
        "{bottom:?}"
    );

    // q closes the panel back onto the list, checking nothing out.
    let cmds = m.update(key("q"));
    assert!(cmds.is_empty() && m.over == Overlay::None && m.detail.is_none());
    assert_eq!(m.mode, Mode::Prs);

    // ctrl-b and ctrl-y act on the pull request's URL.
    let copied = Arc::new(Mutex::new(Vec::new()));
    let had = copied.clone();
    m.backend.copy_url = Arc::new(move |u| {
        copied.lock().unwrap().push(u.to_string());
        Ok(())
    });
    open_pr_panel(&mut m);
    m.update(key("ctrl+y"));
    assert_eq!(
        had.lock().unwrap().last().map(String::as_str),
        Some("https://github.com/acme/alpha/pull/9")
    );
    assert!(m.note.contains("pull request URL copied"), "{:?}", m.note);
}

// The panel's enter checks out the pull request the panel shows, even when
// the cursor moved while gh was answering.
#[test]
fn pr_panel_checks_out_what_it_shows() {
    let root = TempDir::new();
    let (mut m, _) = pr_model(&root);
    open_pr_list(&mut m);
    set_query(&mut m, "login");
    let cmds = m.update(key("enter"));
    set_query(&mut m, "typo");
    feed(&mut m, cmds);
    assert_eq!(m.over, Overlay::Detail, "{:?}", m.note);
    let cmds = m.update(key("enter"));
    assert!(is_quit(&cmds), "{:?}", m.note);
    assert_eq!(m.result.arg, "/tmp/wt/alpha-login");
}

// gh failing to read the pull request in full still opens the panel, on
// what the listing carried, with gh's reason in the title's place: the
// checkout behind the panel's enter stays reachable.
#[test]
fn pr_panel_opens_on_the_listing_when_gh_fails() {
    let root = TempDir::new();
    let (mut m, _) = pr_model(&root);
    m.backend.pr_of = Arc::new(|_, _| Err("gh pr view: timed out".into()));
    open_pr_list(&mut m);
    set_query(&mut m, "login");
    open_pr_panel(&mut m);
    let panel = text(&screen(&m));
    assert!(panel.contains("gh pr view: timed out"), "{panel}");
    let cmds = m.update(key("enter"));
    assert!(is_quit(&cmds) && m.result.arg == "/tmp/wt/alpha-login");
}

// An answer about a list the user has left is dropped, panel and all.
#[test]
fn pr_panel_drops_a_stale_answer() {
    let root = TempDir::new();
    let (mut m, _) = pr_model(&root);
    open_pr_list(&mut m);
    let cmds = m.update(key("enter"));
    m.update(key("ctrl+j"));
    assert_eq!(m.mode, Mode::Repos);
    feed(&mut m, cmds);
    assert!(m.over == Overlay::None && m.detail.is_none());
    assert_eq!(m.busy, "");
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
        m.backend.prs_of = Arc::new(move |_| res.clone());
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
    m.backend.gh = gh;
    m.backend.prs_of = Arc::new(|_| {
        Ok(vec![pull_request(
            7,
            "Add login",
            "feat/login",
            false,
            false,
            "acme",
        )])
    });
    m.backend.pr_of = Arc::new(|_, n| Ok(pr_in_full(n)));
    open_pr_list(&mut m);
    open_pr_panel(&mut m);
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

#[test]
fn stale_busy_answers_do_not_clear_newer_work() {
    let root = TempDir::new();
    let (mut m, r) = branch_model(&root);
    let want = r.path();
    m.backend.prs_of = Arc::new(move |dir| {
        assert_eq!(dir, want);
        Ok(vec![pull_request(
            7,
            "Add login",
            "feat/login",
            false,
            false,
            "acme",
        )])
    });
    let remote_cmds = open_branches(&mut m);
    let Some(Msg::RemoteBranches(remote)) = answer(remote_cmds) else {
        panic!("no answer from the remotes")
    };
    let pr_cmds = m.update(key("ctrl+j"));
    let Some(Msg::Prs(prs)) = answer(pr_cmds) else {
        panic!("no answer from gh")
    };

    m.update(Msg::RemoteBranches(remote));
    assert!(m.busy.contains("asking GitHub"), "{:?}", m.busy);
    m.update(Msg::Prs(prs));
    assert_eq!(m.busy, "");

    let (mut m, r) = branch_model(&root);
    let want = r.path();
    m.backend.prs_of = Arc::new(move |dir| {
        assert_eq!(dir, want);
        Ok(vec![pull_request(
            7,
            "Add login",
            "feat/login",
            false,
            false,
            "acme",
        )])
    });
    let pr_cmds = m.update(key("ctrl+j"));
    let Some(Msg::Prs(prs)) = answer(pr_cmds) else {
        panic!("no answer from gh")
    };
    let remote_cmds = open_branches(&mut m);
    let Some(Msg::RemoteBranches(remote)) = answer(remote_cmds) else {
        panic!("no answer from the remotes")
    };

    m.update(Msg::Prs(prs));
    assert!(m.busy.contains("asking the remotes"), "{:?}", m.busy);
    m.update(Msg::RemoteBranches(remote));
    assert_eq!(m.busy, "");
}

#[test]
fn repo_scan_and_done_only_clear_their_own_busy_line() {
    let root = TempDir::new();
    let (mut m, _) = pr_model(&root);
    let scan_cmds = run_slash(&mut m, "/dirty");
    assert!(
        m.busy.contains("checking repository status"),
        "{:?}",
        m.busy
    );
    let Some(Msg::RepoStates(tag, states)) = answer(scan_cmds) else {
        panic!("no answer from the repository scan")
    };
    let pr_cmds = m.update(key("ctrl+j"));
    let Some(Msg::Prs(prs)) = answer(pr_cmds) else {
        panic!("no answer from gh")
    };

    m.update(Msg::RepoStates(tag, states));
    assert!(m.busy.contains("asking GitHub"), "{:?}", m.busy);
    m.update(Msg::Prs(prs));
    assert_eq!(m.busy, "");

    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/charlie".into(),
    };
    let mut m = new_model(std::slice::from_ref(&r), "");
    let cmds = run_slash(&mut m, "/create acme/delta");
    feed(&mut m, cmds);
    let cmds = m.update(ch('y'));
    assert!(m.busy.contains("creating"), "{:?}", m.busy);
    let Some(Msg::Done(done)) = answer(cmds) else {
        panic!("no answer from the create task")
    };
    m.start_busy("asking GitHub about github.com/acme/charlie…");

    m.update(Msg::Done(done));
    assert!(m.busy.contains("asking GitHub"), "{:?}", m.busy);
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
    // And with the issue's panel up, detail and scroll included: it is
    // nearly the whole screen, so small terminals clip it hardest.
    m.detail = Some(crate::finder::issues::Detail {
        subject: crate::finder::issues::Subject::Issue(issue(
            7,
            "A body long enough to fold, and fold again, many times over. ",
        )),
        scroll: 5,
        lines: std::cell::RefCell::new(None),
    });
    m.over = Overlay::Detail;
    for w in 0..30 {
        for h in 0..12 {
            (m.w, m.h) = (w, h);
            screen(&m);
        }
    }
}
