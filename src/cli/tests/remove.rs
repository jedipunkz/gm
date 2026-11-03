//! gm remove.

use super::*;

// gm rm takes the worktrees along with --force, so the work in each has
// to be named before the question, not only the repository's own.
#[test]
fn remove_names_the_work_in_the_worktrees() {
    let root = TempDir::new();
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    let t = tree(&root.path());
    let login = t.worktree_dir(&r, "feat/login");
    let timeout = t.worktree_dir(&r, "fix/timeout");
    repo::add_worktree(&root.path(), &r.path(), &login, "feat/login").unwrap();
    repo::add_worktree(&root.path(), &r.path(), &timeout, "fix/timeout").unwrap();
    write(&paths::join(&login, "wip.txt"), "wip\n");

    let res = run_in(&t, "", Config::default(), |a| {
        a.remove(&args(&["-y", "acme/alpha"]))
    });
    res.res.unwrap();
    assert!(
        res.err
            .contains("2 worktrees will go too: feat/login (1 changed), fix/timeout"),
        "{}",
        res.err
    );
    // The repository itself is clean, so it is not called dirty.
    assert!(!res.err.contains("alpha has uncommitted"), "{}", res.err);
    assert!(!exists(&r.path()) && !exists(&login));
}

// The bug where gm remove --dry-run printed the path and nothing else: the
// warnings a real run gives before asking are what a dry run is for.
#[test]
fn remove_dry_run_says_what_would_go() {
    let root = TempDir::new();
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    let t = tree(&root.path());
    let login = t.worktree_dir(&r, "feat/login");
    repo::add_worktree(&root.path(), &r.path(), &login, "feat/login").unwrap();
    write(&paths::join(&r.path(), "wip.txt"), "wip\n");
    write(&paths::join(&login, "wip.txt"), "wip\n");
    mkdir(&t.worktree_dir(&r, "scratch"));

    let res = run_in(&t, "", Config::default(), |a| {
        a.remove(&args(&["--dry-run", "acme/alpha"]))
    });
    res.res.unwrap();
    for want in [
        "alpha has uncommitted changes",
        "1 worktree will go too: feat/login (1 changed)",
        "also removed, not a worktree: scratch",
        "would remove",
    ] {
        assert!(res.err.contains(want), "{want:?} missing from {}", res.err);
    }
    assert!(exists(&r.path()) && exists(&login));
}

// A checkout whose home is outside gm's worktree directory is named by its
// path, and gm remove leaves it where it is: the warning is the only thing
// that says where git's entry points.
#[test]
fn remove_names_and_leaves_a_checkout_outside_the_worktree_dir() {
    let root = TempDir::new();
    let other = TempDir::new();
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    let t = tree(&root.path());
    let login = t.worktree_dir(&r, "feat/login");
    repo::add_worktree(&root.path(), &r.path(), &login, "feat/login").unwrap();
    let victim = paths::join(&other.path(), "victim");
    mkdir(&victim);
    write(&paths::join(&victim, "keep.txt"), "keep\n");
    retarget_worktree(&r.path(), &victim);

    let res = run_in(&t, "", Config::default(), |a| {
        a.remove(&args(&["-y", "acme/alpha"]))
    });
    res.res.unwrap();
    assert!(
        res.err
            .contains(&format!("1 worktree will go too: feat/login at {victim}")),
        "{}",
        res.err
    );
    assert!(
        res.err.contains(&format!(
            "left in place, not under {}",
            repo::worktrees_dir(&r)
        )),
        "{}",
        res.err
    );
    assert!(!exists(&r.path()) && !exists(&login), "gm's own went");
    assert!(exists(&victim) && exists(&paths::join(&victim, "keep.txt")));
}

// retarget_worktree points the one worktree entry a repository holds at dir,
// the way a copied or hand-edited repository would.
fn retarget_worktree(repo_dir: &str, dir: &str) {
    let admin = paths::join(repo_dir, ".git/worktrees");
    let mut names: Vec<String> = std::fs::read_dir(&admin)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 1, "one entry to retarget");
    names.sort();
    write(
        &paths::join(&paths::join(&admin, &names.remove(0)), "gitdir"),
        &format!("{dir}/.git\n"),
    );
}
