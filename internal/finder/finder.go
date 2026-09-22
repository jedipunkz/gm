// Package finder is the interactive repository picker: a fuzzy filter over the
// repositories, ranked by match quality and then by how often they are used.
//
// The best match sits at the BOTTOM of the list, next to the prompt and the
// cursor's resting place, so the most likely repository needs zero keystrokes.
package finder

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

	"github.com/jedipunkz/gm/internal/config"
	"github.com/jedipunkz/gm/internal/repo"
)

// DefaultWorktreeKey opens the worktree list, and DefaultRemoteKey the
// selected repository's remote, when gm.toml says nothing.
const (
	DefaultWorktreeKey = "ctrl-w"
	DefaultRemoteKey   = "ctrl-alt-b"
)

// reserved are the Ctrl chords the finder already answers to; binding an
// action to one of them would shadow quitting or moving. Only plain Ctrl
// chords can collide: the finder's own keys carry no other modifier.
var reserved = map[byte]string{
	'c': "quit",
	'n': "move down",
	'p': "move up",
}

// Keys are the finder's configurable chords.
type Keys struct {
	Worktree config.Chord // open and close the worktree list
	Remote   config.Chord // open the selected repository's remote
}

// check refuses a binding that would shadow one of the finder's fixed keys,
// or that two actions would answer to at once.
func (k Keys) check() error {
	for _, c := range []struct {
		name  string
		chord config.Chord
	}{{"worktree_key", k.Worktree}, {"remote_key", k.Remote}} {
		if !c.chord.Plain() {
			continue // Alt or Shift can never collide with the fixed keys
		}
		if what, taken := reserved[c.chord.Letter]; taken {
			return fmt.Errorf("%s cannot be %s: the finder uses it to %s", c.name, c.chord.Display, what)
		}
	}
	if k.Worktree.Key() == k.Remote.Key() {
		return fmt.Errorf("worktree_key and remote_key are both %s", k.Worktree.Display)
	}
	return nil
}

// Run draws the finder and returns the path the user chose, or "" if they
// quit. It draws on the terminal itself, never on stdout: stdout carries the
// chosen path back to the shell binding.
func Run(repos []repo.Repo, h *repo.History, theme Theme, keys Keys) (string, error) {
	if err := keys.check(); err != nil {
		return "", err
	}
	opts := []tea.ProgramOption{tea.WithOutput(os.Stderr)}
	if tty, err := os.OpenFile("/dev/tty", os.O_RDWR, 0); err == nil {
		defer tty.Close()
		opts = []tea.ProgramOption{tea.WithInput(tty), tea.WithOutput(tty)}
	}
	res, err := tea.NewProgram(newModel(repos, h, theme, keys), opts...).Run()
	if err != nil {
		return "", err
	}
	m, ok := res.(model)
	if !ok {
		return "", nil
	}
	return m.chosen, nil
}

// item is one row: a repository in the main list, a worktree in the Ctrl-W
// list. label is what is drawn and matched, path is what Enter yields.
type item struct {
	label string
	path  string
	score float64    // frecency; zero for worktrees
	seen  repo.Visit // the visit log; empty for worktrees
}

type source []item

func (s source) String(i int) string { return s[i].label }
func (s source) Len() int            { return len(s) }

// mode says which list is on screen.
type mode int

const (
	modeRepos mode = iota
	modeWorktrees
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

// statusMsg carries one repository's git status back to the UI thread.
type statusMsg struct {
	path   string
	status repo.Status
}

type model struct {
	all     []item        // ascending by frecency: the best is last
	view    []int         // indices into all, same convention
	matched map[int][]int // item index -> matched rune positions
	cursor  int           // index into view
	input   textinput.Model
	status  map[string]repo.Status
	st      Styles
	w, h    int
	chosen  string

	mode   mode
	origin string // in worktree mode, the repository the list belongs to
	saved  *stash
	keys   Keys
	// worktreesOf is the seam the tests replace; it is repo.Worktrees in
	// every real run.
	worktreesOf func(dir string) ([]repo.Worktree, error)
}

func newModel(repos []repo.Repo, hist *repo.History, theme Theme, keys Keys) model {
	now := time.Now()
	items := make([]item, 0, len(repos))
	for _, r := range repos {
		v := hist.Visit(r.Path())
		items = append(items, item{label: r.Rel, path: r.Path(), score: v.Score(now), seen: v})
	}
	sort.Slice(items, func(i, j int) bool {
		if items[i].score != items[j].score {
			return items[i].score < items[j].score
		}
		return items[i].label < items[j].label
	})

	st := theme.Styles()
	in := textinput.New()
	in.Prompt = "❯ "
	// No placeholder: an empty prompt says "type" clearly enough, and the
	// virtual cursor sits on top of the first character of one anyway.
	in.SetVirtualCursor(true)
	in.Focus()
	ts := textinput.DefaultDarkStyles()
	if theme.Light {
		ts = textinput.DefaultLightStyles()
	}
	ts.Focused.Prompt = fg(theme.Blue)
	ts.Focused.Text = fg(theme.Fg)
	in.SetStyles(ts)

	m := model{
		all:         items,
		input:       in,
		status:      map[string]repo.Status{},
		st:          st,
		w:           80,
		h:           24,
		keys:        keys,
		worktreesOf: repo.Worktrees,
	}
	m.filter()
	return m
}

func (m model) Init() tea.Cmd {
	return tea.Batch(textinput.Blink, m.loadStatus())
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

// loadStatus asks git about the selected repository off the UI thread.
func (m model) loadStatus() tea.Cmd {
	it, ok := m.current()
	if !ok {
		return nil
	}
	p := it.path
	if _, done := m.status[p]; done {
		return nil
	}
	return func() tea.Msg {
		return statusMsg{path: p, status: repo.Describe(p)}
	}
}

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.w, m.h = msg.Width, msg.Height
		return m, nil

	case statusMsg:
		m.status[msg.path] = msg.status
		return m, nil

	case tea.KeyPressMsg:
		// The configurable keys cannot be switch cases.
		switch msg.String() {
		case m.keys.Worktree.Key():
			if m.mode == modeWorktrees {
				m.restore()
				return m, m.loadStatus()
			}
			return m.openWorktrees()
		case m.keys.Remote.Key():
			return m, m.openRemote()
		}
		switch msg.String() {
		case "ctrl+c":
			return m, tea.Quit
		case "esc", "ctrl+g":
			// These back out of the worktree list before they quit gm; from
			// the repository list Ctrl-G does nothing, since it is the key
			// that opened gm in the first place.
			if m.mode == modeWorktrees {
				m.restore()
				return m, m.loadStatus()
			}
			if msg.String() == "esc" {
				return m, tea.Quit
			}
			return m, nil
		case "enter":
			if it, ok := m.current(); ok {
				m.chosen = it.path
			}
			return m, tea.Quit
		case "down", "ctrl+n":
			if m.cursor < len(m.view)-1 {
				m.cursor++
			}
			return m, m.loadStatus()
		case "up", "ctrl+p":
			if m.cursor > 0 {
				m.cursor--
			}
			return m, m.loadStatus()
		}
	}

	var cmd tea.Cmd
	before := m.input.Value()
	m.input, cmd = m.input.Update(msg)
	if m.input.Value() != before {
		m.filter()
	}
	return m, tea.Batch(cmd, m.loadStatus())
}

func (m model) View() tea.View {
	rows := m.h - 4 // the bordered input box, plus the hint line under it
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
	// Top-align the info pane: it reads top-down, unlike the list, which hangs
	// from the prompt. A pane taller than the window loses its tail rather
	// than its name and path.
	if len(info) > len(lines) {
		info = info[:len(lines)]
	} else if pad := len(lines) - len(info); pad > 0 {
		info = append(info, make([]string, pad)...)
	}

	var b strings.Builder
	for i := range lines {
		if infoW == 0 {
			b.WriteString(lines[i] + "\n")
			continue
		}
		b.WriteString(lines[i] + m.st.Divider.Render(" │ ") + info[i] + "\n")
	}
	b.WriteString(m.st.Box.Width(m.w - 2).Render(m.input.View()))
	b.WriteString("\n " + m.helpLine(m.w-1))

	v := tea.NewView(b.String())
	v.AltScreen = true
	return v
}

// openRemote hands the selected repository's remote to a browser. A
// repository without one, or one whose remote is not a URL, does nothing
// visible: the finder is showing a list, not reporting on git.
func (m model) openRemote() tea.Cmd {
	it, ok := m.current()
	if !ok {
		return nil
	}
	path := it.path
	remote := m.status[path].Remote
	return func() tea.Msg {
		if remote == "" {
			// Not fetched yet for this row; ask git now rather than making
			// the key do nothing on the first press.
			remote = repo.Describe(path).Remote
		}
		if remote == "" {
			return nil
		}
		url, err := repo.BrowseURL(remote)
		if err != nil {
			return nil
		}
		_ = openURL(url)
		return nil
	}
}

// hint is one key and what it does, for the line under the prompt.
type hint struct{ key, what string }

// helpLine draws the key hints, dropping the ones that do not fit rather than
// wrapping onto a second line.
func (m model) helpLine(width int) string {
	wt := m.keys.Worktree.Short()
	hints := []hint{
		{"↑↓ ctrl-p/n", "move"},
		{"enter", "jump"},
		{wt, "worktrees"},
		{"esc", "quit"},
		{m.keys.Remote.Short(), "remote"},
	}
	if m.mode == modeWorktrees {
		hints = []hint{
			{"↑↓ ctrl-p/n", "move"},
			{"enter", "jump"},
			{wt + "/g/esc", "repos"},
			{m.keys.Remote.Short(), "remote"},
		}
	}

	const sep = "  ·  "
	var b strings.Builder
	used := 0
	for i, h := range hints {
		lead := ""
		if i > 0 {
			lead = sep
		}
		w := lipgloss.Width(lead) + lipgloss.Width(h.key) + 1 + lipgloss.Width(h.what)
		if used+w > width {
			break
		}
		used += w
		b.WriteString(m.st.Help.Render(lead))
		b.WriteString(m.st.HelpKey.Render(h.key))
		b.WriteString(m.st.Help.Render(" " + h.what))
	}
	return b.String()
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

// infoLines stacks each field's label above its value, so a long path or
// remote URL gets the pane's full width instead of what a label column leaves.
func (m model) infoLines(w int) []string {
	it, ok := m.current()
	if !ok {
		return []string{m.st.Dim.Render("no match")}
	}

	var out []string
	field := func(k, v string, style lipgloss.Style) {
		if v == "" {
			v = "-"
		}
		out = append(out, m.st.Label.Render(k))
		out = append(out, wrap(v, w, style)...)
	}

	out = append(out, wrap(it.label, w, m.st.Name)...)
	if m.mode == modeWorktrees {
		out = append(out, wrap(m.origin, w, m.st.Dim)...)
	}
	out = append(out, "")
	field("path", tildify(it.path), m.st.Path)

	s, loaded := m.status[it.path]
	if !loaded {
		field("git", "loading…", m.st.Dim)
		return out
	}
	if m.mode == modeRepos {
		field("remote", s.Remote, m.st.Remote)
	}
	field("branch", s.Branch, m.st.Branch)
	field("last commit", s.Commit, m.st.Commit)
	if s.Dirty > 0 {
		field("status", fmt.Sprintf("%d changed", s.Dirty), m.st.Dirty)
	} else {
		field("status", "clean", m.st.Clean)
	}
	if m.mode == modeRepos {
		if it.seen.Count > 0 {
			field("visits", fmt.Sprintf("%d, last %s", it.seen.Count, ago(time.Unix(it.seen.Last, 0))), m.st.Visits)
		} else {
			field("visits", "never", m.st.Dim)
		}
	}
	return out
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

	m.saved = &stash{all: m.all, view: m.view, matched: m.matched, cursor: m.cursor, query: m.input.Value()}
	m.origin = it.label
	m.mode = modeWorktrees

	// Reversed, so git's first worktree — the main one — lands at the bottom
	// next to the cursor, the way the best match does in the main list.
	items := make([]item, 0, len(wts))
	for i := len(wts) - 1; i >= 0; i-- {
		items = append(items, item{label: wts[i].Label(), path: wts[i].Path})
	}
	m.all = items
	m.input.SetValue("")
	m.filter()
	return m, m.loadStatus()
}

// restore puts the repository list back, query and cursor included.
func (m *model) restore() {
	if m.saved == nil {
		return
	}
	m.all, m.view, m.matched, m.cursor = m.saved.all, m.saved.view, m.saved.matched, m.saved.cursor
	m.input.SetValue(m.saved.query)
	m.mode, m.origin, m.saved = modeRepos, "", nil
}

// wrap renders v across as many lines of width w as it needs, so a long path
// or remote URL is folded rather than cut.
func wrap(v string, w int, style lipgloss.Style) []string {
	if w < 1 {
		return nil
	}
	return strings.Split(style.Width(w).Render(v), "\n")
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
