package finder

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"
	"github.com/jedipunkz/gm/internal/repo"
)

// TestWorktreeMode pins the Ctrl-W list: it replaces the repositories, the
// main worktree rests under the cursor, typing filters it, Enter yields that
// worktree's path, and Esc puts the repository list back untouched.
func TestWorktreeMode(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{
		{Root: root, Rel: "github.com/acme/alpha"},
		{Root: root, Rel: "github.com/acme/bravo"},
	}
	m := newTestModel(t, repos, "")
	m.input.SetValue("bravo")
	m.filter()
	m.worktreesOf = func(dir string) ([]repo.Worktree, error) {
		if dir != repos[1].Path() {
			t.Errorf("asked for the worktrees of %q, want %q", dir, repos[1].Path())
		}
		return []repo.Worktree{
			{Path: dir, Branch: "main"},
			{Path: "/tmp/wt/bravo-login", Branch: "feat/login"},
			{Path: "/tmp/wt/bravo-fix", Branch: "fix/timeout"},
		}, nil
	}

	next, _ := m.openWorktrees()
	m = next.(model)
	if m.mode != modeWorktrees {
		t.Fatal("Ctrl-W did not open the worktree list")
	}
	if len(m.view) != 3 {
		t.Fatalf("worktree list has %d rows, want 3", len(m.view))
	}
	if it, _ := m.current(); it.label != "main" {
		t.Errorf("cursor rests on %q, want the main worktree", it.label)
	}
	if m.input.Value() != "" {
		t.Errorf("the repository query %q leaked into the worktree list", m.input.Value())
	}

	m.input.SetValue("login")
	m.filter()
	it, ok := m.current()
	if !ok || it.label != "feat/login" {
		t.Fatalf("filtering for \"login\" selected %q", it.label)
	}
	if it.path != "/tmp/wt/bravo-login" {
		t.Errorf("Enter would jump to %q, want the worktree path", it.path)
	}

	m.restore()
	if m.mode != modeRepos {
		t.Fatal("Esc did not return to the repository list")
	}
	if m.input.Value() != "bravo" {
		t.Errorf("the repository query came back as %q, want bravo", m.input.Value())
	}
	if it, _ := m.current(); it.label != "github.com/acme/bravo" {
		t.Errorf("the repository selection came back as %q", it.label)
	}
}

// TestWorktreeModeKeysGoBack pins the three ways out of the worktree list:
// Ctrl-W toggles it off, Ctrl-G closes it, and Esc does too. Only Esc quits
// gm, and only from the repository list.
func TestWorktreeModeKeysGoBack(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}

	keys := map[string]tea.KeyPressMsg{
		"ctrl+w": {Code: 'w', Mod: tea.ModCtrl},
		"ctrl+g": {Code: 'g', Mod: tea.ModCtrl},
		"esc":    {Code: tea.KeyEscape},
	}
	for key, press := range keys {
		m := newTestModel(t, repos, "")
		m.worktreesOf = func(dir string) ([]repo.Worktree, error) {
			return []repo.Worktree{{Path: dir, Branch: "main"}}, nil
		}
		next, _ := m.openWorktrees()
		m = next.(model)
		if m.mode != modeWorktrees {
			t.Fatal("the worktree list did not open")
		}

		next, cmd := m.Update(press)
		m = next.(model)
		if m.mode != modeRepos {
			t.Errorf("%s did not return to the repository list", key)
		}
		if isQuit(cmd) {
			t.Errorf("%s quit gm instead of closing the worktree list", key)
		}
	}

	// From the repository list, Esc still quits and Ctrl-G does nothing.
	m := newTestModel(t, repos, "")
	if _, cmd := m.Update(tea.KeyPressMsg{Code: tea.KeyEscape}); !isQuit(cmd) {
		t.Error("Esc should quit from the repository list")
	}
	if _, cmd := m.Update(tea.KeyPressMsg{Code: 'g', Mod: tea.ModCtrl}); isQuit(cmd) {
		t.Error("Ctrl-G should not quit from the repository list")
	}
}

// TestWorktreeModeLeavesTheListAloneOnError guards the case where git cannot
// answer: the finder must stay usable rather than empty itself.
func TestWorktreeModeLeavesTheListAloneOnError(t *testing.T) {
	root := t.TempDir()
	m := newTestModel(t, []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}, "")
	m.worktreesOf = func(string) ([]repo.Worktree, error) { return nil, errors.New("not a git repository") }

	next, _ := m.openWorktrees()
	m = next.(model)
	if m.mode != modeRepos || len(m.view) != 1 {
		t.Errorf("a failed worktree listing changed the finder: mode=%v rows=%d", m.mode, len(m.view))
	}
}

// TestWorktreeCreateAndRemove drives the whole thing against real git: the
// branch is checked out where gm says it belongs, the row appears, and
// removing it takes both away.
func TestWorktreeCreateAndRemove(t *testing.T) {
	root := t.TempDir()
	r := repo.Repo{Root: root, Rel: "github.com/acme/alpha"}
	realRepo(t, r.Path())

	m := newTestModel(t, []repo.Repo{r}, "")
	m.w, m.h = 90, 18
	next, _ := m.openWorktrees() // the real repo.Worktrees, not a stub
	m = next.(model)
	if m.mode != modeWorktrees || len(m.view) != 1 {
		t.Fatalf("the worktree list has %d rows", len(m.view))
	}

	m, _ = runSlash(t, m, "/create feat/login")
	if m.over != overlayConfirm {
		t.Fatalf("/create did not ask: %q", m.note)
	}
	// The rendered panel folds long paths, and where it folds depends on the
	// width of the temporary directory, so the destination is checked on the
	// model and only the fixed words on the screen.
	if !strings.Contains(m.ask.dir, repo.WorktreeRoot) {
		t.Errorf("the worktree would go to %q, want it under %s", m.ask.dir, repo.WorktreeRoot)
	}
	box := stripANSI(m.View().Content)
	for _, want := range []string{"create worktree", "feat/login, new branch"} {
		if !strings.Contains(box, want) {
			t.Errorf("the question does not mention %q:\n%s", want, box)
		}
	}

	next, cmd := m.Update(tea.KeyPressMsg{Code: 'y', Text: "y"})
	next, _ = next.(model).Update(cmd())
	m = next.(model)

	dir := filepath.Join(root, repo.WorktreeRoot, "github.com/acme/alpha/feat/login")
	if _, err := os.Stat(filepath.Join(dir, ".git")); err != nil {
		t.Fatalf("no worktree at %s: %v", dir, err)
	}
	if !repo.BranchExists(r.Path(), "feat/login") {
		t.Error("the branch was not created")
	}
	if got := rows(m); len(got) != 2 {
		t.Fatalf("the list has %v, want the new worktree in it", got)
	}
	if it, _ := m.current(); it.path != dir {
		t.Errorf("the new worktree is not selected, %q is", it.label)
	}
	// A worktree must not show up as a repository: the dot in .worktrees is
	// what keeps it out.
	found, err := repo.FindRepos(root)
	if err != nil || len(found) != 1 || found[0] != r.Path() {
		t.Errorf("FindRepos() = %v, %v; want just the repository", found, err)
	}

	// Now take it away.
	m, _ = runSlash(t, m, "/remove")
	if m.over != overlayConfirm {
		t.Fatalf("/remove did not ask: %q", m.note)
	}
	next, cmd = m.Update(tea.KeyPressMsg{Code: 'y', Text: "y"})
	next, _ = next.(model).Update(cmd())
	m = next.(model)

	if _, err := os.Stat(dir); !os.IsNotExist(err) {
		t.Errorf("the worktree is still on disk: %v", err)
	}
	if got := rows(m); len(got) != 1 {
		t.Errorf("the removed worktree is still listed: %v", got)
	}
	if !strings.Contains(m.note, "removed") {
		t.Errorf("the finder did not report it: %q", m.note)
	}
}

// TestWorktreeCreateRefusesADuplicate says so instead of letting git fail.
func TestWorktreeCreateRefusesADuplicate(t *testing.T) {
	root := t.TempDir()
	r := repo.Repo{Root: root, Rel: "github.com/acme/alpha"}
	realRepo(t, r.Path())
	dir := filepath.Join(root, repo.WorktreeRoot, "github.com/acme/alpha/feat/login")
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}

	m := newTestModel(t, []repo.Repo{r}, "")
	next, _ := m.openWorktrees()
	m = next.(model)

	m, _ = runSlash(t, m, "/create feat/login")
	if m.over != overlayNone {
		t.Error("/create asked about a directory that already exists")
	}
	if !strings.Contains(m.note, "already exists") {
		t.Errorf("the note does not explain why: %q", m.note)
	}
}

// TestWorktreeCreateRefusesABogusBranch says so under the prompt rather than
// letting git print a page of hints over the list.
func TestWorktreeCreateRefusesABogusBranch(t *testing.T) {
	root := t.TempDir()
	r := repo.Repo{Root: root, Rel: "github.com/acme/alpha"}
	realRepo(t, r.Path())

	m := newTestModel(t, []repo.Repo{r}, "")
	next, _ := m.openWorktrees()
	m = next.(model)

	m, cmd := runSlash(t, m, "/create ../../escaped")
	if cmd != nil || m.over != overlayNone {
		t.Error("/create acted on a name that is not a branch name")
	}
	if !strings.Contains(m.note, "not a branch name") {
		t.Errorf("the note does not explain why: %q", m.note)
	}
	if _, err := os.Stat(filepath.Join(root, repo.WorktreeRoot)); !os.IsNotExist(err) {
		t.Errorf("it made directories anyway: %v", err)
	}
}

// TestRemoveRefusesTheMainWorktree: the top checkout is the repository
// itself, and git will not remove it either.
func TestRemoveRefusesTheMainWorktree(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}
	m := newTestModel(t, repos, "")
	m.worktreesOf = func(dir string) ([]repo.Worktree, error) {
		return []repo.Worktree{{Path: dir, Branch: "main"}}, nil
	}
	next, _ := m.openWorktrees()
	m = next.(model)

	m, cmd := runSlash(t, m, "/remove")
	if isQuit(cmd) || m.over != overlayNone {
		t.Error("/remove asked about the repository itself")
	}
	if !strings.Contains(m.note, "repository itself") {
		t.Errorf("the note does not explain why: %q", m.note)
	}
}
