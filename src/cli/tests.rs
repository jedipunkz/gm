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

// Help and the version answer without gm.toml or the roots: a broken file
// must not hide how to fix it, or what version is broken.
#[test]
fn help_and_version_need_no_config() {
    for a in ["-h", "--help", "help", "-v", "--version", "version"] {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        run_with(
            &args(&[a]),
            || Err("gm.toml: unknown key \"bogus\"".into()),
            &mut out,
            &mut err,
        )
        .unwrap_or_else(|e| panic!("gm {a}: {e}"));
        assert!(String::from_utf8(out).unwrap().contains("gm"), "gm {a}");
    }
    // Every other command still stops on the broken file.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let e = run_with(
        &args(&["list"]),
        || Err("gm.toml: unknown key \"bogus\"".into()),
        &mut out,
        &mut err,
    )
    .unwrap_err();
    assert!(e.0.contains("bogus"), "{e}");
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
    // ctrl-m, ctrl-i and ctrl-j are Enter, Tab and line feed in a terminal:
    // binding one takes that key over in the shell.
    for bad in [
        "alt-r",
        "ctrl-shift-b",
        "ctrl-alt-b",
        "ctrl-m",
        "ctrl-i",
        "ctrl-j",
    ] {
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

// /get is split like a command line: flags work the way they do for gm get,
// and the path printed is the repository's, not the raw argument's.
#[test]
fn finder_get_takes_flags() {
    let root = TempDir::new();
    let dst = root.join("example.com/acme/alpha");
    git_repo(&dst);
    let r = run_in(&tree(&root.path()), "", Config::default(), |a| {
        a.act(
            finder::Outcome {
                action: Action::Get,
                arg: "-s  example.com/acme/alpha".into(),
            },
            &mut History::open(""),
        )
    });
    r.res.unwrap();
    assert_eq!(r.out.trim(), dst);
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
        "1 worktree will be removed too: feat/login (1 changed)",
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
        res.err.contains(&format!(
            "1 worktree will be removed too: feat/login at {victim}"
        )),
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

// A command without its argument says what it needs.
#[test]
fn reference_commands_report_their_usage() {
    let root = TempDir::new();
    let get = run_in(&tree(&root.path()), "", Config::default(), |a| a.get(&[]));
    assert!(
        get.res.unwrap_err().0.contains("usage: gm get"),
        "{}",
        get.err
    );
    let create = run_in(&tree(&root.path()), "", Config::default(), |a| {
        a.create(&[])
    });
    assert!(
        create.res.unwrap_err().0.contains("usage: gm create"),
        "{}",
        create.err
    );
}

// version is a command as well as a flag: the same answer either way, from
// the table's own dispatcher.
#[test]
fn plain_dispatch_prints_the_version() {
    let root = TempDir::new();
    let r = run_in(&tree(&root.path()), "", Config::default(), |a| {
        a.dispatch(&args(&["version"]))
    });
    r.res.unwrap();
    assert!(r.out.contains("gm "), "{}", r.out);
    // An unknown word folds back into the usage, asking for exit status 2.
    let u = run_in(&tree(&root.path()), "", Config::default(), |a| {
        a.dispatch(&args(&["no-such-command"]))
    });
    assert_eq!(exit_code(&u.res.unwrap_err()), Some(2));
}

// The exit a caller sees when gm ends without a message of its own.
#[test]
fn exit_code_matches_the_special_errors() {
    assert_eq!(exit_code(&USAGE_ERROR.into()), Some(2));
    assert_eq!(exit_code(&HELP.into()), Some(0));
    assert_eq!(exit_code(&"gm.toml: unknown key".into()), None);
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

// A missing or unknown verb says what the verbs are rather than doing one.
#[test]
fn wt_usage() {
    let root = TempDir::new();
    for a in [
        &[][..],
        &["add"],
        &["create", "only-one-arg"],
        &["remove", "only-one-arg"],
        &["expire", "only-one-arg"],
    ] {
        let r = run_in(&tree(&root.path()), "", Config::default(), |app| {
            app.wt(&args(a))
        });
        assert!(r.res.is_err(), "gm wt {a:?} was accepted");
    }
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

// Budgets for the git processes gm's work costs. The count is taken in a
// child process, one per measured operation: tests run in parallel in one
// process, so a counter shared by threads would sum other tests' spawns into
// the measurement, and a fresh child counts only its own. The budgets are
// measured numbers with a little headroom, not aspirations; raising one to
// fit a slower implementation is a regression, not a fix.
const SPAWN_BUDGET_OP: &str = "GM_SPAWN_BUDGET_OP";
const SPAWN_BUDGET_TEST: &str = "cli::tests::git_spawn_budgets_hold";

/// SPAWN_BUDGETS is the least a run costs today, per measured operation.
/// Each budget is the measured count with one process of headroom, and list
/// costs none at all. A repository row costs two git processes in status
/// (status, remote -v) and one more when it has remotes to count unpushed
/// work against.
const SPAWN_BUDGETS: &[(&str, usize)] = &[
    // gm status over one repository with two worktrees: three rows to read.
    ("status", 7),
    // gm list over five repositories: the tree walk is the filesystem, so
    // no row costs a process.
    ("list", 0),
    // worktrees of one repository, then the changed files of one worktree.
    ("worktrees", 6),
    // gm wt create of a new branch in a fresh clone.
    ("wt-create", 5),
];

#[test]
fn git_spawn_budgets_hold() {
    let Ok(op) = std::env::var(SPAWN_BUDGET_OP) else {
        let exe = std::env::current_exe().unwrap();
        for (op, _) in SPAWN_BUDGETS {
            let out = std::process::Command::new(&exe)
                .args(["--exact", SPAWN_BUDGET_TEST, "--nocapture"])
                .env(SPAWN_BUDGET_OP, op)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git spawn budget for {op}:\n{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
        return;
    };

    let measured = match op.as_str() {
        "status" => measure_status_spawns(),
        "list" => measure_list_spawns(),
        "worktrees" => measure_worktrees_spawns(),
        "wt-create" => measure_wt_create_spawns(),
        _ => panic!("no such measured operation: {op}"),
    };
    let budget = SPAWN_BUDGETS.iter().find(|(o, _)| o == &op).unwrap().1;
    println!("git spawns {measured} (budget {budget})");
    assert!(
        measured <= budget,
        "{op} spawned {measured} git processes, budget {budget}"
    );
}

// measure_status_spawns sets one repository up with two worktrees, runs
// gm status over the tree and returns how many git processes that cost.
fn measure_status_spawns() -> usize {
    let tmp = TempDir::new();
    let root = tmp.join("root");
    let t = tree(&root);
    let r = Repo {
        root: root.clone(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    for b in ["feat/login", "feat/other"] {
        repo::add_worktree(&root, &r.path(), &t.worktree_dir(&r, b), b).unwrap();
    }
    repo::git_spawns_reset();
    let run = run_in(&t, "", Config::default(), |app| app.status(&args(&[])));
    run.res.unwrap();
    repo::git_spawns_reset()
}

// measure_list_spawns runs gm list over five repositories and returns how
// many git processes that cost.
fn measure_list_spawns() -> usize {
    let tmp = TempDir::new();
    let root = tmp.join("root");
    let t = tree(&root);
    for name in ["alpha", "bravo", "charlie", "delta", "echo"] {
        git_repo(&paths::join(&paths::join(&root, "github.com/acme"), name));
    }
    repo::git_spawns_reset();
    let run = run_in(&t, "", Config::default(), |app| app.list(&args(&[])));
    run.res.unwrap();
    repo::git_spawns_reset()
}

// measure_worktrees_spawns reads the worktrees of one repository and the
// changed files of one of them, and returns how many git processes that cost.
fn measure_worktrees_spawns() -> usize {
    let tmp = TempDir::new();
    let root = tmp.join("root");
    let t = tree(&root);
    let r = Repo {
        root: root.clone(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    let login = t.worktree_dir(&r, "feat/login");
    repo::add_worktree(&root, &r.path(), &login, "feat/login").unwrap();
    repo::git_spawns_reset();
    let wts = repo::worktrees(&r.path()).unwrap();
    assert_eq!(wts.len(), 2, "{wts:?}");
    let _ = repo::changed_files(&wts[1].path);
    repo::git_spawns_reset()
}

// measure_wt_create_spawns checks a new branch out of a fresh clone with
// gm wt create and returns how many git processes that cost.
fn measure_wt_create_spawns() -> usize {
    let tmp = TempDir::new();
    let origin = tmp.join("origin.git");
    git(
        &tmp.path(),
        &["init", "-q", "--bare", "-b", "main", &origin],
    );
    let root = tmp.join("root");
    let t = tree(&root);
    let r = Repo {
        root: root.clone(),
        rel: "github.com/acme/alpha".into(),
    };
    mkdir(&paths::dir(&r.path()));
    git(&tmp.path(), &["clone", "-q", &origin, &r.path()]);
    repo::git_spawns_reset();
    let run = run_in(&t, "", Config::default(), |app| {
        app.wt_create(&args(&["github.com/acme/alpha", "feat/login"]))
    });
    run.res.unwrap();
    repo::git_spawns_reset()
}
