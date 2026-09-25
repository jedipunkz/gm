package finder

import (
	"errors"
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
	// The remote has one branch the last fetch did not bring, and repeats the
	// ones it did.
	m.remoteBranchesOf = func(string) ([]repo.Branch, error) {
		return []repo.Branch{
			{Name: "feat/login", Remote: "origin/feat/login", Unfetched: true},
			{Name: "main", Remote: "origin/main", Unfetched: true},
			{Name: "feat/new", Remote: "origin/feat/new", Unfetched: true},
		}, nil
	}
	return m, r
}

// TestBranchMode pins the Ctrl-L list: the newest branch rests under the
// cursor, a remote-only branch reads as the remote's, Enter on a branch that
// is already checked out goes there, and the key toggles the list off.
func TestBranchMode(t *testing.T) {
	m, r := branchModel(t)

	next, cmd := m.Update(ctrlL)
	m = next.(model)
	if m.mode != modeBranches {
		t.Fatal("Ctrl-L did not open the branch list")
	}
	if got := rows(m); strings.Join(got, " ") != "origin/feat/login fix/timeout main" {
		t.Errorf("the branch list reads %v", got)
	}
	next, _ = m.Update(answer(cmd))
	m = next.(model)
	if got := stripANSI(m.helpLine(90)); !strings.Contains(got, "enter check out") || !strings.Contains(got, "ctrl-l/g/esc repos") {
		t.Errorf("the hints do not describe the branch list: %q", got)
	}
	if got := stripANSI(strings.Join(m.infoLines(40), "\n")); !strings.Contains(got, "github.com/acme/alpha") {
		t.Errorf("the pane does not name the repository:\n%s", got)
	}

	_, cmd = m.Update(tea.KeyPressMsg{Code: tea.KeyEnter})
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

// TestRemoteBranchesJoinTheList puts what only the remote has at the top of
// the branch list, once, without moving the selection.
func TestRemoteBranchesJoinTheList(t *testing.T) {
	m, _ := branchModel(t)
	next, cmd := m.Update(ctrlL)
	m = next.(model)
	if !strings.Contains(m.busy, "asking the remotes") {
		t.Errorf("nothing says the remotes are being asked: %q", m.busy)
	}
	before, _ := m.current()
	msg := answer(cmd)

	next, _ = m.Update(msg)
	m = next.(model)
	if m.busy != "" {
		t.Errorf("the wait outlived the answer: %q", m.busy)
	}
	want := "origin/feat/new origin/feat/login fix/timeout main"
	if got := strings.Join(rows(m), " "); got != want {
		t.Errorf("the branch list reads %q, want %q", got, want)
	}
	if it, _ := m.current(); it.label != before.label {
		t.Errorf("the selection moved from %q to %q", before.label, it.label)
	}

	// The same answer again adds nothing.
	next, _ = m.Update(msg)
	if got := strings.Join(rows(next.(model)), " "); got != want {
		t.Errorf("a second answer changed the list to %q", got)
	}

	m.input.SetValue("new")
	m.filter()
	pane := stripANSI(strings.Join(m.infoLines(60), "\n"))
	if !strings.Contains(pane, "not fetched yet") {
		t.Errorf("the pane does not say the branch is unfetched:\n%s", pane)
	}
}

// TestRemoteBranchesSayWhyNot keeps the local list when a remote cannot be
// reached, and names the remote under the prompt.
func TestRemoteBranchesSayWhyNot(t *testing.T) {
	m, _ := branchModel(t)
	m.remoteBranchesOf = func(string) ([]repo.Branch, error) {
		return nil, errors.New("origin: Could not read from remote repository.")
	}
	next, cmd := m.Update(ctrlL)
	next, _ = next.Update(answer(cmd))
	m = next.(model)
	if len(m.view) != 3 || !strings.Contains(m.note, "origin: Could not read") {
		t.Errorf("rows=%v note=%q", rows(m), m.note)
	}

	// An answer for a list that has since closed is dropped.
	m, _ = branchModel(t)
	next, cmd = m.Update(ctrlL)
	closed, _ := next.Update(ctrlL)
	after, _ := closed.Update(answer(cmd))
	if after.(model).mode != modeRepos || len(after.(model).view) != 1 {
		t.Error("an answer for a closed branch list changed the repository list")
	}
}

// TestUnfetchedBranchCheckOut fetches a branch the clone has never seen and
// checks it out, against real git.
func TestUnfetchedBranchCheckOut(t *testing.T) {
	root := t.TempDir()
	upstream := filepath.Join(t.TempDir(), "upstream")
	realRepo(t, upstream)
	r := repo.Repo{Root: root, Rel: "github.com/acme/alpha"}
	if out, err := exec.Command("git", "clone", "-q", upstream, r.Path()).CombinedOutput(); err != nil {
		t.Fatalf("git clone: %v\n%s", err, out)
	}
	if out, err := exec.Command("git", "-C", upstream, "branch", "feat/new").CombinedOutput(); err != nil {
		t.Fatalf("git branch: %v\n%s", err, out)
	}

	var mm tea.Model = newTestModel(t, []repo.Repo{r}, "")
	mm, cmd := mm.Update(ctrlL)
	mm, _ = mm.Update(answer(cmd))
	for _, c := range "new" {
		mm, _ = mm.Update(tea.KeyPressMsg{Code: c, Text: string(c)})
	}
	if it, _ := mm.(model).current(); it.label != "origin/feat/new" {
		t.Fatalf("selected %q, rows %v", it.label, rows(mm.(model)))
	}
	mm, cmd = mm.Update(tea.KeyPressMsg{Code: tea.KeyEnter})
	if !strings.Contains(mm.(model).busy, "fetching origin/feat/new") {
		t.Errorf("the wait does not say it fetches: %q", mm.(model).busy)
	}
	done, quit := mm.Update(answer(cmd))
	dir := filepath.Join(root, repo.WorktreeRoot, "github.com/acme/alpha/feat/new")
	if _, err := os.Stat(filepath.Join(dir, ".git")); err != nil {
		t.Fatalf("no worktree at %s: %v (%q)", dir, err, done.(model).note)
	}
	if res := done.(model).result; !isQuit(quit) || res.Arg != dir {
		t.Errorf("the finder yielded %+v, want a jump to %s", res, dir)
	}
}
