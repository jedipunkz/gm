//! gm migrate.

use super::*;

fn init_repo(dir: &str, origin: &str) -> String {
    mkdir(dir);
    git(dir, &["init", "-q"]);
    if !origin.is_empty() {
        git(dir, &["remote", "add", "origin", origin]);
    }
    dir.to_string()
}

// What -r is for: find the working copies under a directory, leave the
// ones already in the tree alone, and report the ones that cannot move
// instead of abandoning the run.
#[test]
fn migrate_recursive() {
    let base = TempDir::new();
    let root = base.join("tree");
    mkdir(&root);
    let good = init_repo(&base.join("src/good"), "https://github.com/acme/good");
    let also = init_repo(
        &base.join("src/deeper/also"),
        "git@github.com:acme/also.git",
    );
    let no_remote = init_repo(&base.join("src/noremote"), "");
    // An origin that would put it outside the root is not a place to go.
    let escaping = init_repo(&base.join("src/escaping"), "https://github.com/../../evil");
    init_repo(
        &paths::join(&root, "github.com/acme/already"),
        "https://github.com/acme/already",
    );
    // A .git file is a worktree or a submodule, and moving it breaks the link.
    let linked = base.join("src/linked");
    mkdir(&linked);
    write(&paths::join(&linked, ".git"), "gitdir: /elsewhere\n");

    let r = run_in(&tree(&root), "", Config::default(), |a| {
        a.migrate(&args(&["-r", "--dry-run", &base.path()]))
    });
    r.res.unwrap();
    for want in [
        format!("{good} -> {}", paths::join(&root, "github.com/acme/good")),
        format!("{also} -> {}", paths::join(&root, "github.com/acme/also")),
        format!("{no_remote}: has no origin remote"),
        format!("{escaping}: has an origin gm cannot read"),
        format!("{linked}: is a worktree or submodule"),
    ] {
        assert!(
            r.err.contains(&want),
            "the output is missing {want:?}:\n{}",
            r.err
        );
    }
    // A repository already under the root is not a candidate at all.
    assert!(!r.err.contains("already"), "{}", r.err);
    // --dry-run moves nothing.
    assert!(exists(&good));
}

// A worktree's .git file names the repository's path, so moving the
// repository alone leaves every checkout answering "not a git repository".
// Both kinds are repaired: one elsewhere, and one kept inside the
// repository's directory, which moves with it.
// The dry run names where a repository and its checkouts would go, and a
// destination already taken is fatal for a named directory but only a skip
// under -r.
#[test]
fn migrate_dry_run_and_a_taken_destination() {
    let base = TempDir::new();
    let root = base.join("tree");
    mkdir(&root);
    let src = init_repo(&base.join("src/alpha"), "https://github.com/acme/alpha");
    let outside = base.join("wt/login");
    git(&src, &["commit", "-q", "--allow-empty", "-m", "init"]);
    repo::add_worktree(&base.path(), &src, &outside, "feat/login").unwrap();
    let norepo = base.join("src/norepo");
    mkdir(&norepo);

    let r = run_in(&tree(&root), "", Config::default(), |a| {
        a.migrate(&args(&["--dry-run", &src]))
    });
    r.res.unwrap();
    let dst = paths::join(&root, "github.com/acme/alpha");
    assert!(
        r.err.contains(&format!("would move {src} -> {dst}")),
        "{}",
        r.err
    );
    assert!(
        r.err.contains("and repair its 1 worktree: feat/login"),
        "{}",
        r.err
    );
    assert!(exists(&src) && !exists(&dst), "the dry run moved nothing");
    // A destination already on disk is fatal for a named directory.
    mkdir(&dst);
    let r = run_in(&tree(&root), "", Config::default(), |a| {
        a.migrate(&args(&[&src]))
    });
    let e = r.res.expect_err("a taken destination must stop the move");
    assert!(e.0.contains("which already exists"), "{e}");

    // Under -r the taken destination is only a skip on a candidate; the
    // search turns up repositories only, so the empty directory is silent.
    let r = run_in(&tree(&root), "", Config::default(), |a| {
        a.migrate(&args(&["--dry-run", "-r", &base.join("src")]))
    });
    r.res.unwrap();
    assert!(
        r.err.contains(&format!(
            "skip     {src}: would move to {dst}, which already exists"
        )),
        "the taken destination was not skipped:\n{}",
        r.err
    );
    // A directory that is not a repository is not a candidate at all.
    assert!(!r.err.contains("norepo"), "{}", r.err);
}

#[test]
fn migrate_repairs_the_worktrees() {
    let base = TempDir::new();
    let root = base.join("tree");
    mkdir(&root);
    let src = base.join("src/alpha");
    git_repo(&src);
    git(
        &src,
        &["remote", "add", "origin", "https://github.com/acme/alpha"],
    );
    let outside = base.join("wt/login");
    repo::add_worktree(&base.path(), &src, &outside, "feat/login").unwrap();
    repo::add_worktree(&base.path(), &src, &paths::join(&src, "inner"), "fix/inner").unwrap();

    // --dry-run says so, and touches nothing.
    let dry = run_in(&tree(&root), "", Config::default(), |a| {
        a.migrate(&args(&["--dry-run", &src]))
    });
    dry.res.unwrap();
    // In git's order, which is not the order they were made in.
    for want in ["and repair its 2 worktrees: ", "feat/login", "fix/inner"] {
        assert!(dry.err.contains(want), "{want:?}:\n{}", dry.err);
    }
    assert!(exists(&src));

    let r = run_in(&tree(&root), "", Config::default(), |a| {
        a.migrate(&args(&["-y", &src]))
    });
    r.res.unwrap();
    let dst = paths::join(&root, "github.com/acme/alpha");
    assert_eq!(r.out.trim(), dst);
    assert!(r.err.contains("repaired 2 worktrees"), "{}", r.err);
    for wt in [outside, paths::join(&dst, "inner")] {
        assert!(
            repo::git_in(&wt, &["status", "--porcelain"]).is_ok(),
            "{wt} is still broken"
        );
    }
    assert_eq!(repo::other_worktrees_of(&dst).len(), 2);
}

// Naming a directory is a claim that it should move, so a problem with it
// is fatal rather than a skip.
#[test]
fn migrate_named_directory_still_errors() {
    let base = TempDir::new();
    let no_remote = init_repo(&base.join("src/noremote"), "");
    let r = run_in(&tree(&base.join("tree")), "", Config::default(), |a| {
        a.migrate(&args(&["--dry-run", &no_remote]))
    });
    assert!(r.res.unwrap_err().0.contains("has no origin remote"));
}

// A repository a secondary root already holds is in place there: a stray
// clone of it must not become a second copy under the primary root, and
// the placed one must not be moved to the primary root either.
#[test]
fn migrate_respects_the_other_roots() {
    let base = TempDir::new();
    let (first, second) = (base.join("r1"), base.join("r2"));
    mkdir(&first);
    let placed = init_repo(
        &paths::join(&second, "github.com/acme/x"),
        "https://github.com/acme/x",
    );
    let stray = init_repo(&base.join("stray/x"), "https://github.com/acme/x");
    let t = Tree {
        roots: vec![first.clone(), second.clone()],
    };

    let r = run_in(&t, "", Config::default(), |a| {
        a.migrate(&args(&["--dry-run", &placed]))
    });
    r.res.unwrap();
    assert!(r.err.contains(&format!("in place {placed}")), "{}", r.err);

    let r = run_in(&t, "", Config::default(), |a| {
        a.migrate(&args(&["--dry-run", &stray]))
    });
    let e = r.res.expect_err("a second copy of acme/x would be made").0;
    assert!(e.contains(&placed), "{e}");
}
