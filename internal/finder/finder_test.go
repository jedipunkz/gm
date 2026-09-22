package finder

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"

	"github.com/jedipunkz/gm/internal/repo"
)

// newTestModel builds a finder over repos with an empty visit log, unless
// bumped is set, in which case that repository is the favourite.
func newTestModel(t *testing.T, repos []repo.Repo, bumped string) model {
	t.Helper()
	hist := repo.OpenHistory(filepath.Join(t.TempDir(), "frecency.json"))
	if bumped != "" {
		// The log prunes repositories that are gone, so it has to exist.
		if err := os.MkdirAll(bumped, 0o755); err != nil {
			t.Fatal(err)
		}
		if err := hist.Bump(bumped); err != nil {
			t.Fatal(err)
		}
	}
	theme, err := LookupTheme("")
	if err != nil {
		t.Fatal(err)
	}
	return newModel(repos, hist, theme)
}

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

// TestInfoPaneStacksAndWraps pins the info pane: a label sits on its own line
// above its value, and a value longer than the pane is folded, never cut.
func TestInfoPaneStacksAndWraps(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}
	m := newTestModel(t, repos, "")
	m.status[repos[0].Path()] = repo.Status{
		Remote: "https://github.com/acme/alpha",
		Branch: "main",
		Commit: "abc1234  2 hours ago  a subject long enough to need two lines in the pane",
	}

	const w = 30
	lines := m.infoLines(w)
	var plain []string
	for _, l := range lines {
		plain = append(plain, strings.TrimRight(stripANSI(l), " "))
	}
	joined := strings.Join(plain, "\n")

	for _, label := range []string{"path", "remote", "branch", "last commit", "status", "visits"} {
		if !strings.Contains(joined, "\n"+label+"\n") && !strings.HasPrefix(joined, label+"\n") {
			t.Errorf("%q is not on a line of its own:\n%s", label, joined)
		}
	}
	for i, l := range plain {
		if len([]rune(l)) > w {
			t.Errorf("line %d is %d columns wide, want at most %d: %q", i, len([]rune(l)), w, l)
		}
	}
	// The subject is too long for one line, so it must appear on two.
	if !strings.Contains(joined, "abc1234") || !strings.Contains(joined, "two lines") {
		t.Errorf("the commit line lost text:\n%s", joined)
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

func TestMatchScoreBeatsScatteredMatches(t *testing.T) {
	const run = "github.com/jedipunkz/miniecs"
	const scattered = "github.com/jedipunkz/spacex-ipo-checker"
	runScore := matchScore(run, substringMatch(run, "miniec"))
	// m-i-n from the host and user, then i, e, c spread through the name.
	scatteredScore := matchScore(scattered, []int{9, 14, 17, 29, 35, 36})
	if runScore <= scatteredScore {
		t.Errorf("contiguous match scored %v, scattered scored %v", runScore, scatteredScore)
	}
}

func TestThemes(t *testing.T) {
	for _, name := range ThemeNames() {
		th, err := LookupTheme(name)
		if err != nil {
			t.Fatalf("LookupTheme(%q) = %v", name, err)
		}
		// A zero field renders as the terminal default, which reads as a hole
		// in the palette.
		for field, v := range map[string]string{
			"BgHi": th.BgHi, "Border": th.Border, "Comment": th.Comment, "Fg": th.Fg,
			"Blue": th.Blue, "Cyan": th.Cyan, "Magenta": th.Magenta,
			"Green": th.Green, "Yellow": th.Yellow, "Orange": th.Orange, "Red": th.Red,
		} {
			if !strings.HasPrefix(v, "#") || len(v) != 7 {
				t.Errorf("theme %s: %s = %q, want a #rrggbb color", name, field, v)
			}
		}
		th.Styles() // must not panic on any theme
	}

	if _, err := LookupTheme(""); err != nil {
		t.Errorf("the empty name must fall back to %s: %v", DefaultTheme, err)
	}
	if _, err := LookupTheme("nope"); err == nil {
		t.Error("LookupTheme(\"nope\") = nil, want an error")
	}
}

// ansi renders a #rrggbb color the way lipgloss writes it into the output.
func ansi(layer, hex string) string {
	var r, g, b int
	if _, err := fmt.Sscanf(hex, "#%02x%02x%02x", &r, &g, &b); err != nil {
		panic(err)
	}
	return fmt.Sprintf("%s;2;%d;%d;%d", layer, r, g, b)
}

func stripANSI(s string) string {
	var b strings.Builder
	for i := 0; i < len(s); {
		if s[i] == 0x1b {
			for i < len(s) && s[i] != 'm' {
				i++
			}
			i++
			continue
		}
		b.WriteByte(s[i])
		i++
	}
	return b.String()
}

// TestWorktreeMode pins the Ctrl-W list: it replaces the repositories, the
// main worktree rests under the cursor, typing filters it, Enter yields that
// worktree's path, and Esc puts the repository list back untouched.
func TestWorktreeMode(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{
		{Root: root, Rel: "github.com/acme/alpha"},
		{Root: root, Rel: "github.com/acme/bravo"},
	}
	m := newTestModel(t, repos, "")
	m.input.SetValue("bravo")
	m.filter()
	m.worktreesOf = func(dir string) ([]repo.Worktree, error) {
		if dir != repos[1].Path() {
			t.Errorf("asked for the worktrees of %q, want %q", dir, repos[1].Path())
		}
		return []repo.Worktree{
			{Path: dir, Branch: "main"},
			{Path: "/tmp/wt/bravo-login", Branch: "feat/login"},
			{Path: "/tmp/wt/bravo-fix", Branch: "fix/timeout"},
		}, nil
	}

	next, _ := m.openWorktrees()
	m = next.(model)
	if m.mode != modeWorktrees {
		t.Fatal("Ctrl-W did not open the worktree list")
	}
	if len(m.view) != 3 {
		t.Fatalf("worktree list has %d rows, want 3", len(m.view))
	}
	if it, _ := m.current(); it.label != "main" {
		t.Errorf("cursor rests on %q, want the main worktree", it.label)
	}
	if m.input.Value() != "" {
		t.Errorf("the repository query %q leaked into the worktree list", m.input.Value())
	}

	m.input.SetValue("login")
	m.filter()
	it, ok := m.current()
	if !ok || it.label != "feat/login" {
		t.Fatalf("filtering for \"login\" selected %q", it.label)
	}
	if it.path != "/tmp/wt/bravo-login" {
		t.Errorf("Enter would jump to %q, want the worktree path", it.path)
	}

	m.restore()
	if m.mode != modeRepos {
		t.Fatal("Esc did not return to the repository list")
	}
	if m.input.Value() != "bravo" {
		t.Errorf("the repository query came back as %q, want bravo", m.input.Value())
	}
	if it, _ := m.current(); it.label != "github.com/acme/bravo" {
		t.Errorf("the repository selection came back as %q", it.label)
	}
}

// TestWorktreeModeKeysGoBack pins the three ways out of the worktree list:
// Ctrl-W toggles it off, Ctrl-G closes it, and Esc does too. Only Esc quits
// gm, and only from the repository list.
func TestWorktreeModeKeysGoBack(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}

	keys := map[string]tea.KeyPressMsg{
		"ctrl+w": {Code: 'w', Mod: tea.ModCtrl},
		"ctrl+g": {Code: 'g', Mod: tea.ModCtrl},
		"esc":    {Code: tea.KeyEscape},
	}
	for key, press := range keys {
		m := newTestModel(t, repos, "")
		m.worktreesOf = func(dir string) ([]repo.Worktree, error) {
			return []repo.Worktree{{Path: dir, Branch: "main"}}, nil
		}
		next, _ := m.openWorktrees()
		m = next.(model)
		if m.mode != modeWorktrees {
			t.Fatal("the worktree list did not open")
		}

		next, cmd := m.Update(press)
		m = next.(model)
		if m.mode != modeRepos {
			t.Errorf("%s did not return to the repository list", key)
		}
		if isQuit(cmd) {
			t.Errorf("%s quit gm instead of closing the worktree list", key)
		}
	}

	// From the repository list, Esc still quits and Ctrl-G does nothing.
	m := newTestModel(t, repos, "")
	if _, cmd := m.Update(tea.KeyPressMsg{Code: tea.KeyEscape}); !isQuit(cmd) {
		t.Error("Esc should quit from the repository list")
	}
	if _, cmd := m.Update(tea.KeyPressMsg{Code: 'g', Mod: tea.ModCtrl}); isQuit(cmd) {
		t.Error("Ctrl-G should not quit from the repository list")
	}
}

// isQuit reports whether a command is tea.Quit, which is what ends the finder.
func isQuit(cmd tea.Cmd) bool {
	if cmd == nil {
		return false
	}
	_, ok := cmd().(tea.QuitMsg)
	return ok
}

// TestWorktreeModeLeavesTheListAloneOnError guards the case where git cannot
// answer: the finder must stay usable rather than empty itself.
func TestWorktreeModeLeavesTheListAloneOnError(t *testing.T) {
	root := t.TempDir()
	m := newTestModel(t, []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}, "")
	m.worktreesOf = func(string) ([]repo.Worktree, error) { return nil, errors.New("not a git repository") }

	next, _ := m.openWorktrees()
	m = next.(model)
	if m.mode != modeRepos || len(m.view) != 1 {
		t.Errorf("a failed worktree listing changed the finder: mode=%v rows=%d", m.mode, len(m.view))
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

// TestUnselectedRowsAreLifted pins the contrast ladder in the list: an
// unselected row sits between the comment colour and the foreground, so it
// reads without competing with the selected row.
func TestUnselectedRowsAreLifted(t *testing.T) {
	dist := func(a, b string) int {
		var ar, ag, ab, br, bg, bb int
		fmt.Sscanf(a, "#%02x%02x%02x", &ar, &ag, &ab)
		fmt.Sscanf(b, "#%02x%02x%02x", &br, &bg, &bb)
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

func TestBlendRejectsJunk(t *testing.T) {
	if got := blend("not a colour", "#ffffff", 0.5); got != "not a colour" {
		t.Errorf("blend() = %q, want the input back untouched", got)
	}
	if got := blend("#000000", "#ffffff", 1); got != "#ffffff" {
		t.Errorf("blend(black, white, 1) = %q, want #ffffff", got)
	}
}
