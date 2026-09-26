//! The CLI's tests: one run per behavior, both streams read back. They live
//! apart from the code the way the finder's do.

use super::*;
use ratatui::text::Span;

use crate::paths;
use crate::repo::Repo;
use crate::testutil::{TempDir, exists, git, git_repo, mkdir, write};

/// Run is one command run against a tree, with both streams captured.
struct Run {
    out: String,
    err: String,
    res: Result<()>,
}

fn run_in(tree: &Tree, state: &str, cfg: Config, f: impl FnOnce(&mut App) -> Result<()>) -> Run {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let res = {
        let mut a = App {
            cfg,
            tree: tree.clone(),
            state: state.into(),
            out: &mut out,
            err: &mut err,
        };
        f(&mut a)
    };
    Run {
        out: String::from_utf8(out).unwrap(),
        err: String::from_utf8(err).unwrap(),
        res,
    }
}

fn tree(root: &str) -> Tree {
    Tree {
        roots: vec![root.to_string()],
    }
}

fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

// The help text must not drift away from the table it is built from.
#[test]
fn usage_covers_every_command() {
    let u = usage();
    for c in COMMANDS {
        assert!(
            u.contains(&format!("gm {}", c.name)),
            "usage does not mention {}",
            c.name
        );
    }
}

// A new command must not shadow an existing name or alias.
#[test]
fn names_are_unique() {
    let mut seen = HashMap::new();
    for c in COMMANDS {
        for n in std::iter::once(&c.name).chain(c.aliases) {
            assert!(seen.insert(*n, c.name).is_none(), "{n} is claimed twice");
        }
    }
}

#[test]
fn flags_parse_the_go_way() {
    let tmp = TempDir::new();
    let flags = [
        Flag {
            names: &["u", "update"],
            value: None,
            usage: "",
        },
        Flag {
            names: &["b", "branch"],
            value: Some("branch"),
            usage: "",
        },
    ];
    let got = |a: &[&str]| {
        let mut p = None;
        let r = run_in(&tree(&tmp.path()), "", Config::default(), |app| {
            p = Some(parse(app, "gm t", &flags, &args(a))?);
            Ok(())
        });
        r.res.map(|_| {
            let p = p.unwrap();
            (p.on("u"), p.value("b"), p.args)
        })
    };
    assert_eq!(
        got(&["-u", "--branch", "x", "a", "-b"]).unwrap(),
        (true, "x".into(), args(&["a", "-b"]))
    );
    assert_eq!(
        got(&["--update=false", "-b=y", "--", "-u"]).unwrap(),
        (false, "y".into(), args(&["-u"]))
    );
    assert!(got(&["-u=TRUE"]).unwrap().0 && !got(&["-u=F"]).unwrap().0);
    assert_eq!(exit_code(&got(&["-u=yes"]).unwrap_err()), Some(2));
    assert_eq!(exit_code(&got(&["-x"]).unwrap_err()), Some(2));
    assert_eq!(exit_code(&got(&["-b"]).unwrap_err()), Some(2));
    assert_eq!(exit_code(&got(&["-h"]).unwrap_err()), Some(0));
}

// The spelling each shell needs, and no template placeholder left over.
#[test]
fn shell_snippets_bind_the_configured_key() {
    let tmp = TempDir::new();
    let cfg = |key: &str| Config {
        launch_key: key.into(),
        ..Default::default()
    };
    for (sh, want) in [
        ("fish", r"bind \cr __gm_jump"),
        ("zsh", "bindkey '^r' __gm_jump"),
        ("bash", r#"bind -x '"\C-r": __gm_jump'"#),
    ] {
        let r = run_in(&tree(&tmp.path()), "", cfg("ctrl-r"), |a| {
            a.shell(&args(&[sh]))
        });
        r.res.unwrap();
        assert!(
            r.out.contains(want),
            "gm shell {sh} does not bind Ctrl-R:\n{}",
            r.out
        );
        assert!(
            r.out.contains("Ctrl-R jumps"),
            "gm shell {sh} does not name the key:\n{}",
            r.out
        );
        assert!(
            !r.out.contains("{{"),
            "gm shell {sh} left a placeholder:\n{}",
            r.out
        );
    }
    // An unusable key must stop gm rather than print a binding that
    // silently does nothing.
    for bad in ["alt-r", "ctrl-shift-b", "ctrl-alt-b"] {
        let r = run_in(&tree(&tmp.path()), "", cfg(bad), |a| {
            a.shell(&args(&["zsh"]))
        });
        assert!(r.res.is_err(), "gm shell accepted {bad}");
    }
}

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

// Recording the visit and printing the path is the whole contract between
// the finder and the shell binding, and one jump is one visit.
#[test]
fn act_jump_records_once_and_prints() {
    let root = TempDir::new();
    let dst = root.join("github.com/acme/alpha");
    mkdir(&dst);
    let mut hist = History::open(&root.join("frecency.json"));
    let r = run_in(&tree(&root.path()), "", Config::default(), |a| {
        a.act(
            finder::Outcome {
                action: Action::Jump,
                arg: dst.clone(),
            },
            &mut hist,
        )
    });
    r.res.unwrap();
    assert_eq!(r.out.trim(), dst);
    assert_eq!(hist.visit(&dst).count, 1);
}

// Quitting the finder leaves no trace.
#[test]
fn act_none_does_nothing() {
    let root = TempDir::new();
    let mut hist = History::open(&root.join("frecency.json"));
    let r = run_in(&tree(&root.path()), "", Config::default(), |a| {
        a.act(finder::Outcome::default(), &mut hist)
    });
    r.res.unwrap();
    assert_eq!(r.out + &r.err, "");
}

// gm get writes the visit log itself, in the middle of act(); the copy the
// finder was handed was read before that. Saving the finder's copy
// afterwards would throw that write away, and anything else written to the
// log in that window.
#[test]
fn act_get_keeps_what_get_recorded() {
    let root = TempDir::new();
    let state = root.join("state/frecency.json");
    let dst = root.join("example.com/acme/alpha");
    git_repo(&dst); // already cloned: gm get reports it without the network
    let other = root.join("example.com/acme/other");
    git_repo(&other);

    // The finder reads the log when it opens, and something writes to it
    // afterwards.
    let mut hist = History::open(&state);
    History::open(&state).bump(&other).unwrap();

    let r = run_in(&tree(&root.path()), &state, Config::default(), |a| {
        a.act(
            finder::Outcome {
                action: Action::Get,
                arg: "example.com/acme/alpha".into(),
            },
            &mut hist,
        )
    });
    r.res.unwrap();
    assert!(
        r.out.contains(&dst),
        "the path was not printed: {:?}",
        r.out
    );

    let after = History::open(&state);
    assert_eq!(after.visit(&dst).count, 1, "the repository gm get reported");
    assert_eq!(
        after.visit(&other).count,
        1,
        "a visit written while the finder was open was discarded"
    );
}

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
    let again = run_in(&t, &state, Config::default(), |a| {
        a.wt(&args(&["create", "acme/alpha", "feat/login"]))
    });
    assert!(again.res.is_err());

    let gone = run_in(&t, &state, Config::default(), |a| {
        a.wt(&args(&["remove", "-y", "acme/alpha", "feat/login"]))
    });
    gone.res.unwrap();
    assert!(!exists(&dir));
    assert!(!exists(&root.join(".worktrees/github.com")));
}

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
            .contains("2 worktrees will be removed too: feat/login (1 changed), fix/timeout"),
        "{}",
        res.err
    );
    // The repository itself is clean, so it is not called dirty.
    assert!(!res.err.contains("alpha has uncommitted"), "{}", res.err);
    assert!(!exists(&r.path()) && !exists(&login));
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

// A missing or unknown verb says what the verbs are rather than doing one.
#[test]
fn wt_usage() {
    let root = TempDir::new();
    for a in [&[][..], &["add"], &["create", "only-one-arg"]] {
        let r = run_in(&tree(&root.path()), "", Config::default(), |app| {
            app.wt(&args(a))
        });
        assert!(r.res.is_err(), "gm wt {a:?} was accepted");
    }
}
