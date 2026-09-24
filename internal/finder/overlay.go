package finder

import (
	"fmt"
	"strconv"
	"strings"

	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"

	"github.com/jedipunkz/gm/internal/repo"
)

// overlay says which panel is drawn over the list.
type overlay int

const (
	overlayNone overlay = iota
	overlayHelp
	overlayConfirm
)

// change is what a confirmation will carry out. It is not an Action: none of
// these leave the finder, they happen under it.
type change int

const (
	changeNone change = iota
	changeCreate
	changeRemove
	changeAddWorktree
	changeRemoveWorktree
	changeCheckOut   // a worktree for a branch in the branch list, then go there
	changeCheckOutPR // a worktree for a pull request, made by gh, then go there
)

// pending is the change a confirmation is waiting on. Nothing has happened
// yet when one is on screen.
type pending struct {
	kind   change
	arg    string   // the path to remove, the reference to create, the branch to check out
	dir    string   // where a worktree will go, or which one goes away
	from   string   // the remote branch a new branch starts at, "origin/feature"
	title  string   // "remove", "create worktree"
	detail []string // what it will do, a line each
	force  bool     // there is work in it and the user has been told
}

// doneMsg carries the outcome of a confirmed change back to the UI thread.
type doneMsg struct {
	kind  change
	label string // how the new row reads, if one was made
	path  string
	err   error
}

// overlay draws a panel over the list, centred, leaving what is behind it
// visible around the edges.
func (m model) overlay(base, box string) string {
	x := max((m.w-lipgloss.Width(box))/2, 0)
	y := max((m.h-lipgloss.Height(box))/2, 0)
	// A Layer draws only its own content; the Compositor is what reads the
	// positions and the z-order and puts one over the other.
	return lipgloss.NewCanvas(m.w, m.h).
		Compose(lipgloss.NewCompositor(
			lipgloss.NewLayer(base),
			lipgloss.NewLayer(box).X(x).Y(y).Z(1),
		)).
		Render()
}

// perform carries out a confirmed change, off the UI thread.
func (m model) perform(a pending) tea.Cmd {
	tree, repoAt := m.tree, m.repoAt
	return func() tea.Msg {
		done := func(path string, label string, err error) tea.Msg {
			return doneMsg{kind: a.kind, path: path, label: label, err: err}
		}
		switch a.kind {
		case changeRemove:
			r, ok := tree.At(a.arg)
			if !ok {
				return done("", "", fmt.Errorf("%s is not under any root", a.arg))
			}
			if err := repo.Delete(r); err != nil {
				return done("", "", err)
			}
			return done(r.Path(), r.Rel, nil)

		case changeCreate:
			r, err := tree.Create(a.arg, false)
			if err != nil {
				return done("", "", err)
			}
			return done(r.Path(), r.Rel, nil)

		case changeAddWorktree, changeCheckOut:
			if err := repo.AddWorktreeFrom(repoAt, a.dir, a.arg, a.from); err != nil {
				return done("", "", err)
			}
			return done(a.dir, a.arg, nil)

		case changeCheckOutPR:
			n, err := strconv.Atoi(a.arg)
			if err == nil {
				err = repo.CheckOutPullRequest(repoAt, a.dir, n)
			}
			if err != nil {
				return done("", "", err)
			}
			return done(a.dir, a.arg, nil)

		case changeRemoveWorktree:
			if err := repo.RemoveWorktree(repoAt, a.dir, a.force); err != nil {
				return done("", "", err)
			}
			return done(a.dir, "", nil)
		}
		return nil
	}
}

// confirm puts the question on screen. Nothing happens until it is answered.
func (m model) confirm(p pending) model {
	m.over, m.ask, m.note = overlayConfirm, p, ""
	return m
}

// boxWidth is how wide a panel may be: narrow enough to leave the list
// visible around it, and never wider than the window.
func (m model) boxWidth() int {
	return max(min(m.w-10, 76), 20)
}

// confirmBox asks before anything changes, and says exactly what will. It is
// narrower than the help panel: a question should not blot out the list it is
// asking about.
func (m model) confirmBox() string {
	w := min(m.boxWidth(), 56)
	rows := []string{m.st.Label.Render(m.ask.title)}
	for _, d := range m.ask.detail {
		rows = append(rows, wrapSegs([]seg{{d, m.st.Subject}}, w, "  ")...)
	}
	rows = append(rows, "",
		m.st.RefLocal.Render("y")+m.st.Subject.Render(" do it")+
			m.st.Dim.Render("   ")+m.st.RefLocal.Render("n")+m.st.Subject.Render(" cancel"))
	return m.st.Box.Padding(0, 1).Render(strings.Join(rows, "\n"))
}

// helpBox draws the command list as a panel.
func (m model) helpBox() string {
	rows := make([]string, 0, len(commands)+2)
	rows = append(rows, m.st.Label.Render("commands"), "")

	names := 0
	for _, c := range commands {
		names = max(names, lipgloss.Width(c.label()))
	}
	// The descriptions fold under themselves rather than push the panel wider
	// than the window.
	w := max(m.boxWidth()-names-2, 20)
	for _, c := range commands {
		pad := strings.Repeat(" ", names-lipgloss.Width(c.label()))
		lines := wrapSegs([]seg{{c.what, m.st.Subject}}, w, "")
		rows = append(rows, m.st.RefLocal.Render(c.label())+pad+"  "+lines[0])
		for _, extra := range lines[1:] {
			rows = append(rows, strings.Repeat(" ", names+2)+extra)
		}
	}
	rows = append(rows, "", m.st.Dim.Render("q or esc closes this"))

	return m.st.Box.Padding(0, 1).Render(strings.Join(rows, "\n"))
}

// confirmWorktree asks about checking a branch out beside the repository.
// The path is gm's to decide, so the branch name is all it needs.
func (m model) confirmWorktree(branch string) (model, tea.Cmd) {
	dir, why := m.worktreeFor(branch)
	if why != "" {
		m.note = why
		return m, nil
	}

	start := "new branch"
	if repo.BranchExists(m.repoAt, branch) {
		start = "existing branch"
	}
	return m.confirm(pending{
		kind:   changeAddWorktree,
		arg:    branch,
		dir:    dir,
		title:  "create worktree",
		detail: []string{branch + ", " + start, tildify(dir)},
	}), nil
}

// panelKey is the whole keyboard while a panel is up. Everything it does not
// answer to is swallowed, so nothing moves behind it.
func (m model) panelKey(msg tea.KeyPressMsg) (tea.Model, tea.Cmd) {
	switch m.over {
	case overlayHelp:
		switch msg.String() {
		case "q", "esc", "ctrl+c", "enter":
			m.over = overlayNone
		}

	case overlayConfirm:
		switch msg.String() {
		case "y", "Y":
			a := m.ask
			m.over, m.ask = overlayNone, pending{}
			return m, m.perform(a)
		case "n", "N", "q", "esc", "ctrl+c":
			m.over, m.ask, m.note = overlayNone, pending{}, "cancelled"
		}
	}
	return m, nil
}
