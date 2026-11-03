//! What the CLI does with what the finder decided.

use super::*;

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
