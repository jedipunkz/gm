package cli

import (
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/jedipunkz/gm/internal/finder"
	"github.com/jedipunkz/gm/internal/repo"
)

// TestActCreate runs what the finder's /create asks for: a repository where
// the reference says it belongs, with its origin already set, and its path on
// stdout so the shell binding lands in it.
func TestActCreate(t *testing.T) {
	root := t.TempDir()
	a := &app{tree: &repo.Tree{Roots: []string{root}}}
	hist := repo.OpenHistory(filepath.Join(t.TempDir(), "frecency.json"))

	out := captureStdoutErr(t, func() error {
		return a.act(finder.Result{Action: finder.ActionCreate, Arg: "acme/alpha"}, hist)
	})

	dst := filepath.Join(root, "github.com/acme/alpha")
	if !repo.IsRepo(dst) {
		t.Fatalf("no repository at %s", dst)
	}
	if got, _ := repo.GitIn(dst, "remote", "get-url", "origin"); got != "https://github.com/acme/alpha" {
		t.Errorf("origin is %q", got)
	}
	if !strings.Contains(out, dst) {
		t.Errorf("the new path was not printed: %q", out)
	}
	if hist.Visit(dst).Count != 1 {
		t.Error("the visit was not recorded")
	}
}

// TestActRemoveNeedsConfirmation is the safety net: with nothing on stdin the
// prompt answers no, and the repository survives.
func TestActRemoveNeedsConfirmation(t *testing.T) {
	root := t.TempDir()
	dst := filepath.Join(root, "github.com/acme/alpha")
	if err := os.MkdirAll(filepath.Join(dst, ".git"), 0o755); err != nil {
		t.Fatal(err)
	}
	a := &app{tree: &repo.Tree{Roots: []string{root}}}
	hist := repo.OpenHistory(filepath.Join(t.TempDir(), "frecency.json"))

	out := captureStdoutErr(t, func() error {
		return a.act(finder.Result{Action: finder.ActionRemove, Arg: dst}, hist)
	})
	if !repo.IsRepo(dst) {
		t.Fatal("the repository was removed without a confirmation")
	}
	if !strings.Contains(out, "skipped") {
		t.Errorf("the refusal was not reported: %q", out)
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
