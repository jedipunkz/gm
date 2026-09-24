package finder

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"
	"github.com/jedipunkz/gm/internal/config"
	"github.com/jedipunkz/gm/internal/repo"
)

// newTestModel builds a finder over repos with an empty visit log, unless
// bumped is set, in which case that repository is the favourite.
func newTestModel(t *testing.T, repos []repo.Repo, bumped string) model {
	t.Helper()
	hist := repo.OpenHistory(filepath.Join(t.TempDir(), "frecency.json"))
	if bumped != "" {
		// The log prunes repositories that are gone, so it has to exist.
		if err := os.MkdirAll(bumped, 0o755); err != nil {
			t.Fatal(err)
		}
		if err := hist.Bump(bumped); err != nil {
			t.Fatal(err)
		}
	}
	theme, err := LookupTheme("")
	if err != nil {
		t.Fatal(err)
	}
	root := ""
	if len(repos) > 0 {
		root = repos[0].Root
	}
	return newModel(&repo.Tree{Roots: []string{root}}, repos, hist, theme, testKeys(t))
}

// testKeys are the default chords, parsed the way a real run parses them.
func testKeys(t *testing.T) Keys {
	t.Helper()
	wt, err := config.ParseChord("", DefaultWorktreeKey)
	if err != nil {
		t.Fatal(err)
	}
	br, err := config.ParseChord("", DefaultBranchKey)
	if err != nil {
		t.Fatal(err)
	}
	pr, err := config.ParseChord("", DefaultPRKey)
	if err != nil {
		t.Fatal(err)
	}
	rm, err := config.ParseChord("", DefaultRemoteKey)
	if err != nil {
		t.Fatal(err)
	}
	return Keys{Worktree: wt, Branch: br, PR: pr, Remote: rm}
}

// ansi renders a #rrggbb color the way lipgloss writes it into the output.
func ansi(layer, hex string) string {
	var r, g, b int
	if _, err := fmt.Sscanf(hex, "#%02x%02x%02x", &r, &g, &b); err != nil {
		panic(err)
	}
	return fmt.Sprintf("%s;2;%d;%d;%d", layer, r, g, b)
}

func stripANSI(s string) string {
	var b strings.Builder
	for i := 0; i < len(s); {
		if s[i] == 0x1b {
			for i < len(s) && s[i] != 'm' {
				i++
			}
			i++
			continue
		}
		b.WriteByte(s[i])
		i++
	}
	return b.String()
}

// isQuit reports whether a command is tea.Quit, which is what ends the finder.
func isQuit(cmd tea.Cmd) bool {
	if cmd == nil {
		return false
	}
	_, ok := cmd().(tea.QuitMsg)
	return ok
}

// runSlash types a command and presses Enter, returning what the finder did.
func runSlash(t *testing.T, m model, name string) (model, tea.Cmd) {
	t.Helper()
	m.input.SetValue(name)
	next, cmd := m.Update(tea.KeyPressMsg{Code: tea.KeyEnter})
	return next.(model), cmd
}

func rows(m model) []string {
	out := make([]string, 0, len(m.view))
	for _, i := range m.view {
		out = append(out, m.all[i].label)
	}
	return out
}

// dirtyModel is a finder over three repositories, two of which have
// uncommitted work, with the scan stubbed so no git runs.
func dirtyModel(t *testing.T) (model, []repo.Repo) {
	t.Helper()
	root := t.TempDir()
	repos := []repo.Repo{
		{Root: root, Rel: "github.com/acme/alpha"},
		{Root: root, Rel: "github.com/acme/bravo"},
		{Root: root, Rel: "github.com/acme/charlie"},
	}
	m := newTestModel(t, repos, "")
	m.w, m.h = 90, 14
	m.dirtyOf = func([]string) map[string]bool {
		return map[string]bool{
			repos[0].Path(): true,
			repos[1].Path(): false,
			repos[2].Path(): true,
		}
	}
	return m, repos
}

func repoExists(path string) bool {
	_, err := os.Stat(filepath.Join(path, ".git"))
	return err == nil
}

// realRepo makes a repository with one commit, which is what git needs before
// it will hand out a worktree.
func realRepo(t *testing.T, dir string) {
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
