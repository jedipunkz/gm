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

var ctrlM = tea.KeyPressMsg{Code: 'm', Mod: tea.ModCtrl}

func pullRequest(n int, title, branch string, draft, fork bool, owner string) repo.PullRequest {
	p := repo.PullRequest{Number: n, Title: title, Branch: branch, Draft: draft, Fork: fork}
	p.Author.Login, p.HeadOwner.Login = owner, owner
	return p
}

// prModel is a finder over two repositories; alpha has three open pull
// requests, one of them already checked out, and gh is stubbed.
func prModel(t *testing.T) (model, []repo.Repo) {
	t.Helper()
	root := t.TempDir()
	repos := []repo.Repo{
		{Root: root, Rel: "github.com/acme/bravo"},
		{Root: root, Rel: "github.com/acme/alpha"},
	}
	m := newTestModel(t, repos, "")
	m.w, m.h = 100, 16
	m.input.SetValue("alpha")
	m.filter()
	m.worktreesOf = func(dir string) ([]repo.Worktree, error) {
		return []repo.Worktree{
			{Path: dir, Branch: "main"},
			{Path: "/tmp/wt/alpha-login", Branch: "feat/login"},
		}, nil
	}
	m.prsOf = func(dir string) ([]repo.PullRequest, error) {
		if dir != repos[1].Path() {
			t.Errorf("asked gh about %q, want %q", dir, repos[1].Path())
		}
		// Newest first, the way gh lists them.
		return []repo.PullRequest{
			pullRequest(9, "Fix typo", "main", false, true, "bob"),
			pullRequest(8, "Try a new parser", "exp/parser", true, false, "acme"),
			pullRequest(7, "Add login", "feat/login", false, false, "acme"),
		}, nil
	}
	return m, repos
}

// openPRList presses the key and hands the finder gh's answer.
func openPRList(t *testing.T, m model, key tea.KeyPressMsg) model {
	t.Helper()
	next, cmd := m.Update(key)
	m = next.(model)
	if cmd == nil {
		t.Fatal("the key did not ask gh")
	}
	if !strings.Contains(m.note, "asking GitHub") {
		t.Errorf("nothing says gh is being asked: %q", m.note)
	}
	next, _ = m.Update(cmd())
	return next.(model)
}

// TestPRMode pins the Ctrl-M list: newest at the bottom, drafts marked, a
// checked-out pull request goes to its worktree, and a fork's main is not
// mistaken for the repository's own.
func TestPRMode(t *testing.T) {
	m, repos := prModel(t)
	m = openPRList(t, m, ctrlM)
	if m.mode != modePRs {
		t.Fatalf("the pull request list did not open: %q", m.note)
	}
	want := "#7 Add login|#8 [draft] Try a new parser|#9 Fix typo"
	if got := strings.Join(rows(m), "|"); got != want {
		t.Errorf("the list reads %q, want %q", got, want)
	}
	if got := stripANSI(m.helpLine(100)); !strings.Contains(got, "enter check out") || !strings.Contains(got, "ctrl-m/g/esc repos") {
		t.Errorf("the hints do not describe the list: %q", got)
	}
	pane := stripANSI(strings.Join(m.infoLines(40), "\n"))
	for _, s := range []string{repos[1].Rel, "bob:main", "none yet"} {
		if !strings.Contains(pane, s) {
			t.Errorf("the pane does not show %q:\n%s", s, pane)
		}
	}

	// The fork's main is the selected row, and has no worktree of its own.
	if it, _ := m.current(); it.path != "" {
		t.Errorf("the fork's main resolved to %q", it.path)
	}

	m.input.SetValue("login")
	m.filter()
	next, cmd := m.Update(tea.KeyPressMsg{Code: tea.KeyEnter})
	if res := next.(model).result; !isQuit(cmd) || res.Action != ActionJump || res.Arg != "/tmp/wt/alpha-login" {
		t.Errorf("Enter on a checked-out pull request yielded %+v", res)
	}

	back, _ := m.Update(ctrlM)
	if back.(model).mode != modeRepos {
		t.Error("Ctrl-M did not close the list")
	}
	if it, _ := back.(model).current(); it.label != repos[1].Rel {
		t.Errorf("going back selected %q", it.label)
	}
}

// TestPRsCommand opens the same list as the key.
func TestPRsCommand(t *testing.T) {
	m, _ := prModel(t)
	next, cmd := runSlash(t, m, "/prs")
	if cmd == nil {
		t.Fatal("/prs did not ask gh")
	}
	after, _ := next.Update(cmd())
	if after.(model).mode != modePRs {
		t.Error("/prs did not open the pull request list")
	}
}

// TestPRModeDropsAStaleAnswer: gh is slow, and an answer about a repository
// the cursor has left must not replace the list.
func TestPRModeDropsAStaleAnswer(t *testing.T) {
	m, _ := prModel(t)
	next, cmd := m.Update(ctrlM)
	m = next.(model)
	m.input.SetValue("bravo")
	m.filter()
	next, _ = m.Update(cmd())
	if next.(model).mode != modeRepos {
		t.Error("an answer about alpha opened while bravo was selected")
	}
}

// TestPRModeSaysWhyNot keeps the repository list when gh fails or has
// nothing, and says so under the prompt.
func TestPRModeSaysWhyNot(t *testing.T) {
	for _, c := range []struct {
		prs  []repo.PullRequest
		err  error
		note string
	}{
		{nil, errors.New("gh is not installed: pull requests come from the GitHub CLI"), "gh is not installed"},
		{nil, nil, "no open pull requests"},
	} {
		m, _ := prModel(t)
		m.prsOf = func(string) ([]repo.PullRequest, error) { return c.prs, c.err }
		m = openPRList(t, m, ctrlM)
		if m.mode != modeRepos || !strings.Contains(m.note, c.note) {
			t.Errorf("mode=%v note=%q, want the repository list and %q", m.mode, m.note, c.note)
		}
	}
}

// TestPRCheckOut has a stand-in gh make the worktree, and leaves the finder
// for it.
func TestPRCheckOut(t *testing.T) {
	root := t.TempDir()
	r := repo.Repo{Root: root, Rel: "github.com/acme/alpha"}
	realRepo(t, r.Path())
	bin := t.TempDir()
	// gh pr checkout <n> --worktree <dir>
	script := "#!/bin/sh\nexec git worktree add -q -b \"pr-$3\" \"$5\"\n"
	if err := os.WriteFile(filepath.Join(bin, "gh"), []byte(script), 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("PATH", bin+string(os.PathListSeparator)+os.Getenv("PATH"))

	m := newTestModel(t, []repo.Repo{r}, "")
	m.prsOf = func(string) ([]repo.PullRequest, error) {
		return []repo.PullRequest{pullRequest(7, "Add login", "feat/login", false, false, "acme")}, nil
	}
	m = openPRList(t, m, ctrlM)
	next, cmd := m.Update(tea.KeyPressMsg{Code: tea.KeyEnter})
	if cmd == nil {
		t.Fatalf("Enter did not start the checkout: %q", next.(model).note)
	}
	msg := cmd()
	if _, ok := msg.(doneMsg); !ok {
		t.Fatalf("Enter ran %T, want the checkout", msg)
	}
	done, quit := next.Update(msg)
	dir := filepath.Join(root, repo.WorktreeRoot, "github.com/acme/alpha/feat/login")
	if _, err := os.Stat(filepath.Join(dir, ".git")); err != nil {
		t.Fatalf("no worktree at %s: %v (%q)", dir, err, done.(model).note)
	}
	if res := done.(model).result; !isQuit(quit) || res.Action != ActionJump || res.Arg != dir {
		t.Errorf("the finder yielded %+v, want a jump to %s", res, dir)
	}
}
