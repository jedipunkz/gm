//! Usage text, flag parsing and dispatch.

use super::*;

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

#[test]
fn usage_covers_every_flag_spelling_without_advertising_wrong_wt_flags() {
    let u = usage();
    for command in COMMANDS {
        let line = u
            .lines()
            .find(|line| {
                line.trim_start()
                    .starts_with(&format!("gm {}", command.name))
            })
            .unwrap_or_else(|| panic!("usage does not mention {}:\n{u}", command.name));
        for flag in command
            .flags
            .iter()
            .chain(command.verbs.iter().flat_map(|verb| verb.flags.iter()))
        {
            for name in flag.names {
                let prefix = if name.len() == 1 { "-" } else { "--" };
                let spelling = format!("{prefix}{name}");
                let has_spelling = line
                    .split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
                    .any(|token| token == spelling);
                assert!(
                    has_spelling,
                    "usage for {} does not mention {spelling}:\n{line}",
                    command.name
                );
            }
        }
    }
    let wt = u
        .lines()
        .find(|line| line.trim_start().starts_with("gm wt "))
        .unwrap();
    let create = wt.split(" | ").next().unwrap();
    assert!(
        !create.contains("-y"),
        "wt create must not advertise -y:\n{create}"
    );
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

#[test]
fn missing_arguments_use_the_same_derived_command_syntax() {
    let root = TempDir::new();
    let get = run_in(&tree(&root.path()), "", Config::default(), |a| a.get(&[]));
    assert_eq!(
        get.res.unwrap_err().0,
        "usage: gm get [-u|--update] [--ssh] [--shallow] [-s|--silent] [-l|--look] [--no-recursive] [-b <branch>|--branch <branch>] <repo>..."
    );

    let create = run_in(&tree(&root.path()), "", Config::default(), |a| {
        a.create(&[])
    });
    assert_eq!(
        create.res.unwrap_err().0,
        "usage: gm create [--ssh] <repo>|<user>/<repo>|<host>/<user>/<repo>"
    );

    let remove = run_in(&tree(&root.path()), "", Config::default(), |a| {
        a.remove(&[])
    });
    assert_eq!(
        remove.res.unwrap_err().0,
        "usage: gm remove [--dry-run] [-y] <repo>..."
    );

    let migrate = run_in(&tree(&root.path()), "", Config::default(), |a| {
        a.migrate(&[])
    });
    assert_eq!(
        migrate.res.unwrap_err().0,
        "usage: gm migrate [--dry-run] [-y] [-r|--recursive] <directory>..."
    );

    let shell = run_in(&tree(&root.path()), "", Config::default(), |a| a.shell(&[]));
    assert_eq!(shell.res.unwrap_err().0, "usage: gm shell <fish|zsh|bash>");

    let wt = run_in(&tree(&root.path()), "", Config::default(), |a| {
        a.wt(&args(&["create", "only-one-arg"]))
    });
    let error = wt.res.unwrap_err().0;
    assert!(error.contains("gm wt create <repo> <branch>"), "{error}");
    assert!(
        error.contains("gm wt remove [--dry-run] [-y] <repo> <branch>"),
        "{error}"
    );
    assert!(
        error.contains("gm wt expire [--dry-run] [-y] <repo> <days>d"),
        "{error}"
    );
    assert!(!error.contains("gm wt create [-y]"), "{error}");
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
