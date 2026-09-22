package repo

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/jedipunkz/gm/internal/config"
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

func TestOpenResolvesRootsInOrder(t *testing.T) {
	home, err := os.UserHomeDir()
	if err != nil {
		t.Fatal(err)
	}

	t.Run("the config file, with ~ expanded", func(t *testing.T) {
		t.Setenv("GM_ROOT", "")
		tree, err := Open(config.Config{Root: "~/code"})
		if err != nil {
			t.Fatal(err)
		}
		want := filepath.Join(home, "code")
		if len(tree.Roots) != 1 || tree.Primary() != want {
			t.Errorf("Roots = %v, want [%s]", tree.Roots, want)
		}
	})

	t.Run("GM_ROOT still wins", func(t *testing.T) {
		t.Setenv("GM_ROOT", "/from-env")
		tree, err := Open(config.Config{Root: "/from-file"})
		if err != nil {
			t.Fatal(err)
		}
		if tree.Primary() != "/from-env" {
			t.Errorf("Primary() = %q, want /from-env", tree.Primary())
		}
	})

	t.Run("a broken config stops gm", func(t *testing.T) {
		t.Setenv("GM_ROOT", "")
		if _, err := Open(config.Config{Root: 42}); err == nil {
			t.Error("Open() with a non-string root should fail")
		}
	})
}

// TestTreeListAndResolve walks a real directory tree: repositories are found,
// nested ones are not descended into, and an ambiguous query is refused.
func TestTreeListAndResolve(t *testing.T) {
	root := t.TempDir()
	for _, rel := range []string{
		"github.com/acme/alpha",
		"github.com/acme/bravo",
		"github.com/other/alpha",
		"github.com/acme/alpha/vendor/nested", // inside a repository: skipped
	} {
		if err := os.MkdirAll(filepath.Join(root, rel, ".git"), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	tree := &Tree{Roots: []string{root}}

	repos, err := tree.List()
	if err != nil {
		t.Fatal(err)
	}
	if len(repos) != 3 {
		t.Fatalf("List() found %d repositories, want 3: %v", len(repos), repos)
	}

	r, err := tree.Resolve("bravo")
	if err != nil {
		t.Fatal(err)
	}
	if r.Rel != "github.com/acme/bravo" {
		t.Errorf("Resolve(\"bravo\") = %q", r.Rel)
	}
	if _, err := tree.Resolve("alpha"); err == nil {
		t.Error("Resolve() must refuse an ambiguous query")
	}
	if _, err := tree.Resolve("nothing"); err == nil {
		t.Error("Resolve() must fail when nothing matches")
	}
}

func TestVisitScoreRanksRecentFirst(t *testing.T) {
	now := time.Now()
	recent := Visit{Count: 2, Last: now.Add(-30 * time.Minute).Unix()}
	stale := Visit{Count: 10, Last: now.Add(-60 * 24 * time.Hour).Unix()}
	if recent.Score(now) <= stale.Score(now) {
		t.Errorf("two visits this hour (%v) should outrank ten last year (%v)",
			recent.Score(now), stale.Score(now))
	}
	if (Visit{}).Score(now) != 0 {
		t.Error("an unvisited repository must score 0")
	}
}

// TestHistoryRoundTrip pins the log's contract: a bump survives a reload, and
// a repository that no longer exists is pruned.
func TestHistoryRoundTrip(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "frecency.json")
	live := filepath.Join(dir, "live")
	if err := os.MkdirAll(live, 0o755); err != nil {
		t.Fatal(err)
	}

	h := OpenHistory(path)
	if err := h.Bump(live); err != nil {
		t.Fatal(err)
	}
	if err := h.Bump(filepath.Join(dir, "gone")); err != nil {
		t.Fatal(err)
	}

	again := OpenHistory(path)
	if got := again.Visit(live).Count; got != 1 {
		t.Errorf("visit count = %d, want 1", got)
	}
	if got := again.Visit(filepath.Join(dir, "gone")).Count; got != 0 {
		t.Errorf("a removed repository is still logged (count %d)", got)
	}
}

func TestParseWorktrees(t *testing.T) {
	const out = `worktree /home/u/ghq/github.com/u/agx
HEAD 0123456789abcdef0123456789abcdef01234567
branch refs/heads/main

worktree /home/u/.worktrees/agx-login
HEAD 89abcdef0123456789abcdef0123456789abcdef
branch refs/heads/feat/login

worktree /home/u/.worktrees/agx-detached
HEAD fedcba9876543210fedcba9876543210fedcba98
detached
`
	got := parseWorktrees(out)
	if len(got) != 3 {
		t.Fatalf("parsed %d worktrees, want 3: %v", len(got), got)
	}
	// The main worktree comes first, the way git reports it.
	if got[0].Path != "/home/u/ghq/github.com/u/agx" || got[0].Branch != "main" {
		t.Errorf("first worktree = %+v", got[0])
	}
	if got[1].Branch != "feat/login" || got[1].Label() != "feat/login" {
		t.Errorf("second worktree = %+v", got[1])
	}
	if got[2].Branch != "" || got[2].Label() != "fedcba9" {
		t.Errorf("a detached HEAD should be labelled by its hash, got %q", got[2].Label())
	}
	if len(parseWorktrees("")) != 0 {
		t.Error("empty output must parse to no worktrees")
	}
	if w := (Worktree{Path: "/x", Bare: true}); w.Label() != "(bare)" {
		t.Errorf("a bare repository is labelled %q", w.Label())
	}
}

func TestBrowseURL(t *testing.T) {
	const want = "https://github.com/x-motemen/ghq"
	for _, remote := range []string{
		"https://github.com/x-motemen/ghq",
		"https://github.com/x-motemen/ghq.git",
		"ssh://git@github.com/x-motemen/ghq.git",
		"git@github.com:x-motemen/ghq.git",
	} {
		got, err := BrowseURL(remote)
		if err != nil {
			t.Errorf("BrowseURL(%q) = %v", remote, err)
			continue
		}
		if got != want {
			t.Errorf("BrowseURL(%q) = %q, want %q", remote, got, want)
		}
	}

	// A git URL's port addresses the git service, not the web one.
	if got, _ := BrowseURL("ssh://git@git.example.com:2222/g/p.git"); got != "https://git.example.com/g/p" {
		t.Errorf("BrowseURL() kept the git port: %q", got)
	}
	if _, err := BrowseURL(""); err == nil {
		t.Error("BrowseURL(\"\") should fail")
	}
}

// TestFindReposAndContains covers the walk gm migrate -r relies on: every
// working copy is found once, nothing inside one is descended into, dotted
// directories are left alone, and a missing directory is not an error.
func TestFindReposAndContains(t *testing.T) {
	dir := t.TempDir()
	for _, rel := range []string{
		"projects/alpha",
		"projects/nested/bravo",
		"projects/alpha/vendor/inner", // inside a repository: not reported
		".cache/charlie",              // dotted: not descended into
	} {
		if err := os.MkdirAll(filepath.Join(dir, rel, ".git"), 0o755); err != nil {
			t.Fatal(err)
		}
	}

	found, err := FindRepos(dir)
	if err != nil {
		t.Fatal(err)
	}
	want := map[string]bool{
		filepath.Join(dir, "projects/alpha"):        true,
		filepath.Join(dir, "projects/nested/bravo"): true,
	}
	if len(found) != len(want) {
		t.Fatalf("FindRepos() = %v, want %d entries", found, len(want))
	}
	for _, p := range found {
		if !want[p] {
			t.Errorf("FindRepos() returned %q", p)
		}
	}

	if got, err := FindRepos(filepath.Join(dir, "nope")); err != nil || got != nil {
		t.Errorf("FindRepos(missing) = %v, %v; want nil, nil", got, err)
	}

	tree := &Tree{Roots: []string{filepath.Join(dir, "projects")}}
	for path, want := range map[string]bool{
		filepath.Join(dir, "projects"):       true,
		filepath.Join(dir, "projects/alpha"): true,
		filepath.Join(dir, ".cache/charlie"): false,
		filepath.Join(dir, "projects-other"): false, // a prefix, not a parent
	} {
		if got := tree.Contains(path); got != want {
			t.Errorf("Contains(%q) = %v, want %v", path, got, want)
		}
	}
}

func TestParseCommits(t *testing.T) {
	const nul = "\x00"
	out := strings.Join([]string{
		"b1b7b91" + nul + "HEAD -> feat/migrate-scan, origin/feat/migrate-scan" + nul + "refactor: name the flag -r",
		"06f966e" + nul + "" + nul + "feat: add gm migrate -r",
		"d7f3a4c" + nul + "tag: v1.2.0, origin/main, main, release/main" + nul + "Merge pull request #24",
	}, "\n")

	got := parseCommits(out, []string{"origin"})
	if len(got) != 3 {
		t.Fatalf("parseCommits() = %v, want 3 commits", got)
	}

	if got[0].Hash != "b1b7b91" || got[0].Subject != "refactor: name the flag -r" {
		t.Errorf("first commit = %+v", got[0])
	}
	// "HEAD -> branch" is two decorations, coloured differently.
	want := []Ref{
		{Name: "HEAD", Kind: RefHead},
		{Name: "feat/migrate-scan", Kind: RefLocal},
		{Name: "origin/feat/migrate-scan", Kind: RefRemote},
	}
	if len(got[0].Refs) != len(want) {
		t.Fatalf("first commit refs = %+v, want %+v", got[0].Refs, want)
	}
	for i, w := range want {
		if got[0].Refs[i] != w {
			t.Errorf("ref %d = %+v, want %+v", i, got[0].Refs[i], w)
		}
	}

	if len(got[1].Refs) != 0 {
		t.Errorf("an undecorated commit got refs: %+v", got[1].Refs)
	}

	// A local branch whose name starts with a slash-separated segment is not
	// a remote one: only the configured remotes make it remote.
	kinds := map[string]RefKind{}
	for _, r := range got[2].Refs {
		kinds[r.Name] = r.Kind
	}
	for name, want := range map[string]RefKind{
		"tag: v1.2.0":  RefTag,
		"origin/main":  RefRemote,
		"main":         RefLocal,
		"release/main": RefLocal,
	} {
		if kinds[name] != want {
			t.Errorf("%q classified as %v, want %v", name, kinds[name], want)
		}
	}

	if got := parseCommits("", nil); got != nil {
		t.Errorf("parseCommits(\"\") = %v, want nil", got)
	}
	// A line git could not format is dropped, not turned into a bad row.
	if got := parseCommits("no separators here", nil); len(got) != 0 {
		t.Errorf("parseCommits(junk) = %v", got)
	}
}
