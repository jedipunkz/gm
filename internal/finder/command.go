package finder

import (
	"sort"
	"strings"

	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"
)

// A slash command is typed into the same box as the filter. Only a slash at
// the very start of an empty query begins one — a repository path is full of
// slashes, and every one of those has to keep filtering.
const commandPrefix = "/"

// command is one slash command. Adding another is one entry here; the help
// popup and the completion both read this table.
type command struct {
	name string
	arg  string // what the argument is called in the help, empty when it takes none
	what string
	run  func(m model, arg string) (model, tea.Cmd)
}

// label is how the command is written in the help popup.
func (c command) label() string {
	if c.arg == "" {
		return c.name
	}
	return c.name + " " + c.arg
}

var commands = []command{
	{"/help", "", "show this list", func(m model, _ string) (model, tea.Cmd) {
		m.help = true
		return m, nil
	}},
	{"/create", "<repo>", "create a repository with its origin set, then go there", func(m model, arg string) (model, tea.Cmd) {
		return m.leaveWith(ActionCreate, arg)
	}},
	{"/get", "<repo>", "clone a repository, then go there", func(m model, arg string) (model, tea.Cmd) {
		return m.leaveWith(ActionGet, arg)
	}},
	{"/remove", "", "remove the selected repository, after confirming", func(m model, _ string) (model, tea.Cmd) {
		if m.mode != modeRepos {
			m.note = "/remove applies to the repository list"
			return m, nil
		}
		it, ok := m.current()
		if !ok {
			m.note = "nothing is selected"
			return m, nil
		}
		return m.leaveWith(ActionRemove, it.path)
	}},
	{"/worktrees", "", "list the worktrees of the selected repository", func(m model, _ string) (model, tea.Cmd) {
		next, cmd := m.openWorktrees()
		return next.(model), cmd
	}},
	{"/remote", "", "open the selected repository's remote in a browser", func(m model, _ string) (model, tea.Cmd) {
		return m, m.openRemote()
	}},
	{"/dirty", "", "show only repositories with uncommitted work; again shows all", func(m model, _ string) (model, tea.Cmd) {
		if m.mode != modeRepos {
			m.note = "/dirty applies to the repository list"
			return m, nil
		}
		if m.dirtyOnly {
			m.dirtyOnly = false
			m.filter()
			m.cursor = len(m.view) - 1
			return m, nil
		}
		m.dirtyOnly = true
		if m.dirty == nil {
			// The answer needs a git call per repository, so it is asked for
			// the first time someone wants it, not at startup.
			m.scanning = true
			m.note = "checking every repository for uncommitted work…"
			return m, m.scanDirty()
		}
		m.filter()
		m.cursor = len(m.view) - 1
		return m, nil
	}},
}

func commandNames() []string {
	names := make([]string, 0, len(commands))
	for _, c := range commands {
		names = append(names, c.name)
	}
	sort.Strings(names)
	return names
}

// isCommand reports whether the input is being typed as a command rather than
// as a filter.
func isCommand(s string) bool { return strings.HasPrefix(s, commandPrefix) }

// leaveWith closes the finder and hands the work to gm, which runs it on the
// terminal the user can see: a clone's progress, a password prompt and a
// confirmation all belong there, not inside an alternate screen.
func (m model) leaveWith(a Action, arg string) (model, tea.Cmd) {
	m.result = Result{Action: a, Arg: arg}
	return m, tea.Quit
}

// runCommand executes what the user typed. An unknown command, or one missing
// its argument, leaves a note under the prompt rather than doing something
// surprising.
func (m model) runCommand(typed string) (model, tea.Cmd) {
	name, arg, _ := strings.Cut(strings.TrimSpace(typed), " ")
	arg = strings.TrimSpace(arg)
	for _, c := range commands {
		if c.name != name {
			continue
		}
		if c.arg != "" && arg == "" {
			m.note = name + " needs an argument: " + c.label()
			return m, nil
		}
		m.input.SetValue("")
		m.note = ""
		m.filter()
		return c.run(m, arg)
	}
	m.note = "unknown command " + name + " — /help lists them"
	return m, nil
}

// helpView draws the command list as a panel in the middle of the screen.
func (m model) helpView() string {
	rows := make([]string, 0, len(commands)+2)
	rows = append(rows, m.st.Label.Render("commands"), "")

	width := 0
	for _, c := range commands {
		width = max(width, lipgloss.Width(c.label()))
	}
	for _, c := range commands {
		pad := strings.Repeat(" ", width-lipgloss.Width(c.label()))
		rows = append(rows, m.st.RefLocal.Render(c.label())+pad+"  "+m.st.Subject.Render(c.what))
	}
	rows = append(rows, "", m.st.Dim.Render("q or esc closes this"))

	box := m.st.Box.Padding(0, 1).Render(strings.Join(rows, "\n"))
	return lipgloss.Place(m.w, m.h, lipgloss.Center, lipgloss.Center, box)
}
