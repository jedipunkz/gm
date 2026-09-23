package finder

import (
	"fmt"
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"
	"github.com/jedipunkz/gm/internal/repo"
)

// TestFinderPutsBestAtBottom pins the core TUI contract: the cursor rests on
// the most-used repository, and it is the last row drawn.
func TestFinderPutsBestAtBottom(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{
		{Root: root, Rel: "github.com/acme/alpha"},
		{Root: root, Rel: "github.com/acme/bravo"},
		{Root: root, Rel: "github.com/other/charlie"},
	}
	m := newTestModel(t, repos, repos[1].Path()) // bravo is the favourite

	if got := len(m.view); got != 3 {
		t.Fatalf("view has %d rows, want 3", got)
	}
	if m.cursor != len(m.view)-1 {
		t.Errorf("cursor at %d, want the bottom row %d", m.cursor, len(m.view)-1)
	}
	if it, _ := m.current(); it.label != "github.com/acme/bravo" {
		t.Errorf("selected %q, want the most-visited github.com/acme/bravo", it.label)
	}

	m.input.SetValue("charlie")
	m.filter()
	if len(m.view) != 1 {
		t.Fatalf("filter kept %d rows, want 1", len(m.view))
	}
	if it, _ := m.current(); it.label != "github.com/other/charlie" {
		t.Errorf("filtered selection is %q, want github.com/other/charlie", it.label)
	}
}

// TestRankingPrefersTheRepositoryName guards the case that made scattered
// matches across the shared "github.com/user/" prefix outrank a literal hit in
// the repository name.
func TestRankingPrefersTheRepositoryName(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{
		{Root: root, Rel: "github.com/jedipunkz/detect-minecraft-versions"},
		{Root: root, Rel: "github.com/jedipunkz/miniecs"},
		{Root: root, Rel: "github.com/jedipunkz/spacex-ipo-checker"},
	}
	m := newTestModel(t, repos, "")
	m.input.SetValue("miniec")
	m.filter()

	it, ok := m.current()
	if !ok {
		t.Fatal("nothing selected")
	}
	if it.label != "github.com/jedipunkz/miniecs" {
		t.Errorf("selected %q, want github.com/jedipunkz/miniecs", it.label)
	}

	// The highlight must sit on the literal "miniec", not be scattered across
	// the host and user segments.
	hits := m.matched[m.view[m.cursor]]
	want := strings.Index(it.label, "miniec")
	for n, got := range hits {
		if got != want+n {
			t.Fatalf("highlight at %v, want the run starting at %d", hits, want)
		}
	}
	if len(hits) != len("miniec") {
		t.Errorf("highlighted %d characters, want %d", len(hits), len("miniec"))
	}
}

// TestUnselectedRowsAreLifted pins the contrast ladder in the list: an
// unselected row sits between the comment colour and the foreground, so it
// reads without competing with the selected row.
func TestUnselectedRowsAreLifted(t *testing.T) {
	dist := func(a, b string) int {
		var ar, ag, ab, br, bg, bb int
		_, _ = fmt.Sscanf(a, "#%02x%02x%02x", &ar, &ag, &ab)
		_, _ = fmt.Sscanf(b, "#%02x%02x%02x", &br, &bg, &bb)
		abs := func(n int) int {
			if n < 0 {
				return -n
			}
			return n
		}
		return abs(ar-br) + abs(ag-bg) + abs(ab-bb)
	}

	for _, name := range ThemeNames() {
		th, err := LookupTheme(name)
		if err != nil {
			t.Fatal(err)
		}
		row := blend(th.Comment, th.Fg, rowLift)
		if row == th.Comment {
			t.Errorf("theme %s: the row colour is still the comment colour", name)
		}
		// Closer to the comment colour than to the foreground: the selected
		// row has to stay the brightest thing in the list.
		if dist(row, th.Comment) >= dist(row, th.Fg) {
			t.Errorf("theme %s: row %s drifted past halfway to the foreground %s", name, row, th.Fg)
		}
	}

	// The unselected row on screen must carry the lifted colour, not the
	// comment colour the labels still use.
	root := t.TempDir()
	m := newTestModel(t, []repo.Repo{
		{Root: root, Rel: "github.com/acme/alpha"},
		{Root: root, Rel: "github.com/acme/bravo"},
	}, "")
	m.w, m.h = 80, 10
	th := themes[DefaultTheme]
	var row string
	for _, line := range strings.Split(m.View().Content, "\n") {
		if strings.Contains(stripANSI(line), "acme/alpha") {
			row = line
			break
		}
	}
	if row == "" {
		t.Fatal("the unselected repository is not on screen")
	}
	if !strings.Contains(row, ansi("38", blend(th.Comment, th.Fg, rowLift))) {
		t.Errorf("the unselected row is not painted in the lifted colour:\n%q", row)
	}
}

// TestCursorSurvivesACommand guards the bug where typing a slash threw the
// selection back to the best match: the rows do not move while a command is
// typed, so neither should the cursor.
func TestCursorSurvivesACommand(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{
		{Root: root, Rel: "github.com/acme/alpha"},
		{Root: root, Rel: "github.com/acme/bravo"},
		{Root: root, Rel: "github.com/acme/charlie"},
	}
	m := newTestModel(t, repos, "")

	// Move up one from the bottom row, the way ↑ does.
	next, _ := m.Update(tea.KeyPressMsg{Code: tea.KeyUp})
	m = next.(model)
	want, _ := m.current()
	if m.cursor != len(m.view)-2 {
		t.Fatalf("cursor at %d, want one above the bottom", m.cursor)
	}

	for _, r := range "/he" {
		next, _ = m.Update(tea.KeyPressMsg{Code: r, Text: string(r)})
		m = next.(model)
		got, ok := m.current()
		if !ok || got.label != want.label {
			t.Fatalf("after typing %q the selection moved to %q, want %q", string(r), got.label, want.label)
		}
	}

	// Erasing it back to an empty query must not move it either.
	for range "/he" {
		next, _ = m.Update(tea.KeyPressMsg{Code: tea.KeyBackspace})
		m = next.(model)
	}
	if got, _ := m.current(); got.label != want.label {
		t.Errorf("erasing the command moved the selection to %q, want %q", got.label, want.label)
	}

	// A real query still puts the cursor on the best match.
	next, _ = m.Update(tea.KeyPressMsg{Code: 'a', Text: "a"})
	m = next.(model)
	if m.cursor != len(m.view)-1 {
		t.Errorf("a query left the cursor at %d, want the bottom row %d", m.cursor, len(m.view)-1)
	}
}

// TestFindThenActOnIt is the flow the slash commands exist for: filter down
// to a repository, clear the box, and run a command on what is still
// selected. Clearing must not throw the selection back to the best match.
func TestFindThenActOnIt(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{
		{Root: root, Rel: "github.com/jedipunkz/agx"},
		{Root: root, Rel: "github.com/jedipunkz/gm"},
		{Root: root, Rel: "github.com/jedipunkz/miniecs"},
	}
	var mm tea.Model = newTestModel(t, repos, "")

	for _, r := range "agx" {
		mm, _ = mm.Update(tea.KeyPressMsg{Code: r, Text: string(r)})
	}
	m := mm.(model)
	if it, _ := m.current(); it.label != "github.com/jedipunkz/agx" {
		t.Fatalf("the query selected %q", it.label)
	}
	if hint := stripANSI(m.helpLine(90)); !strings.Contains(hint, "esc clear") {
		t.Errorf("the hint line does not offer to clear the query: %q", hint)
	}

	// Esc clears the query rather than quitting, and holds the selection.
	mm, cmd := m.Update(tea.KeyPressMsg{Code: tea.KeyEscape})
	m = mm.(model)
	if isQuit(cmd) {
		t.Fatal("Esc quit instead of clearing the query")
	}
	if m.input.Value() != "" {
		t.Errorf("the query survived: %q", m.input.Value())
	}
	if len(m.view) != len(repos) {
		t.Errorf("clearing left %d rows", len(m.view))
	}
	if it, _ := m.current(); it.label != "github.com/jedipunkz/agx" {
		t.Fatalf("clearing moved the selection to %q", it.label)
	}

	// And the command asks about it.
	m, _ = runSlash(t, m, "/remove")
	if m.over != overlayConfirm || m.ask.arg != repos[0].Path() {
		t.Errorf("/remove asked about %+v, want the selected repository", m.ask)
	}

	// With the box empty, Esc quits as it always did.
	empty := newTestModel(t, repos, "")
	if _, cmd := empty.Update(tea.KeyPressMsg{Code: tea.KeyEscape}); !isQuit(cmd) {
		t.Error("Esc should quit when there is nothing to clear")
	}
}

// TestDirtyFilter covers /dirty end to end: the scan runs off the UI thread,
// the list keeps every row until the answer lands, and running it again shows
// everything.
func TestDirtyFilter(t *testing.T) {
	m, repos := dirtyModel(t)

	m, cmd := runSlash(t, m, "/dirty")
	if cmd == nil {
		t.Fatal("/dirty started no scan")
	}
	// Nothing is hidden while the answer is still coming.
	if len(m.view) != len(repos) {
		t.Errorf("the list shrank before the scan finished: %v", rows(m))
	}
	if !strings.Contains(stripANSI(m.helpLine(90)), "checking") {
		t.Errorf("the scan is not announced: %q", stripANSI(m.helpLine(90)))
	}

	next, _ := m.Update(cmd())
	m = next.(model)
	if got := rows(m); len(got) != 2 || got[0] != "github.com/acme/alpha" || got[1] != "github.com/acme/charlie" {
		t.Fatalf("after the scan the list is %v, want the two dirty ones", got)
	}
	if m.cursor != len(m.view)-1 {
		t.Errorf("cursor at %d, want the bottom row", m.cursor)
	}
	if hint := stripANSI(m.helpLine(90)); !strings.Contains(hint, "dirty only") {
		t.Errorf("the hint line does not say the filter is on: %q", hint)
	}

	// A query narrows what is left, rather than bringing the clean ones back.
	m.input.SetValue("charlie")
	m.filter()
	if got := rows(m); len(got) != 1 || got[0] != "github.com/acme/charlie" {
		t.Errorf("filtering inside /dirty gave %v", got)
	}
	m.input.SetValue("bravo")
	m.filter()
	if got := rows(m); len(got) != 0 {
		t.Errorf("a clean repository came back through the query: %v", got)
	}

	// Running it again shows everything, and does not scan twice.
	m.input.SetValue("")
	m.filter()
	m, cmd = runSlash(t, m, "/dirty")
	if cmd != nil {
		t.Error("/dirty scanned again instead of reusing the answer")
	}
	if len(m.view) != len(repos) {
		t.Errorf("turning the filter off left %v", rows(m))
	}
	if strings.Contains(stripANSI(m.helpLine(90)), "dirty only") {
		t.Error("the hint line still claims the filter is on")
	}
}

// TestDirtyFilterIsForRepositories declines in the worktree list rather than
// filtering something the answer does not describe.
func TestDirtyFilterIsForRepositories(t *testing.T) {
	m, _ := dirtyModel(t)
	m.worktreesOf = func(dir string) ([]repo.Worktree, error) {
		return []repo.Worktree{{Path: dir, Branch: "main"}}, nil
	}
	next, _ := m.openWorktrees()
	m = next.(model)

	m, cmd := runSlash(t, m, "/dirty")
	if cmd != nil || m.dirtyOnly {
		t.Error("/dirty applied itself to the worktree list")
	}
	if !strings.Contains(m.note, "repository list") {
		t.Errorf("the note does not explain why: %q", m.note)
	}
}
