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
