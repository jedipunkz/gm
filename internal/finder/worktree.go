package finder

import (
	tea "charm.land/bubbletea/v2"
)

// mode says which list is on screen.
type mode int

const (
	modeRepos mode = iota
	modeWorktrees
	modeBranches
	modePRs
)

// stash is the repository list put aside while the worktree list is up, so
// Esc can put it back exactly as it was.
type stash struct {
	all     []item
	view    []int
	matched map[int][]int
	cursor  int
	query   string
}

// openWorktrees replaces the repository list with the checkouts of the
// selected repository. A repository git cannot answer for is left alone: the
// list simply does not change.
func (m model) openWorktrees() (tea.Model, tea.Cmd) {
	it, ok := m.current()
	if !ok || m.mode != modeRepos {
		return m, nil
	}
	wts, err := m.worktreesOf(it.path)
	if err != nil || len(wts) == 0 {
		return m, nil
	}

	// Reversed, so git's first worktree — the main one — lands at the bottom
	// next to the cursor, the way the best match does in the main list.
	items := make([]item, 0, len(wts))
	for i := len(wts) - 1; i >= 0; i-- {
		items = append(items, item{label: wts[i].Label(), path: wts[i].Path})
	}
	m = m.replaceList(modeWorktrees, it, items)
	return m, m.loadStatus()
}

// replaceList puts a list that belongs to the repository row it in place of
// the repository list, which is kept aside for restore.
func (m model) replaceList(md mode, it item, items []item) model {
	m.saved = &stash{all: m.all, view: m.view, matched: m.matched, cursor: m.cursor, query: m.input.Value()}
	m.origin, m.repoAt = it.label, it.path
	m.mode = md
	m.all = items
	m.input.SetValue("")
	// A different list entirely: the held selection means nothing in it.
	m.view = nil
	m.filter()
	return m
}

// switchTo answers the worktree, branch and pull request keys. Each toggles
// its own list, and goes from another one straight to its own: all of them
// belong to the repository the repository list has selected.
func (m model) switchTo(md mode) (tea.Model, tea.Cmd) {
	if m.mode == md {
		m.restore()
		return m, m.loadStatus()
	}
	m.restore()
	switch md {
	case modeBranches:
		return m.openBranches()
	case modePRs:
		return m.openPRs()
	}
	return m.openWorktrees()
}

// restore puts the repository list back, query and cursor included.
func (m *model) restore() {
	if m.saved == nil {
		return
	}
	m.all, m.view, m.matched, m.cursor = m.saved.all, m.saved.view, m.saved.matched, m.saved.cursor
	m.input.SetValue(m.saved.query)
	m.mode, m.origin, m.repoAt, m.saved = modeRepos, "", "", nil
}
