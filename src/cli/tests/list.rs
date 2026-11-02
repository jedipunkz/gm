//! gm list, gm status and gm root.

use super::*;

// --unique is a property of the whole tree: a query must not shorten a
// name down to one that gm rm would call ambiguous.
#[test]
fn list_unique_stays_unique_under_a_query() {
    let root = TempDir::new();
    for rel in [
        "github.com/alice/gm",
        "github.com/alice/tools",
        "github.com/bob/gm",
    ] {
        mkdir(&root.join(&format!("{rel}/.git")));
    }
    let t = tree(&root.path());
    let r = run_in(&t, "", Config::default(), |a| {
        a.list(&args(&["--unique", "alice"]))
    });
    r.res.unwrap();
    let got: Vec<&str> = r.out.split_whitespace().collect();
    assert_eq!(got, vec!["alice/gm", "tools"]);
    for name in got {
        assert!(
            t.resolve(name).is_ok(),
            "printed name {name:?} does not resolve"
        );
    }
}

#[test]
fn list_unique_disambiguates_matching_roots() {
    let root = TempDir::new();
    let r1 = root.join("r1");
    let r2 = root.join("r2");
    for dir in [&r1, &r2] {
        mkdir(&format!("{dir}/github.com/acme/alpha/.git"));
    }
    let t = Tree {
        roots: vec![r1.clone(), r2.clone()],
    };
    let r = run_in(&t, "", Config::default(), |a| {
        a.list(&args(&["--unique", "alpha"]))
    });
    r.res.unwrap();
    let got: Vec<String> = r.out.split_whitespace().map(str::to_string).collect();
    assert_eq!(
        got,
        vec![
            format!("{r1}/github.com/acme/alpha"),
            format!("{r2}/github.com/acme/alpha"),
        ]
    );
    for name in &got {
        assert!(
            t.resolve(name).is_ok(),
            "printed name {name:?} does not resolve"
        );
    }
}

// Each reason a repository is listed, and the one reason it is not.
#[test]
fn status_finds_unfinished_work() {
    let tmp = TempDir::new();
    let root = tmp.join("root");
    let t = tree(&root);

    // clean: one commit, nothing changed, and an upstream it matches.
    let origin = tmp.join("origin.git");
    git(
        &tmp.path(),
        &["init", "-q", "--bare", "-b", "main", &origin],
    );
    let clean = Repo {
        root: root.clone(),
        rel: "github.com/acme/clean".into(),
    };
    mkdir(&paths::dir(&clean.path()));
    git(&tmp.path(), &["clone", "-q", &origin, &clean.path()]);
    git(
        &clean.path(),
        &["commit", "-q", "--allow-empty", "-m", "one"],
    );
    git(&clean.path(), &["push", "-q", "-u", "origin", "main"]);

    // ahead: the same, with a commit that was never pushed.
    let ahead = Repo {
        root: root.clone(),
        rel: "github.com/acme/ahead".into(),
    };
    git(&tmp.path(), &["clone", "-q", &origin, &ahead.path()]);
    git(
        &ahead.path(),
        &["commit", "-q", "--allow-empty", "-m", "two"],
    );

    // dirty: a repository with no remote at all and an uncommitted file.
    let dirty = Repo {
        root: root.clone(),
        rel: "github.com/acme/dirty".into(),
    };
    git_repo(&dirty.path());
    write(&paths::join(&dirty.path(), "wip.txt"), "x\n");

    // A worktree holds work in progress by definition, and lives under a
    // dotted directory the repository walk never descends into.
    let wt = t.worktree_dir(&dirty, "feat/login");
    repo::add_worktree(&root, &dirty.path(), &wt, "feat/login").unwrap();
    write(&paths::join(&wt, "scratch.txt"), "y\n");

    let status = |a: &[&str]| {
        let r = run_in(&t, "", Config::default(), |app| app.status(&args(a)));
        r.res.unwrap();
        r.out
    };
    let out = status(&[]);
    assert!(
        !out.contains("acme/clean"),
        "a clean repository in sync was listed:\n{out}"
    );
    for want in [
        "github.com/acme/ahead",
        "1 unpushed",
        "github.com/acme/dirty",
        "1 changed file",
        "no upstream",
        ".worktrees/github.com/acme/dirty/feat/login",
    ] {
        assert!(
            out.contains(want),
            "status did not mention {want:?}:\n{out}"
        );
    }

    // --dirty drops the repository whose only news is an unpushed commit.
    let out = status(&["--dirty"]);
    assert!(
        !out.contains("acme/ahead") && out.contains("acme/dirty"),
        "{out}"
    );
    // --unpushed is the other way round.
    let out = status(&["--unpushed"]);
    assert!(
        out.contains("acme/ahead") && !out.contains("acme/dirty"),
        "{out}"
    );
    // -a says so about the ones that are fine, rather than staying silent.
    let out = status(&["-a"]);
    assert!(out.contains("acme/clean") && out.contains("clean"), "{out}");
}

#[test]
fn status_lists_registered_worktrees_not_orphan_directories() {
    let tmp = TempDir::new();
    let root = tmp.join("root");
    let r = Repo {
        root: root.clone(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());

    let external = tmp.join("external/login");
    repo::add_worktree(&tmp.path(), &r.path(), &external, "feat/login").unwrap();
    write(&paths::join(&external, "wip.txt"), "registered worktree\n");

    // This looks like a checkout to a filesystem walk but Git does not list it.
    let orphan = paths::join(&root, ".worktrees/github.com/acme/alpha/orphan");
    mkdir(&paths::join(&orphan, ".git"));
    write(&paths::join(&orphan, "wip.txt"), "orphan\n");

    let run = run_in(&tree(&root), "", Config::default(), |app| {
        app.status(&args(&["-a"]))
    });
    run.res.unwrap();
    assert!(
        run.out.contains(&external),
        "missing registered worktree:\n{}",
        run.out
    );
    assert!(
        !run.out.contains("orphan"),
        "listed an orphan directory:\n{}",
        run.out
    );
}

#[test]
fn status_worktree_rows_match_finder_aggregate() {
    let tmp = TempDir::new();
    let root = tmp.join("root");
    let r = Repo {
        root: root.clone(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    write(&paths::join(&r.path(), "main.txt"), "main work\n");

    let external = tmp.join("external/login");
    repo::add_worktree(&tmp.path(), &r.path(), &external, "feat/login").unwrap();
    write(&paths::join(&external, "login.txt"), "worktree work\n");

    let run = run_in(&tree(&root), "", Config::default(), |app| {
        app.status(&args(&["--dirty"]))
    });
    run.res.unwrap();
    let rows = run.out.lines().collect::<Vec<_>>();
    let reported_dirty = rows
        .iter()
        .map(|line| {
            line.split_once("  ")
                .and_then(|(_, summary)| summary.split_whitespace().next())
                .and_then(|count| count.parse::<usize>().ok())
                .unwrap_or(0)
        })
        .sum::<usize>();
    assert_eq!(
        rows.len(),
        2,
        "expected one row per live checkout:\n{}",
        run.out
    );
    assert_eq!(
        reported_dirty,
        repo::status_with_worktrees(&r.path()).dirty,
        "{}",
        run.out
    );
}

#[test]
fn status_keeps_the_root_spelling_and_deduplicates_symlinked_roots() {
    let tmp = TempDir::new();
    let real_root = tmp.join("real");
    let linked_root = tmp.join("linked");
    std::os::unix::fs::symlink(&real_root, &linked_root).unwrap();
    let r = Repo {
        root: real_root.clone(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    let worktree = paths::join(&real_root, ".worktrees/github.com/acme/alpha/login");
    repo::add_worktree(
        &paths::join(&real_root, ".worktrees"),
        &r.path(),
        &worktree,
        "feat/login",
    )
    .unwrap();

    let roots = Tree {
        roots: vec![linked_root.clone(), real_root],
    };
    let run = run_in(&roots, "", Config::default(), |app| {
        app.status(&args(&["-a", "-p"]))
    });
    run.res.unwrap();
    let rows = run.out.lines().collect::<Vec<_>>();
    let canonical_worktree = std::fs::canonicalize(&worktree)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(rows.len(), 2, "duplicate canonical paths:\n{}", run.out);
    assert!(rows.iter().any(|line| line.starts_with(&linked_root)));
    assert!(!rows.iter().any(|line| line.starts_with(&r.path())));
    assert!(
        rows.iter()
            .any(|line| line.starts_with(&canonical_worktree)),
        "missing worktree path {canonical_worktree}:\n{}",
        run.out
    );
}

// Unpushed work does not have to sit on the branch checked out or on one with
// an upstream: a branch that was never pushed counts, and a repository with
// no remotes keeps the behaviour it had, since everything it has is unshared
// by definition rather than by accident.
#[test]
fn status_counts_unpushed_off_the_checked_out_branch() {
    let tmp = TempDir::new();
    let origin = tmp.join("origin.git");
    git(
        &tmp.path(),
        &["init", "-q", "--bare", "-b", "main", &origin],
    );
    let spare = Repo {
        root: tmp.path(),
        rel: "github.com/acme/spare".into(),
    };
    mkdir(&paths::dir(&spare.path()));
    git(&tmp.path(), &["clone", "-q", &origin, &spare.path()]);
    git(
        &spare.path(),
        &["commit", "-q", "--allow-empty", "-m", "base"],
    );
    git(&spare.path(), &["push", "-q", "-u", "origin", "main"]);

    let status = |a: &[&str]| {
        let r = run_in(&tree(&tmp.path()), "", Config::default(), |app| {
            app.status(&args(a))
        });
        r.res.unwrap();
        r.out
    };
    // In sync: not listed, as before.
    assert!(
        !status(&[]).contains("acme/spare"),
        "an in-sync clone was listed"
    );

    // A commit on a branch nobody pushed. The branch is switched away from,
    // so no shortcut through HEAD would ever see it.
    git(&spare.path(), &["checkout", "-q", "-b", "feat/wip"]);
    git(
        &spare.path(),
        &["commit", "-q", "--allow-empty", "-m", "wip"],
    );
    git(&spare.path(), &["checkout", "-q", "main"]);

    let out = status(&[]);
    assert!(out.contains("github.com/acme/spare"), "{out}");
    assert!(out.contains("1 unpushed"), "{out}");
    let out = status(&["--unpushed"]);
    assert!(out.contains("acme/spare"), "{out}");

    // A repository with no remote keeps today's behaviour.
    let lonely = Repo {
        root: tmp.path(),
        rel: "github.com/acme/lonely".into(),
    };
    git_repo(&lonely.path());
    assert!(
        !status(&[]).contains("acme/lonely"),
        "a repository with no remote was listed for its unpushed work"
    );
}

#[test]
fn status_aligns_summaries_for_wide_names() {
    let root = TempDir::new();
    let ascii = Repo {
        root: root.path(),
        rel: "github.com/acme/z".into(),
    };
    git_repo(&ascii.path());
    write(&paths::join(&ascii.path(), "wip.txt"), "x\n");

    let wide = Repo {
        root: root.path(),
        rel: "github.com/acme/ほげほげ".into(),
    };
    git_repo(&wide.path());
    write(&paths::join(&wide.path(), "wip.txt"), "y\n");

    let t = tree(&root.path());
    let run = run_in(&t, "", Config::default(), |app| {
        app.status(&args(&["--dirty"]))
    });
    run.res.unwrap();
    let out = run.out.lines().map(str::to_string).collect::<Vec<_>>();

    let summary = "1 changed file, no upstream";
    let starts = out
        .iter()
        .map(|line| {
            let i = line.find(summary).expect(line);
            Span::raw(&line[..i]).width()
        })
        .collect::<Vec<_>>();
    assert_eq!(starts.len(), 2, "{out:?}");
    assert_eq!(starts[0], starts[1], "{out:?}");
}

// gm root answers with the primary root, and with --all with every root in
// the order the config names them.
#[test]
fn root_prints_the_roots() {
    let root = TempDir::new();
    let (r1, r2) = (root.join("r1"), root.join("r2"));
    let t = Tree {
        roots: vec![r1.clone(), r2.clone()],
    };
    let res = run_in(&t, "", Config::default(), |a| a.root(&[]));
    res.res.unwrap();
    assert_eq!(res.out.trim(), r1.to_string());
    let res = run_in(&t, "", Config::default(), |a| a.root(&args(&["--all"])));
    res.res.unwrap();
    assert_eq!(res.out.trim(), format!("{r1}\n{r2}"));
}

// gm list without a flag prints the names it sorts by, one per line.
#[test]
fn list_prints_the_rel_paths() {
    let root = TempDir::new();
    let r = Repo {
        root: root.path(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    let res = run_in(&tree(&root.path()), "", Config::default(), |a| a.list(&[]));
    res.res.unwrap();
    assert_eq!(res.out.trim(), "github.com/acme/alpha", "{}", res.out);
}
