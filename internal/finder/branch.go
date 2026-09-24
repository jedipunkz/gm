package finder

import (
	"os"

	tea "charm.land/bubbletea/v2"
)

// openBranches replaces the repository list with the branches of the selected
// repository, local ones and those only a remote has. A branch that is checked
// out somewhere carries that worktree's path, so Enter can simply go there.
func (m model) openBranches() (tea.Model, tea.Cmd) {
	it, ok := m.current()
	if !ok || m.mode != modeRepos {
		return m, nil
	}
	bs, err := m.branchesOf(it.path)
	if err != nil || len(bs) == 0 {
		return m, nil
	}
	at := map[string]string{}
	if wts, err := m.worktreesOf(it.path); err == nil {
		for _, w := range wts {
			if w.Branch != "" {
				at[w.Branch] = w.Path
			}
		}
	}

	// Reversed, so the branch with the newest commit lands at the bottom next
	// to the cursor.
	items := make([]item, 0, len(bs))
	for i := len(bs) - 1; i >= 0; i-- {
		items = append(items, item{label: bs[i].Label(), path: at[bs[i].Name], branch: bs[i]})
	}
	m = m.replaceList(modeBranches, it, items)
	return m, m.loadStatus()
}

// checkOut is Enter in the branch list: go to the branch's worktree, making
// it first when there is none. Making one is not asked about — the branch is
// what Enter was pressed on, and nothing is lost if it was the wrong one.
func (m model) checkOut() (model, tea.Cmd) {
	it, ok := m.current()
	if !ok {
		return m, nil
	}
	if it.path != "" {
		m.result = Result{Action: ActionJump, Arg: it.path}
		return m, tea.Quit
	}
	r, ok := m.tree.At(m.repoAt)
	if !ok {
		m.note = m.repoAt + " is not under any root"
		return m, nil
	}
	dir := m.tree.WorktreeDir(r, it.branch.Name)
	if _, err := os.Stat(dir); err == nil {
		m.note = tildify(dir) + " already exists"
		return m, nil
	}
	m.note = "checking " + it.branch.Name + " out at " + tildify(dir) + "…"
	return m, m.perform(pending{kind: changeCheckOut, arg: it.branch.Name, from: it.branch.Remote, dir: dir})
}
