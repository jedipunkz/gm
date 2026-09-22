package main

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestNormalizeURL(t *testing.T) {
	cases := []struct {
		ref  string
		ssh  bool
		want string
		rel  string
	}{
		{"https://github.com/x-motemen/ghq", false, "https://github.com/x-motemen/ghq", "github.com/x-motemen/ghq"},
		{"https://github.com/x-motemen/ghq.git", false, "https://github.com/x-motemen/ghq.git", "github.com/x-motemen/ghq"},
		{"git@github.com:x-motemen/ghq.git", false, "ssh://git@github.com/x-motemen/ghq.git", "github.com/x-motemen/ghq"},
		{"x-motemen/ghq", false, "https://github.com/x-motemen/ghq", "github.com/x-motemen/ghq"},
		{"gitlab.com/g/p", false, "https://gitlab.com/g/p", "gitlab.com/g/p"},
		{"git.example.com:2222/g/p", false, "ssh://git.example.com/2222/g/p", "git.example.com/2222/g/p"},
		{"x-motemen/ghq", true, "ssh://git@github.com/x-motemen/ghq", "github.com/x-motemen/ghq"},
		{"https://github.com/x-motemen/ghq/", false, "https://github.com/x-motemen/ghq", "github.com/x-motemen/ghq"},
	}
	for _, c := range cases {
		u, err := NormalizeURL(c.ref, c.ssh)
		if err != nil {
			t.Fatalf("NormalizeURL(%q): %v", c.ref, err)
		}
		if got := u.String(); got != c.want {
			t.Errorf("NormalizeURL(%q) = %q, want %q", c.ref, got, c.want)
		}
		if got := RelPathOf(u); got != c.rel {
			t.Errorf("RelPathOf(%q) = %q, want %q", c.ref, got, c.rel)
		}
	}

	for _, bad := range []string{"", "   ", "https://github.com"} {
		if _, err := NormalizeURL(bad, false); err == nil {
			t.Errorf("NormalizeURL(%q) should fail", bad)
		}
	}
}

func TestMatch(t *testing.T) {
	const rel = "github.com/x-motemen/ghq"
	cases := []struct {
		query string
		exact bool
		want  bool
	}{
		{"ghq", false, true},
		{"ghq", true, true},
		{"x-motemen/ghq", true, true},
		{"github.com/x-motemen/ghq", true, true},
		{"motemen/ghq", true, false}, // exact means whole segments
		{"motemen", false, true},
		{"nope", false, false},
		{"", false, true},
	}
	for _, c := range cases {
		if got := Match(rel, c.query, c.exact); got != c.want {
			t.Errorf("Match(%q, exact=%v) = %v, want %v", c.query, c.exact, got, c.want)
		}
	}
}

func TestShortestUnique(t *testing.T) {
	repos := []Repo{
		{Rel: "github.com/a/ghq"},
		{Rel: "github.com/b/ghq"},
		{Rel: "github.com/a/gm"},
	}
	want := []string{"a/ghq", "b/ghq", "gm"}
	got := ShortestUnique(repos)
	for i := range want {
		if got[i] != want[i] {
			t.Errorf("ShortestUnique()[%d] = %q, want %q", i, got[i], want[i])
		}
	}
}

func TestVisitScoreRanksRecentFirst(t *testing.T) {
	now := time.Now()
	recent := visit{Count: 2, Last: now.Add(-30 * time.Minute).Unix()}
	stale := visit{Count: 10, Last: now.Add(-60 * 24 * time.Hour).Unix()}
	if recent.Score(now) <= stale.Score(now) {
		t.Errorf("two visits this hour (%v) should outrank ten last year (%v)",
			recent.Score(now), stale.Score(now))
	}
	if (visit{}).Score(now) != 0 {
		t.Error("an unvisited repository must score 0")
	}
}

// TestFinderPutsBestAtBottom pins the core TUI contract: the cursor rests on
// the most-used repository, and it is the last row drawn.
func TestFinderPutsBestAtBottom(t *testing.T) {
	root := t.TempDir()
	t.Setenv("XDG_STATE_HOME", t.TempDir())

	repos := []Repo{
		{Root: root, Rel: "github.com/acme/alpha"},
		{Root: root, Rel: "github.com/acme/bravo"},
		{Root: root, Rel: "github.com/other/charlie"},
	}
	for _, r := range repos {
		if err := os.MkdirAll(r.Path(), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	if err := Bump(repos[1].Path()); err != nil { // bravo is the favourite
		t.Fatal(err)
	}

	m := newModel(repos)
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
	t.Setenv("XDG_STATE_HOME", t.TempDir())
	repos := []Repo{
		{Root: root, Rel: "github.com/acme/alpha"},
		{Root: root, Rel: "github.com/acme/bravo"},
	}
	m := newModel(repos)
	m.w, m.h = 100, 10
	m.input.SetValue("brav")
	m.filter()
	out := m.View().Content

	for _, want := range []struct{ what, s string }{
		{"rounded input box", "╭"},
		{"input box bottom", "╰"},
		{"selection marker", "▸"},
		{"info pane divider", "│ "},
		{"tokyonight border grey", ansi("38", themes["tokyonight"].border)},
		{"selected row background", ansi("48", themes["tokyonight"].bgHi)},
		// Accents are muted at apply time, so ask for the color the UI uses.
		{"matched characters", ansi("38", mute(themes["tokyonight"].orange))},
	} {
		if !strings.Contains(out, want.s) {
			t.Errorf("view is missing the %s (%q)", want.what, want.s)
		}
	}
	if first, _, _ := strings.Cut(out, "\n"); strings.Contains(first, "1/2") {
		t.Errorf("the count header should be gone, got %q", first)
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

// TestRankingPrefersTheRepositoryName guards the case that made scattered
// matches across the shared "github.com/user/" prefix outrank a literal hit in
// the repository name.
func TestRankingPrefersTheRepositoryName(t *testing.T) {
	root := t.TempDir()
	t.Setenv("XDG_STATE_HOME", t.TempDir())
	repos := []Repo{
		{Root: root, Rel: "github.com/jedipunkz/detect-minecraft-versions"},
		{Root: root, Rel: "github.com/jedipunkz/miniecs"},
		{Root: root, Rel: "github.com/jedipunkz/spacex-ipo-checker"},
	}
	m := newModel(repos)
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

func TestConfigRoots(t *testing.T) {
	write := func(t *testing.T, body string) string {
		t.Helper()
		dir := t.TempDir()
		t.Setenv("XDG_CONFIG_HOME", dir)
		if body != "" {
			if err := os.MkdirAll(filepath.Join(dir, "gm"), 0o755); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(filepath.Join(dir, "gm", "gm.toml"), []byte(body), 0o644); err != nil {
				t.Fatal(err)
			}
		}
		return dir
	}

	t.Run("missing file is not an error", func(t *testing.T) {
		write(t, "")
		got, err := configRoots()
		if err != nil || got != nil {
			t.Errorf("configRoots() = %v, %v; want nil, nil", got, err)
		}
	})

	t.Run("a single root", func(t *testing.T) {
		write(t, "root = \"~/code\"\n")
		got, err := configRoots()
		if err != nil {
			t.Fatal(err)
		}
		if len(got) != 1 || got[0] != "~/code" {
			t.Errorf("configRoots() = %v, want [~/code]", got)
		}
	})

	t.Run("several roots", func(t *testing.T) {
		write(t, "root = [\"/a\", \"/b\"]\n")
		got, err := configRoots()
		if err != nil {
			t.Fatal(err)
		}
		if len(got) != 2 || got[0] != "/a" || got[1] != "/b" {
			t.Errorf("configRoots() = %v, want [/a /b]", got)
		}
	})

	t.Run("Roots honours the file and expands ~", func(t *testing.T) {
		write(t, "root = \"~/code\"\n")
		t.Setenv("GM_ROOT", "")
		got, err := Roots()
		if err != nil {
			t.Fatal(err)
		}
		home, _ := os.UserHomeDir()
		if len(got) != 1 || got[0] != filepath.Join(home, "code") {
			t.Errorf("Roots() = %v, want [%s]", got, filepath.Join(home, "code"))
		}
	})

	t.Run("GM_ROOT still wins", func(t *testing.T) {
		write(t, "root = \"/from-file\"\n")
		t.Setenv("GM_ROOT", "/from-env")
		got, err := Roots()
		if err != nil {
			t.Fatal(err)
		}
		if len(got) != 1 || got[0] != "/from-env" {
			t.Errorf("Roots() = %v, want [/from-env]", got)
		}
	})

	// A broken config must stop gm rather than silently send clones elsewhere.
	for name, body := range map[string]string{
		"malformed":  "root = \n",
		"wrong type": "root = 42\n",
		"mixed list": "root = [\"/a\", 7]\n",
		"empty list": "root = []\n",
	} {
		t.Run(name+" is an error", func(t *testing.T) {
			write(t, body)
			if got, err := configRoots(); err == nil {
				t.Errorf("configRoots() = %v, nil; want an error", got)
			}
		})
	}
}

func TestThemes(t *testing.T) {
	defer applyTheme(defaultTheme)

	for _, name := range themeNames() {
		p := themes[name]
		// A zero field renders as the terminal default, which reads as a hole
		// in the palette.
		for field, v := range map[string]string{
			"bgHi": p.bgHi, "border": p.border, "comment": p.comment, "fg": p.fg,
			"blue": p.blue, "cyan": p.cyan, "magenta": p.magenta,
			"green": p.green, "yellow": p.yellow, "orange": p.orange, "red": p.red,
		} {
			if !strings.HasPrefix(v, "#") || len(v) != 7 {
				t.Errorf("theme %s: %s = %q, want a #rrggbb color", name, field, v)
			}
		}
		if err := applyTheme(name); err != nil {
			t.Errorf("applyTheme(%q) = %v", name, err)
		}
	}

	if err := applyTheme("nope"); err == nil {
		t.Error("applyTheme(\"nope\") = nil, want an error")
	}
}
