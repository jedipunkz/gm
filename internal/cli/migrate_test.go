package cli

import (
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/jedipunkz/gm/internal/repo"
)

// initRepo makes a real working copy: the decisions come from git, so a fake
// directory would not exercise them.
func initRepo(t *testing.T, dir, origin string) string {
	t.Helper()
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	run := func(args ...string) {
		t.Helper()
		cmd := exec.Command("git", args...)
		cmd.Dir = dir
		if out, err := cmd.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v\n%s", args, err, out)
		}
	}
	run("init", "-q")
	if origin != "" {
		run("remote", "add", "origin", origin)
	}
	return dir
}

// TestMigrateRecursive covers what -r is for: find the working copies under a
// directory, leave the ones already in the tree alone, and report the ones
// that cannot move instead of abandoning the run.
func TestMigrateRecursive(t *testing.T) {
	base := t.TempDir()
	root := filepath.Join(base, "tree")
	if err := os.MkdirAll(root, 0o755); err != nil {
		t.Fatal(err)
	}

	good := initRepo(t, filepath.Join(base, "src/good"), "https://github.com/acme/good")
	also := initRepo(t, filepath.Join(base, "src/deeper/also"), "git@github.com:acme/also.git")
	noRemote := initRepo(t, filepath.Join(base, "src/noremote"), "")
	initRepo(t, filepath.Join(root, "github.com/acme/already"), "https://github.com/acme/already")

	// A .git file is a worktree or a submodule, and moving it breaks the link.
	linked := filepath.Join(base, "src/linked")
	if err := os.MkdirAll(linked, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(linked, ".git"), []byte("gitdir: /elsewhere\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	a := &app{tree: &repo.Tree{Roots: []string{root}}}
	out, err := captureStderr(t, func() error { return a.migrate([]string{"-r", "--dry-run", base}) })
	if err != nil {
		t.Fatalf("migrate -r: %v\n%s", err, out)
	}

	for _, want := range []string{
		good + " -> " + filepath.Join(root, "github.com/acme/good"),
		also + " -> " + filepath.Join(root, "github.com/acme/also"),
		noRemote + ": has no origin remote",
		linked + ": is a worktree or submodule",
	} {
		if !strings.Contains(out, want) {
			t.Errorf("the output is missing %q:\n%s", want, out)
		}
	}
	// A repository already under the root is not a candidate at all.
	if strings.Contains(out, "already") {
		t.Errorf("the search reported a repository already in the tree:\n%s", out)
	}
	// --dry-run moves nothing.
	if _, err := os.Stat(good); err != nil {
		t.Errorf("--dry-run moved %s: %v", good, err)
	}
}

// TestMigrateNamedDirectoryStillErrors keeps the single-directory behaviour:
// naming a directory is a claim that it should move, so a problem with it is
// fatal rather than a skip.
func TestMigrateNamedDirectoryStillErrors(t *testing.T) {
	base := t.TempDir()
	root := filepath.Join(base, "tree")
	noRemote := initRepo(t, filepath.Join(base, "src/noremote"), "")

	a := &app{tree: &repo.Tree{Roots: []string{root}}}
	_, err := captureStderr(t, func() error { return a.migrate([]string{"--dry-run", noRemote}) })
	if err == nil {
		t.Fatal("migrate on a repository with no origin should fail")
	}
	if !strings.Contains(err.Error(), "has no origin remote") {
		t.Errorf("migrate failed with %v", err)
	}
}

// captureStderr runs f with stderr redirected, returning what it wrote and
// whatever f returned. gm reports its progress on stderr, so that text is the
// observable behaviour here.
func captureStderr(t *testing.T, f func() error) (string, error) {
	t.Helper()
	r, w, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	old := os.Stderr
	os.Stderr = w

	ferr := f()

	os.Stderr = old
	w.Close()
	var b strings.Builder
	if _, err := io.Copy(&b, r); err != nil {
		t.Fatal(err)
	}
	return b.String(), ferr
}
