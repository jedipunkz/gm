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

	"charm.land/bubbles/v2/spinner"
	"charm.land/bubbles/v2/textinput"
	tea "charm.land/bubbletea/v2"
	"charm.land/lipgloss/v2"

	"github.com/jedipunkz/gm/internal/config"
	"github.com/jedipunkz/gm/internal/repo"
)

// DefaultWorktreeKey opens the worktree list, DefaultBranchKey the branch
// list, DefaultPRKey the pull request list, and DefaultRemoteKey the selected
// repository's remote, when gm.toml says nothing.
const (
	DefaultWorktreeKey = "ctrl-w"
	DefaultBranchKey   = "ctrl-l"
	DefaultPRKey       = "ctrl-j"
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
	Branch   config.Chord // open and close the branch list
	PR       config.Chord // open and close the pull request list
	Remote   config.Chord // open the selected repository's remote
}

// check refuses a binding that would shadow one of the finder's fixed keys,
// or that two actions would answer to at once.
func (k Keys) check() error {
	named := []struct {
		name  string
		chord config.Chord
	}{{"worktree_key", k.Worktree}, {"branch_key", k.Branch}, {"pr_key", k.PR}, {"remote_key", k.Remote}}
	for i, c := range named {
		for _, o := range named[i+1:] {
			if c.chord.Key() == o.chord.Key() {
				return fmt.Errorf("%s and %s are both %s", c.name, o.name, c.chord.Display)
			}
		}
		if !c.chord.Plain() {
			continue // Alt or Shift can never collide with the fixed keys
		}
		if what, taken := reserved[c.chord.Letter]; taken {
			return fmt.Errorf("%s cannot be %s: the finder uses it to %s", c.name, c.chord.Display, what)
		}
	}
	return nil
}

// Action is the work the finder could not finish itself. Making and removing
// repositories and worktrees happen under it, behind a confirmation; what is
// left is going somewhere, and cloning, which wants a terminal of its own for
// its progress and its passwords.
type Action int

const (
	ActionNone Action = iota // the user quit
	ActionJump               // go to Arg, a repository or worktree path
	ActionGet                // clone Arg, a repository reference
)

// Result is what the finder leaves behind for gm to carry out once the
// alternate screen is gone.
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

// statusMsg carries one repository's git status back to the UI thread.
type statusMsg struct {
	path   string
	status repo.Status
}

// probeMsg says the cursor has rested on path for statusDelay. It is dropped
// if the selection moved on in the meantime, which is what keeps a held arrow
// key from asking git about every row it swept past.
type probeMsg struct{ path string }

// statusDelay is how long a row has to stay selected before git is asked
// about it. Key repeat is faster than this, so scrolling through a tree costs
// nothing and the row the eye stops on is described right away.
const statusDelay = 100 * time.Millisecond

type model struct {
	all     []item        // ascending by frecency: the best is last
	view    []int         // indices into all, same convention
	matched map[int][]int // item index -> matched rune positions
	cursor  int           // index into view
	input   textinput.Model
	status  map[string]repo.Status
	probing map[string]bool // paths git is being asked about right now
	st      Styles
	w, h    int
	result  Result

	mode   mode
	origin string // in the worktree or branch list, the repository it belongs to
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
	note      string // a one-line answer under the prompt, cleared on the next keystroke
	// busy says what gm is waiting on — git or GitHub, off the UI thread —
	// and stays under the prompt, with a spinner and the time taken so far,
	// until the answer arrives. Empty when nothing is running.
	busy      string
	busySince time.Time
	spin      spinner.Model
	// worktreesOf, branchesOf, prsOf and dirtyOf are the seams the tests
	// replace; they are repo.Worktrees, repo.Branches, repo.PullRequests and
	// repo.DirtyMap in every real run.
	worktreesOf func(dir string) ([]repo.Worktree, error)
	branchesOf  func(dir string) ([]repo.Branch, error)
	// remoteBranchesOf is repo.RemoteBranches, the other seam for the branch list.
	remoteBranchesOf func(dir string) ([]repo.Branch, error)
	prsOf            func(dir string) ([]repo.PullRequest, error)
	dirtyOf          func(paths []string) map[string]bool
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
		all:              items,
		input:            in,
		status:           map[string]repo.Status{},
		probing:          map[string]bool{},
		spin:             spinner.New(spinner.WithSpinner(spinner.MiniDot), spinner.WithStyle(st.HelpKey)),
		st:               st,
		w:                80,
		h:                24,
		keys:             keys,
		tree:             tree,
		worktreesOf:      repo.Worktrees,
		branchesOf:       repo.Branches,
		remoteBranchesOf: repo.RemoteBranches,
		prsOf:            repo.PullRequests,
		dirtyOf:          repo.DirtyMap,
	}
	m.filter()
	return m
}

func (m model) Init() tea.Cmd {
	return tea.Batch(textinput.Blink, m.loadStatus())
}

func (m model) current() (item, bool) {
	if m.cursor < 0 || m.cursor >= len(m.view) {
		return item{}, false
	}
	return m.all[m.view[m.cursor]], true
}

// loadStatus arms the delay for the selected repository. No git runs yet: the
// probe that follows checks the cursor is still here first.
func (m model) loadStatus() tea.Cmd {
	it, ok := m.current()
	if !ok {
		return nil
	}
	p := it.path
	if p == "" {
		return nil // a branch with no worktree yet: there is nothing to ask git about
	}
	if _, done := m.status[p]; done {
		return nil
	}
	return tea.Tick(statusDelay, func(time.Time) tea.Msg { return probeMsg{path: p} })
}

// probe asks git about path, off the UI thread, unless the cursor has since
// moved elsewhere or the answer is already on its way.
func (m *model) probe(path string) tea.Cmd {
	it, ok := m.current()
	if !ok || it.path != path || m.probing[path] {
		return nil
	}
	if _, done := m.status[path]; done {
		return nil
	}
	m.probing[path] = true
	return func() tea.Msg {
		return statusMsg{path: path, status: repo.Describe(path)}
	}
}

func (m model) Update(msg tea.Msg) (tea.Model, tea.Cmd) {
	switch msg := msg.(type) {
	case tea.WindowSizeMsg:
		m.w, m.h = msg.Width, msg.Height
		return m, nil

	case probeMsg:
		return m, m.probe(msg.path)

	case statusMsg:
		delete(m.probing, msg.path)
		m.status[msg.path] = msg.status
		return m, nil

	case spinner.TickMsg:
		// Ticks stop once nothing is running: the chain ends here.
		if m.busy == "" {
			return m, nil
		}
		var cmd tea.Cmd
		m.spin, cmd = m.spin.Update(msg)
		return m, cmd

	case doneMsg:
		m.busy = ""
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
		case changeCheckOut, changeCheckOutPR:
			m.result = Result{Action: ActionJump, Arg: msg.path}
			return m, tea.Quit
		}
		return m, m.loadStatus()

	case remoteBranchesMsg:
		return m.addRemoteBranches(msg)

	case prsMsg:
		return m.showPRs(msg)

	case dirtyMsg:
		m.dirty, m.note, m.busy = msg, "", ""
		m.filter()
		m.cursor = len(m.view) - 1
		return m, m.loadStatus()

	case tea.KeyPressMsg:
		// A panel is modal: it answers to its own keys and swallows
		// everything else, so nothing moves behind it.
		if m.over != overlayNone {
			return m.panelKey(msg)
		}

		// The configurable keys cannot be switch cases.
		switch msg.String() {
		case m.keys.Worktree.Key():
			return m.switchTo(modeWorktrees)
		case m.keys.Branch.Key():
			return m.switchTo(modeBranches)
		case m.keys.PR.Key():
			return m.switchTo(modePRs)
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
			if m.mode != modeRepos {
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
			if _, typed, ok := splitInput(m.input.Value()); ok {
				next, cmd := m.runCommand(typed)
				return next, cmd
			}
			switch m.mode {
			case modeBranches:
				next, cmd := m.checkOut()
				return next, cmd
			case modePRs:
				next, cmd := m.checkOutPR()
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
		m.input.SetSuggestions(completions(m.input.Value()))
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

// openRemote hands the selected repository's remote to a browser. A
// repository without one, or one whose remote is not a URL, does nothing
// visible: the finder is showing a list, not reporting on git.
func (m model) openRemote() tea.Cmd {
	it, ok := m.current()
	if !ok {
		return nil
	}
	path := it.path
	if path == "" {
		path = m.repoAt // a branch with no worktree: the repository has the remote
	}
	remote := m.status[path].Remote
	return func() tea.Msg {
		if remote == "" {
			// Not fetched yet for this row; ask git now rather than making
			// the key do nothing on the first press.
			remote = repo.OriginURL(path)
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
	if m.busy != "" {
		elapsed := fmt.Sprintf("%s %ds", m.busy, int(time.Since(m.busySince).Seconds()))
		return m.spin.View() + " " + strings.Join(wrapSegs([]seg{{elapsed, m.st.Help}}, width-2, "")[:1], "")
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
	wt, br, pr := m.keys.Worktree.Short(), m.keys.Branch.Short(), m.keys.PR.Short()
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
		{br, "branches"},
		{pr, "prs"},
	}
	switch m.mode {
	case modeWorktrees:
		hints = []hint{
			{"↑↓ ctrl-p/n", "move"},
			{"enter", "jump"},
			{wt + "/g/esc", "repos"},
			{m.keys.Remote.Short(), "remote"},
			{br, "branches"},
			{pr, "prs"},
		}
	case modeBranches:
		hints = []hint{
			{"↑↓ ctrl-p/n", "move"},
			{"enter", "check out"},
			{br + "/g/esc", "repos"},
			{m.keys.Remote.Short(), "remote"},
			{wt, "worktrees"},
			{pr, "prs"},
		}
	case modePRs:
		hints = []hint{
			{"↑↓ ctrl-p/n", "move"},
			{"enter", "check out"},
			{pr + "/g/esc", "repos"},
			{m.keys.Remote.Short(), "remote"},
			{wt, "worktrees"},
			{br, "branches"},
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

// startBusy puts what gm now waits on under the prompt and starts the spinner
// that shows it has not hung. The returned command is the first tick.
func (m *model) startBusy(what string) tea.Cmd {
	m.busy, m.busySince, m.note = what, time.Now(), ""
	return m.spin.Tick
}
