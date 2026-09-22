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

// Action is what the finder decided, beyond picking a path.
type Action int

const (
	ActionNone   Action = iota // the user quit
	ActionJump                 // go to Arg, a repository or worktree path
	ActionCreate               // create Arg, a repository reference
	ActionGet                  // clone Arg, a repository reference
	ActionRemove               // remove Arg, a repository path
)

// Result is what the finder leaves behind. Everything that touches the
// network, the disk or the user's confirmation happens after it has closed,
// on the terminal the user can see.
type Result struct {
	Action Action
	Arg    string
}

// Run draws the finder and returns what the user asked for. It draws on the
// terminal itself, never on stdout: stdout carries the chosen path back to
// the shell binding.
func Run(tree *repo.Tree, repos []repo.Repo, h *repo.History, theme Theme, keys Keys) (Result, error) {
	if err := keys.check(); err != nil {
		return Result{}, err
	}
	opts := []tea.ProgramOption{tea.WithOutput(os.Stderr)}
	if tty, err := os.OpenFile("/dev/tty", os.O_RDWR, 0); err == nil {
		defer func() { _ = tty.Close() }()
		opts = []tea.ProgramOption{tea.WithInput(tty), tea.WithOutput(tty)}
	}
	res, err := tea.NewProgram(newModel(tree, repos, h, theme, keys), opts...).Run()
	if err != nil {
		return Result{}, err
	}
	m, ok := res.(model)
	if !ok {
		return Result{}, nil
	}
	return m.result, nil
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

// overlay says which panel is drawn over the list.
type overlay int

const (
	overlayNone overlay = iota
	overlayHelp
	overlayConfirm
)

// change is what a confirmation will carry out. It is not an Action: none of
// these leave the finder, they happen under it.
type change int

const (
	changeNone change = iota
	changeCreate
	changeRemove
	changeAddWorktree
	changeRemoveWorktree
)

// pending is the change a confirmation is waiting on. Nothing has happened
// yet when one is on screen.
type pending struct {
	kind   change
	arg    string   // the path to remove, the reference to create, the branch to check out
	dir    string   // where a worktree will go, or which one goes away
	title  string   // "remove", "create worktree"
	detail []string // what it will do, a line each
	force  bool     // there is work in it and the user has been told
}

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

// doneMsg carries the outcome of a confirmed change back to the UI thread.
type doneMsg struct {
	kind  change
	label string // how the new row reads, if one was made
	path  string
	err   error
}

// dirtyMsg carries the result of a scan back to the UI thread.
type dirtyMsg map[string]bool

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
	result  Result

	mode   mode
	origin string // in worktree mode, the repository the list belongs to
	repoAt string // ...and where it is on disk
	saved  *stash
	keys   Keys
	tree   *repo.Tree
	query  string  // the query the view was built from; a command is not one
	over   overlay // the panel drawn over the list, if any
	ask    pending // what a confirmation is waiting on
	// dirty holds the answer for every repository once a scan has run; nil
	// until one has. dirtyOnly is the filter itself.
	dirty     map[string]bool
	dirtyOnly bool
	scanning  bool
	note      string // a one-line answer under the prompt, cleared on the next keystroke
	// worktreesOf and dirtyOf are the seams the tests replace; they are
	// repo.Worktrees and repo.DirtyMap in every real run.
	worktreesOf func(dir string) ([]repo.Worktree, error)
	dirtyOf     func(paths []string) map[string]bool
}

func newModel(tree *repo.Tree, repos []repo.Repo, hist *repo.History, theme Theme, keys Keys) model {
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
	// Completing the slash commands as they are typed: the box fills in the
	// rest of the name, and Tab accepts it.
	in.ShowSuggestions = true
	in.SetSuggestions(commandNames())
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
		tree:        tree,
		worktreesOf: repo.Worktrees,
		dirtyOf:     repo.DirtyMap,
	}
	m.filter()
	return m
}

func (m model) Init() tea.Cmd {
	return tea.Batch(textinput.Blink, m.loadStatus())
}

func (m *model) filter() {
	m.matched = map[int][]int{}
	m.note = ""
	q := strings.TrimSpace(m.input.Value())
	// A command is not a query: the list stays as it was while one is typed.
	if isCommand(q) {
		q = ""
	}
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

	case doneMsg:
		if msg.err != nil {
			m.note = msg.err.Error()
			return m, nil
		}
		switch msg.kind {
		case changeRemove, changeRemoveWorktree:
			m.drop(msg.path)
			m.note = "removed " + tildify(msg.path)
		case changeCreate, changeAddWorktree:
			m.add(msg.label, msg.path)
			m.note = "created " + tildify(msg.path)
		}
		return m, m.loadStatus()

	case dirtyMsg:
		m.dirty, m.scanning, m.note = msg, false, ""
		m.filter()
		m.cursor = len(m.view) - 1
		return m, m.loadStatus()

	case tea.KeyPressMsg:
		// A panel is modal: it answers to its own keys and swallows
		// everything else, so nothing moves behind it.
		switch m.over {
		case overlayHelp:
			switch msg.String() {
			case "q", "esc", "ctrl+c", "enter":
				m.over = overlayNone
			}
			return m, nil

		case overlayConfirm:
			switch msg.String() {
			case "y", "Y":
				a := m.ask
				m.over, m.ask = overlayNone, pending{}
				return m, m.perform(a)
			case "n", "N", "q", "esc", "ctrl+c":
				m.over, m.ask, m.note = overlayNone, pending{}, "cancelled"
			}
			return m, nil
		}

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
			// These back out of whatever is narrowing the list before they
			// quit gm: the worktree list first, then a filter. From the
			// repository list with nothing to undo, Ctrl-G does nothing — it
			// is the key that opened gm in the first place.
			if m.mode == modeWorktrees {
				m.restore()
				return m, m.loadStatus()
			}
			// Clearing the query holds the selection, so this is also how you
			// get from a repository you found to a command that acts on it.
			if m.input.Value() != "" {
				m.input.SetValue("")
				m.filter()
				return m, m.loadStatus()
			}
			if m.dirtyOnly {
				m.dirtyOnly = false
				m.filter()
				m.cursor = len(m.view) - 1
				return m, m.loadStatus()
			}
			if msg.String() == "esc" {
				return m, tea.Quit
			}
			return m, nil
		case "enter":
			if typed := m.input.Value(); isCommand(typed) {
				next, cmd := m.runCommand(typed)
				return next, cmd
			}
			if it, ok := m.current(); ok {
				m.result = Result{Action: ActionJump, Arg: it.path}
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

	out := b.String()
	switch m.over {
	case overlayHelp:
		out = m.overlay(out, m.helpBox())
	case overlayConfirm:
		out = m.overlay(out, m.confirmBox())
	}

	v := tea.NewView(out)
	v.AltScreen = true
	return v
}

// overlay draws a panel over the list, centred, leaving what is behind it
// visible around the edges.
func (m model) overlay(base, box string) string {
	x := max((m.w-lipgloss.Width(box))/2, 0)
	y := max((m.h-lipgloss.Height(box))/2, 0)
	// A Layer draws only its own content; the Compositor is what reads the
	// positions and the z-order and puts one over the other.
	return lipgloss.NewCanvas(m.w, m.h).
		Compose(lipgloss.NewCompositor(
			lipgloss.NewLayer(base),
			lipgloss.NewLayer(box).X(x).Y(y).Z(1),
		)).
		Render()
}

// perform carries out a confirmed change, off the UI thread.
func (m model) perform(a pending) tea.Cmd {
	tree, repoAt := m.tree, m.repoAt
	return func() tea.Msg {
		done := func(path string, label string, err error) tea.Msg {
			return doneMsg{kind: a.kind, path: path, label: label, err: err}
		}
		switch a.kind {
		case changeRemove:
			r, ok := tree.At(a.arg)
			if !ok {
				return done("", "", fmt.Errorf("%s is not under any root", a.arg))
			}
			if err := repo.Delete(r); err != nil {
				return done("", "", err)
			}
			return done(r.Path(), r.Rel, nil)

		case changeCreate:
			r, err := tree.Create(a.arg, false)
			if err != nil {
				return done("", "", err)
			}
			return done(r.Path(), r.Rel, nil)

		case changeAddWorktree:
			if err := repo.AddWorktree(repoAt, a.dir, a.arg); err != nil {
				return done("", "", err)
			}
			return done(a.dir, a.arg, nil)

		case changeRemoveWorktree:
			if err := repo.RemoveWorktree(repoAt, a.dir, a.force); err != nil {
				return done("", "", err)
			}
			return done(a.dir, "", nil)
		}
		return nil
	}
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
	// An answer to something the user just typed displaces the hints: it is
	// about to be cleared by their next keystroke anyway.
	if m.note != "" {
		return strings.Join(wrapSegs([]seg{{m.note, m.st.Dirty}}, width, "")[:1], "")
	}
	if isCommand(m.input.Value()) {
		return m.st.Help.Render("enter runs the command  ·  tab completes it  ·  ") +
			m.st.HelpKey.Render("/help") + m.st.Help.Render(" lists them")
	}
	if m.dirtyOnly {
		// The filter has to be visible, or an empty list reads as a bug.
		const label = "dirty only"
		const sep = "  ·  "
		state := m.st.Dirty.Render(label) + m.st.Help.Render(sep)
		return state + m.hints(width-len(label)-len(sep))
	}
	return m.hints(width)
}

func (m model) hints(width int) string {
	wt := m.keys.Worktree.Short()
	// Esc undoes one layer of narrowing at a time, so it has to say which.
	out := "quit"
	switch {
	case m.input.Value() != "":
		out = "clear"
	case m.dirtyOnly:
		out = "show all"
	}
	hints := []hint{
		{"↑↓ ctrl-p/n", "move"},
		{"enter", "jump"},
		{wt, "worktrees"},
		{"esc", out},
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

	// Last, because the pane is clipped from the bottom: on a short terminal
	// the commits go before the name, the path or the status do.
	if len(s.Commits) == 0 {
		field("last commit", "", m.st.Commit)
		return out
	}
	// The count is in the label: the lines fold, so "how many commits am I
	// looking at" is not answerable by counting rows.
	label := fmt.Sprintf("last %d commits", len(s.Commits))
	if len(s.Commits) == 1 {
		label = "last commit"
	}
	out = append(out, m.st.Label.Render(label))
	for _, c := range s.Commits {
		out = append(out, m.commitLines(c, w)...)
	}
	return out
}

// commitLines draws one commit the way `git log --oneline --decorate` does:
// the hash, then the refs pointing at it, then the subject. A line too long
// for the pane is folded, with its continuations indented so one commit still
// reads as one entry.
func (m model) commitLines(c repo.Commit, w int) []string {
	segs := []seg{{c.Hash, m.st.Commit}}
	if len(c.Refs) > 0 {
		segs = append(segs, seg{" (", m.st.Punct})
		for i, r := range c.Refs {
			if i > 0 {
				segs = append(segs, seg{", ", m.st.Punct})
			}
			segs = append(segs, seg{r.Name, m.refStyle(r.Kind)})
		}
		segs = append(segs, seg{")", m.st.Punct})
	}
	segs = append(segs, seg{" " + c.Subject, m.st.Subject})
	return wrapSegs(segs, w, commitIndent)
}

// commitIndent sets the continuation lines of a folded commit in from the
// hashes, so the eye can still count the commits.
const commitIndent = "  "

func (m model) refStyle(k repo.RefKind) lipgloss.Style {
	switch k {
	case repo.RefHead:
		return m.st.RefHead
	case repo.RefRemote:
		return m.st.RefRemote
	case repo.RefTag:
		return m.st.RefTag
	default:
		return m.st.RefLocal
	}
}

// seg is a run of text with one style, the unit wrapping counts in.
type seg struct {
	text  string
	style lipgloss.Style
}

// wrapSegs lays styled pieces out across lines of width w, breaking at a
// space where it can and mid-word when a word is longer than the pane. Every
// line after the first starts with indent. Styles survive the fold: a ref cut
// across two lines keeps its colour on both.
func wrapSegs(segs []seg, w int, indent string) []string {
	if w < 1 {
		return nil
	}
	// Flattening to runes keeps the two concerns apart: where the line breaks
	// falls out of the text, and which style each rune carries is remembered
	// alongside it.
	var (
		runes []rune
		owner []int
	)
	for i, s := range segs {
		for _, r := range s.text {
			runes = append(runes, r)
			owner = append(owner, i)
		}
	}

	render := func(from, to int) string {
		var b strings.Builder
		for i := from; i < to; {
			j := i
			for j < to && owner[j] == owner[i] {
				j++
			}
			b.WriteString(segs[owner[i]].style.Render(string(runes[i:j])))
			i = j
		}
		return b.String()
	}

	var lines []string
	for start, pad := 0, ""; start < len(runes); pad = indent {
		avail := max(w-len([]rune(pad)), 1)
		if len(runes)-start <= avail {
			lines = append(lines, pad+render(start, len(runes)))
			break
		}
		// Break at the last space that fits; failing that, mid-word.
		end := start + avail
		brk := -1
		for i := end; i > start; i-- {
			if runes[i] == ' ' {
				brk = i
				break
			}
		}
		next := end
		if brk > start {
			end, next = brk, brk+1
		}
		lines = append(lines, pad+render(start, end))
		// A fold never starts a line with the spaces it broke on.
		for next < len(runes) && runes[next] == ' ' {
			next++
		}
		start = next
	}
	return lines
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
	m.origin, m.repoAt = it.label, it.path
	m.mode = modeWorktrees

	// Reversed, so git's first worktree — the main one — lands at the bottom
	// next to the cursor, the way the best match does in the main list.
	items := make([]item, 0, len(wts))
	for i := len(wts) - 1; i >= 0; i-- {
		items = append(items, item{label: wts[i].Label(), path: wts[i].Path})
	}
	m.all = items
	m.input.SetValue("")
	// A different list entirely: the held selection means nothing in it.
	m.view = nil
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
	m.mode, m.origin, m.repoAt, m.saved = modeRepos, "", "", nil
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
