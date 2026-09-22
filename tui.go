package main

import (
	"fmt"
	"os"
	"sort"
	"strings"
	"time"

	"charm.land/bubbles/v2/textinput"
	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"
	"github.com/sahilm/fuzzy"
)

// The best match sits at the BOTTOM of the list, next to the prompt and the
// cursor's resting place, so the most likely repository needs zero keystrokes.

func fg(hex string) lipgloss.Style {
	return lipgloss.NewStyle().Foreground(lipgloss.Color(hex))
}

// Set by applyTheme; see theme.go.
var (
	styleRow     lipgloss.Style
	styleRowSel  lipgloss.Style
	styleHit     lipgloss.Style
	styleHitSel  lipgloss.Style
	styleMarker  lipgloss.Style
	styleDivider lipgloss.Style
	styleLabel   lipgloss.Style
	styleName    lipgloss.Style
	stylePath    lipgloss.Style
	styleRemote  lipgloss.Style
	styleBranch  lipgloss.Style
	styleCommit  lipgloss.Style
	styleClean   lipgloss.Style
	styleDirty   lipgloss.Style
	styleVisits  lipgloss.Style
	styleDim     lipgloss.Style
	styleBox     lipgloss.Style
)

type item struct {
	repo  Repo
	score float64
	seen  visit
}

type source []item

func (s source) String(i int) string { return s[i].repo.Rel }
func (s source) Len() int            { return len(s) }

type repoInfo struct {
	remote, branch, commit string
	dirty                  int
}

type infoMsg struct {
	path string
	info repoInfo
}

type model struct {
	all     []item        // ascending by frecency: the best is last
	view    []int         // indices into all, same convention
	matched map[int][]int // item index -> matched rune positions
	cursor  int           // index into view
	input   textinput.Model
	info    map[string]repoInfo
	w, h    int
	chosen  string
}

func newModel(repos []Repo) model {
	freq := LoadFrecency()
	now := time.Now()
	items := make([]item, 0, len(repos))
	for _, r := range repos {
		v := freq[r.Path()]
		items = append(items, item{repo: r, score: v.Score(now), seen: v})
	}
	sort.Slice(items, func(i, j int) bool {
		if items[i].score != items[j].score {
			return items[i].score < items[j].score
		}
		return items[i].repo.Rel < items[j].repo.Rel
	})

	in := textinput.New()
	in.Prompt = "❯ "
	in.Placeholder = "filter"
	in.SetVirtualCursor(true)
	in.Focus()
	st := textinput.DefaultDarkStyles()
	if theme.light {
		st = textinput.DefaultLightStyles()
	}
	st.Focused.Prompt = fg(theme.blue)
	st.Focused.Text = fg(theme.fg)
	st.Focused.Placeholder = fg(theme.comment)
	in.SetStyles(st)

	m := model{all: items, input: in, info: map[string]repoInfo{}, w: 80, h: 24}
	m.filter()
	return m
}

func (m model) Init() tea.Cmd {
	return tea.Batch(textinput.Blink, m.loadInfo())
}

func (m *model) filter() {
	m.matched = map[int][]int{}
	q := strings.TrimSpace(m.input.Value())
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
			rel := m.all[mt.Index].repo.Rel
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
	m.cursor = len(m.view) - 1
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

func (m model) current() (item, bool) {
	if m.cursor < 0 || m.cursor >= len(m.view) {
		return item{}, false
	}
	return m.all[m.view[m.cursor]], true
}

// loadInfo asks git about the selected repository off the UI thread.
func (m model) loadInfo() tea.Cmd {
	it, ok := m.current()
	if !ok {
		return nil
	}
	p := it.repo.Path()
	if _, done := m.info[p]; done {
		return nil
	}
	return func() tea.Msg {
		var i repoInfo
		i.branch, _ = capture(p, "git", "rev-parse", "--abbrev-ref", "HEAD")
		i.remote, _ = capture(p, "git", "remote", "get-url", "origin")
		i.commit, _ = capture(p, "git", "log", "-1", "--format=%h  %cr  %s")
		if st, err := capture(p, "git", "status", "--porcelain"); err == nil && st != "" {
			i.dirty = len(strings.Split(st, "\n"))
		}
		return infoMsg{path: p, info: i}
	}
}

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.w, m.h = msg.Width, msg.Height
		return m, nil

	case infoMsg:
		m.info[msg.path] = msg.info
		return m, nil

	case tea.KeyPressMsg:
		switch msg.String() {
		case "ctrl+c", "esc":
			return m, tea.Quit
		case "enter":
			if it, ok := m.current(); ok {
				m.chosen = it.repo.Path()
			}
			return m, tea.Quit
		case "down", "ctrl+n":
			if m.cursor < len(m.view)-1 {
				m.cursor++
			}
			return m, m.loadInfo()
		case "up", "ctrl+p":
			if m.cursor > 0 {
				m.cursor--
			}
			return m, m.loadInfo()
		}
	}

	var cmd tea.Cmd
	before := m.input.Value()
	m.input, cmd = m.input.Update(msg)
	if m.input.Value() != before {
		m.filter()
	}
	return m, tea.Batch(cmd, m.loadInfo())
}

func (m model) View() tea.View {
	rows := m.h - 3 // the bordered input box
	if rows < 3 {
		rows = 3
	}
	// The list gets the left 3/5: it is what gets scanned.
	listW, infoW := m.w, 0
	if m.w >= 66 {
		infoW = m.w * 2 / 5
		listW = m.w - infoW - 3
	}

	// Window over the view, anchored so the cursor stays visible.
	start := len(m.view) - rows
	if m.cursor < start {
		start = m.cursor
	}
	if start < 0 {
		start = 0
	}
	end := min(start+rows, len(m.view))

	lines := make([]string, 0, rows)
	for i := 0; i < rows-(end-start); i++ {
		lines = append(lines, strings.Repeat(" ", listW)) // the list hangs from the bottom
	}
	for i := start; i < end; i++ {
		lines = append(lines, m.renderRow(i, i == m.cursor, listW))
	}

	var info []string
	if infoW > 0 {
		info = m.infoLines(infoW)
	}
	// Bottom-align the info pane so it sits beside the selection, not adrift
	// at the top of the screen. A pane taller than the window loses its tail
	// rather than its name and path.
	if len(info) > len(lines) {
		info = info[:len(lines)]
	} else if pad := len(lines) - len(info); pad > 0 {
		info = append(make([]string, pad), info...)
	}

	var b strings.Builder
	for i := range lines {
		if infoW == 0 {
			b.WriteString(lines[i] + "\n")
			continue
		}
		b.WriteString(lines[i] + styleDivider.Render(" │ ") + info[i] + "\n")
	}
	b.WriteString(styleBox.Width(m.w - 2).Render(m.input.View()))

	v := tea.NewView(b.String())
	v.AltScreen = true
	return v
}

// renderRow draws one repository, highlighting the characters the query
// matched and, when selected, the whole line.
func (m model) renderRow(i int, selected bool, width int) string {
	it := m.all[m.view[i]]
	base, hit, marker := styleRow, styleHit, "  "
	if selected {
		base, hit, marker = styleRowSel, styleHitSel, styleMarker.Render("▸ ")
	}

	label := highlight(it.repo.Rel, m.matched[m.view[i]], width-2, base, hit)
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

// infoLines stacks each field's label above its value, so a long path or
// remote URL gets the pane's full width instead of what a label column leaves.
func (m model) infoLines(w int) []string {
	it, ok := m.current()
	if !ok {
		return []string{styleDim.Render("no match")}
	}

	var out []string
	field := func(k, v string, st lipgloss.Style) {
		if v == "" {
			v = "-"
		}
		out = append(out, styleLabel.Render(k), st.Render(trunc(v, w)), "")
	}

	out = append(out, styleName.Render(trunc(it.repo.Rel, w)), "")
	field("path", tildify(it.repo.Path()), stylePath)

	i, loaded := m.info[it.repo.Path()]
	if !loaded {
		field("git", "loading…", styleDim)
		return out[:len(out)-1]
	}
	field("remote", i.remote, styleRemote)
	field("branch", i.branch, styleBranch)
	field("commit", i.commit, styleCommit)
	if i.dirty > 0 {
		field("status", fmt.Sprintf("%d changed", i.dirty), styleDirty)
	} else {
		field("status", "clean", styleClean)
	}
	if it.seen.Count > 0 {
		field("visits", fmt.Sprintf("%d, last %s", it.seen.Count, ago(time.Unix(it.seen.Last, 0))), styleVisits)
	} else {
		field("visits", "never", styleDim)
	}
	return out[:len(out)-1] // no trailing blank line
}

func tildify(p string) string {
	home, err := os.UserHomeDir()
	if err != nil || !strings.HasPrefix(p, home+"/") {
		return p
	}
	return "~" + strings.TrimPrefix(p, home)
}

func ago(t time.Time) string {
	d := time.Since(t)
	switch {
	case d < time.Minute:
		return "just now"
	case d < time.Hour:
		return fmt.Sprintf("%dm ago", int(d.Minutes()))
	case d < 24*time.Hour:
		return fmt.Sprintf("%dh ago", int(d.Hours()))
	default:
		return fmt.Sprintf("%dd ago", int(d.Hours()/24))
	}
}

func trunc(s string, w int) string {
	if w <= 1 {
		return ""
	}
	r := []rune(s)
	if len(r) <= w {
		return s
	}
	return "…" + string(r[len(r)-w+1:])
}
