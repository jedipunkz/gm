//! The list: ranking, the query and the repository filters.

use super::*;

#[test]
fn wraps_full_width_text_by_display_cells() {
    let lines = wrap_segs(
        &[Seg::new("漢字仮名", ratatui::style::Style::default())],
        4,
        "",
    );

    assert_eq!(lines.len(), 2);
    assert!(lines.iter().all(|line| line.width() <= 4));
    assert_eq!(
        lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect::<String>(),
        "漢字仮名"
    );
}

#[test]
fn full_width_list_row_occupies_requested_width() {
    let root = TempDir::new();
    let m = new_model(&repos(&root.path(), &["漢字仮名交"]), "");

    let row = m.render_row(0, false, 8);

    assert_eq!(row.width(), 8);
}

// The core contract: the cursor rests on the most-used repository, and it is
// the last row drawn.
#[test]
fn best_at_bottom() {
    let root = TempDir::new();
    let rs = repos(
        &root.path(),
        &[
            "github.com/acme/alpha",
            "github.com/acme/bravo",
            "github.com/other/charlie",
        ],
    );
    let mut m = new_model(&rs, &rs[1].path()); // bravo is the favourite
    assert_eq!(m.view.len(), 3);
    assert_eq!(m.cursor, 2);
    assert_eq!(label(&m), "github.com/acme/bravo");

    set_query(&mut m, "charlie");
    assert_eq!(rows(&m), vec!["github.com/other/charlie"]);
    assert_eq!(label(&m), "github.com/other/charlie");
}

// Scattered matches across the shared "github.com/user/" prefix must not
// outrank a literal hit in the repository name.
#[test]
fn ranking_prefers_the_repository_name() {
    let root = TempDir::new();
    let rs = repos(
        &root.path(),
        &[
            "github.com/jedipunkz/detect-minecraft-versions",
            "github.com/jedipunkz/miniecs",
            "github.com/jedipunkz/spacex-ipo-checker",
        ],
    );
    let mut m = new_model(&rs, "");
    set_query(&mut m, "miniec");
    assert_eq!(label(&m), "github.com/jedipunkz/miniecs");
    // The highlight sits on the literal "miniec", not scattered across the
    // host and user segments.
    let at = label(&m).find("miniec").unwrap();
    assert_eq!(
        m.matched[&m.view[m.cursor]],
        (at..at + 6).collect::<Vec<_>>()
    );
}

// "gm" names github.com/jedipunkz/gm: the repository it names whole rests
// under the cursor even with no visit history, over one that merely carries
// "gm" inside a longer name and one whose earlier "gm" sits in the user.
#[test]
fn ranking_prefers_the_named_repository() {
    let root = TempDir::new();
    let rs = repos(
        &root.path(),
        &[
            "github.com/jedipunkz/gm-note",
            "github.com/gmdev/tools",
            "github.com/jedipunkz/gm",
        ],
    );
    let mut m = new_model(&rs, "");
    set_query(&mut m, "gm");
    assert_eq!(
        rows(&m),
        vec![
            "github.com/gmdev/tools",
            "github.com/jedipunkz/gm-note",
            "github.com/jedipunkz/gm",
        ]
    );
    assert_eq!(label(&m), "github.com/jedipunkz/gm");
    // The highlight sits on the "gm" the repository is named by, not on the
    // first place the letters turn up.
    let at = label(&m).len() - 2;
    assert_eq!(m.matched[&m.view[m.cursor]], vec![at, at + 1]);
}

#[test]
fn extended_query_supports_literal_prefix_suffix_and_exclusion_terms() {
    let root = TempDir::new();
    let rs = repos(
        &root.path(),
        &[
            "github.com/acme/alpha-api",
            "github.com/acme/a-x-l-p-h-a",
            "github.com/other/acme/alpha-api",
            "github.com/acme/forked-api",
        ],
    );
    let mut m = new_model(&rs, "");

    set_query(&mut m, "'alpha");
    assert_eq!(rows(&m).len(), 2);
    assert!(!rows(&m).contains(&"github.com/acme/a-x-l-p-h-a".to_string()));

    // Prefix and suffix operators address the complete displayed label, not
    // just the repository-name segment.
    set_query(&mut m, "^github.com/acme/alpha");
    assert_eq!(rows(&m), vec!["github.com/acme/alpha-api"]);
    set_query(&mut m, "alpha-api$");
    assert_eq!(rows(&m).len(), 2);
    assert!(rows(&m).contains(&"github.com/acme/alpha-api".to_string()));

    set_query(&mut m, "!fork");
    assert_eq!(rows(&m).len(), 3);
    assert!(!rows(&m).contains(&"github.com/acme/forked-api".to_string()));
}

#[test]
fn extended_query_ands_terms_and_unions_their_highlights() {
    let root = TempDir::new();
    let rs = repos(
        &root.path(),
        &[
            "github.com/acme/api",
            "github.com/acme/other",
            "github.com/other/api",
        ],
    );
    let mut m = new_model(&rs, "");

    set_query(&mut m, "acme api");
    assert_eq!(rows(&m), vec!["github.com/acme/api"]);
    let at = "github.com/acme/api".find("acme").unwrap();
    assert_eq!(
        m.matched[&m.view[m.cursor]],
        (at..at + "acme".len())
            .chain("github.com/acme/api".find("api").unwrap().."github.com/acme/api".len())
            .collect::<Vec<_>>()
    );
}

#[test]
fn a_plain_single_term_keeps_legacy_ranking_and_highlights() {
    let root = TempDir::new();
    let rs = repos(
        &root.path(),
        &[
            "github.com/jedipunkz/gm-note",
            "github.com/gmdev/tools",
            "github.com/jedipunkz/gm",
        ],
    );
    let mut m = new_model(&rs, "");

    set_query(&mut m, "gm");
    assert_eq!(
        rows(&m),
        vec![
            "github.com/gmdev/tools",
            "github.com/jedipunkz/gm-note",
            "github.com/jedipunkz/gm",
        ]
    );
    let at = "github.com/jedipunkz/gm".len() - 2;
    assert_eq!(m.matched[&m.view[m.cursor]], vec![at, at + 1]);
}

#[test]
fn empty_and_escaped_operators_are_literal() {
    let root = TempDir::new();
    let rs = repos(
        &root.path(),
        &[
            "github.com/acme/^",
            "github.com/acme/!fork",
            "github.com/acme/alpha$",
            "github.com/acme/'",
            "github.com/acme/$",
            "github.com/acme/forked",
        ],
    );
    let mut m = new_model(&rs, "");

    set_query(&mut m, "^");
    assert_eq!(rows(&m), vec!["github.com/acme/^"]);
    set_query(&mut m, "'");
    assert_eq!(rows(&m), vec!["github.com/acme/'"]);
    set_query(&mut m, "$ ");
    assert_eq!(rows(&m).len(), 2);
    assert!(rows(&m).contains(&"github.com/acme/$".to_string()));
    set_query(&mut m, "\\!fork");
    assert_eq!(rows(&m), vec!["github.com/acme/!fork"]);
    set_query(&mut m, "alpha\\$");
    assert_eq!(rows(&m), vec!["github.com/acme/alpha$"]);
}

#[test]
fn extended_query_keeps_semicolon_and_leading_command_behavior() {
    let root = TempDir::new();
    let rs = repos(
        &root.path(),
        &["github.com/acme/api", "github.com/other/api"],
    );
    let mut m = new_model(&rs, "");
    for c in "acme api;/help".chars() {
        m.update(ch(c));
    }
    assert_eq!(rows(&m), vec!["github.com/acme/api"]);
    assert!(command::is_command(&m.input.value()));

    set_query(&mut m, "/help");
    assert_eq!(m.view.len(), 2, "leading slash remains a command");
}

// The unselected row on screen carries the lifted colour, not the comment
// colour the labels still use.
#[test]
fn unselected_rows_are_lifted() {
    let root = TempDir::new();
    let mut m = new_model(
        &repos(
            &root.path(),
            &["github.com/acme/alpha", "github.com/acme/bravo"],
        ),
        "",
    );
    (m.w, m.h) = (80, 10);
    let buf = screen(&m);
    let y = (0..buf.area.height)
        .find(|&y| row_text(&buf, y).contains("acme/alpha"))
        .expect("alpha is not on screen");
    let th = theme();
    assert!(painted(&buf, Some(y), &blend(th.comment, th.fg, ROW_LIFT)));
}

// The rows do not move while a command is typed, so neither does the cursor.
#[test]
fn cursor_survives_a_command() {
    let root = TempDir::new();
    let mut m = new_model(
        &repos(
            &root.path(),
            &[
                "github.com/acme/alpha",
                "github.com/acme/bravo",
                "github.com/acme/charlie",
            ],
        ),
        "",
    );
    m.update(key("up"));
    assert_eq!(m.cursor, m.view.len() - 2);
    let want = label(&m);

    for c in "/he".chars() {
        m.update(ch(c));
        assert_eq!(label(&m), want, "after typing {c:?} the selection moved");
    }
    // Erasing it back to an empty query must not move it either.
    for _ in 0..3 {
        m.update(key("backspace"));
    }
    assert_eq!(label(&m), want);

    // A real query still puts the cursor on the best match.
    m.update(ch('a'));
    assert_eq!(m.cursor, m.view.len() - 1);
}

// Filter down to a repository, clear the box, and run a command on what is
// still selected. Clearing must not throw the selection back to the best
// match.
#[test]
fn find_then_act_on_it() {
    let root = TempDir::new();
    let rs = repos(
        &root.path(),
        &[
            "github.com/jedipunkz/agx",
            "github.com/jedipunkz/gm",
            "github.com/jedipunkz/miniecs",
        ],
    );
    let mut m = new_model(&rs, "");
    typed(&mut m, "agx");
    assert_eq!(label(&m), "github.com/jedipunkz/agx");
    assert!(line_text(&m.help_line(90)).contains("esc clear"));

    // Esc clears the query rather than quitting, and holds the selection.
    let cmds = m.update(key("esc"));
    assert!(!is_quit(&cmds));
    assert_eq!(m.input.value(), "");
    assert_eq!(m.view.len(), rs.len());
    assert_eq!(label(&m), "github.com/jedipunkz/agx");

    // And the command asks about it.
    run_slash(&mut m, "/remove");
    assert!(
        m.over == Overlay::Confirm && m.ask.arg == rs[0].path(),
        "{:?}",
        m.ask
    );

    // With the box empty, Esc quits as it always did — with an issue key
    // that is not the default: the default ctrl-[ is esc and opens the
    // issue list instead, see esc_opens_the_issue_list.
    let mut esc_quits = {
        let mut m = new_model(&rs, "");
        m.keys.issue = parse_chord("", "ctrl-e").unwrap();
        m
    };
    assert!(is_quit(&esc_quits.update(key("esc"))));
}

#[test]
fn unpushed_filter_starts_a_repository_scan() {
    let root = TempDir::new();
    let (mut m, _) = dirty_model(&root);
    let cmds = run_slash(&mut m, "/unpushed");
    assert!(!cmds.is_empty(), "/unpushed started no scan");
    assert!(line_text(&m.help_line(90)).contains("checking"));
    assert_eq!(m.view.len(), 3, "rows disappeared before the scan finished");
    feed(&mut m, cmds);
    assert_eq!(
        rows(&m),
        vec!["github.com/acme/bravo", "github.com/acme/charlie"]
    );
    assert!(line_text(&m.help_line(90)).contains("unpushed only"));
}

#[test]
fn unpushed_scan_filters_the_saved_repository_list_when_it_finishes_elsewhere() {
    let root = TempDir::new();
    let (mut m, _) = dirty_model(&root);
    let cmds = run_slash(&mut m, "/unpushed");
    only_main(&mut m);
    open_worktrees(&mut m);

    feed(&mut m, cmds);
    assert_eq!(m.mode, Mode::Worktrees);
    assert_eq!(rows(&m), vec!["main"]);

    m.update(key("esc"));
    assert_eq!(
        rows(&m),
        vec!["github.com/acme/bravo", "github.com/acme/charlie"]
    );
    assert!(line_text(&m.help_line(90)).contains("unpushed only"));
}

// /dirty end to end: the scan runs off the UI thread, the list keeps every
// row until the answer lands, and running it again shows everything.
#[test]
fn dirty_filter() {
    let root = TempDir::new();
    let (mut m, rs) = dirty_model(&root);
    let cmds = run_slash(&mut m, "/dirty");
    assert!(!cmds.is_empty(), "/dirty started no scan");
    // Nothing is hidden while the answer is still coming.
    assert_eq!(m.view.len(), rs.len());
    assert!(line_text(&m.help_line(90)).contains("checking"));

    feed(&mut m, cmds);
    assert_eq!(
        rows(&m),
        vec!["github.com/acme/alpha", "github.com/acme/charlie"]
    );
    assert_eq!(m.cursor, m.view.len() - 1);
    assert!(line_text(&m.help_line(90)).contains("dirty only"));

    // A query narrows what is left, rather than bringing the clean ones back.
    set_query(&mut m, "charlie");
    assert_eq!(rows(&m), vec!["github.com/acme/charlie"]);
    set_query(&mut m, "bravo");
    assert!(
        rows(&m).is_empty(),
        "a clean repository came back through the query"
    );

    // Running it again shows everything, and does not scan twice.
    set_query(&mut m, "");
    let cmds = run_slash(&mut m, "/dirty");
    assert!(
        cmds.is_empty(),
        "/dirty scanned again instead of reusing the answer"
    );
    assert_eq!(m.view.len(), rs.len());
    assert!(!line_text(&m.help_line(90)).contains("dirty only"));
}

// It declines in the worktree list rather than filtering something the answer
// does not describe.
#[test]
fn dirty_filter_is_for_repositories() {
    let root = TempDir::new();
    let (mut m, _) = dirty_model(&root);
    only_main(&mut m);
    open_worktrees(&mut m);
    let cmds = run_slash(&mut m, "/dirty");
    assert!(cmds.is_empty() && !m.dirty_only);
    assert!(m.note.contains("repository list"), "{:?}", m.note);
}

#[test]
fn dirty_and_unpushed_filters_combine_and_escape_clears_them() {
    let root = TempDir::new();
    let (mut m, rs) = dirty_model(&root);
    let cmds = run_slash(&mut m, "/dirty");
    assert!(!cmds.is_empty());
    assert!(run_slash(&mut m, "/unpushed").is_empty());
    feed(&mut m, cmds);
    assert_eq!(rows(&m), vec!["github.com/acme/charlie"]);
    assert!(line_text(&m.help_line(90)).contains("dirty + unpushed"));

    assert!(run_slash(&mut m, "/dirty").is_empty());
    assert_eq!(
        rows(&m),
        vec!["github.com/acme/bravo", "github.com/acme/charlie"]
    );
    assert!(line_text(&m.help_line(90)).contains("unpushed only"));

    m.update(key("esc"));
    assert_eq!(
        rows(&m),
        rs.iter().map(|r| r.rel.as_str()).collect::<Vec<_>>()
    );
    assert!(!m.dirty_only && !m.unpushed_only);
    assert!(!line_text(&m.help_line(90)).contains("only"));
}

#[test]
fn unpushed_filter_is_for_repositories() {
    let root = TempDir::new();
    let (mut m, _) = dirty_model(&root);
    only_main(&mut m);
    open_worktrees(&mut m);
    let cmds = run_slash(&mut m, "/unpushed");
    assert!(cmds.is_empty() && !m.unpushed_only);
    assert!(m.note.contains("repository list"), "{:?}", m.note);
}

/// dirty_everywhere stubs the scan to call every path it is asked about dirty.
fn dirty_everywhere(m: &mut Model) {
    m.state_scan_of = Arc::new(|ps| {
        ps.iter()
            .map(|p| {
                let s = repo::State {
                    dirty: 1,
                    ..Default::default()
                };
                (p.clone(), s)
            })
            .collect()
    });
}

// The bug where the scan ran once per finder: work done in another terminal
// never showed up. Switching the filter on again asks git again, and the old
// answer filters the list until the new one lands.
#[test]
fn dirty_filter_scans_again_when_switched_on_again() {
    let root = TempDir::new();
    let (mut m, _) = dirty_model(&root);
    let cmds = run_slash(&mut m, "/dirty");
    feed(&mut m, cmds);
    assert!(
        run_slash(&mut m, "/dirty").is_empty(),
        "switching off scanned"
    );

    dirty_everywhere(&mut m);
    let cmds = run_slash(&mut m, "/dirty");
    assert!(!cmds.is_empty(), "/dirty did not scan again");
    assert_eq!(
        rows(&m),
        vec!["github.com/acme/alpha", "github.com/acme/charlie"],
        "the last answer did not hold until the new one"
    );
    feed(&mut m, cmds);
    assert_eq!(m.view.len(), 3, "{:?}", rows(&m));
}

// The bug where a repository made after the scan vanished under the filter:
// it had no answer, and no answer read as clean.
#[test]
fn a_repository_created_under_a_filter_is_scanned() {
    let root = TempDir::new();
    let (mut m, _) = dirty_model(&root);
    let cmds = run_slash(&mut m, "/dirty");
    feed(&mut m, cmds);
    dirty_everywhere(&mut m);

    let cmds = run_slash(&mut m, "/create acme/delta");
    feed(&mut m, cmds);
    let cmds = m.update(ch('y'));
    let rescan = feed(&mut m, cmds);
    // Until git has answered for it, the new row is not hidden.
    assert!(
        rows(&m).contains(&"github.com/acme/delta".to_string()),
        "{:?}",
        rows(&m)
    );
    let Some(Msg::RepoState(path, state)) = answer(rescan) else {
        panic!("the created repository was not scanned")
    };
    assert!(path.ends_with("github.com/acme/delta"), "{path}");
    m.update(Msg::RepoState(path, state));
    assert_eq!(
        m.current().map(|it| it.label.as_str()),
        Some("github.com/acme/delta"),
        "the answer moved the selection off the new row"
    );
}
