//! The worktree list.

use super::*;

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

    let cmds = m.open_worktrees();
    assert_eq!(m.mode, Mode::Repos, "the list changed before git answered");
    assert!(m.busy.contains("worktrees of"), "{:?}", m.busy);
    feed(&mut m, cmds);
    assert_eq!(m.mode, Mode::Worktrees);
    assert_eq!(m.busy, "");
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

// git is asked off the UI thread, and an answer about a repository the
// cursor has left is dropped rather than replacing the list it is on.
#[test]
fn worktree_mode_drops_a_stale_answer() {
    let root = TempDir::new();
    let rs = repos(
        &root.path(),
        &["github.com/acme/alpha", "github.com/acme/bravo"],
    );
    let mut m = new_model(&rs, "");
    only_main(&mut m);
    let cmds = m.update(key("ctrl+w"));
    m.update(key("up"));
    feed(&mut m, cmds);
    assert_eq!(m.mode, Mode::Repos);
    assert_eq!(m.busy, "", "the wait outlived its answer");
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
        open_worktrees(&mut m);
        assert_eq!(m.mode, Mode::Worktrees);
        let cmds = m.update(key(k));
        assert_eq!(m.mode, Mode::Repos, "{k}");
        assert!(
            !is_quit(&cmds),
            "{k} quit gm instead of closing the worktree list"
        );
    }
    // Esc quits from the repository list with an issue key that is not the
    // default: the default ctrl-[ is esc and opens the issue list instead,
    // see esc_opens_the_issue_list.
    let mut esc_quits = {
        let mut m = new_model(&rs, "");
        m.keys.issue = parse_chord("", "ctrl-e").unwrap();
        m
    };
    assert!(is_quit(&esc_quits.update(key("esc"))));
    assert!(!is_quit(&new_model(&rs, "").update(key("ctrl+g"))));
}

// When git cannot answer, the finder stays usable and says why on the hint
// line, the way the pull request list does; silence reads as a broken key.
#[test]
fn worktree_mode_leaves_the_list_alone_on_error() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    m.worktrees_of = Arc::new(|_| Err("not a git repository".into()));
    open_worktrees(&mut m);
    assert!(m.mode == Mode::Repos && m.view.len() == 1);
    assert!(m.note.contains("not a git repository"), "{:?}", m.note);
}

// An empty answer from Ctrl-L has a name too: every fresh gm create has no
// branch yet, so pressing Ctrl-L on one must not look like a broken key.
#[test]
fn empty_lists_are_named() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    m.branches_of = Arc::new(|_| Ok(vec![]));
    open_branches(&mut m);
    assert!(m.mode == Mode::Repos, "{:?}", m.note);
    assert!(
        m.note.contains("no branches in github.com/acme/alpha"),
        "{:?}",
        m.note
    );

    m.worktrees_of = Arc::new(|_| Ok(vec![]));
    open_worktrees(&mut m);
    assert!(
        m.note.contains("no worktrees in github.com/acme/alpha"),
        "{:?}",
        m.note
    );
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
    open_worktrees(&mut m); // the real repo::worktrees, not a stub
    assert!(m.mode == Mode::Worktrees && m.view.len() == 1);

    let cmds = run_slash(&mut m, "/create feat/login");
    assert_eq!(m.over, Overlay::Confirm, "{:?}", m.note);
    assert!(m.ask.dir.contains(repo::WORKTREE_ROOT), "{:?}", m.ask.dir);
    assert!(m.update(ch('y')).is_empty(), "y must wait for git");
    assert!(m.ask.wait.is_some());
    feed(&mut m, cmds);
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

    // Now take it away, and its branch: it holds nothing HEAD lacks.
    open_worktrees(&mut m); // the rows that come from git carry their branch
    m.cursor = 0;
    ask_remove(&mut m);
    assert_eq!(m.over, Overlay::Confirm, "{:?}", m.note);
    assert!(view_text(&m).contains("branch feat/login goes too"));
    let cmds = m.update(ch('y'));
    feed(&mut m, cmds);
    assert!(!exists(&dir));
    assert!(!repo::branch_exists(&r.path(), "feat/login"));
    assert_eq!(rows(&m).len(), 1);
    assert!(
        m.note.ends_with(", deleted branch feat/login"),
        "{:?}",
        m.note
    );
}

// /expire acts on the whole list, not the selected row: the panel names every
// worktree that goes and the one kept for its work, and yes takes them off the
// list. The reflog is made to read old; git does the rest for real.
#[test]
fn expire_removes_the_idle_worktrees() {
    let root = TempDir::new();
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    let tree = Tree {
        roots: vec![root.path()],
    };
    let wt_root = paths::join(&r.root, repo::WORKTREE_ROOT);
    for b in ["old/a", "old/b", "busy"] {
        repo::add_worktree(&wt_root, &r.path(), &tree.worktree_dir(&r, b), b).unwrap();
    }
    let busy = tree.worktree_dir(&r, "busy");
    let mut m = new_model(std::slice::from_ref(&r), "");
    (m.w, m.h) = (90, 24);
    m.worktrees_of = Arc::new(|dir| {
        let mut wts = repo::worktrees(dir)?;
        for w in wts.iter_mut().filter(|w| w.used_at > 0) {
            w.used_at = 1;
        }
        Ok(wts)
    });
    let dirty = busy.clone();
    m.changed_of = Arc::new(move |p| usize::from(repo::same_path(p, &dirty)));
    open_worktrees(&mut m);
    assert_eq!(rows(&m).len(), 4);

    let cmds = run_slash(&mut m, "/expire 30d");
    feed(&mut m, cmds);
    assert_eq!(m.over, Overlay::Confirm, "{:?}", m.note);
    let view = view_text(&m);
    for want in [
        "remove 2 worktrees",
        "old/a",
        "old/b",
        "kept, uncommitted changes: busy",
    ] {
        assert!(view.contains(want), "{want:?}:\n{view}");
    }

    let cmds = m.update(ch('y'));
    feed(&mut m, cmds);
    assert_eq!(m.note, "removed 2 worktrees, 2 branches");
    assert_eq!(rows(&m).len(), 2);
    assert!(exists(&busy));
    assert!(!exists(&tree.worktree_dir(&r, "old/a")));
}

#[test]
fn expire_scan_is_async_and_y_waits_for_its_result() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    let mut m = new_model(&rs, "");
    (m.w, m.h) = (90, 16);
    m.mode = Mode::Worktrees;
    m.repo_at = rs[0].path();

    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let old = paths::join(&root.path(), "old");
    m.worktrees_of = Arc::new(move |_| {
        started_tx.send(()).unwrap();
        release_rx.lock().unwrap().recv().unwrap();
        Ok(vec![Worktree {
            path: old.clone(),
            branch: "old".into(),
            used_at: 1,
            ..Default::default()
        }])
    });
    m.changed_of = Arc::new(|_| 0);

    // Keep the UI call on a separate thread so a synchronous scan can be
    // released and reported as a red assertion instead of hanging the test.
    let (ui_tx, ui_rx) = mpsc::channel();
    let ui = std::thread::spawn(move || {
        let cmds = run_slash(&mut m, "/expire 30d");
        ui_tx.send((m, cmds)).unwrap();
    });
    let early = ui_rx.recv_timeout(Duration::from_secs(1)).ok();
    let returned_early = early.is_some();
    let (mut m, cmds) = match early {
        Some(result) => result,
        None => {
            release_tx.send(()).unwrap();
            ui_rx.recv().unwrap()
        }
    };
    ui.join().unwrap();
    assert!(returned_early, "/expire waited for its worktree scan");

    let view = view_text(&m);
    assert!(view.contains("checking for uncommitted work"), "{view}");
    assert!(!view.contains("y do it"), "{view}");
    assert!(
        m.update(ch('y')).is_empty(),
        "y acted before the scan answered"
    );
    assert_eq!(m.over, Overlay::Confirm);

    let task = cmds
        .into_iter()
        .find_map(|cmd| match cmd {
            Cmd::Task(task) => Some(task),
            _ => None,
        })
        .expect("/expire did not schedule a scan task");
    let (answer_tx, answer_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || answer_tx.send(task()).unwrap());
    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("the scan task did not start");

    // The UI can still process input and render while git's scan is delayed.
    assert!(
        m.update(ch('y')).is_empty(),
        "y acted while the scan was still delayed"
    );
    assert!(m.update(Msg::Resize(100, 20)).is_empty());
    assert_eq!((m.w, m.h), (100, 20));
    let view = view_text(&m);
    assert!(view.contains("checking for uncommitted work"), "{view}");
    assert!(!view.contains("y do it"), "{view}");

    release_tx.send(()).unwrap();
    let answer = answer_rx.recv().unwrap().expect("the scan had no answer");
    worker.join().unwrap();
    m.update(answer);
    assert_eq!(m.ask.wts.len(), 1);
    assert!(
        m.ask
            .detail
            .iter()
            .any(|line| line.contains("idle 30+ days"))
    );
    assert!(m.ask.wait.is_none(), "the scan result did not unlock y");
    assert!(view_text(&m).contains("y do it"));
    assert!(
        m.update(ch('y'))
            .iter()
            .any(|cmd| matches!(cmd, Cmd::Task(_)))
    );
}

#[test]
fn expire_belongs_to_the_worktree_list() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    run_slash(&mut m, "/expire 30d");
    assert_eq!(m.over, Overlay::None);
    assert!(m.note.contains("worktree list"), "{:?}", m.note);
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
    open_worktrees(&mut m);
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
    open_worktrees(&mut m);
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
    open_worktrees(&mut m);
    let cmds = run_slash(&mut m, "/remove");
    assert!(!is_quit(&cmds) && m.over == Overlay::None);
    assert!(m.note.contains("repository itself"), "{:?}", m.note);
}

// git reports the resolved path of a worktree, which is not the path the
// finder walked when the root sits behind a symlink. The main worktree must
// still be recognised, or /remove offers to delete the repository itself.
#[test]
fn remove_refuses_the_main_worktree_behind_a_symlink() {
    let base = TempDir::new();
    let real = base.join("real");
    let link = base.join("link");
    mkdir(&paths::join(&real, "github.com/acme/alpha"));
    std::os::unix::fs::symlink(&real, &link).unwrap();

    let mut m = new_model(&repos(&link, &["github.com/acme/alpha"]), "");
    m.worktrees_of = Arc::new(|dir| {
        Ok(vec![Worktree {
            path: std::fs::canonicalize(dir)
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            branch: "main".into(),
            ..Default::default()
        }])
    });
    open_worktrees(&mut m);
    let cmds = run_slash(&mut m, "/remove");
    assert!(!is_quit(&cmds) && m.over == Overlay::None);
    assert!(m.note.contains("repository itself"), "{:?}", m.note);
}
