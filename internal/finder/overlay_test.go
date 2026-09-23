package finder

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"
	"github.com/jedipunkz/gm/internal/repo"
)

// TestHelpPopup covers the command list: Enter on /help opens it, it names
// every command, it swallows keys while it is up, and q or Esc closes it.
func TestHelpPopup(t *testing.T) {
	root := t.TempDir()
	m := newTestModel(t, []repo.Repo{
		{Root: root, Rel: "github.com/acme/alpha"},
		{Root: root, Rel: "github.com/acme/bravo"},
	}, "")
	m.w, m.h = 90, 20

	m.input.SetValue("/help")
	next, _ := m.Update(tea.KeyPressMsg{Code: tea.KeyEnter})
	open := next.(model)
	if open.over != overlayHelp {
		t.Fatal("/help did not open the command list")
	}
	if open.result.Action != ActionNone {
		t.Error("/help decided something")
	}
	if open.input.Value() != "" {
		t.Errorf("the command was left in the box: %q", open.input.Value())
	}

	view := stripANSI(open.View().Content)
	for _, c := range commands {
		if !strings.Contains(view, c.name) || !strings.Contains(view, c.what) {
			t.Errorf("the popup does not describe %s:\n%s", c.name, view)
		}
	}
	if !strings.Contains(view, "esc") {
		t.Errorf("the popup does not say how to close it:\n%s", view)
	}

	// Moving is impossible while it is up.
	cursor := open.cursor
	moved, _ := open.Update(tea.KeyPressMsg{Code: tea.KeyUp})
	if moved.(model).cursor != cursor || moved.(model).over != overlayHelp {
		t.Error("a key reached the list behind the popup")
	}

	for _, key := range []tea.KeyPressMsg{{Code: 'q'}, {Code: tea.KeyEscape}} {
		closed, cmd := open.Update(key)
		if closed.(model).over != overlayNone {
			t.Errorf("%v did not close the popup", key)
		}
		if isQuit(cmd) {
			t.Errorf("%v quit gm instead of closing the popup", key)
		}
	}
}

// TestHelpShowsArguments keeps the popup honest about what each command takes.
func TestHelpShowsArguments(t *testing.T) {
	root := t.TempDir()
	m := newTestModel(t, []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}, "")
	m.w, m.h = 90, 24
	m.over = overlayHelp

	view := stripANSI(m.View().Content)
	for _, want := range []string{"/create <repo>", "/get <repo>", "/remove"} {
		if !strings.Contains(view, want) {
			t.Errorf("the popup does not show %q:\n%s", want, view)
		}
	}
}

// TestConfirmRemove takes the whole path: the question names what will be
// lost, y does it, the row goes, and the finder says so without leaving.
func TestConfirmRemove(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{
		{Root: root, Rel: "github.com/acme/alpha"},
		{Root: root, Rel: "github.com/acme/bravo"},
	}
	for _, r := range repos {
		if err := os.MkdirAll(filepath.Join(r.Path(), ".git"), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	m := newTestModel(t, repos, "")
	m.w, m.h = 90, 16
	m.status[repos[1].Path()] = repo.Status{Dirty: 3}

	m, _ = runSlash(t, m, "/remove") // bravo, the bottom row
	if m.over != overlayConfirm {
		t.Fatal("/remove did not ask")
	}
	if m.ask.arg != repos[1].Path() {
		t.Errorf("the question is about %q, want the selected repository", m.ask.arg)
	}
	// Only fold-proof words are looked for on screen: where a long path
	// breaks depends on the width of the temporary directory.
	box := stripANSI(m.View().Content)
	for _, want := range []string{"remove", "3 uncommitted changes", "y do it", "n cancel"} {
		if !strings.Contains(box, want) {
			t.Errorf("the question does not mention %q:\n%s", want, box)
		}
	}
	// It is a panel over the list, not a screen of its own.
	if !strings.Contains(box, "❯") {
		t.Errorf("the panel replaced the whole screen:\n%s", box)
	}

	next, cmd := m.Update(tea.KeyPressMsg{Code: 'y', Text: "y"})
	m = next.(model)
	if cmd == nil {
		t.Fatal("y did not do anything")
	}
	if m.over != overlayNone {
		t.Error("the question stayed on screen")
	}

	next, _ = m.Update(cmd())
	m = next.(model)
	if isQuit(cmd) || m.result.Action != ActionNone {
		t.Error("removing ended the finder")
	}
	if _, err := os.Stat(repos[1].Path()); !os.IsNotExist(err) {
		t.Errorf("the repository is still on disk: %v", err)
	}
	if got := rows(m); len(got) != 1 || got[0] != "github.com/acme/alpha" {
		t.Errorf("the removed row is still listed: %v", got)
	}
	if !strings.Contains(m.note, "removed") {
		t.Errorf("the finder did not report it: %q", m.note)
	}
}

// TestConfirmCancel leaves everything alone.
func TestConfirmCancel(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}
	if err := os.MkdirAll(filepath.Join(repos[0].Path(), ".git"), 0o755); err != nil {
		t.Fatal(err)
	}
	m := newTestModel(t, repos, "")

	m, _ = runSlash(t, m, "/remove")
	for _, key := range []tea.KeyPressMsg{{Code: 'n', Text: "n"}, {Code: tea.KeyEscape}} {
		asked, _ := runSlash(t, m, "/remove")
		next, cmd := asked.Update(key)
		after := next.(model)
		if cmd != nil {
			t.Errorf("%v started the removal anyway", key)
		}
		if after.over != overlayNone || after.ask.kind != changeNone {
			t.Errorf("%v left the question up", key)
		}
		if !repoExists(repos[0].Path()) {
			t.Fatalf("%v removed the repository", key)
		}
		if after.note != "cancelled" {
			t.Errorf("%v said %q", key, after.note)
		}
	}
}

// TestConfirmCreate makes the repository where the reference says, adds the
// row and selects it, all without leaving the finder.
func TestConfirmCreate(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}
	m := newTestModel(t, repos, "")
	m.w, m.h = 90, 16

	m, _ = runSlash(t, m, "/create acme/bravo")
	if m.over != overlayConfirm {
		t.Fatal("/create did not ask")
	}
	if m.ask.arg != "acme/bravo" {
		t.Errorf("the question is about %q", m.ask.arg)
	}
	box := stripANSI(m.View().Content)
	for _, want := range []string{"create", "origin https://github.com/acme/bravo"} {
		if !strings.Contains(box, want) {
			t.Errorf("the question does not mention %q:\n%s", want, box)
		}
	}

	next, cmd := m.Update(tea.KeyPressMsg{Code: 'y', Text: "y"})
	next, _ = next.(model).Update(cmd())
	m = next.(model)

	dst := filepath.Join(root, "github.com/acme/bravo")
	if !repoExists(dst) {
		t.Fatalf("no repository at %s", dst)
	}
	if got, _ := repo.GitIn(dst, "remote", "get-url", "origin"); got != "https://github.com/acme/bravo" {
		t.Errorf("origin is %q", got)
	}
	if it, _ := m.current(); it.path != dst {
		t.Errorf("the new repository is not selected, %q is", it.label)
	}
	if !strings.Contains(m.note, "created") {
		t.Errorf("the finder did not report it: %q", m.note)
	}
}

// TestRemoveSaysTheWorktreesGoToo: they are deleted along with the
// repository, so the question has to name them before anyone says yes.
func TestRemoveSaysTheWorktreesGoToo(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}
	m := newTestModel(t, repos, "")
	m.w, m.h = 90, 18
	m.worktreesOf = func(dir string) ([]repo.Worktree, error) {
		return []repo.Worktree{
			{Path: dir, Branch: "main"}, // the repository itself
			{Path: "/tmp/wt/login", Branch: "feat/login"},
			{Path: "/tmp/wt/timeout", Branch: "fix/timeout"},
		}, nil
	}

	m, _ = runSlash(t, m, "/remove")
	if m.over != overlayConfirm {
		t.Fatalf("/remove did not ask: %q", m.note)
	}
	box := stripANSI(m.View().Content)
	for _, want := range []string{"2 worktrees will go too", "feat/login", "fix/timeout"} {
		if !strings.Contains(box, want) {
			t.Errorf("the question does not mention %q:\n%s", want, box)
		}
	}
	// The repository is not one of its own worktrees.
	if strings.Contains(box, "3 worktrees") {
		t.Errorf("the repository was counted as a worktree:\n%s", box)
	}
}

// TestRemoveSaysNothingWithoutWorktrees keeps the ordinary question short.
func TestRemoveSaysNothingWithoutWorktrees(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}
	m := newTestModel(t, repos, "")
	m.w, m.h = 90, 18
	m.worktreesOf = func(dir string) ([]repo.Worktree, error) {
		return []repo.Worktree{{Path: dir, Branch: "main"}}, nil
	}

	m, _ = runSlash(t, m, "/remove")
	// "worktree" on its own would match the hint line under the prompt.
	if box := stripANSI(m.View().Content); strings.Contains(box, "will go too") {
		t.Errorf("the question talks about worktrees there are none of:\n%s", box)
	}
}
