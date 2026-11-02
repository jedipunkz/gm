//! The details pane.

use super::*;

// A label sits on its own line above its value, and a value longer than the
// pane is folded, never cut.
#[test]
fn info_pane_stacks_and_wraps() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    let mut m = new_model(&rs, "");
    m.status.insert(
        rs[0].path(),
        Status {
            remote: "https://github.com/acme/alpha".into(),
            branch: "main".into(),
            commits: vec![Commit {
                hash: "abc1234".into(),
                subject: "a subject long enough to need two lines in the pane".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
    );
    let w = 30;
    let joined = lines_text(&m.info_lines(w));
    for label in [
        "repository",
        "path",
        "remote",
        "branch",
        "status",
        "visits",
        "last commit",
    ] {
        assert!(
            joined.contains(&format!("\n{label}\n")) || joined.starts_with(&format!("{label}\n")),
            "{label:?} is not on a line of its own:\n{joined}"
        );
    }
    for l in joined.lines() {
        assert!(l.chars().count() <= w, "{l:?} is wider than {w}");
    }
    assert!(joined.contains("https://github.com/acme"));
    // A commit folds too, with its continuations indented.
    assert!(
        joined.contains("abc1234") && joined.contains(&format!("\n{}", info::COMMIT_INDENT)),
        "{joined}"
    );
    // Nothing is spaced apart: a blank line costs a row of the commits.
    assert!(!joined.contains("\n\n"), "{joined}");
}

// The hash, HEAD, a local branch, a remote-tracking branch and a tag each get
// their own colour, the way `git log --oneline --decorate` does.
#[test]
fn commit_line_colours() {
    let root = TempDir::new();
    let m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    let th = theme();
    let r = |name: &str, kind| Ref {
        name: name.into(),
        kind,
    };
    let lines = m.commit_lines(
        &Commit {
            hash: "b1b7b91".into(),
            refs: vec![
                r("HEAD", RefKind::Head),
                r("feat/x", RefKind::Local),
                r("origin/feat/x", RefKind::Remote),
                r("tag: v1.0", RefKind::Tag),
            ],
            subject: "refactor: name the flag".into(),
        },
        120,
    );
    assert_eq!(lines.len(), 1, "a line that fits should not fold");
    for (what, hex) in [
        ("hash", blend(th.blue, th.comment, 0.35)),
        ("HEAD", th.cyan.to_string()),
        ("local branch", th.blue.to_string()),
        ("remote branch", blend(th.blue, th.comment, 0.5)),
        ("subject", blend(th.comment, th.fg, ROW_LIFT)),
    ] {
        assert!(
            line_painted(&lines[0], &hex),
            "the {what} is not painted {hex}"
        );
    }
    assert_eq!(
        line_text(&lines[0]),
        "b1b7b91 (HEAD, feat/x, origin/feat/x, tag: v1.0) refactor: name the flag"
    );
}

// A long commit folds instead of being cut: no text is lost, no line is wider
// than the pane, and the continuations are indented.
#[test]
fn commit_line_wraps() {
    let root = TempDir::new();
    let m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    let c = Commit {
        hash: "b1b7b91".into(),
        refs: vec![Ref {
            name: "origin/a-long-branch-name".into(),
            kind: RefKind::Remote,
        }],
        subject: "a subject far too long for the pane to hold in one line".into(),
    };
    let whole = "b1b7b91 (origin/a-long-branch-name) a subject far too long for the pane to hold in one line";
    let squash = |s: &str| s.replace(' ', "");
    for w in [80, 40, 20, 8, 3] {
        let lines = m.commit_lines(&c, w);
        let mut words = String::new();
        for (i, l) in lines.iter().enumerate() {
            let plain = line_text(l);
            assert!(plain.chars().count() <= w, "width {w}, line {i}: {plain:?}");
            if i > 0 {
                assert!(
                    plain.starts_with(info::COMMIT_INDENT),
                    "width {w}, line {i} is not indented"
                );
            }
            words.push_str(plain.strip_prefix(info::COMMIT_INDENT).unwrap_or(&plain));
        }
        // Folding must not lose or duplicate anything.
        assert_eq!(squash(&words), squash(whole), "width {w}");
    }
    // The fold keeps the colours.
    let lines = m.commit_lines(&c, 24);
    assert!(lines.len() >= 2);
    let th = theme();
    assert!(line_painted(&lines[0], &blend(th.blue, th.comment, 0.35)));
}

// The field names are the quiet half of the pane: the comment colour in bold,
// against values that keep the theme's own colours.
#[test]
fn pane_labels_stand_out() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    let mut m = new_model(&rs, "");
    m.status.insert(
        rs[0].path(),
        Status {
            remote: "https://github.com/acme/alpha".into(),
            branch: "main".into(),
            ..Default::default()
        },
    );
    let lines = m.info_lines(40);
    let label = lines
        .iter()
        .find(|l| line_text(l) == "path")
        .expect("no path label");
    let th = theme();
    assert!(line_painted(label, th.comment));
    assert!(
        label
            .spans
            .iter()
            .all(|s| s.style.add_modifier.contains(Modifier::BOLD)),
        "the label is not bold"
    );
    for hex in [th.fg, th.green, th.cyan, th.magenta] {
        assert!(
            !line_painted(label, hex),
            "the label carries the value colour {hex}"
        );
    }
}

// Worktrees in the pane are how a row says whether the worktree key is worth
// pressing on it; a repository without any says nothing at all.
#[test]
fn worktrees_in_the_details_pane() {
    let root = TempDir::new();
    let rs = repos(&root.path(), &["github.com/acme/alpha"]);
    let mut m = new_model(&rs, "");
    let path = rs[0].path();
    let pane = |m: &Model| lines_text(&m.info_lines(40));

    m.status.insert(
        path.clone(),
        Status {
            branch: "main".into(),
            ..Default::default()
        },
    );
    assert!(!pane(&m).contains("worktrees"));

    let now = repo::now();
    let wt = |name: &str, ago: i64| Worktree {
        path: format!("{}/.worktrees/{name}", root.path()),
        branch: name.into(),
        committed_at: now - ago,
        ..Default::default()
    };
    let detached = Worktree {
        path: format!("{}/.worktrees/b", root.path()),
        head: "abc1234def".into(),
        ..Default::default()
    };
    m.status.insert(
        path.clone(),
        Status {
            branch: "main".into(),
            worktrees: vec![wt("feat/login", 3600), detached],
            ..Default::default()
        },
    );
    let got = pane(&m);
    assert!(got.contains("\nlast 2 worktrees\n"), "{got}");
    // A detached checkout has no branch to name it, so it goes by its hash.
    assert!(
        got.contains("feat/login") && got.contains("abc1234"),
        "{got}"
    );
    assert!(got.contains("1h ago"), "{got}");

    // Newest first, and only three.
    m.status.insert(
        path,
        Status {
            branch: "main".into(),
            worktrees: vec![
                wt("stale", 30 * 86400),
                wt("newest", 60),
                wt("mid", 2 * 86400),
                wt("second", 3600),
            ],
            ..Default::default()
        },
    );
    let got = pane(&m);
    assert!(got.contains("\nlast 3 of 4 worktrees\n"), "{got}");
    assert!(!got.contains("stale"), "{got}");
    assert!(
        got.find("newest").unwrap() < got.find("second").unwrap(),
        "{got}"
    );
}
