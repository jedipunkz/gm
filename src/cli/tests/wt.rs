//! gm wt.

use super::*;

// The round trip a script does: make a worktree, read where it went off
// stdout, then take it away again.
#[test]
fn wt_create_and_remove() {
    let root = TempDir::new();
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    let t = tree(&root.path());
    let state = root.join("state/frecency.json");

    let made = run_in(&t, &state, Config::default(), |a| {
        a.wt(&args(&["create", "acme/alpha", "feat/login"]))
    });
    made.res.unwrap();
    let dir = made.out.trim().to_string();
    assert_eq!(dir, t.worktree_dir(&r, "feat/login"));
    assert!(exists(&paths::join(&dir, ".git")));

    // The same branch twice is a mistake worth stopping at, not a silent
    // no-op: the second call would otherwise look like it worked.
    // gm new and gm rm answer under wt as well.
    let again = run_in(&t, &state, Config::default(), |a| {
        a.wt(&args(&["new", "acme/alpha", "feat/login"]))
    });
    assert!(again.res.unwrap_err().0.contains("already exists"));

    let gone = run_in(&t, &state, Config::default(), |a| {
        a.wt(&args(&["rm", "-y", "acme/alpha", "feat/login"]))
    });
    gone.res.unwrap();
    assert!(!exists(&dir));
    assert!(
        gone.err.contains("deleted  branch feat/login"),
        "{}",
        gone.err
    );
    assert!(!exists(&root.join(".worktrees/github.com")));
}

// The main worktree is the repository itself, and not in the list this
// checks against.
#[test]
fn wt_remove_refuses_what_is_not_a_worktree() {
    let root = TempDir::new();
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    let res = run_in(&tree(&root.path()), "", Config::default(), |a| {
        a.wt(&args(&["remove", "-y", "acme/alpha", "main"]))
    });
    assert!(res.res.unwrap_err().0.contains("no worktree"));
    assert!(exists(&r.path()));
}

// age_worktrees writes an epoch reflog line into every worktree's HEAD log,
// so gm reads all of them as idle since the epoch. The log's last line is
// where git's own writing shows up, so this line goes last.
fn age_worktrees(repo_dir: &str) {
    let admin = paths::join(repo_dir, ".git/worktrees");
    for e in std::fs::read_dir(&admin).unwrap() {
        let log = paths::join(
            &paths::join(&admin, &e.unwrap().file_name().to_string_lossy()),
            "logs/HEAD",
        );
        let line = "0000000000000000000000000000000000000000 0000000000000000000000000000000000000000 t t 1 +0000\taged\n";
        let old = std::fs::read_to_string(&log).unwrap_or_default();
        write(&log, &format!("{old}{line}"));
    }
}

#[test]
fn wt_expire_removes_the_idle_and_keeps_the_busy() {
    let root = TempDir::new();
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    let t = tree(&root.path());
    let wt_root = paths::join(&r.root, repo::WORKTREE_ROOT);
    for b in ["old/a", "old/b", "busy"] {
        repo::add_worktree(&wt_root, &r.path(), &t.worktree_dir(&r, b), b).unwrap();
    }
    write(
        &paths::join(&t.worktree_dir(&r, "busy"), "wip.txt"),
        "wip\n",
    );
    age_worktrees(&r.path());
    let old_a = t.worktree_dir(&r, "old/a");
    let old_b = t.worktree_dir(&r, "old/b");
    let busy = t.worktree_dir(&r, "busy");

    // The dry run names everything and touches nothing.
    let res = run_in(&t, "", Config::default(), |a| {
        a.wt(&args(&["expire", "--dry-run", "acme/alpha", "30d"]))
    });
    res.res.unwrap();
    // git reports the resolved paths, which on macOS is /private/var where
    // the tree was walked as /var, so the expectations are resolved too.
    let seen = |w: &str, dir: &str, after: &str| {
        let mut line = format!(
            "{w}{}",
            std::fs::canonicalize(dir).unwrap().to_string_lossy()
        );
        line.push_str(after);
        assert!(res.err.contains(&line), "{line:?}:\n{}", res.err);
    };
    seen("keep     ", &busy, " (uncommitted changes)");
    seen("would remove ", &old_a, "");
    seen("would remove ", &old_b, "");
    assert!(exists(&old_a) && exists(&busy));

    let res = run_in(&t, "", Config::default(), |a| {
        a.wt(&args(&["expire", "-y", "acme/alpha", "30d"]))
    });
    res.res.unwrap();
    assert!(
        res.err.contains("removed  2 worktrees, 2 branches"),
        "{}",
        res.err
    );
    assert!(!exists(&old_a) && !exists(&old_b) && exists(&busy));
    assert!(
        repo::git_in(&r.path(), &["show-ref", "--verify", "refs/heads/old/a"]).is_err(),
        "the branch went with the worktree"
    );

    // What is left is busy, so there is nothing idle left to remove.
    let res = run_in(&t, "", Config::default(), |a| {
        a.wt(&args(&["expire", "acme/alpha", "30d"]))
    });
    res.res.unwrap();
    assert!(
        res.err
            .contains(&format!("no worktree of {} idle for 30 days", r.rel)),
        "{}",
        res.err
    );
}

#[test]
fn wt_expire_reports_the_verb_errors() {
    let root = TempDir::new();
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    let t = tree(&root.path());
    let u = run_in(&t, "", Config::default(), |a| {
        a.wt(&args(&["expire", "acme/alpha"]))
    });
    assert!(u.res.unwrap_err().0.contains("gm wt expire"), "{}", u.err);
    let age = run_in(&t, "", Config::default(), |a| {
        a.wt(&args(&["expire", "acme/alpha", "30x"]))
    });
    assert!(
        age.res.unwrap_err().0.contains("is not an age"),
        "{}",
        age.err
    );
}

// A branch whose work holds commits that are nowhere else stays: the worktree
// goes, and the refusal is named.
#[test]
fn wt_remove_keeps_an_unmerged_branch() {
    let root = TempDir::new();
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    let t = tree(&root.path());

    // A branch that already exists is checked out where it is, not started.
    let pub_ = "feat/public";
    git(&r.path(), &["branch", pub_]);
    let made = run_in(&t, "", Config::default(), |a| {
        a.wt(&args(&["create", "acme/alpha", pub_]))
    });
    made.res.unwrap();
    assert!(made.err.contains("existing branch"), "{}", made.err);

    let login = t.worktree_dir(&r, "feat/login");
    let made = run_in(&t, "", Config::default(), |a| {
        a.wt(&args(&["create", "acme/alpha", "feat/login"]))
    });
    made.res.unwrap();
    assert!(made.err.contains("new branch"), "{}", made.err);

    // A commit the branch holds is work nobody else has, so -d refuses.
    write(&paths::join(&login, "work.txt"), "wip\n");
    git(&login, &["add", "work.txt"]);
    git(&login, &["commit", "-q", "-m", "wip"]);

    let res = run_in(&t, "", Config::default(), |a| {
        a.wt(&args(&["remove", "-y", "acme/alpha", "feat/login"]))
    });
    res.res.unwrap();
    assert!(
        res.err.contains("kept     (not merged) branch feat/login"),
        "{}",
        res.err
    );
    // The other checkout was never asked to go.
    assert!(!exists(&login) && exists(&t.worktree_dir(&r, pub_)));
}

#[test]
fn wt_remove_dry_run_names_uncommitted_work() {
    let root = TempDir::new();
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    let t = tree(&root.path());
    let login = t.worktree_dir(&r, "feat/login");
    repo::add_worktree(&root.path(), &r.path(), &login, "feat/login").unwrap();
    write(&paths::join(&login, "wip.txt"), "wip\n");

    let res = run_in(&t, "", Config::default(), |a| {
        a.wt(&args(&["remove", "--dry-run", "acme/alpha", "feat/login"]))
    });
    res.res.unwrap();
    assert!(res.err.contains("has uncommitted changes"), "{}", res.err);
    assert!(res.err.contains("would remove"), "{}", res.err);
    assert!(exists(&login));
}
