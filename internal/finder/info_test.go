package finder

import (
	"strings"
	"testing"
	"time"

	"github.com/jedipunkz/gm/internal/repo"
)

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

// TestPaneLabelsStandOut pins the field names as the quiet half of the
// details pane: the comment colour in bold, against values that keep the
// theme's own colours. A label painted like a value reads as one.
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
	if !strings.Contains(label, ansi("38", th.Comment)) {
		t.Errorf("the label is not painted in the comment colour: %q", label)
	}
	// lipgloss folds bold into the same escape as the colour, as "1;".
	if !strings.Contains(label, "\x1b[1;") {
		t.Errorf("the label is not bold: %q", label)
	}
	// The values are what carry colour; a label wearing one would compete
	// with them instead of naming them.
	for _, hex := range []string{th.Fg, th.Green, th.Cyan, th.Magenta} {
		if strings.Contains(label, ansi("38", hex)) {
			t.Errorf("the label carries the value colour %s: %q", hex, label)
		}
	}
}

// TestWorktreesInTheDetailsPane is how a row says whether the worktree key is
// worth pressing on it. A repository without any says nothing at all: the
// pane is for what is there.
func TestWorktreesInTheDetailsPane(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}
	m := newTestModel(t, repos, "")
	path := repos[0].Path()

	pane := func() string {
		var plain []string
		for _, l := range m.infoLines(40) {
			plain = append(plain, strings.TrimRight(stripANSI(l), " "))
		}
		return strings.Join(plain, "\n")
	}

	m.status[path] = repo.Status{Branch: "main"}
	if strings.Contains(pane(), "worktrees") {
		t.Errorf("a repository with no worktrees mentioned them:\n%s", pane())
	}

	now := time.Now()
	wt := func(name string, d time.Duration) repo.Worktree {
		return repo.Worktree{Path: root + "/.worktrees/" + name, Branch: name, CommittedAt: now.Add(-d).Unix()}
	}
	m.status[path] = repo.Status{
		Branch:    "main",
		Worktrees: []repo.Worktree{wt("feat/login", time.Hour), {Path: root + "/.worktrees/b", Head: "abc1234def"}},
	}
	got := pane()
	if !strings.Contains(got, "\nlast 2 worktrees\n") {
		t.Errorf("no worktrees label:\n%s", got)
	}
	// A detached checkout has no branch to name it, so it goes by its hash.
	if !strings.Contains(got, "feat/login") || !strings.Contains(got, "abc1234") {
		t.Errorf("a worktree is missing from the pane:\n%s", got)
	}
	if !strings.Contains(got, "1h ago") {
		t.Errorf("no commit date beside the worktree:\n%s", got)
	}

	// Newest first, and only three: a repository with many checkouts must not
	// push the commits below it off the pane.
	m.status[path] = repo.Status{
		Branch: "main",
		Worktrees: []repo.Worktree{
			wt("stale", 30*24*time.Hour),
			wt("newest", time.Minute),
			wt("mid", 2*24*time.Hour),
			wt("second", time.Hour),
		},
	}
	got = pane()
	if !strings.Contains(got, "\nlast 3 of 4 worktrees\n") {
		t.Errorf("the label does not say how many were left out:\n%s", got)
	}
	if strings.Contains(got, "stale") {
		t.Errorf("the oldest worktree was not dropped:\n%s", got)
	}
	if i, j := strings.Index(got, "newest"), strings.Index(got, "second"); i < 0 || j < 0 || i > j {
		t.Errorf("the worktrees are not newest first:\n%s", got)
	}
}
