package finder

import (
	"fmt"
	"strconv"

	tea "charm.land/bubbletea/v2"

	"github.com/jedipunkz/gm/internal/repo"
)

// prsMsg carries gh's answer about one repository back to the UI thread.
type prsMsg struct {
	path string // the repository it was asked about
	prs  []repo.PullRequest
	err  error
}

// openPRs asks gh for the selected repository's open pull requests, off the
// UI thread: it goes to GitHub, and the finder stays usable while it does.
func (m model) openPRs() (tea.Model, tea.Cmd) {
	it, ok := m.current()
	if !ok || m.mode != modeRepos {
		return m, nil
	}
	busy := m.startBusy("asking GitHub about " + it.label + "…")
	prsOf, path := m.prsOf, it.path
	return m, tea.Batch(busy, func() tea.Msg {
		prs, err := prsOf(path)
		return prsMsg{path: path, prs: prs, err: err}
	})
}

// showPRs puts the answer on screen, if the repository it is about is still
// the one selected in the repository list; otherwise the user has moved on.
func (m model) showPRs(msg prsMsg) (tea.Model, tea.Cmd) {
	m.busy = ""
	it, ok := m.current()
	if !ok || m.mode != modeRepos || it.path != msg.path {
		return m, nil
	}
	if msg.err != nil {
		m.note = msg.err.Error()
		return m, nil
	}
	if len(msg.prs) == 0 {
		m.note = "no open pull requests in " + it.label
		return m, nil
	}

	// A pull request is already checked out when its branch is, or when its
	// worktree is where gm files it. A fork's branch is matched by place
	// only: its main is not the repository's main.
	byBranch, byPath := map[string]string{}, map[string]bool{}
	if wts, err := m.worktreesOf(it.path); err == nil {
		for _, w := range wts {
			byPath[w.Path] = true
			if w.Branch != "" {
				byBranch[w.Branch] = w.Path
			}
		}
	}
	r, inTree := m.tree.At(it.path)
	at := func(p repo.PullRequest) string {
		if !p.Fork {
			if path, ok := byBranch[p.Branch]; ok {
				return path
			}
		}
		if inTree && repo.ValidBranch(p.Checkout()) {
			if dir := m.tree.WorktreeDir(r, p.Checkout()); byPath[dir] {
				return dir
			}
		}
		return ""
	}

	// gh lists the newest first; reversed, it lands at the bottom next to the
	// cursor.
	items := make([]item, 0, len(msg.prs))
	for i := len(msg.prs) - 1; i >= 0; i-- {
		p := msg.prs[i]
		items = append(items, item{label: p.Label(), path: at(p), pr: p})
	}
	m = m.replaceList(modePRs, it, items)
	return m, m.loadStatus()
}

// checkOutPR is Enter in the pull request list: go to the pull request's
// worktree, having gh make it first when there is none.
func (m model) checkOutPR() (model, tea.Cmd) {
	it, ok := m.current()
	if !ok {
		return m, nil
	}
	if it.path != "" {
		m.result = Result{Action: ActionJump, Arg: it.path}
		return m, tea.Quit
	}
	dir, why := m.worktreeFor(it.pr.Checkout())
	if why != "" {
		m.note = why
		return m, nil
	}
	busy := m.startBusy(fmt.Sprintf("checking #%d out at %s…", it.pr.Number, tildify(dir)))
	return m, tea.Batch(busy, m.perform(pending{kind: changeCheckOutPR, arg: strconv.Itoa(it.pr.Number), dir: dir}))
}
