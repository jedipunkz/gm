package finder

import (
	"strings"
	"testing"

	tea "charm.land/bubbletea/v2"
	"github.com/jedipunkz/gm/internal/repo"
)

// TestSlashOnlyStartsACommandAtTheStart pins the rule that makes slash
// commands unambiguous: repository paths are full of slashes, and only a
// leading one means a command.
func TestSlashOnlyStartsACommandAtTheStart(t *testing.T) {
	root := t.TempDir()
	repos := []repo.Repo{
		{Root: root, Rel: "github.com/acme/alpha"},
		{Root: root, Rel: "github.com/other/bravo"},
	}
	m := newTestModel(t, repos, "")

	// A slash inside the query is an ordinary filter character.
	m.input.SetValue("acme/alpha")
	m.filter()
	if len(m.view) != 1 {
		t.Fatalf("\"acme/alpha\" matched %d rows, want 1", len(m.view))
	}
	if it, _ := m.current(); it.label != "github.com/acme/alpha" {
		t.Errorf("selected %q", it.label)
	}

	// A leading slash is a command, so the list is left alone.
	m.input.SetValue("/help")
	m.filter()
	if len(m.view) != len(repos) {
		t.Errorf("a command filtered the list down to %d rows", len(m.view))
	}
	if !isCommand("/help") || isCommand("acme/alpha") {
		t.Error("isCommand disagrees with the rule")
	}
}

// TestUnknownCommand leaves the finder alone and says so under the prompt.
func TestUnknownCommand(t *testing.T) {
	root := t.TempDir()
	m := newTestModel(t, []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}, "")
	m.input.SetValue("/nope")

	next, cmd := m.Update(tea.KeyPressMsg{Code: tea.KeyEnter})
	after := next.(model)
	if isQuit(cmd) || after.result.Action != ActionNone {
		t.Error("an unknown command should not end the finder")
	}
	note := stripANSI(after.helpLine(90))
	if !strings.Contains(note, "/nope") || !strings.Contains(note, "/help") {
		t.Errorf("the note does not explain itself: %q", note)
	}

	// The next keystroke clears it.
	after.input.SetValue("/n")
	after.filter()
	if after.note != "" {
		t.Errorf("the note survived a keystroke: %q", after.note)
	}
}

// TestCommandCompletion types a command in and checks the box fills the rest
// in, and that Tab accepts what it offered.
func TestCommandCompletion(t *testing.T) {
	root := t.TempDir()
	var m tea.Model = newTestModel(t, []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}, "")
	for _, r := range "/wo" {
		m, _ = m.Update(tea.KeyPressMsg{Code: r, Text: string(r)})
	}
	if got := stripANSI(m.(model).input.View()); !strings.Contains(got, "/worktrees") {
		t.Errorf("the box does not complete the command: %q", got)
	}
	m, _ = m.Update(tea.KeyPressMsg{Code: tea.KeyTab})
	if got := m.(model).input.Value(); got != "/worktrees" {
		t.Errorf("tab produced %q", got)
	}
}

// TestActionsNeedTheirArgument refuses to guess what to create or clone.
func TestActionsNeedTheirArgument(t *testing.T) {
	root := t.TempDir()
	m := newTestModel(t, []repo.Repo{{Root: root, Rel: "github.com/acme/alpha"}}, "")

	for _, typed := range []string{"/create", "/get", "/create   "} {
		next, cmd := runSlash(t, m, typed)
		if isQuit(cmd) || next.result.Action != ActionNone {
			t.Errorf("%q acted without an argument", typed)
		}
		if !strings.Contains(next.note, "<repo>") {
			t.Errorf("%q does not say what it needs: %q", typed, next.note)
		}
	}
}
