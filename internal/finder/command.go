package finder

import (
	"fmt"
	"os"
	"sort"
	"strconv"
	"strings"

	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"

	"github.com/jedipunkz/gm/internal/repo"
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
		m.over = overlayHelp
		return m, nil
	}},
	{"/create", "<repo>|<branch>", "create a repository, or a worktree", func(m model, arg string) (model, tea.Cmd) {
		if m.mode == modeWorktrees {
			return m.confirmWorktree(arg)
		}
		u, err := repo.NormalizeURL(arg, false)
		if err != nil {
			m.note = err.Error()
			return m, nil
		}
		dst := m.tree.PathFor(repo.RelPathOf(u))
		return m.confirm(pending{
			kind:   changeCreate,
			arg:    arg,
			title:  "create repository",
			detail: []string{tildify(dst), "origin " + u.String()},
		}), nil
	}},
	{"/get", "<repo>", "clone a repository, then go there", func(m model, arg string) (model, tea.Cmd) {
		return m.leaveWith(ActionGet, arg)
	}},
	{"/remove", "", "remove the selected repository or worktree", func(m model, _ string) (model, tea.Cmd) {
		it, ok := m.current()
		if !ok {
			m.note = "nothing is selected"
			return m, nil
		}
		// The status of the selected row is already loaded, so the warning
		// costs nothing and is the one thing worth knowing before saying yes.
		dirty := 0
		if s, ok := m.status[it.path]; ok {
			dirty = s.Dirty
		}
		detail := []string{tildify(it.path)}
		if dirty > 0 {
			detail = append(detail, fmt.Sprintf("%d uncommitted changes will be lost", dirty))
		}

		if m.mode == modeWorktrees {
			if it.path == m.repoAt {
				m.note = "that is the repository itself, not a worktree of it"
				return m, nil
			}
			return m.confirm(pending{
				kind:   changeRemoveWorktree,
				dir:    it.path,
				title:  "remove worktree " + it.label,
				detail: detail,
				force:  dirty > 0,
			}), nil
		}
		return m.confirm(pending{
			kind:   changeRemove,
			arg:    it.path,
			title:  "remove repository",
			detail: detail,
		}), nil
	}},
	{"/worktrees", "", "list the worktrees of the selected repository", func(m model, _ string) (model, tea.Cmd) {
		next, cmd := m.openWorktrees()
		return next.(model), cmd
	}},
	{"/remote", "", "open the selected repository's remote in a browser", func(m model, _ string) (model, tea.Cmd) {
		return m, m.openRemote()
	}},
	{"/dirty", "", "show only repositories with uncommitted work", func(m model, _ string) (model, tea.Cmd) {
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

// confirmWorktree asks about checking a branch out beside the repository.
// The path is gm's to decide, so the branch name is all it needs.
func (m model) confirmWorktree(branch string) (model, tea.Cmd) {
	if !repo.ValidBranch(branch) {
		m.note = strconv.Quote(branch) + " is not a branch name"
		return m, nil
	}
	r, ok := m.tree.At(m.repoAt)
	if !ok {
		m.note = m.repoAt + " is not under any root"
		return m, nil
	}
	dir := m.tree.WorktreeDir(r, branch)
	if _, err := os.Stat(dir); err == nil {
		m.note = tildify(dir) + " already exists"
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

// confirm puts the question on screen. Nothing happens until it is answered.
func (m model) confirm(p pending) model {
	m.over, m.ask, m.note = overlayConfirm, p, ""
	return m
}

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
