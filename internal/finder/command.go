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
	what string
	run  func(m model) (model, tea.Cmd)
}

var commands = []command{
	{"/help", "show this list", func(m model) (model, tea.Cmd) {
		m.help = true
		return m, nil
	}},
	{"/worktrees", "list the worktrees of the selected repository", func(m model) (model, tea.Cmd) {
		next, cmd := m.openWorktrees()
		return next.(model), cmd
	}},
	{"/remote", "open the selected repository's remote in a browser", func(m model) (model, tea.Cmd) {
		return m, m.openRemote()
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

// runCommand executes what the user typed. An unknown command leaves a note
// under the prompt rather than doing something surprising.
func (m model) runCommand(typed string) (model, tea.Cmd) {
	name := strings.TrimSpace(typed)
	for _, c := range commands {
		if c.name == name {
			m.input.SetValue("")
			m.note = ""
			m.filter()
			return c.run(m)
		}
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
		width = max(width, lipgloss.Width(c.name))
	}
	for _, c := range commands {
		pad := strings.Repeat(" ", width-lipgloss.Width(c.name))
		rows = append(rows, m.st.RefLocal.Render(c.name)+pad+"  "+m.st.Subject.Render(c.what))
	}
	rows = append(rows, "", m.st.Dim.Render("q or esc closes this"))

	box := m.st.Box.Padding(0, 1).Render(strings.Join(rows, "\n"))
	return lipgloss.Place(m.w, m.h, lipgloss.Center, lipgloss.Center, box)
}
