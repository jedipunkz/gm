package finder

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"
	"github.com/jedipunkz/gm/internal/config"
	"github.com/jedipunkz/gm/internal/repo"
)

// TestViewChrome pins the finder's layout: no header, a bordered prompt, a
// highlighted selection, and the query's characters picked out inside it.
func TestViewChrome(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{
		{Root: root, Rel: "github.com/acme/alpha"},
		{Root: root, Rel: "github.com/acme/bravo"},
	}
	m := newTestModel(t, repos, "")
	m.w, m.h = 100, 10
	m.input.SetValue("brav")
	m.filter()
	out := m.View().Content

	th := themes[DefaultTheme]
	for _, want := range []struct{ what, s string }{
		{"rounded input box", "╭"},
		{"input box bottom", "╰"},
		{"selection marker", "▸"},
		{"info pane divider", "│ "},
		{"theme border grey", ansi("38", th.Border)},
		{"selected row background", ansi("48", th.BgHi)},
		{"matched characters", ansi("38", th.Orange)},
	} {
		if !strings.Contains(out, want.s) {
			t.Errorf("view is missing the %s (%q)", want.what, want.s)
		}
	}
	if first, _, _ := strings.Cut(out, "\n"); strings.Contains(first, "1/2") {
		t.Errorf("the count header should be gone, got %q", first)
	}
}

// TestPromptStartsEmpty guards the input box against a placeholder, which
// reads as something the user already typed.
func TestPromptStartsEmpty(t *testing.T) {
	root := t.TempDir()
	m := newTestModel(t, []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}, "")
	m.w, m.h = 80, 10

	var prompt string
	for _, line := range strings.Split(stripANSI(m.View().Content), "\n") {
		if strings.Contains(line, "❯") {
			prompt = line
			break
		}
	}
	if prompt == "" {
		t.Fatal("the view has no prompt line")
	}
	_, rest, _ := strings.Cut(prompt, "❯")
	if got := strings.TrimSpace(strings.Trim(rest, "│")); got != "" {
		t.Errorf("the prompt starts with %q, want nothing", got)
	}
}

// TestRunRejectsReservedKeys guards the chords that would leave the finder
// impossible to quit or move around in.
func TestRunRejectsReservedKeys(t *testing.T) {
	theme, _ := LookupTheme("")
	hist := repo.OpenHistory(filepath.Join(t.TempDir(), "frecency.json"))
	keys := testKeys(t)
	for _, name := range []string{"ctrl-c", "ctrl-n", "ctrl-p"} {
		c, err := config.ParseChord(name, DefaultWorktreeKey)
		if err != nil {
			t.Fatal(err)
		}
		k := keys
		k.Worktree = c
		if _, err := Run(nil, nil, hist, theme, k); err == nil {
			t.Errorf("Run() accepted %s, which the finder already uses", name)
		}
		k = keys
		k.Remote = c
		if _, err := Run(nil, nil, hist, theme, k); err == nil {
			t.Errorf("Run() accepted %s for remote_key, which the finder already uses", name)
		}
	}

	// Two actions cannot answer to the same chord.
	k := keys
	k.Remote = k.Worktree
	if _, err := Run(nil, nil, hist, theme, k); err == nil {
		t.Error("Run() accepted the same chord for both keys")
	}
}

// TestWorktreeKeyIsConfigurable pins the two places the configured chord has
// to reach: the key that opens the list, and the hint line that names it.
func TestWorktreeKeyIsConfigurable(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}
	hist := repo.OpenHistory(filepath.Join(t.TempDir(), "frecency.json"))
	theme, err := LookupTheme("")
	if err != nil {
		t.Fatal(err)
	}
	wt, err := config.ParseChord("ctrl-t", DefaultWorktreeKey)
	if err != nil {
		t.Fatal(err)
	}
	keys := testKeys(t)
	keys.Worktree = wt

	m := newModel(&repo.Tree{Roots: []string{root}}, repos, hist, theme, keys)
	m.w, m.h = 90, 12
	m.worktreesOf = func(dir string) ([]repo.Worktree, error) {
		return []repo.Worktree{{Path: dir, Branch: "main"}}, nil
	}

	if got := stripANSI(m.helpLine(90)); !strings.Contains(got, "ctrl-t worktrees") {
		t.Errorf("the hint line does not name the configured key: %q", got)
	}

	// The old default must no longer do anything.
	next, _ := m.Update(tea.KeyPressMsg{Code: 'w', Mod: tea.ModCtrl})
	if next.(model).mode != modeRepos {
		t.Error("ctrl-w still opened the worktree list")
	}

	next, _ = m.Update(tea.KeyPressMsg{Code: 't', Mod: tea.ModCtrl})
	opened := next.(model)
	if opened.mode != modeWorktrees {
		t.Fatal("the configured key did not open the worktree list")
	}
	if got := stripANSI(opened.helpLine(90)); !strings.Contains(got, "ctrl-t/g/esc repos") {
		t.Errorf("the worktree hints do not name the configured key: %q", got)
	}
	// And it toggles back off.
	back, _ := opened.Update(tea.KeyPressMsg{Code: 't', Mod: tea.ModCtrl})
	if back.(model).mode != modeRepos {
		t.Error("the configured key did not close the worktree list")
	}
}

// TestRemoteKeyInHints checks the hint line names the configured chord.
func TestRemoteKeyInHints(t *testing.T) {
	root := t.TempDir()
	m := newTestModel(t, []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}, "")
	if got := stripANSI(m.helpLine(120)); !strings.Contains(got, "ctrl-alt-b remote") {
		t.Errorf("the hint line does not name the remote key: %q", got)
	}
}

// TestHelpLine pins the hints under the prompt: they name the keys for the
// list that is up, the key names carry their own colour, and a narrow window
// drops hints instead of wrapping.
func TestHelpLine(t *testing.T) {
	root := t.TempDir()
	m := newTestModel(t, []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}, "")
	m.w, m.h = 90, 12

	lines := strings.Split(m.View().Content, "\n")
	help := lines[len(lines)-1]
	plain := stripANSI(help)
	for _, want := range []string{"↑↓ ctrl-p/n move", "enter jump", "ctrl-w worktrees", "esc quit"} {
		if !strings.Contains(plain, want) {
			t.Errorf("the hint line is missing %q: %q", want, plain)
		}
	}
	if !strings.Contains(help, ansi("38", themes[DefaultTheme].Blue)) {
		t.Errorf("the key names are not coloured: %q", help)
	}

	// In the worktree list the same keys go back instead of out.
	m.worktreesOf = func(dir string) ([]repo.Worktree, error) {
		return []repo.Worktree{{Path: dir, Branch: "main"}}, nil
	}
	next, _ := m.openWorktrees()
	wm := next.(model)
	wm.w, wm.h = 90, 12
	wl := strings.Split(wm.View().Content, "\n")
	plain = stripANSI(wl[len(wl)-1])
	if !strings.Contains(plain, "ctrl-w/g/esc repos") {
		t.Errorf("the worktree hints are wrong: %q", plain)
	}

	// Too narrow for everything: the tail is dropped, nothing wraps.
	narrow := stripANSI(m.helpLine(24))
	if strings.Contains(narrow, "\n") {
		t.Errorf("the hint line wrapped: %q", narrow)
	}
	if strings.Contains(narrow, "esc") {
		t.Errorf("a hint that does not fit was drawn anyway: %q", narrow)
	}
	if !strings.Contains(narrow, "↑↓ ctrl-p/n move") {
		t.Errorf("the first hint was dropped: %q", narrow)
	}
}

// TestOpenRemote pins the remote key: it hands the browser an https URL built
// from the selected repository's origin, and does nothing at all when there
// is no origin.
func TestOpenRemote(t *testing.T) {
	var opened []string
	old := openURL
	openURL = func(u string) error { opened = append(opened, u); return nil }
	defer func() { openURL = old }()

	root := t.TempDir()
	repos := []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}
	m := newTestModel(t, repos, "")
	m.status[repos[0].Path()] = repo.Status{Remote: "git@github.com:acme/alpha.git"}

	next, cmd := m.Update(tea.KeyPressMsg{Code: 'b', Mod: tea.ModCtrl | tea.ModAlt})
	if cmd == nil {
		t.Fatal("the remote key produced no command")
	}
	cmd()
	if len(opened) != 1 || opened[0] != "https://github.com/acme/alpha" {
		t.Errorf("opened %v, want the https URL", opened)
	}
	// The finder stays where it was: this is a side action, not navigation.
	after := next.(model)
	if after.mode != modeRepos || after.result.Action != ActionNone {
		t.Errorf("the finder moved: mode=%v action=%v", after.mode, after.result.Action)
	}

	// A repository whose remote git cannot supply opens nothing.
	opened = nil
	m.status[repos[0].Path()] = repo.Status{Remote: ""}
	_, cmd = m.Update(tea.KeyPressMsg{Code: 'b', Mod: tea.ModCtrl | tea.ModAlt})
	cmd()
	if len(opened) != 0 {
		t.Errorf("opened %v for a repository with no origin", opened)
	}
}

// TestGetLeavesTheFinder: cloning needs the network, a progress bar and
// sometimes a passphrase, so it is the one action that still happens outside.
func TestGetLeavesTheFinder(t *testing.T) {
	root := t.TempDir()
	m := newTestModel(t, []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}, "")

	m, cmd := runSlash(t, m, "/get github.com/acme/charlie")
	if !isQuit(cmd) {
		t.Error("/get did not close the finder")
	}
	if m.result.Action != ActionGet || m.result.Arg != "github.com/acme/charlie" {
		t.Errorf("/get produced %+v", m.result)
	}
}

// TestEscClearsTheFilter pins the way out of /dirty: one key, and only once
// there is nothing left to undo does Esc quit.
func TestEscClearsTheFilter(t *testing.T) {
	m, repos := dirtyModel(t)

	m, cmd := runSlash(t, m, "/dirty")
	next, _ := m.Update(cmd())
	m = next.(model)
	if len(m.view) != 2 {
		t.Fatalf("the filter is not on: %v", rows(m))
	}
	if hint := stripANSI(m.helpLine(90)); !strings.Contains(hint, "esc show all") {
		t.Errorf("the hint line does not offer the way out: %q", hint)
	}

	next, cmd = m.Update(tea.KeyPressMsg{Code: tea.KeyEscape})
	m = next.(model)
	if isQuit(cmd) {
		t.Fatal("Esc quit instead of clearing the filter")
	}
	if len(m.view) != len(repos) || m.dirtyOnly {
		t.Errorf("Esc left the filter on: %v", rows(m))
	}
	if m.cursor != len(m.view)-1 {
		t.Errorf("cursor at %d, want the bottom row", m.cursor)
	}
	if hint := stripANSI(m.helpLine(90)); !strings.Contains(hint, "esc quit") {
		t.Errorf("the hint line still offers to clear a filter: %q", hint)
	}

	// With nothing left to undo, Esc quits as it always did.
	if _, cmd := m.Update(tea.KeyPressMsg{Code: tea.KeyEscape}); !isQuit(cmd) {
		t.Error("Esc should quit once the filter is off")
	}
}

// TestEscLeavesTheWorktreeListFirst keeps the order of the escalation: the
// worktree list is backed out of before the filter is.
func TestEscLeavesTheWorktreeListFirst(t *testing.T) {
	m, _ := dirtyModel(t)
	m.worktreesOf = func(dir string) ([]repo.Worktree, error) {
		return []repo.Worktree{{Path: dir, Branch: "main"}}, nil
	}

	m, cmd := runSlash(t, m, "/dirty")
	next, _ := m.Update(cmd())
	m = next.(model)

	opened, _ := m.openWorktrees()
	m = opened.(model)
	if m.mode != modeWorktrees {
		t.Fatal("the worktree list did not open")
	}

	next, _ = m.Update(tea.KeyPressMsg{Code: tea.KeyEscape})
	m = next.(model)
	if m.mode != modeRepos {
		t.Fatal("Esc did not leave the worktree list")
	}
	if !m.dirtyOnly {
		t.Error("Esc cleared the filter on the way out of the worktree list")
	}
}

// TestRemovingTheLastRepositoryLeavesAUsableFinder: an empty list has no
// selection, and everything that reads one has to cope.
func TestRemovingTheLastRepositoryLeavesAUsableFinder(t *testing.T) {
	root := t.TempDir()
	r := repo.Repo{Root: root, Rel: "github.com/acme/alpha"}
	if err := os.MkdirAll(filepath.Join(r.Path(), ".git"), 0o755); err != nil {
		t.Fatal(err)
	}
	m := newTestModel(t, []repo.Repo{r}, "")
	m.w, m.h = 80, 12

	m, _ = runSlash(t, m, "/remove")
	next, cmd := m.Update(tea.KeyPressMsg{Code: 'y', Text: "y"})
	next, _ = next.(model).Update(cmd())
	m = next.(model)

	if len(m.view) != 0 {
		t.Fatalf("the list still has %v", rows(m))
	}
	if _, ok := m.current(); ok {
		t.Error("an empty list still reports a selection")
	}
	// None of these may panic on an empty list.
	if out := stripANSI(m.View().Content); !strings.Contains(out, "no match") {
		t.Errorf("the pane does not say the list is empty:\n%s", out)
	}
	for _, key := range []tea.KeyPressMsg{{Code: tea.KeyUp}, {Code: tea.KeyDown}, {Code: tea.KeyEnter}} {
		after, _ := m.Update(key)
		if after.(model).result.Arg != "" {
			t.Errorf("%v chose something out of an empty list", key)
		}
	}
	// And a command that needs a selection says so.
	m, _ = runSlash(t, m, "/remove")
	if m.over != overlayNone || !strings.Contains(m.note, "nothing is selected") {
		t.Errorf("/remove on an empty list: over=%v note=%q", m.over, m.note)
	}
}
