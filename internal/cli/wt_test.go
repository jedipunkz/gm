package cli

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/jedipunkz/gm/internal/repo"
)

// TestWtCreateAndRemove is the round trip a script does: make a worktree,
// read where it went off stdout, then take it away again.
func TestWtCreateAndRemove(t *testing.T) {
	// The visit log is a real file under the user's state directory, and a
	// test has no business writing to it.
	t.Setenv("XDG_STATE_HOME", t.TempDir())
	root := t.TempDir()
	r := repo.Repo{Root: root, Rel: "github.com/acme/alpha"}
	gitRepo(t, r.Path())
	a := &app{tree: &repo.Tree{Roots: []string{root}}}

	out := captureStdout(t, func() {
		if err := a.wt([]string{"create", "acme/alpha", "feat/login"}); err != nil {
			t.Fatal(err)
		}
	})
	dir := strings.TrimSpace(out)
	want := a.tree.WorktreeDir(r, "feat/login")
	if dir != want {
		t.Fatalf("printed %q, want %q", dir, want)
	}
	if _, err := os.Stat(filepath.Join(dir, ".git")); err != nil {
		t.Fatalf("no worktree at %s: %v", dir, err)
	}

	// The same branch twice is a mistake worth stopping at, not a silent
	// no-op: the second call would otherwise look like it worked.
	if err := a.wt([]string{"create", "acme/alpha", "feat/login"}); err == nil {
		t.Error("creating the same worktree twice was allowed")
	}

	if err := a.wt([]string{"remove", "-y", "acme/alpha", "feat/login"}); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(dir); !os.IsNotExist(err) {
		t.Errorf("%s survived: %v", dir, err)
	}
}

// TestWtRemoveRefusesWhatIsNotAWorktree covers the main worktree too: it is
// the repository itself, and it is not in the list this checks against.
func TestWtRemoveRefusesWhatIsNotAWorktree(t *testing.T) {
	root := t.TempDir()
	r := repo.Repo{Root: root, Rel: "github.com/acme/alpha"}
	gitRepo(t, r.Path())
	a := &app{tree: &repo.Tree{Roots: []string{root}}}

	err := a.wt([]string{"remove", "-y", "acme/alpha", "main"})
	if err == nil {
		t.Fatal("removed a worktree that does not exist")
	}
	if !strings.Contains(err.Error(), "no worktree") {
		t.Errorf("unhelpful error: %v", err)
	}
	if _, statErr := os.Stat(r.Path()); statErr != nil {
		t.Errorf("the repository itself was touched: %v", statErr)
	}
}

// TestWtUsage: a missing or unknown verb says what the verbs are rather than
// doing one of them.
func TestWtUsage(t *testing.T) {
	a := &app{tree: &repo.Tree{Roots: []string{t.TempDir()}}}
	for _, args := range [][]string{nil, {"add"}, {"create", "only-one-arg"}} {
		if err := a.wt(args); err == nil {
			t.Errorf("gm wt %v was accepted", args)
		}
	}
}
