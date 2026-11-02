//! The branch list.

use super::*;

// The newest branch rests under the cursor, a remote-only branch reads as the
// remote's, Enter on a branch that is already checked out goes there, and the
// key toggles the list off.
#[test]
fn branch_mode() {
    let root = TempDir::new();
    let (mut m, r) = branch_model(&root);
    let cmds = m.update(key("ctrl+l"));
    assert!(m.busy.contains("branches of"), "{:?}", m.busy);
    let cmds = feed(&mut m, cmds);
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
    let cmds = other.update(key("ctrl+w"));
    feed(&mut other, cmds);
    assert_eq!(other.mode, Mode::Worktrees);
    let cmds = m.update(key("esc"));
    assert!(m.mode == Mode::Repos && !is_quit(&cmds));
}

// git is asked off the UI thread, and an answer about a repository the
// cursor has left is dropped rather than replacing the list it is on.
#[test]
fn branch_mode_drops_a_stale_answer() {
    let root = TempDir::new();
    let (mut m, _) = branch_model(&root);
    let cmds = m.update(key("ctrl+l"));
    set_query(&mut m, "nothing matches this");
    feed(&mut m, cmds);
    assert_eq!(m.mode, Mode::Repos);
    assert_eq!(m.busy, "", "the wait outlived its answer");
}

#[test]
fn branches_command() {
    let root = TempDir::new();
    let (mut m, _) = branch_model(&root);
    let cmds = run_slash(&mut m, "/branches");
    feed(&mut m, cmds);
    assert_eq!(m.mode, Mode::Branches);
}

// A branch the query names whole outranks a longer one that only carries the
// same characters inside another name: "main" answers "main", while
// "fix/mainline" is a different branch. The named branch rests under the
// cursor, where Enter means it.
#[test]
fn branch_query_prefers_the_named_branch() {
    let root = TempDir::new();
    let (mut m, _) = branch_model(&root);
    open_branches(&mut m);
    // A branch made after main, the way a feature branch arrives later: newer,
    // so without the tie-break it would take main's place at the bottom.
    m.all.push(Item {
        label: "fix/mainline".into(),
        branch: Branch {
            name: "fix/mainline".into(),
            ..Default::default()
        },
        ..Default::default()
    });
    set_query(&mut m, "main");
    assert_eq!(rows(&m).join(" "), "fix/mainline main");
    assert_eq!(label(&m), "main");
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
    open_branches(&mut m);
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

// A confirmed worktree made from a branch row updates that row, not the list
// with a second item that has lost its branch identity. Its repository details
// are stale once the worktree operation succeeds.
#[test]
fn add_worktree_updates_branch_row_and_invalidates_status() {
    let root = TempDir::new();
    let (mut m, r) = branch_model(&root);
    git_repo(&r.path());
    git(&r.path(), &["branch", "fix/timeout"]);
    m.status.insert(
        r.path(),
        Status {
            branch: "main".into(),
            ..Default::default()
        },
    );
    open_branches(&mut m);
    m.busy.clear(); // this test does not run the remote-branch task
    set_query(&mut m, "timeout");
    assert_eq!(label(&m), "fix/timeout");

    let cmds = run_slash(&mut m, "/create fix/timeout");
    feed(&mut m, cmds);
    assert_eq!(m.ask.mode, Mode::Branches);
    assert_eq!(m.ask.repo_at, r.path());
    let cmds = m.update(ch('y'));
    assert!(m.busy.contains("creat"), "{:?}", m.busy);
    let msg = answer(cmds).expect("confirmed worktree did not run");
    m.update(msg);

    let matches: Vec<_> = m
        .all
        .iter()
        .filter(|it| it.branch.name == "fix/timeout")
        .collect();
    assert_eq!(matches.len(), 1, "{:?}", rows(&m));
    assert!(!matches[0].path.is_empty());
    assert!(!m.status.contains_key(&r.path()));
}

// The same in-place update applies to a pull-request row, identified by the
// branch the PR checks out.
#[test]
fn add_worktree_updates_pull_request_row() {
    let root = TempDir::new();
    let (mut m, rs) = pr_model(&root);
    git_repo(&rs[1].path());
    open_pr_list(&mut m);
    set_query(&mut m, "parser");
    assert!(label(&m).contains("Try a new parser"));

    let cmds = run_slash(&mut m, "/create exp/parser");
    feed(&mut m, cmds);
    let msg = answer(m.update(ch('y'))).expect("confirmed worktree did not run");
    m.update(msg);

    let matches: Vec<_> = m.all.iter().filter(|it| it.pr.number == 8).collect();
    assert_eq!(matches.len(), 1, "{:?}", rows(&m));
    assert!(!matches[0].path.is_empty());
}

// If a confirmed repository creation finishes after opening a secondary list,
// it updates the saved repository list and leaves the visible list alone.
#[test]
fn create_updates_saved_repository_list_only() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    let mut m = new_model(&rs, "");
    only_main(&mut m);
    let cmds = run_slash(&mut m, "/create acme/bravo");
    feed(&mut m, cmds);
    let cmds = m.update(ch('y'));
    assert!(m.busy.contains("creat"), "{:?}", m.busy);

    open_worktrees(&mut m);
    let visible = rows(&m);
    let msg = answer(cmds).expect("confirmed repository creation did not run");
    m.update(msg);
    assert_eq!(rows(&m), visible);

    m.restore();
    assert_eq!(
        rows(&m),
        vec!["github.com/acme/alpha", "github.com/acme/bravo"]
    );
    assert!(m.note.contains("created"), "{:?}", m.note);
}

// What only the remote has goes at the top of the list, once, without moving
// the selection.
#[test]
fn remote_branches_join_the_list() {
    let root = TempDir::new();
    let (mut m, _) = branch_model(&root);
    let cmds = open_branches(&mut m);
    assert!(m.busy.contains("asking the remotes"), "{:?}", m.busy);
    let before = label(&m);
    let Some(Msg::RemoteBranches(msg)) = answer(cmds) else {
        panic!("no answer from the remotes")
    };
    let again = RemoteBranches {
        path: msg.path.clone(),
        branches: msg.branches.clone(),
        busy_tag: msg.busy_tag,
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
    let cmds = open_branches(&mut m);
    feed(&mut m, cmds);
    assert!(
        m.view.len() == 3 && m.note.contains("origin: Could not read"),
        "{:?} {:?}",
        rows(&m),
        m.note
    );

    let (mut m, _) = branch_model(&root);
    let cmds = open_branches(&mut m);
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
    let cmds = open_branches(&mut m);
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
