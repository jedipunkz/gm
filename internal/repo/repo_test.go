package repo

import (
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"slices"
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

// TestHistoryBumpKeepsConcurrentVisits covers the window the finder holds open:
// a visit written by another gm after this History was opened must survive this
// History's own bump.
func TestHistoryBumpKeepsConcurrentVisits(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "frecency.json")
	a, b := filepath.Join(dir, "a"), filepath.Join(dir, "b")
	for _, p := range []string{a, b} {
		if err := os.MkdirAll(p, 0o755); err != nil {
			t.Fatal(err)
		}
	}

	h := OpenHistory(path) // the finder opens the log and holds it
	if err := OpenHistory(path).Bump(b); err != nil {
		t.Fatal(err)
	}
	if err := h.Bump(a); err != nil {
		t.Fatal(err)
	}

	again := OpenHistory(path)
	if got := again.Visit(a).Count; got != 1 {
		t.Errorf("visit count for a = %d, want 1", got)
	}
	if got := again.Visit(b).Count; got != 1 {
		t.Errorf("visit written while the finder was open is lost (count %d, want 1)", got)
	}
	if got := h.Visit(b).Count; got != 1 {
		t.Errorf("in-memory log disagrees with the file (count %d, want 1)", got)
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

func TestDirtyMap(t *testing.T) {
	dir := t.TempDir()
	git := func(wd string, args ...string) {
		t.Helper()
		cmd := exec.Command("git", args...)
		cmd.Dir = wd
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v\n%s", args, err, out)
		}
	}
	mk := func(name string) string {
		p := filepath.Join(dir, name)
		if err := os.MkdirAll(p, 0o755); err != nil {
			t.Fatal(err)
		}
		git(p, "init", "-q")
		return p
	}

	clean := mk("clean")
	dirty := mk("dirty")
	if err := os.WriteFile(filepath.Join(dirty, "scratch.txt"), []byte("wip\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	// Not a repository at all; git cannot answer, which counts as clean.
	plain := filepath.Join(dir, "plain")
	if err := os.MkdirAll(plain, 0o755); err != nil {
		t.Fatal(err)
	}

	got := DirtyMap([]string{clean, dirty, plain})
	if len(got) != 3 {
		t.Fatalf("DirtyMap() returned %d entries, want 3: %v", len(got), got)
	}
	if got[dirty] != true {
		t.Errorf("a repository with an untracked file is not reported dirty")
	}
	if got[clean] != false || got[plain] != false {
		t.Errorf("clean=%v plain=%v, want both false", got[clean], got[plain])
	}

	if got := DirtyMap(nil); len(got) != 0 {
		t.Errorf("DirtyMap(nil) = %v", got)
	}
}

func TestTreeAt(t *testing.T) {
	root := t.TempDir()
	tree := &Tree{Roots: []string{root}}

	r, ok := tree.At(filepath.Join(root, "github.com/acme/alpha"))
	if !ok || r.Rel != "github.com/acme/alpha" || r.Root != root {
		t.Errorf("At() = %+v, %v", r, ok)
	}
	for _, outside := range []string{
		filepath.Join(filepath.Dir(root), "elsewhere"),
		root, // the root itself is not a repository in it
	} {
		if r, ok := tree.At(outside); ok {
			t.Errorf("At(%q) = %+v, want no match", outside, r)
		}
	}
}

// gitRepo makes a repository with one commit, which is what git wants before
// it hands out a worktree.
func gitRepo(t *testing.T, dir string) {
	t.Helper()
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	for _, args := range [][]string{
		{"init", "-q", "-b", "main"},
		{"-c", "user.email=t@e.x", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "init"},
	} {
		cmd := exec.Command("git", args...)
		cmd.Dir = dir
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v\n%s", args, err, out)
		}
	}
}

// captureStderr runs f with stderr redirected and returns what reached it.
func captureStderr(t *testing.T, f func()) string {
	t.Helper()
	r, w, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	old := os.Stderr
	os.Stderr = w
	f()
	os.Stderr = old
	_ = w.Close()
	var b strings.Builder
	if _, err := io.Copy(&b, r); err != nil {
		t.Fatal(err)
	}
	return b.String()
}

// TestGitOutputStaysOffTheScreen guards the finder's display: these run while
// the TUI owns the terminal, so git must print nothing and put what it had to
// say in the error instead.
func TestGitOutputStaysOffTheScreen(t *testing.T) {
	base := t.TempDir()
	main := filepath.Join(base, "github.com/acme/alpha")
	gitRepo(t, main)
	tree := &Tree{Roots: []string{base}}
	r := Repo{Root: base, Rel: "github.com/acme/alpha"}
	dir := tree.WorktreeDir(r, "feat/login")

	// git says "Preparing worktree (new branch 'feat/login')" on success.
	if out := captureStderr(t, func() {
		if err := AddWorktree(main, dir, "feat/login"); err != nil {
			t.Error(err)
		}
	}); out != "" {
		t.Errorf("AddWorktree printed %q", out)
	}

	// And a page of hints on failure.
	var err error
	if out := captureStderr(t, func() { err = AddWorktree(main, dir, "feat/login") }); out != "" {
		t.Errorf("a failing AddWorktree printed %q", out)
	}
	if err == nil {
		t.Fatal("adding the same worktree twice should fail")
	}
	if !strings.Contains(err.Error(), "already exists") {
		t.Errorf("the error lost git's explanation: %v", err)
	}

	if out := captureStderr(t, func() {
		if _, err := tree.Create("acme/bravo", false); err != nil {
			t.Error(err)
		}
	}); out != "" {
		t.Errorf("Create printed %q", out)
	}

	if out := captureStderr(t, func() {
		if err := RemoveWorktree(main, dir, false); err != nil {
			t.Error(err)
		}
	}); out != "" {
		t.Errorf("RemoveWorktree printed %q", out)
	}
}

// TestValidBranch keeps a name that is not a branch name out of the path a
// worktree is about to be made at.
func TestValidBranch(t *testing.T) {
	for _, ok := range []string{"main", "feat/login", "release-1.2"} {
		if !ValidBranch(ok) {
			t.Errorf("ValidBranch(%q) = false", ok)
		}
	}
	for _, bad := range []string{"", "-force", "../../escaped", "a..b", "feat login", "~x", "x:y"} {
		if ValidBranch(bad) {
			t.Errorf("ValidBranch(%q) = true", bad)
		}
	}

	// And nothing is created on the way to finding out.
	base := t.TempDir()
	main := filepath.Join(base, "github.com/acme/alpha")
	gitRepo(t, main)
	tree := &Tree{Roots: []string{base}}
	r := Repo{Root: base, Rel: "github.com/acme/alpha"}

	if err := AddWorktree(main, tree.WorktreeDir(r, "../../escaped"), "../../escaped"); err == nil {
		t.Fatal("a bogus branch name should be refused")
	}
	if _, err := os.Stat(filepath.Join(base, WorktreeRoot)); !os.IsNotExist(err) {
		t.Errorf("it made directories anyway: %v", err)
	}
}

// TestDeleteTakesTheWorktrees guards the bug where a removed repository left
// its checkouts behind, pointing at a .git that was gone.
func TestDeleteTakesTheWorktrees(t *testing.T) {
	base := t.TempDir()
	r := Repo{Root: base, Rel: "github.com/acme/alpha"}
	gitRepo(t, r.Path())
	tree := &Tree{Roots: []string{base}}

	login := tree.WorktreeDir(r, "feat/login")
	timeout := tree.WorktreeDir(r, "fix/timeout")
	for branch, dir := range map[string]string{"feat/login": login, "fix/timeout": timeout} {
		if err := AddWorktree(r.Path(), dir, branch); err != nil {
			t.Fatal(err)
		}
	}
	// One of them has work in it: the repository is going regardless, so this
	// must not stop the removal half way.
	if err := os.WriteFile(filepath.Join(login, "scratch.txt"), []byte("wip\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	if got := OtherWorktrees(r); len(got) != 2 {
		t.Fatalf("OtherWorktrees() = %v, want the two that are not the repository", got)
	}

	if err := Delete(r); err != nil {
		t.Fatal(err)
	}
	for _, dir := range []string{r.Path(), login, timeout} {
		if _, err := os.Stat(dir); !os.IsNotExist(err) {
			t.Errorf("%s survived: %v", dir, err)
		}
	}
	// And nothing empty is left standing over them.
	if _, err := os.Stat(filepath.Join(base, WorktreeRoot)); !os.IsNotExist(err) {
		t.Errorf("%s survived: %v", WorktreeRoot, err)
	}
	if _, err := os.Stat(filepath.Join(base, "github.com")); !os.IsNotExist(err) {
		t.Errorf("github.com survived: %v", err)
	}
}

// TestDeleteWithAWorktreeGitLostTrackOf: a checkout git can no longer remove
// must not block the removal, or the repository can never be deleted.
func TestDeleteWithAWorktreeGitLostTrackOf(t *testing.T) {
	base := t.TempDir()
	r := Repo{Root: base, Rel: "github.com/acme/alpha"}
	gitRepo(t, r.Path())
	tree := &Tree{Roots: []string{base}}

	dir := tree.WorktreeDir(r, "feat/login")
	if err := AddWorktree(r.Path(), dir, "feat/login"); err != nil {
		t.Fatal(err)
	}
	// Break git's link to it, the way deleting .git by hand would.
	if err := os.Remove(filepath.Join(dir, ".git")); err != nil {
		t.Fatal(err)
	}

	if err := Delete(r); err != nil {
		t.Fatalf("Delete() = %v, want it to get through anyway", err)
	}
	for _, p := range []string{r.Path(), dir} {
		if _, err := os.Stat(p); !os.IsNotExist(err) {
			t.Errorf("%s survived: %v", p, err)
		}
	}
}

// TestDeleteWithoutWorktrees is the ordinary case, unchanged.
func TestDeleteWithoutWorktrees(t *testing.T) {
	base := t.TempDir()
	r := Repo{Root: base, Rel: "github.com/acme/alpha"}
	gitRepo(t, r.Path())

	if got := OtherWorktrees(r); len(got) != 0 {
		t.Errorf("OtherWorktrees() = %v, want none", got)
	}
	if err := Delete(r); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(r.Path()); !os.IsNotExist(err) {
		t.Errorf("the repository survived: %v", err)
	}
}

// TestRemotesReadsNamesAndOriginInOneCall pins the parse of `git remote -v`,
// which answers what used to take two git processes.
func TestRemotesReadsNamesAndOriginInOneCall(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "repo")
	gitRepo(t, dir)
	for _, args := range [][]string{
		{"remote", "add", "upstream", "https://example.com/upstream.git"},
		{"remote", "add", "origin", "git@github.com:acme/alpha.git"},
	} {
		cmd := exec.Command("git", args...)
		cmd.Dir = dir
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v\n%s", args, err, out)
		}
	}

	names, origin := remotes(dir)
	if origin != "git@github.com:acme/alpha.git" {
		t.Errorf("origin = %q", origin)
	}
	// Each remote is listed twice, for fetch and for push, and has to be
	// counted once: the names tell a remote ref from a local branch.
	want := []string{"origin", "upstream"}
	if !reflect.DeepEqual(names, want) {
		t.Errorf("names = %v, want %v", names, want)
	}

	// A repository with no remotes is normal, not an error.
	bare := filepath.Join(t.TempDir(), "bare")
	gitRepo(t, bare)
	if names, origin := remotes(bare); names != nil || origin != "" {
		t.Errorf("no remotes: got %v %q", names, origin)
	}
}

// TestDescribeReportsTheOtherWorktrees covers the field the details pane
// draws. The main worktree is the repository itself and must not be in it,
// which on macOS means comparing /private/var with /var.
func TestDescribeReportsTheOtherWorktrees(t *testing.T) {
	base := t.TempDir()
	r := Repo{Root: base, Rel: "github.com/acme/alpha"}
	gitRepo(t, r.Path())

	if got := Describe(r.Path()).Worktrees; len(got) != 0 {
		t.Errorf("a repository with only its main worktree reported %v", got)
	}

	tree := &Tree{Roots: []string{base}}
	if err := AddWorktree(r.Path(), tree.WorktreeDir(r, "feat/login"), "feat/login"); err != nil {
		t.Fatal(err)
	}
	got := Describe(r.Path()).Worktrees
	if len(got) != 1 || got[0].Label() != "feat/login" {
		t.Fatalf("Worktrees = %v, want just feat/login", got)
	}
	// The date is what ranks the checkouts in the details pane, so an
	// undated worktree would silently sort to the bottom.
	if got[0].CommittedAt <= 0 {
		t.Errorf("worktree %q has no commit date", got[0].Label())
	}
}

// TestParseStatus covers every shape of the `## ` header git writes, which is
// the whole of what gm status knows about a repository.
func TestParseStatus(t *testing.T) {
	cases := []struct {
		name string
		out  string
		want State
	}{
		{"tracking and drifted", "## main...origin/main [ahead 1, behind 2]",
			State{Branch: "main", Upstream: true, Ahead: 1, Behind: 2}},
		{"ahead only", "## main...origin/main [ahead 3]",
			State{Branch: "main", Upstream: true, Ahead: 3}},
		{"in sync", "## main...origin/main", State{Branch: "main", Upstream: true}},
		{"no upstream", "## main", State{Branch: "main"}},
		{"detached", "## HEAD (no branch)", State{Branch: "HEAD"}},
		{"empty repository", "## No commits yet on main", State{Branch: "main"}},
		// The upstream branch was deleted: there is no count to read, and the
		// branch still tracks something as far as the config is concerned.
		{"gone upstream", "## main...origin/main [gone]", State{Branch: "main", Upstream: true}},
		{"changes counted", "## main...origin/main\n M a.go\n?? b.go\nD  c.go",
			State{Branch: "main", Upstream: true, Dirty: 3}},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			if got := parseStatus(c.out); got != c.want {
				t.Errorf("parseStatus(%q) = %+v, want %+v", c.out, got, c.want)
			}
		})
	}

	// Being behind is the remote's news, not work anyone left behind.
	if (State{Behind: 4, Upstream: true}).Unfinished() {
		t.Error("behind alone counted as unfinished work")
	}
	if !(State{Ahead: 1, Upstream: true}).Unfinished() {
		t.Error("an unpushed commit did not count as unfinished work")
	}
}

// TestParseBranches keeps one row per branch: a local branch hides its remote
// namesake, and a remote's HEAD is not a branch at all.
func TestParseBranches(t *testing.T) {
	out := strings.Join([]string{
		"refs/heads/main",
		"refs/remotes/origin/HEAD",
		"refs/remotes/origin/main",
		"refs/remotes/origin/feat/login",
		"refs/heads/release/main",
		"refs/remotes/upstream/feat/login",
		"refs/remotes/upstream/fix",
	}, "\n")
	got := parseBranches(out, []string{"origin", "upstream"})
	want := []Branch{
		{Name: "main"},
		{Name: "feat/login", Remote: "origin/feat/login"},
		{Name: "release/main"},
		{Name: "fix", Remote: "upstream/fix"},
	}
	if !slices.Equal(got, want) {
		t.Errorf("parseBranches = %+v, want %+v", got, want)
	}
}

// TestAddWorktreeFromARemoteBranch checks a branch only a remote has out as a
// local branch that tracks it.
func TestAddWorktreeFromARemoteBranch(t *testing.T) {
	upstream := filepath.Join(t.TempDir(), "upstream")
	gitRepo(t, upstream)
	git := func(dir string, args ...string) string {
		t.Helper()
		out, err := GitIn(dir, args...)
		if err != nil {
			t.Fatalf("git %v: %v", args, err)
		}
		return out
	}
	git(upstream, "branch", "feat/login")

	clone := filepath.Join(t.TempDir(), "clone")
	if err := gitQuiet("clone", "-q", upstream, clone); err != nil {
		t.Fatal(err)
	}
	bs, err := Branches(clone)
	if err != nil {
		t.Fatal(err)
	}
	if !slices.Contains(bs, Branch{Name: "feat/login", Remote: "origin/feat/login"}) {
		t.Fatalf("Branches = %+v, want origin/feat/login among them", bs)
	}

	dir := filepath.Join(t.TempDir(), "wt")
	if err := AddWorktreeFrom(clone, dir, "feat/login", "origin/feat/login"); err != nil {
		t.Fatal(err)
	}
	if got := git(dir, "rev-parse", "--abbrev-ref", "feat/login@{upstream}"); got != "origin/feat/login" {
		t.Errorf("the new branch tracks %q", got)
	}
}

// TestParsePullRequests reads gh's JSON, and files a fork's branch under its
// owner so a fork's main does not land on the repository's own.
func TestParsePullRequests(t *testing.T) {
	out := []byte(`[
		{"number":7,"title":"Add login","headRefName":"feat/login","isDraft":false,"isCrossRepository":false,
		 "author":{"login":"alice"},"headRepositoryOwner":{"login":"acme"}},
		{"number":9,"title":"Fix typo","headRefName":"main","isDraft":true,"isCrossRepository":true,
		 "author":{"login":"bob"},"headRepositoryOwner":{"login":"bob"}}
	]`)
	prs, err := parsePullRequests(out)
	if err != nil {
		t.Fatal(err)
	}
	if len(prs) != 2 {
		t.Fatalf("parsed %d pull requests", len(prs))
	}
	for _, c := range []struct {
		pr              PullRequest
		label, checkout string
	}{
		{prs[0], "#7 Add login", "feat/login"},
		{prs[1], "#9 [draft] Fix typo", "bob/main"},
	} {
		if c.pr.Label() != c.label || c.pr.Checkout() != c.checkout {
			t.Errorf("#%d reads %q, filed under %q; want %q, %q", c.pr.Number, c.pr.Label(), c.pr.Checkout(), c.label, c.checkout)
		}
	}
	if prs[1].Author.Login != "bob" {
		t.Errorf("author = %q", prs[1].Author.Login)
	}
}

// TestCheckOutPullRequest runs a stand-in gh that records its arguments and
// makes the worktree the way the real one would.
func TestCheckOutPullRequest(t *testing.T) {
	main := filepath.Join(t.TempDir(), "main")
	gitRepo(t, main)
	bin := t.TempDir()
	script := "#!/bin/sh\necho \"$@\" > \"$0.args\"\nexec git worktree add -b feat/login \"$5\"\n"
	if err := os.WriteFile(filepath.Join(bin, "gh"), []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", bin+string(os.PathListSeparator)+os.Getenv("PATH"))

	dir := filepath.Join(t.TempDir(), "wt", "feat", "login")
	if err := CheckOutPullRequest(main, dir, 7); err != nil {
		t.Fatal(err)
	}
	args, err := os.ReadFile(filepath.Join(bin, "gh.args"))
	if err != nil {
		t.Fatal(err)
	}
	if got := strings.TrimSpace(string(args)); got != "pr checkout 7 --worktree "+dir {
		t.Errorf("gh ran with %q", got)
	}
	if b, _ := GitIn(dir, "branch", "--show-current"); b != "feat/login" {
		t.Errorf("the worktree is on %q", b)
	}

	// A gh that fails says why, in its own words.
	fail := "#!/bin/sh\necho 'could not find pull request' >&2\nexit 1\n"
	if err := os.WriteFile(filepath.Join(bin, "gh"), []byte(fail), 0o755); err != nil {
		t.Fatal(err)
	}
	err = CheckOutPullRequest(main, filepath.Join(t.TempDir(), "x"), 8)
	if err == nil || !strings.Contains(err.Error(), "could not find pull request") {
		t.Errorf("err = %v, want gh's message", err)
	}
}

// TestRemoteBranches finds what a remote has that the last fetch did not
// bring, and fetches one of them into a worktree that tracks it.
func TestRemoteBranches(t *testing.T) {
	upstream := filepath.Join(t.TempDir(), "upstream")
	gitRepo(t, upstream)
	git := func(dir string, args ...string) string {
		t.Helper()
		out, err := GitIn(dir, args...)
		if err != nil {
			t.Fatalf("git %v: %v", args, err)
		}
		return out
	}
	git(upstream, "branch", "old")
	clone := filepath.Join(t.TempDir(), "clone")
	if err := gitQuiet("clone", "-q", upstream, clone); err != nil {
		t.Fatal(err)
	}
	// Pushed after the clone: only the remote knows about it.
	git(upstream, "branch", "feat/new")

	bs, err := RemoteBranches(clone)
	if err != nil {
		t.Fatal(err)
	}
	want := []Branch{{Name: "feat/new", Remote: "origin/feat/new", Unfetched: true}}
	if !slices.Equal(bs, want) {
		t.Fatalf("RemoteBranches = %+v, want %+v", bs, want)
	}

	if err := FetchBranch(clone, bs[0]); err != nil {
		t.Fatal(err)
	}
	dir := filepath.Join(t.TempDir(), "wt")
	if err := AddWorktreeFrom(clone, dir, "feat/new", "origin/feat/new"); err != nil {
		t.Fatal(err)
	}
	if got := git(dir, "rev-parse", "--abbrev-ref", "feat/new@{upstream}"); got != "origin/feat/new" {
		t.Errorf("the new branch tracks %q", got)
	}

	// A refspec is never built out of a name git would not take.
	if err := FetchBranch(clone, Branch{Name: "a:b", Remote: "origin/a:b"}); err == nil {
		t.Error("FetchBranch accepted a name with a colon in it")
	}
}

// TestRemoteBranchesUnreachable reports the remote it could not reach and
// asks for nothing on the way.
func TestRemoteBranchesUnreachable(t *testing.T) {
	clone := filepath.Join(t.TempDir(), "clone")
	gitRepo(t, clone)
	if _, err := GitIn(clone, "remote", "add", "origin", filepath.Join(t.TempDir(), "gone")); err != nil {
		t.Fatal(err)
	}
	bs, err := RemoteBranches(clone)
	if err == nil || !strings.HasPrefix(err.Error(), "origin: ") || len(bs) != 0 {
		t.Errorf("RemoteBranches = %+v, %v; want an error naming origin", bs, err)
	}
}

func TestParseLsRemote(t *testing.T) {
	out := "abc\trefs/heads/main\nabd\trefs/heads/feat/x\nabe\trefs/tags/v1\ngarbage\n"
	got := parseLsRemote(out, "up")
	want := []Branch{
		{Name: "main", Remote: "up/main", Unfetched: true},
		{Name: "feat/x", Remote: "up/feat/x", Unfetched: true},
	}
	if !slices.Equal(got, want) {
		t.Errorf("parseLsRemote = %+v, want %+v", got, want)
	}
}
