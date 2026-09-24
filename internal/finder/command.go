package finder

import (
	"fmt"
	"sort"
	"strconv"
	"strings"

	tea "charm.land/bubbletea/v2"

	"github.com/jedipunkz/gm/internal/repo"
)

// A slash command is typed into the same box as the filter. Only a slash at
// the very start of an empty query begins one — a repository path is full of
// slashes, and every one of those has to keep filtering.
const commandPrefix = "/"

// commandSep ends a query so a command can follow it — "gm;/remove" finds a
// repository and acts on it in one go. No repository path holds one.
const commandSep = ";"

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
		if m.mode != modeRepos {
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
		if m.mode == modeBranches || m.mode == modePRs {
			m.note = "/remove applies to repositories and worktrees"
			return m, nil
		}
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
		// The worktrees go with it, so the question has to say so.
		if wts, err := m.worktreesOf(it.path); err == nil {
			var labels []string
			for _, w := range wts {
				// The main worktree is the repository itself. git prints the
				// resolved path, so a mismatch here is one entry too many
				// rather than a wrong answer.
				if !repo.SamePath(w.Path, it.path) {
					labels = append(labels, w.Label())
				}
			}
			if len(labels) > 0 {
				detail = append(detail, fmt.Sprintf("%s will go too: %s",
					plural(len(labels), "worktree", "worktrees"), strings.Join(labels, ", ")))
			}
		}
		return m.confirm(pending{
			kind:   changeRemove,
			arg:    it.path,
			title:  "remove repository",
			detail: detail,
		}), nil
	}},
	{"/worktrees", "", "list the worktrees of the selected repository", func(m model, _ string) (model, tea.Cmd) {
		if m.mode == modeWorktrees {
			return m, nil
		}
		next, cmd := m.switchTo(modeWorktrees)
		return next.(model), cmd
	}},
	{"/branches", "", "list the branches of the selected repository", func(m model, _ string) (model, tea.Cmd) {
		if m.mode == modeBranches {
			return m, nil
		}
		next, cmd := m.switchTo(modeBranches)
		return next.(model), cmd
	}},
	{"/prs", "", "list the open pull requests of the repository", func(m model, _ string) (model, tea.Cmd) {
		if m.mode == modePRs {
			return m, nil
		}
		next, cmd := m.switchTo(modePRs)
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
			busy := m.startBusy("checking every repository for uncommitted work…")
			return m, tea.Batch(busy, m.scanDirty())
		}
		m.filter()
		m.cursor = len(m.view) - 1
		return m, nil
	}},
}

// plural counts a thing in the words for it.
func plural(n int, one, many string) string {
	if n == 1 {
		return "1 " + one
	}
	return strconv.Itoa(n) + " " + many
}

func commandNames() []string {
	names := make([]string, 0, len(commands))
	for _, c := range commands {
		names = append(names, c.name)
	}
	sort.Strings(names)
	return names
}

// splitInput tells the query in the box from a command typed with it: a
// command either is the whole input, or follows the query after commandSep.
func splitInput(s string) (query, cmd string, ok bool) {
	if strings.HasPrefix(s, commandPrefix) {
		return "", s, true
	}
	return strings.Cut(s, commandSep)
}

// isCommand reports whether the input is being typed as a command rather than
// only as a filter.
func isCommand(s string) bool {
	_, _, ok := splitInput(s)
	return ok
}

// completions offers the command names as the whole box would read with them,
// the query in front of the separator included.
func completions(s string) []string {
	names := commandNames()
	if q, _, ok := splitInput(s); ok && q != "" {
		for i, n := range names {
			names[i] = q + commandSep + n
		}
	}
	return names
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
