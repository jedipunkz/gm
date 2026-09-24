package finder

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"
	"github.com/jedipunkz/gm/internal/repo"
)

var ctrlL = tea.KeyPressMsg{Code: 'l', Mod: tea.ModCtrl}

// branchModel is a finder over one repository whose branches are stubbed:
// main is checked out in the repository itself, feat/login lives only on
// origin, and fix/timeout is local with no worktree.
func branchModel(t *testing.T) (model, repo.Repo) {
	t.Helper()
	root := t.TempDir()
	r := repo.Repo{Root: root, Rel: "github.com/acme/alpha"}
	m := newTestModel(t, []repo.Repo{r}, "")
	m.w, m.h = 90, 14
	m.worktreesOf = func(dir string) ([]repo.Worktree, error) {
		return []repo.Worktree{{Path: dir, Branch: "main"}}, nil
	}
	m.branchesOf = func(dir string) ([]repo.Branch, error) {
		if dir != r.Path() {
			t.Errorf("asked for the branches of %q, want %q", dir, r.Path())
		}
		// Newest first, the way git sorts them.
		return []repo.Branch{
			{Name: "main"},
			{Name: "fix/timeout"},
			{Name: "feat/login", Remote: "origin/feat/login"},
		}, nil
	}
	return m, r
}

// TestBranchMode pins the Ctrl-L list: the newest branch rests under the
// cursor, a remote-only branch reads as the remote's, Enter on a branch that
// is already checked out goes there, and the key toggles the list off.
func TestBranchMode(t *testing.T) {
	m, r := branchModel(t)

	next, _ := m.Update(ctrlL)
	m = next.(model)
	if m.mode != modeBranches {
		t.Fatal("Ctrl-L did not open the branch list")
	}
	if got := rows(m); strings.Join(got, " ") != "origin/feat/login fix/timeout main" {
		t.Errorf("the branch list reads %v", got)
	}
	if got := stripANSI(m.helpLine(90)); !strings.Contains(got, "enter check out") || !strings.Contains(got, "ctrl-l/g/esc repos") {
		t.Errorf("the hints do not describe the branch list: %q", got)
	}
	if got := stripANSI(strings.Join(m.infoLines(40), "\n")); !strings.Contains(got, "github.com/acme/alpha") {
		t.Errorf("the pane does not name the repository:\n%s", got)
	}

	_, cmd := m.Update(tea.KeyPressMsg{Code: tea.KeyEnter})
	if !isQuit(cmd) {
		t.Fatal("Enter on a checked-out branch did not leave the finder")
	}
	jumped, _ := m.Update(tea.KeyPressMsg{Code: tea.KeyEnter})
	if res := jumped.(model).result; res.Action != ActionJump || res.Arg != r.Path() {
		t.Errorf("Enter on main yielded %+v, want a jump to the repository", res)
	}

	back, _ := m.Update(ctrlL)
	if back.(model).mode != modeRepos {
		t.Error("Ctrl-L did not close the branch list")
	}
	other, _ := m.Update(tea.KeyPressMsg{Code: 'w', Mod: tea.ModCtrl})
	if other.(model).mode != modeWorktrees {
		t.Error("Ctrl-W from the branch list did not open the worktree list")
	}
	esc, cmd := m.Update(tea.KeyPressMsg{Code: tea.KeyEscape})
	if esc.(model).mode != modeRepos || isQuit(cmd) {
		t.Error("Esc did not return to the repository list")
	}
}

// TestBranchesCommand opens the same list as the key.
func TestBranchesCommand(t *testing.T) {
	m, _ := branchModel(t)
	m, _ = runSlash(t, m, "/branches")
	if m.mode != modeBranches {
		t.Error("/branches did not open the branch list")
	}
}

// TestBranchCheckOut makes a worktree for a branch that has none, against
// real git, and leaves the finder for it once git is done.
func TestBranchCheckOut(t *testing.T) {
	root := t.TempDir()
	r := repo.Repo{Root: root, Rel: "github.com/acme/alpha"}
	realRepo(t, r.Path())
	if out, err := exec.Command("git", "-C", r.Path(), "branch", "fix/timeout").CombinedOutput(); err != nil {
		t.Fatalf("git branch: %v\n%s", err, out)
	}

	var mm tea.Model = newTestModel(t, []repo.Repo{r}, "")
	mm, _ = mm.Update(ctrlL)
	for _, c := range "timeout" {
		mm, _ = mm.Update(tea.KeyPressMsg{Code: c, Text: string(c)})
	}
	if it, _ := mm.(model).current(); it.label != "fix/timeout" {
		t.Fatalf("selected %q", it.label)
	}
	mm, cmd := mm.Update(tea.KeyPressMsg{Code: tea.KeyEnter})
	if cmd == nil {
		t.Fatal("Enter did not start making the worktree")
	}
	// Run once: the command is git making the worktree.
	msg := answer(cmd)
	if _, ok := msg.(doneMsg); !ok {
		t.Fatalf("Enter ran %T, want the worktree being made", msg)
	}
	done, quit := mm.Update(msg)
	dir := filepath.Join(root, repo.WorktreeRoot, "github.com/acme/alpha/fix/timeout")
	if _, err := os.Stat(filepath.Join(dir, ".git")); err != nil {
		t.Fatalf("no worktree at %s: %v", dir, err)
	}
	if !isQuit(quit) {
		t.Errorf("the finder stayed open after the worktree was made: %q", done.(model).note)
	}
	if res := done.(model).result; res.Action != ActionJump || res.Arg != dir {
		t.Errorf("the finder yielded %+v, want a jump to %s", res, dir)
	}
}
