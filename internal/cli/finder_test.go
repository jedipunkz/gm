package cli

import (
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/jedipunkz/gm/internal/finder"
	"github.com/jedipunkz/gm/internal/repo"
)

// gitRepo makes a repository with one commit, to clone from.
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

// TestActJump records the visit and prints the path, which is the whole
// contract between the finder and the shell binding.
func TestActJump(t *testing.T) {
	root := t.TempDir()
	dst := filepath.Join(root, "github.com/acme/alpha")
	if err := os.MkdirAll(dst, 0o755); err != nil {
		t.Fatal(err)
	}
	a := &app{tree: &repo.Tree{Roots: []string{root}}}
	hist := repo.OpenHistory(filepath.Join(t.TempDir(), "frecency.json"))

	out := captureStdoutErr(t, func() error {
		return a.act(finder.Result{Action: finder.ActionJump, Arg: dst}, hist)
	})
	if strings.TrimSpace(out) != dst {
		t.Errorf("printed %q, want %q", out, dst)
	}
	if hist.Visit(dst).Count != 1 {
		t.Error("the visit was not recorded")
	}
}

// TestActNoneDoesNothing: quitting the finder leaves no trace.
func TestActNoneDoesNothing(t *testing.T) {
	a := &app{tree: &repo.Tree{Roots: []string{t.TempDir()}}}
	hist := repo.OpenHistory(filepath.Join(t.TempDir(), "frecency.json"))
	if out := captureStdoutErr(t, func() error { return a.act(finder.Result{}, hist) }); out != "" {
		t.Errorf("quitting printed %q", out)
	}
}

// captureStdoutErr runs f with both streams redirected and returns what it
// wrote to either.
func captureStdoutErr(t *testing.T, f func() error) string {
	t.Helper()
	r, w, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	outOld, errOld := os.Stdout, os.Stderr
	os.Stdout, os.Stderr = w, w

	ferr := f()

	os.Stdout, os.Stderr = outOld, errOld
	_ = w.Close()
	var b strings.Builder
	if _, err := io.Copy(&b, r); err != nil {
		t.Fatal(err)
	}
	if ferr != nil {
		t.Fatalf("act: %v\n%s", ferr, b.String())
	}
	return b.String()
}

// TestActGetKeepsWhatGetRecorded guards a lost write. gm get writes the visit
// log itself, in the middle of act(); the copy the finder was handed was read
// before that. Saving the finder's copy afterwards throws the write away —
// gm get's own record of the repository, and anything else written to the log
// in that window.
func TestActGetKeepsWhatGetRecorded(t *testing.T) {
	root := t.TempDir()
	// repo.Bump always writes the user's own log, so point it somewhere
	// disposable or this test edits real data.
	state := t.TempDir()
	t.Setenv("XDG_STATE_HOME", state)
	histPath := filepath.Join(state, "gm", "frecency.json")

	dst := filepath.Join(root, "example.com/acme/alpha")
	gitRepo(t, dst) // already cloned: gm get reports it without the network
	other := filepath.Join(root, "example.com/acme/other")
	gitRepo(t, other)

	// The finder reads the log when it opens...
	hist := repo.OpenHistory(histPath)
	// ...and something writes to it afterwards, which is what gm get does in
	// the middle of the call below.
	if err := repo.Bump(other); err != nil {
		t.Fatal(err)
	}

	a := &app{tree: &repo.Tree{Roots: []string{root}}}
	out := captureStdoutErr(t, func() error {
		return a.act(finder.Result{Action: finder.ActionGet, Arg: "example.com/acme/alpha"}, hist)
	})
	if !strings.Contains(out, dst) {
		t.Errorf("the path was not printed: %q", out)
	}

	after := repo.OpenHistory(histPath)
	if got := after.Visit(dst).Count; got != 1 {
		t.Errorf("the repository gm get reported counted %d times, want 1", got)
	}
	if got := after.Visit(other).Count; got != 1 {
		t.Errorf("a visit written while the finder was open was discarded (count %d)", got)
	}
}

// TestActJumpCountsOnce: a row that was simply chosen is the finder's to
// record, and it records one.
func TestActJumpCountsOnce(t *testing.T) {
	root := t.TempDir()
	dst := filepath.Join(root, "github.com/acme/alpha")
	if err := os.MkdirAll(dst, 0o755); err != nil {
		t.Fatal(err)
	}
	a := &app{tree: &repo.Tree{Roots: []string{root}}}
	hist := repo.OpenHistory(filepath.Join(t.TempDir(), "frecency.json"))

	captureStdoutErr(t, func() error {
		return a.act(finder.Result{Action: finder.ActionJump, Arg: dst}, hist)
	})
	if got := hist.Visit(dst).Count; got != 1 {
		t.Errorf("one jump counted as %d visits, want 1", got)
	}
}
