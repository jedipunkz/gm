package finder

import (
	"os"
	"strconv"

	tea "charm.land/bubbletea/v2"

	"github.com/jedipunkz/gm/internal/repo"
)

// openBranches replaces the repository list with the branches of the selected
// repository, local ones and those only a remote has. A branch that is checked
// out somewhere carries that worktree's path, so Enter can simply go there.
//
// The list is what git already knows, so it is up at once; the remotes are
// asked in the background for what the last fetch did not bring.
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
	busy := m.startBusy("asking the remotes of " + it.label + " for their branches…")
	remoteOf, path := m.remoteBranchesOf, it.path
	return m, tea.Batch(m.loadStatus(), busy, func() tea.Msg {
		bs, err := remoteOf(path)
		return remoteBranchesMsg{path: path, branches: bs, err: err}
	})
}

// remoteBranchesMsg carries what the remotes of one repository have that the
// last fetch did not bring.
type remoteBranchesMsg struct {
	path     string
	branches []repo.Branch
	err      error
}

// addRemoteBranches puts the remotes' answer into the branch list it was asked
// for, at the top: nothing says how recent they are. The selection stays on
// the row it was on.
func (m model) addRemoteBranches(msg remoteBranchesMsg) (tea.Model, tea.Cmd) {
	m.busy = ""
	if m.mode != modeBranches || m.repoAt != msg.path {
		return m, nil
	}
	// Skip what the list already has: a local branch hides its remote
	// namesake, and a second answer for the same list adds nothing.
	known := map[string]bool{}
	for _, it := range m.all {
		if it.branch.Remote == "" {
			known[it.branch.Name] = true
		} else {
			known[it.branch.Remote] = true
		}
	}
	var added []item
	for _, b := range msg.branches {
		if known[b.Remote] || known[b.Name] {
			continue
		}
		known[b.Remote] = true
		added = append(added, item{label: b.Label(), branch: b})
	}
	if len(added) > 0 {
		keep := ""
		if it, ok := m.current(); ok {
			keep = it.label
		}
		m.all = append(added, m.all...)
		m.view = nil
		m.filter()
		for i, idx := range m.view {
			if m.all[idx].label == keep {
				m.cursor = i
			}
		}
	}
	if msg.err != nil {
		m.note = msg.err.Error()
	}
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
	dir, why := m.worktreeFor(it.branch.Name)
	if why != "" {
		m.note = why
		return m, nil
	}
	what := "checking " + it.branch.Name + " out at " + tildify(dir) + "…"
	if it.branch.Unfetched {
		what = "fetching " + it.branch.Remote + " and checking it out at " + tildify(dir) + "…"
	}
	busy := m.startBusy(what)
	return m, tea.Batch(busy, m.perform(pending{
		kind: changeCheckOut, arg: it.branch.Name, from: it.branch.Remote, fetch: it.branch.Unfetched, dir: dir,
	}))
}

// worktreeFor is where a new worktree filed under name goes in the repository
// the list belongs to, or why one cannot go there. The name is checked before
// a path is built from it: a branch name comes from whoever pushed it.
func (m model) worktreeFor(name string) (dir, why string) {
	if !repo.ValidBranch(name) {
		return "", strconv.Quote(name) + " is not a branch name"
	}
	r, ok := m.tree.At(m.repoAt)
	if !ok {
		return "", m.repoAt + " is not under any root"
	}
	dir = m.tree.WorktreeDir(r, name)
	if _, err := os.Stat(dir); err == nil {
		return "", tildify(dir) + " already exists"
	}
	return dir, ""
}
