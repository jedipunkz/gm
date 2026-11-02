//! gm get and gm create.

use super::*;

// get searches every configured root before cloning into the primary one,
// and the finder returns the root where the repository was found.
#[test]
fn get_finds_repository_under_a_secondary_root() {
    let base = TempDir::new();
    let blocked = base.join("blocked");
    write(&blocked, "not a directory");
    let primary = paths::join(&blocked, "root");
    let secondary = base.join("secondary");
    let dst = paths::join(&secondary, "example.com/acme/alpha");
    git_repo(&dst);
    let t = Tree {
        roots: vec![primary.clone(), secondary],
    };
    let state = base.join("state/frecency.json");

    let already_present = run_in(&t, &state, Config::default(), |a| {
        a.get(&args(&["example.com/acme/alpha"]))
    });
    already_present.res.unwrap();
    assert!(
        already_present.err.contains(&format!("exists   {dst}")),
        "{}",
        already_present.err
    );
    assert!(!exists(&paths::join(&primary, "example.com/acme/alpha")));
    assert_eq!(History::open(&state).visit(&dst).count, 1);

    let update = run_in(&t, &state, Config::default(), |a| {
        a.get(&args(&["-u", "example.com/acme/alpha"]))
    });
    update.res.unwrap();
    assert!(
        update.err.contains(&format!("update   {dst}")),
        "{}",
        update.err
    );
    assert!(!exists(&paths::join(&primary, "example.com/acme/alpha")));
    assert_eq!(History::open(&state).visit(&dst).count, 2);

    let mut hist = History::open(&state);
    let finder_get = run_in(&t, &state, Config::default(), |a| {
        a.act(
            finder::Outcome {
                action: Action::Get,
                arg: "example.com/acme/alpha".into(),
            },
            &mut hist,
        )
    });
    finder_get.res.unwrap();
    assert_eq!(finder_get.out.trim(), dst);
    assert!(finder_get.err.contains(&format!("exists   {dst}")));
    assert!(!exists(&paths::join(&primary, "example.com/acme/alpha")));
    assert_eq!(History::open(&state).visit(&dst).count, 3);
}

// A reference with ".." in it is refused before anything is made, so no
// directory appears outside the root.
#[test]
fn get_and_create_refuse_to_leave_the_root() {
    let base = TempDir::new();
    let root = base.join("root");
    mkdir(&root);
    for cmd in ["get", "create"] {
        let r = run_in(&tree(&root), "", Config::default(), |a| {
            let reference = args(&["example.com/../../evil"]);
            if cmd == "get" {
                a.get(&reference)
            } else {
                a.create(&reference)
            }
        });
        assert!(r.res.is_err(), "gm {cmd} accepted it");
        assert!(
            !exists(&base.join("evil")),
            "gm {cmd} made a directory outside the root"
        );
    }
}

// gm get -u matches ghq: the checked-out branch moves across what the fetch
// brought when the tree is clean, a divergence is left alone and said, and
// so is dirt.
#[test]
fn get_u_fast_forwards_a_clean_clone() {
    let tmp = TempDir::new();
    let origin = tmp.join("origin.git");
    git(
        &tmp.path(),
        &["init", "-q", "--bare", "-b", "main", &origin],
    );
    let reference = format!("file://localhost{origin}");
    let rel = repo::rel_path_of(&repo::normalize_url(&reference, false).unwrap());

    // Someone else has a commit pushed already, and the clone is made before
    // it arrives.
    let w = tmp.join("w");
    git(&tmp.path(), &["clone", "-q", &origin, &w]);
    write(&paths::join(&w, "a.txt"), "1\n");
    git(&w, &["add", "a.txt"]);
    git(&w, &["commit", "-q", "-m", "one"]);
    git(&w, &["push", "-q", "-u", "origin", "main"]);
    let root = tmp.join("root");
    let dst = paths::join(&root, &rel);
    mkdir(&paths::dir(&dst));
    git(&tmp.path(), &["clone", "-q", &origin, &dst]);
    let head = git(&dst, &["rev-parse", "HEAD"]);

    // Another commit arrives after the clone was made.
    write(&paths::join(&w, "a.txt"), "2\n");
    git(&w, &["add", "a.txt"]);
    git(&w, &["commit", "-q", "-m", "two"]);
    git(&w, &["push", "-q", "origin", "main"]);

    let r = run_in(&tree(&root), "", Config::default(), |a| {
        a.get(&args(&["-u", &reference]))
    });
    r.res.unwrap();
    let out = r.err.clone();
    assert!(out.contains(&format!("ff       {dst}: 1 commit")), "{out}");
    assert_ne!(head, git(&dst, &["rev-parse", "HEAD"]));
    assert_eq!(
        git(&w, &["rev-parse", "HEAD"]),
        git(&dst, &["rev-parse", "HEAD"])
    );

    // Nothing new brought: the clone stays where it is, and no ff is said.
    let r = run_in(&tree(&root), "", Config::default(), |a| {
        a.get(&args(&["-u", &reference]))
    });
    r.res.unwrap();
    assert!(!r.err.contains(&format!("ff       {dst}")), "{}", r.err);

    // A local commit makes the branch diverge; it is never merged.
    write(&paths::join(&dst, "b.txt"), "local\n");
    git(&dst, &["add", "b.txt"]);
    git(&dst, &["commit", "-q", "-m", "local"]);
    let diverged_head = git(&dst, &["rev-parse", "HEAD"]);
    write(&paths::join(&w, "a.txt"), "3\n");
    git(&w, &["add", "a.txt"]);
    git(&w, &["commit", "-q", "-m", "three"]);
    git(&w, &["push", "-q", "origin", "main"]);

    let r = run_in(&tree(&root), "", Config::default(), |a| {
        a.get(&args(&["-u", &reference]))
    });
    r.res.unwrap();
    assert!(
        r.err.contains(&format!("skip     {dst}: diverged")),
        "{}",
        r.err
    );
    assert_eq!(diverged_head, git(&dst, &["rev-parse", "HEAD"]));

    // Dirt is are refused a move as well: nothing is merged into it.
    write(&paths::join(&dst, "c.txt"), "uncommitted\n");
    let r = run_in(&tree(&root), "", Config::default(), |a| {
        a.get(&args(&["-u", &reference]))
    });
    r.res.unwrap();
    assert!(
        r.err.contains(&format!("skip     {dst}: dirty")),
        "{}",
        r.err
    );
    assert_eq!(diverged_head, git(&dst, &["rev-parse", "HEAD"]));
}

// The clone gm get made owns its submodules, so -u keeps them with it: the
// submodule is brought to what the fetched commits name.
#[test]
fn get_u_updates_submodules() {
    let tmp = TempDir::new();
    // The submodule has two commits waiting to be picked up.
    let sub_origin = tmp.join("sub.git");
    git(
        &tmp.path(),
        &["init", "-q", "--bare", "-b", "main", &sub_origin],
    );
    let sub = tmp.join("sub");
    git(&tmp.path(), &["clone", "-q", &sub_origin, &sub]);
    write(&paths::join(&sub, "f.txt"), "1\n");
    git(&sub, &["add", "f.txt"]);
    git(&sub, &["commit", "-q", "-m", "one"]);
    git(&sub, &["push", "-q", "-u", "origin", "main"]);

    // The parent records it, and the clone owns it from the start.
    let parent_origin = tmp.join("parent.git");
    git(
        &tmp.path(),
        &["init", "-q", "--bare", "-b", "main", &parent_origin],
    );
    let p = tmp.join("p-src");
    git(&tmp.path(), &["clone", "-q", &parent_origin, &p]);
    git(
        &p,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            "-q",
            &sub_origin,
            "sub",
        ],
    );
    git(&p, &["commit", "-q", "-m", "with submodule"]);
    git(&p, &["push", "-q", "-u", "origin", "main"]);

    let reference = format!("file://localhost{parent_origin}");
    let rel = repo::rel_path_of(&repo::normalize_url(&reference, false).unwrap());
    let root = tmp.join("root");
    let dst = paths::join(&root, &rel);
    mkdir(&paths::dir(&dst));
    git(
        &tmp.path(),
        &[
            "-c",
            "protocol.file.allow=always",
            "clone",
            "-q",
            "--recursive",
            &parent_origin,
            &dst,
        ],
    );
    // gm's own update fetches the submodule through file transport, so it is
    // allowed where it runs — not globally, just in this clone.
    git(&dst, &["config", "protocol.file.allow", "always"]);
    let old = git(&paths::join(&dst, "sub"), &["rev-parse", "HEAD"]);

    // The submodule moves; the parent records the move.
    write(&paths::join(&sub, "f.txt"), "2\n");
    git(&sub, &["add", "f.txt"]);
    git(&sub, &["commit", "-q", "-m", "two"]);
    git(&sub, &["push", "-q", "origin", "main"]);
    let _ = git(&p, &["-C", "sub", "pull", "-q"]);
    git(&p, &["add", "sub"]);
    git(&p, &["commit", "-q", "-m", "new submodule"]);
    git(&p, &["push", "-q", "origin", "main"]);

    let r = run_in(&tree(&root), "", Config::default(), |a| {
        a.get(&args(&["-u", &reference]))
    });
    r.res.unwrap();
    assert_ne!(old, git(&paths::join(&dst, "sub"), &["rev-parse", "HEAD"]));
    assert_eq!(
        git(&sub, &["rev-parse", "HEAD"]),
        git(&paths::join(&dst, "sub"), &["rev-parse", "HEAD"])
    );
}

// One repository under two roots is ambiguous to gm get: cloning a
// second copy or updating the first is a decision the user has to make.
#[test]
fn get_refuses_a_repository_two_roots_have() {
    let base = TempDir::new();
    let first = base.join("first");
    let second = base.join("second");
    let rel = "github.com/acme/alpha";
    for root in [&first, &second] {
        git_repo(&paths::join(root, rel));
    }
    let t = Tree {
        roots: vec![first.clone(), second.clone()],
    };

    let r = run_in(&t, "", Config::default(), |a| {
        a.get(&args(&["github.com/acme/alpha"]))
    });
    let e = r.res.expect_err("gm get picked one of two roots silently");
    assert!(
        e.0.contains(&first) && e.0.contains(&second),
        "the error does not name both copies:\n{}",
        e.0
    );

    let r = run_in(&t, "", Config::default(), |a| {
        a.get(&args(&["-u", "github.com/acme/alpha"]))
    });
    assert!(
        r.res.is_err(),
        "gm get -u updated one copy without being told which:\n{}",
        r.res.unwrap_err().0
    );
}

#[test]
fn get_prunes_host_after_failed_clone() {
    let base = TempDir::new();
    let root = base.join("root");
    mkdir(&root);
    let source = base.join("missing-repository");
    let reference = format!("file://localhost{source}");

    let r = run_in(&tree(&root), "", Config::default(), |a| {
        a.get(&args(&[&reference]))
    });

    let host = paths::join(&root, "localhost");
    let err = r.res.expect_err("gm get succeeded for a missing source");
    assert!(!exists(&host), "failed clone left host path {host}");
    assert_eq!(err.0, "exit status 128", "gm get changed the clone error");
}

// The clone flags are read the way help words them: shallow, one branch,
// silent.
#[test]
fn get_takes_the_clone_flavours() {
    let tmp = TempDir::new();
    let origin = tmp.join("origin.git");
    git(
        &tmp.path(),
        &["init", "-q", "--bare", "-b", "main", &origin],
    );
    // A second branch, so -b has something to check out.
    let w = tmp.join("w");
    git(&tmp.path(), &["clone", "-q", &origin, &w]);
    git(&w, &["commit", "-q", "--allow-empty", "-m", "one"]);
    git(&w, &["push", "-q", "origin", "HEAD:feature", "HEAD:main"]);
    let reference = format!("file://localhost{origin}");

    let root = tmp.join("root");
    let res = run_in(&tree(&root), "", Config::default(), |a| {
        a.get(&args(&[
            "--shallow",
            "-b",
            "feature",
            "-s",
            "--no-recursive",
            &reference,
        ]))
    });
    res.res.unwrap();
    let dst = repo::rel_path_of(&repo::normalize_url(&reference, false).unwrap());
    let dst = paths::join(&root, &dst);
    assert!(
        exists(&paths::join(&dst, ".git/shallow")),
        "not a shallow clone"
    );
    assert_eq!(
        repo::git_in(&dst, &["branch", "--show-current"]).unwrap(),
        "feature"
    );
}

// -p means full paths wherever gm takes it; SSH is spelled out, so gm get -p
// and gm create -p are mistakes rather than a quiet switch of protocol.
#[test]
fn ssh_is_its_own_flag() {
    let root = TempDir::new();
    let t = tree(&root.path());
    let made = run_in(&t, "", Config::default(), |a| {
        a.create(&args(&["--ssh", "github.com/acme/alpha"]))
    });
    let dir = made.res.map(|_| made.out.trim().to_string()).unwrap();
    assert_eq!(
        repo::git_in(&dir, &["remote", "get-url", "origin"]).unwrap(),
        "ssh://git@github.com/acme/alpha"
    );
    for cmd in ["get", "create"] {
        let r = run_in(&t, "", Config::default(), |a| {
            a.dispatch(&args(&[cmd, "-p", "acme/bravo"]))
        });
        assert_eq!(r.res, Err(USAGE_ERROR.into()), "gm {cmd} -p was accepted");
    }
}
