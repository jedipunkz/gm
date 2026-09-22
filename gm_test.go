package main

import (
	"os"
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
