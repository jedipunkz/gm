use super::*;

fn init_repo(dir: &str, origin: &str) -> String {
    mkdir(dir);
    git(dir, &["init", "-q"]);
    if !origin.is_empty() {
        git(dir, &["remote", "add", "origin", origin]);
    }
    dir.to_string()
}

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
    let escaping = init_repo(&base.join("src/escaping"), "https://github.com/../../evil");
    init_repo(
        &paths::join(&root, "github.com/acme/already"),
        "https://github.com/acme/already",
    );
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
    assert!(!r.err.contains("already"), "{}", r.err);
    assert!(exists(&good));
}

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
    mkdir(&dst);
    let r = run_in(&tree(&root), "", Config::default(), |a| {
        a.migrate(&args(&[&src]))
    });
    let e = r.res.expect_err("a taken destination must stop the move");
    assert!(e.0.contains("which already exists"), "{e}");

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

    let dry = run_in(&tree(&root), "", Config::default(), |a| {
        a.migrate(&args(&["--dry-run", &src]))
    });
    dry.res.unwrap();
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

#[test]
fn migrate_named_directory_still_errors() {
    let base = TempDir::new();
    let no_remote = init_repo(&base.join("src/noremote"), "");
    let r = run_in(&tree(&base.join("tree")), "", Config::default(), |a| {
        a.migrate(&args(&["--dry-run", &no_remote]))
    });
    assert!(r.res.unwrap_err().0.contains("has no origin remote"));
}

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
