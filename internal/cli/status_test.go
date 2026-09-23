package cli

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/jedipunkz/gm/internal/repo"
)

// git runs one git command in dir and fails the test if it does not work.
func git(t *testing.T, dir string, args ...string) {
	t.Helper()
	cmd := exec.Command("git", append([]string{"-c", "user.email=t@e.x", "-c", "user.name=t"}, args...)...)
	cmd.Dir = dir
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("git %v: %v\n%s", args, err, out)
	}
}

// TestStatusFindsUnfinishedWork covers each reason a repository is listed and
// the one reason it is not.
func TestStatusFindsUnfinishedWork(t *testing.T) {
	root := t.TempDir()
	a := &app{tree: &repo.Tree{Roots: []string{root}}}

	// clean: one commit, nothing changed, and an upstream it matches.
	origin := filepath.Join(t.TempDir(), "origin.git")
	git(t, t.TempDir(), "init", "-q", "--bare", "-b", "main", origin)
	clean := repo.Repo{Root: root, Rel: "github.com/acme/clean"}
	if err := os.MkdirAll(filepath.Dir(clean.Path()), 0o755); err != nil {
		t.Fatal(err)
	}
	git(t, filepath.Dir(clean.Path()), "clone", "-q", origin, clean.Path())
	git(t, clean.Path(), "commit", "-q", "--allow-empty", "-m", "one")
	git(t, clean.Path(), "push", "-q", "-u", "origin", "main")

	// ahead: the same, with a commit that was never pushed.
	ahead := repo.Repo{Root: root, Rel: "github.com/acme/ahead"}
	git(t, filepath.Dir(clean.Path()), "clone", "-q", origin, ahead.Path())
	git(t, ahead.Path(), "commit", "-q", "--allow-empty", "-m", "two")

	// dirty: a repository with no remote at all and an uncommitted file.
	dirty := repo.Repo{Root: root, Rel: "github.com/acme/dirty"}
	gitRepo(t, dirty.Path())
	if err := os.WriteFile(filepath.Join(dirty.Path(), "wip.txt"), []byte("x\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	// A worktree holds work in progress by definition, and lives under a
	// dotted directory the repository walk never descends into.
	wt := a.tree.WorktreeDir(dirty, "feat/login")
	if err := repo.AddWorktree(dirty.Path(), wt, "feat/login"); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(wt, "scratch.txt"), []byte("y\n"), 0o644); err != nil {
		t.Fatal(err)
	}

	out := captureStdout(t, func() {
		if err := a.status(nil); err != nil {
			t.Fatal(err)
		}
	})
	if strings.Contains(out, "acme/clean") {
		t.Errorf("a clean repository in sync was listed:\n%s", out)
	}
	for _, want := range []string{
		"github.com/acme/ahead",
		"1 ahead",
		"github.com/acme/dirty",
		"1 changed file",
		"no upstream",
		".worktrees/github.com/acme/dirty/feat/login",
	} {
		if !strings.Contains(out, want) {
			t.Errorf("status did not mention %q:\n%s", want, out)
		}
	}

	// --dirty drops the repository whose only news is an unpushed commit.
	out = captureStdout(t, func() {
		if err := a.status([]string{"--dirty"}); err != nil {
			t.Fatal(err)
		}
	})
	if strings.Contains(out, "acme/ahead") {
		t.Errorf("--dirty listed a repository with no changes:\n%s", out)
	}
	if !strings.Contains(out, "acme/dirty") {
		t.Errorf("--dirty lost the dirty repository:\n%s", out)
	}

	// --unpushed is the other way round.
	out = captureStdout(t, func() {
		if err := a.status([]string{"--unpushed"}); err != nil {
			t.Fatal(err)
		}
	})
	if !strings.Contains(out, "acme/ahead") || strings.Contains(out, "acme/dirty") {
		t.Errorf("--unpushed listed the wrong repositories:\n%s", out)
	}

	// -a says so about the ones that are fine, rather than staying silent.
	out = captureStdout(t, func() {
		if err := a.status([]string{"-a"}); err != nil {
			t.Fatal(err)
		}
	})
	if !strings.Contains(out, "acme/clean") || !strings.Contains(out, "clean") {
		t.Errorf("-a did not list the clean repository:\n%s", out)
	}
}
