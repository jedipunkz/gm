package finder

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"

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
	if it, _ := m.current(); it.repo.Rel != "github.com/acme/bravo" {
		t.Errorf("selected %q, want the most-visited github.com/acme/bravo", it.repo.Rel)
	}

	m.input.SetValue("charlie")
	m.filter()
	if len(m.view) != 1 {
		t.Fatalf("filter kept %d rows, want 1", len(m.view))
	}
	if it, _ := m.current(); it.repo.Rel != "github.com/other/charlie" {
		t.Errorf("filtered selection is %q, want github.com/other/charlie", it.repo.Rel)
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

	for _, label := range []string{"path", "remote", "branch", "commit", "status", "visits"} {
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
	if it.repo.Rel != "github.com/jedipunkz/miniecs" {
		t.Errorf("selected %q, want github.com/jedipunkz/miniecs", it.repo.Rel)
	}

	// The highlight must sit on the literal "miniec", not be scattered across
	// the host and user segments.
	hits := m.matched[m.view[m.cursor]]
	want := strings.Index(it.repo.Rel, "miniec")
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
