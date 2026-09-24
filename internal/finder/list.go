package finder

import (
	"sort"
	"strings"

	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"
	"github.com/sahilm/fuzzy"

	"github.com/jedipunkz/gm/internal/repo"
)

// item is one row: a repository in the main list, a worktree in the Ctrl-W
// list. label is what is drawn and matched, path is what Enter yields.
type item struct {
	label string
	path  string
	score float64    // frecency; zero for worktrees
	seen  repo.Visit // the visit log; empty for worktrees
	// branch is the branch a row of the branch list stands for. Such a row
	// has a path only when the branch is already checked out somewhere.
	branch repo.Branch
	// pr is the pull request a row of the pull request list stands for, with
	// a path the same way.
	pr repo.PullRequest
}

type source []item

func (s source) String(i int) string { return s[i].label }
func (s source) Len() int            { return len(s) }

// dirtyMsg carries the result of a scan back to the UI thread.
type dirtyMsg map[string]bool

func (m *model) filter() {
	m.matched = map[int][]int{}
	m.note = ""
	// A command is not a query: the list stays as the query before it left it.
	q, _, _ := splitInput(strings.TrimSpace(m.input.Value()))
	q = strings.TrimSpace(q)
	// The selection follows the item, not the row number, whenever the query
	// is not making a new ranking statement — a command being typed, or the
	// query being cleared. Clearing it has to hold the selection still, or
	// there is no way to find a repository and then act on it.
	hold := m.view != nil && (q == m.query || q == "")
	held := -1
	if hold && m.cursor >= 0 && m.cursor < len(m.view) {
		held = m.view[m.cursor]
	}
	m.query = q
	if q == "" {
		m.view = make([]int, len(m.all))
		for i := range m.all {
			m.view[i] = i
		}
	} else {
		lower := strings.ToLower(q)
		matches := fuzzy.FindFrom(q, source(m.all))
		score := make(map[int]float64, len(matches))
		idx := make([]int, 0, len(matches))
		for _, mt := range matches {
			rel := m.all[mt.Index].label
			// fuzzy finds candidates; where it landed the characters is its
			// own guess, and a literal hit beats that guess every time.
			pos := substringMatch(rel, lower)
			if pos == nil {
				pos = mt.MatchedIndexes
			}
			score[mt.Index] = matchScore(rel, pos)
			m.matched[mt.Index] = runeIndexes(rel, pos)
			idx = append(idx, mt.Index)
		}
		// Ascending, so the strongest match lands at the bottom; frecency
		// breaks ties between equally good matches.
		sort.SliceStable(idx, func(a, b int) bool {
			if score[idx[a]] != score[idx[b]] {
				return score[idx[a]] < score[idx[b]]
			}
			return m.all[idx[a]].score < m.all[idx[b]].score
		})
		m.view = idx
	}
	m.view = m.keepDirty(m.view)

	if held >= 0 {
		for i, idx := range m.view {
			if idx == held {
				m.cursor = i
				return
			}
		}
	}
	m.cursor = len(m.view) - 1
}

// drop takes a removed repository out of the list, so the screen matches the
// disk without walking the tree again.
func (m *model) drop(path string) {
	kept := make([]item, 0, len(m.all))
	for _, it := range m.all {
		if it.path != path {
			kept = append(kept, it)
		}
	}
	m.all = kept
	delete(m.status, path)
	delete(m.probing, path)
	m.view = nil
	m.filter()
}

// add puts a new repository at the bottom of the list and selects it: it is
// the one thing the user is certain to want next.
func (m *model) add(rel, path string) {
	m.all = append(m.all, item{label: rel, path: path})
	m.view = nil
	m.input.SetValue("")
	m.filter()
	for i, idx := range m.view {
		if m.all[idx].path == path {
			m.cursor = i
		}
	}
}

// keepDirty drops the rows that have no uncommitted work, when the filter is
// on. It applies to repositories only: the worktree list is a different
// question, and a scan that has not finished yet hides nothing.
func (m model) keepDirty(view []int) []int {
	if !m.dirtyOnly || m.mode != modeRepos || m.dirty == nil {
		return view
	}
	kept := make([]int, 0, len(view))
	for _, i := range view {
		if m.dirty[m.all[i].path] {
			kept = append(kept, i)
		}
	}
	return kept
}

// scanDirty asks git about every repository at once, off the UI thread.
func (m model) scanDirty() tea.Cmd {
	paths := make([]string, 0, len(m.all))
	for _, it := range m.all {
		paths = append(paths, it.path)
	}
	scan := m.dirtyOf
	return func() tea.Msg { return dirtyMsg(scan(paths)) }
}

// runeIndexes converts fuzzy's byte offsets into rune positions, which is what
// rendering counts in.
func runeIndexes(s string, byteIdx []int) []int {
	if len(byteIdx) == 0 {
		return nil
	}
	want := make(map[int]bool, len(byteIdx))
	for _, b := range byteIdx {
		want[b] = true
	}
	out := make([]int, 0, len(byteIdx))
	ri := 0
	for bi := range s {
		if want[bi] {
			out = append(out, ri)
		}
		ri++
	}
	return out
}

// renderRow draws one repository, highlighting the characters the query
// matched and, when selected, the whole line.
func (m model) renderRow(i int, selected bool, width int) string {
	it := m.all[m.view[i]]
	base, hit, marker := m.st.Row, m.st.Hit, "  "
	if selected {
		base, hit, marker = m.st.RowSel, m.st.HitSel, m.st.Marker.Render("▸ ")
	}

	label := highlight(it.label, m.matched[m.view[i]], width-2, base, hit)
	gap := width - 2 - lipgloss.Width(label)
	if gap > 0 {
		label += base.Render(strings.Repeat(" ", gap))
	}
	return marker + label
}

// highlight renders s truncated to width, with the runes at hits in their own
// style. The tail is kept: the repository name matters more than the host.
func highlight(s string, hits []int, width int, base, hit lipgloss.Style) string {
	runes := []rune(s)
	off, prefix := 0, ""
	if width <= 1 {
		return ""
	}
	if len(runes) > width {
		off = len(runes) - width + 1
		prefix = "…"
		runes = runes[off:]
	}
	isHit := make(map[int]bool, len(hits))
	for _, h := range hits {
		isHit[h-off] = true
	}

	var b strings.Builder
	if prefix != "" {
		b.WriteString(base.Render(prefix))
	}
	// Emit runs of same-styled runes so one escape sequence covers many cells.
	for i := 0; i < len(runes); {
		j, on := i, isHit[i]
		for j < len(runes) && isHit[j] == on {
			j++
		}
		st := base
		if on {
			st = hit
		}
		b.WriteString(st.Render(string(runes[i:j])))
		i = j
	}
	return b.String()
}
