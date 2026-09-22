package finder

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"

	"github.com/jedipunkz/gm/internal/config"
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
	return newModel(repos, hist, theme, testKeys(t))
}

// testKeys are the default chords, parsed the way a real run parses them.
func testKeys(t *testing.T) Keys {
	t.Helper()
	wt, err := config.ParseChord("", DefaultWorktreeKey)
	if err != nil {
		t.Fatal(err)
	}
	rm, err := config.ParseChord("", DefaultRemoteKey)
	if err != nil {
		t.Fatal(err)
	}
	return Keys{Worktree: wt, Remote: rm}
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
		Remote:  "https://github.com/acme/alpha",
		Branch:  "main",
		Commits: []repo.Commit{{Hash: "abc1234", Subject: "a subject long enough to need two lines in the pane"}},
	}

	const w = 30
	lines := m.infoLines(w)
	var plain []string
	for _, l := range lines {
		plain = append(plain, strings.TrimRight(stripANSI(l), " "))
	}
	joined := strings.Join(plain, "\n")

	for _, label := range []string{"path", "remote", "branch", "status", "visits", "last commit"} {
		if !strings.Contains(joined, "\n"+label+"\n") && !strings.HasPrefix(joined, label+"\n") {
			t.Errorf("%q is not on a line of its own:\n%s", label, joined)
		}
	}
	for i, l := range plain {
		if len([]rune(l)) > w {
			t.Errorf("line %d is %d columns wide, want at most %d: %q", i, len([]rune(l)), w, l)
		}
	}
	// The remote is too long for one line, so it must appear on two.
	if !strings.Contains(joined, "https://github.com/acme") {
		t.Errorf("the remote line lost text:\n%s", joined)
	}
	// A commit folds too, with its continuations indented.
	if !strings.Contains(joined, "abc1234") || !strings.Contains(joined, "\n"+commitIndent) {
		t.Errorf("the commit line did not fold:\n%s", joined)
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

func TestBlendRejectsJunk(t *testing.T) {
	if got := blend("not a colour", "#ffffff", 0.5); got != "not a colour" {
		t.Errorf("blend() = %q, want the input back untouched", got)
	}
	if got := blend("#000000", "#ffffff", 1); got != "#ffffff" {
		t.Errorf("blend(black, white, 1) = %q, want #ffffff", got)
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

	m := newModel(repos, hist, theme, keys)
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
		if _, err := Run(nil, hist, theme, k); err == nil {
			t.Errorf("Run() accepted %s, which the finder already uses", name)
		}
		k = keys
		k.Remote = c
		if _, err := Run(nil, hist, theme, k); err == nil {
			t.Errorf("Run() accepted %s for remote_key, which the finder already uses", name)
		}
	}

	// Two actions cannot answer to the same chord.
	k := keys
	k.Remote = k.Worktree
	if _, err := Run(nil, hist, theme, k); err == nil {
		t.Error("Run() accepted the same chord for both keys")
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
	if after.mode != modeRepos || after.chosen != "" {
		t.Errorf("the finder moved: mode=%v chosen=%q", after.mode, after.chosen)
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

// TestRemoteKeyInHints checks the hint line names the configured chord.
func TestRemoteKeyInHints(t *testing.T) {
	root := t.TempDir()
	m := newTestModel(t, []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}, "")
	if got := stripANSI(m.helpLine(120)); !strings.Contains(got, "ctrl-alt-b remote") {
		t.Errorf("the hint line does not name the remote key: %q", got)
	}
}

// TestCommitLineColours pins the decoration colours against the theme: the
// hash, HEAD, a local branch, a remote-tracking branch and a tag each get
// their own, the way `git log --oneline --decorate` does.
func TestCommitLineColours(t *testing.T) {
	root := t.TempDir()
	m := newTestModel(t, []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}, "")
	th := themes[DefaultTheme]

	lines := m.commitLines(repo.Commit{
		Hash: "b1b7b91",
		Refs: []repo.Ref{
			{Name: "HEAD", Kind: repo.RefHead},
			{Name: "feat/x", Kind: repo.RefLocal},
			{Name: "origin/feat/x", Kind: repo.RefRemote},
			{Name: "tag: v1.0", Kind: repo.RefTag},
		},
		Subject: "refactor: name the flag",
	}, 120)
	if len(lines) != 1 {
		t.Fatalf("a line that fits should not fold: %q", lines)
	}
	line := lines[0]

	for _, want := range []struct{ what, hex string }{
		{"hash", blend(th.Blue, th.Comment, 0.35)},
		{"HEAD", th.Cyan},
		{"local branch", th.Blue},
		{"remote branch", blend(th.Blue, th.Comment, 0.5)},
		{"subject", blend(th.Comment, th.Fg, rowLift)},
	} {
		if !strings.Contains(line, ansi("38", want.hex)) {
			t.Errorf("the %s is not painted %s:\n%q", want.what, want.hex, line)
		}
	}

	plain := stripANSI(line)
	if plain != "b1b7b91 (HEAD, feat/x, origin/feat/x, tag: v1.0) refactor: name the flag" {
		t.Errorf("the line reads %q", plain)
	}
}

// TestCommitLineWraps folds a long commit instead of cutting it: no text is
// lost, no line is wider than the pane, and the continuations are indented so
// one commit still reads as one entry.
func TestCommitLineWraps(t *testing.T) {
	root := t.TempDir()
	m := newTestModel(t, []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}, "")
	c := repo.Commit{
		Hash:    "b1b7b91",
		Refs:    []repo.Ref{{Name: "origin/a-long-branch-name", Kind: repo.RefRemote}},
		Subject: "a subject far too long for the pane to hold in one line",
	}
	const whole = "b1b7b91 (origin/a-long-branch-name) a subject far too long for the pane to hold in one line"
	for _, w := range []int{80, 40, 20, 8, 3} {
		lines := m.commitLines(c, w)
		var words []string
		for i, l := range lines {
			plain := stripANSI(l)
			if got := len([]rune(plain)); got > w {
				t.Errorf("width %d, line %d is %d columns: %q", w, i, got, plain)
			}
			if i > 0 && !strings.HasPrefix(plain, commitIndent) {
				t.Errorf("width %d, line %d is not indented: %q", w, i, plain)
			}
			words = append(words, strings.TrimPrefix(plain, commitIndent))
		}
		// Folding must not lose or duplicate anything. Where the break lands
		// is not the point, so compare without the spaces: a break at a space
		// drops it, and a break mid-word does not.
		squash := func(s string) string { return strings.ReplaceAll(s, " ", "") }
		if got := squash(strings.Join(words, "")); got != squash(whole) {
			t.Errorf("width %d lost text:\n got %q\nwant %q", w, got, squash(whole))
		}
	}

	// The fold keeps the colours: a ref broken across two lines is still a ref
	// on both.
	lines := m.commitLines(c, 24)
	if len(lines) < 2 {
		t.Fatalf("width 24 should fold: %q", lines)
	}
	th := themes[DefaultTheme]
	if !strings.Contains(lines[0], ansi("38", blend(th.Blue, th.Comment, 0.35))) {
		t.Errorf("the first line lost the hash colour: %q", lines[0])
	}
}

// TestPaneLabelsStandOut pins the field names as the brightest thing in the
// details pane: the theme's foreground, in bold, not the comment colour the
// dim text uses.
func TestPaneLabelsStandOut(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}
	m := newTestModel(t, repos, "")
	m.status[repos[0].Path()] = repo.Status{Remote: "https://github.com/acme/alpha", Branch: "main"}

	var label string
	for _, l := range m.infoLines(40) {
		if stripANSI(l) == "path" {
			label = l
			break
		}
	}
	if label == "" {
		t.Fatal("the pane has no path label")
	}

	th := themes[DefaultTheme]
	if !strings.Contains(label, ansi("38", th.Fg)) {
		t.Errorf("the label is not painted in the foreground colour: %q", label)
	}
	// lipgloss folds bold into the same escape as the colour, as "1;".
	if !strings.Contains(label, "\x1b[1;") {
		t.Errorf("the label is not bold: %q", label)
	}
	if strings.Contains(label, ansi("38", th.Comment)) {
		t.Errorf("the label still carries the comment colour: %q", label)
	}
}

// TestSlashOnlyStartsACommandAtTheStart pins the rule that makes slash
// commands unambiguous: repository paths are full of slashes, and only a
// leading one means a command.
func TestSlashOnlyStartsACommandAtTheStart(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{
		{Root: root, Rel: "github.com/acme/alpha"},
		{Root: root, Rel: "github.com/other/bravo"},
	}
	m := newTestModel(t, repos, "")

	// A slash inside the query is an ordinary filter character.
	m.input.SetValue("acme/alpha")
	m.filter()
	if len(m.view) != 1 {
		t.Fatalf("\"acme/alpha\" matched %d rows, want 1", len(m.view))
	}
	if it, _ := m.current(); it.label != "github.com/acme/alpha" {
		t.Errorf("selected %q", it.label)
	}

	// A leading slash is a command, so the list is left alone.
	m.input.SetValue("/help")
	m.filter()
	if len(m.view) != len(repos) {
		t.Errorf("a command filtered the list down to %d rows", len(m.view))
	}
	if !isCommand("/help") || isCommand("acme/alpha") {
		t.Error("isCommand disagrees with the rule")
	}
}

// TestHelpPopup covers the command list: Enter on /help opens it, it names
// every command, it swallows keys while it is up, and q or Esc closes it.
func TestHelpPopup(t *testing.T) {
	root := t.TempDir()
	m := newTestModel(t, []repo.Repo{
		{Root: root, Rel: "github.com/acme/alpha"},
		{Root: root, Rel: "github.com/acme/bravo"},
	}, "")
	m.w, m.h = 90, 20

	m.input.SetValue("/help")
	next, _ := m.Update(tea.KeyPressMsg{Code: tea.KeyEnter})
	open := next.(model)
	if !open.help {
		t.Fatal("/help did not open the command list")
	}
	if open.chosen != "" {
		t.Error("/help chose a repository")
	}
	if open.input.Value() != "" {
		t.Errorf("the command was left in the box: %q", open.input.Value())
	}

	view := stripANSI(open.View().Content)
	for _, c := range commands {
		if !strings.Contains(view, c.name) || !strings.Contains(view, c.what) {
			t.Errorf("the popup does not describe %s:\n%s", c.name, view)
		}
	}
	if !strings.Contains(view, "esc") {
		t.Errorf("the popup does not say how to close it:\n%s", view)
	}

	// Moving is impossible while it is up.
	cursor := open.cursor
	moved, _ := open.Update(tea.KeyPressMsg{Code: tea.KeyUp})
	if moved.(model).cursor != cursor || !moved.(model).help {
		t.Error("a key reached the list behind the popup")
	}

	for _, key := range []tea.KeyPressMsg{{Code: 'q'}, {Code: tea.KeyEscape}} {
		closed, cmd := open.Update(key)
		if closed.(model).help {
			t.Errorf("%v did not close the popup", key)
		}
		if isQuit(cmd) {
			t.Errorf("%v quit gm instead of closing the popup", key)
		}
	}
}

// TestUnknownCommand leaves the finder alone and says so under the prompt.
func TestUnknownCommand(t *testing.T) {
	root := t.TempDir()
	m := newTestModel(t, []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}, "")
	m.input.SetValue("/nope")

	next, cmd := m.Update(tea.KeyPressMsg{Code: tea.KeyEnter})
	after := next.(model)
	if isQuit(cmd) || after.chosen != "" {
		t.Error("an unknown command should not end the finder")
	}
	note := stripANSI(after.helpLine(90))
	if !strings.Contains(note, "/nope") || !strings.Contains(note, "/help") {
		t.Errorf("the note does not explain itself: %q", note)
	}

	// The next keystroke clears it.
	after.input.SetValue("/n")
	after.filter()
	if after.note != "" {
		t.Errorf("the note survived a keystroke: %q", after.note)
	}
}

// TestCommandCompletion types a command in and checks the box fills the rest
// in, and that Tab accepts what it offered.
func TestCommandCompletion(t *testing.T) {
	root := t.TempDir()
	var m tea.Model = newTestModel(t, []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}, "")
	for _, r := range "/wo" {
		m, _ = m.Update(tea.KeyPressMsg{Code: r, Text: string(r)})
	}
	if got := stripANSI(m.(model).input.View()); !strings.Contains(got, "/worktrees") {
		t.Errorf("the box does not complete the command: %q", got)
	}
	m, _ = m.Update(tea.KeyPressMsg{Code: tea.KeyTab})
	if got := m.(model).input.Value(); got != "/worktrees" {
		t.Errorf("tab produced %q", got)
	}
}
