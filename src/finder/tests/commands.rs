//! The slash commands.

use super::*;

// Repository paths are full of slashes, and only a leading one means a
// command.
#[test]
fn slash_only_starts_a_command_at_the_start() {
    let root = TempDir::new();
    let mut m = new_model(
        &repos(
            &root.path(),
            &["github.com/acme/alpha", "github.com/other/bravo"],
        ),
        "",
    );
    set_query(&mut m, "acme/alpha");
    assert_eq!(rows(&m), vec!["github.com/acme/alpha"]);
    set_query(&mut m, "/help");
    assert_eq!(m.view.len(), 2, "a command filtered the list");
    assert!(command::is_command("/help") && !command::is_command("acme/alpha"));
}

#[test]
fn unknown_command() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    let cmds = run_slash(&mut m, "/nope");
    assert!(!is_quit(&cmds) && m.result.action == Action::None);
    let note = line_text(&m.help_line(90));
    assert!(note.contains("/nope") && note.contains("/help"), "{note:?}");
    // The next keystroke clears it.
    set_query(&mut m, "/n");
    assert_eq!(m.note, "");
}

// The box fills the rest of a command in, and Tab accepts what it offered.
#[test]
fn command_completion() {
    let root = TempDir::new();
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    typed(&mut m, "/wo");
    let shown: String = m
        .input
        .view("❯ ", m.st.prompt, m.st.text, m.st.suggestion)
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(shown.contains("/worktrees"), "{shown:?}");
    m.update(key("tab"));
    assert_eq!(m.input.value(), "/worktrees");
    // A prefix typed in another case completes to a name Enter knows, and
    // the query in front of it is kept as typed.
    let mut m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    typed(&mut m, "Alpha;/CR");
    m.update(key("tab"));
    assert_eq!(m.input.value(), "Alpha;/create");
}

// What to create or clone is not guessed.
#[test]
fn actions_need_their_argument() {
    let root = TempDir::new();
    let m = new_model(&repos(&root.path(), &["github.com/acme/alpha"]), "");
    for typed in ["/create", "/get", "/create   "] {
        let mut next = m.clone();
        let cmds = run_slash(&mut next, typed);
        assert!(
            !is_quit(&cmds) && next.result.action == Action::None,
            "{typed:?} acted"
        );
        assert!(next.note.contains("<repo>"), "{typed:?}: {:?}", next.note);
    }
}

// Find a repository, then act on it without emptying the box first: a
// semicolon ends the query.
#[test]
fn command_after_a_query() {
    let root = TempDir::new();
    let rs = repos(
        &root.path(),
        &[
            "github.com/acme/alpha",
            "github.com/acme/bravo",
            "github.com/other/charlie",
        ],
    );
    let mut m = new_model(&rs, "");
    for (i, c) in "bravo;/remov".chars().enumerate() {
        m.update(ch(c));
        // The query keeps filtering while the command is typed after it.
        if i >= 4 {
            assert_eq!(
                label(&m),
                "github.com/acme/bravo",
                "after {:?}",
                m.input.value()
            );
        }
    }
    let shown: String = m
        .input
        .view("", m.st.prompt, m.st.text, m.st.suggestion)
        .iter()
        .map(|s| s.content.as_ref())
        .collect();
    assert!(shown.contains("bravo;/remove"), "{shown:?}");
    m.update(key("tab"));
    assert_eq!(m.input.value(), "bravo;/remove");
    m.update(key("enter"));
    assert!(
        m.over == Overlay::Confirm && m.ask.arg == rs[1].path(),
        "{:?}",
        m.ask
    );
    for (s, want) in [
        ("bravo;", true),
        ("bravo;/help", true),
        ("acme/alpha", false),
    ] {
        assert_eq!(command::is_command(s), want, "{s}");
    }
}
