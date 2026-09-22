package main

import (
	"fmt"
	"sort"
	"strings"
	"time"

	"github.com/charmbracelet/bubbles/textinput"
	tea "github.com/charmbracelet/bubbletea"
	"github.com/charmbracelet/lipgloss"
	"github.com/sahilm/fuzzy"
)

// The best match sits at the BOTTOM of the list, next to the prompt and the
// cursor's resting place, so the most likely repository needs zero keystrokes.

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
	err                    error
}

type infoMsg struct {
	path string
	info repoInfo
}

type model struct {
	all    []item // ascending by frecency: the best is last
	view   []int  // indices into all, same convention
	cursor int    // index into view
	input  textinput.Model
	info   map[string]repoInfo
	w, h   int
	chosen string
}

var (
	styleSel    = lipgloss.NewStyle().Bold(true).Foreground(lipgloss.AdaptiveColor{Light: "#005f87", Dark: "#5fd7ff"})
	styleDim    = lipgloss.NewStyle().Foreground(lipgloss.AdaptiveColor{Light: "#767676", Dark: "#8a8a8a"})
	styleKey    = lipgloss.NewStyle().Foreground(lipgloss.AdaptiveColor{Light: "#8700af", Dark: "#d787ff"})
	styleDirty  = lipgloss.NewStyle().Foreground(lipgloss.AdaptiveColor{Light: "#af5f00", Dark: "#ffaf5f"})
	styleHeader = lipgloss.NewStyle().Bold(true)
)

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
	in.Focus()

	m := model{all: items, input: in, info: map[string]repoInfo{}, w: 80, h: 24}
	m.filter()
	return m
}

func (m model) Init() tea.Cmd { return textinput.Blink }

func (m *model) filter() {
	q := strings.TrimSpace(m.input.Value())
	if q == "" {
		m.view = make([]int, len(m.all))
		for i := range m.all {
			m.view[i] = i
		}
	} else {
		matches := fuzzy.FindFrom(q, source(m.all))
		score := make(map[int]int, len(matches))
		idx := make([]int, 0, len(matches))
		for _, mt := range matches {
			score[mt.Index] = mt.Score
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
		i.commit, i.err = capture(p, "git", "log", "-1", "--format=%h  %cr  %s")
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

	case tea.KeyMsg:
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

func (m model) View() string {
	rows := m.h - 2 // prompt + header
	if rows < 3 {
		rows = 3
	}
	// Below ~60 columns there is no room for two panes, so the info pane goes
	// away rather than overflowing the line.
	listW, infoW := m.w-1, 0
	if m.w >= 60 {
		listW = m.w * 45 / 100
		infoW = m.w - listW - 3
	}

	// Window over the view, kept anchored so the cursor stays visible.
	start := len(m.view) - rows
	if m.cursor < start {
		start = m.cursor
	}
	if start < 0 {
		start = 0
	}
	end := start + rows
	if end > len(m.view) {
		end = len(m.view)
	}

	lines := make([]string, 0, rows)
	for i := 0; i < rows-(end-start); i++ {
		lines = append(lines, "") // pad the top: the list hangs from the bottom
	}
	for i := start; i < end; i++ {
		it := m.all[m.view[i]]
		label := trunc(it.repo.Rel, listW-2)
		if i == m.cursor {
			lines = append(lines, styleSel.Render("▸ "+label))
		} else {
			lines = append(lines, "  "+styleDim.Render(label))
		}
	}

	var info []string
	if infoW > 0 {
		info = m.infoLines(infoW)
	}
	for len(info) < len(lines) {
		info = append(info, "")
	}

	var b strings.Builder
	header := fmt.Sprintf("%d/%d", len(m.view), len(m.all))
	fmt.Fprintf(&b, "%s  %s\n", styleHeader.Render("gm"), styleDim.Render(header))
	for i := range lines {
		if infoW == 0 {
			fmt.Fprintf(&b, "%s\n", lines[i])
			continue
		}
		fmt.Fprintf(&b, "%-*s │ %s\n", listW, lines[i], info[i])
	}
	b.WriteString(m.input.View())
	return b.String()
}

func (m model) infoLines(w int) []string {
	it, ok := m.current()
	if !ok {
		return []string{styleDim.Render("no match")}
	}
	kv := func(k, v string) string {
		if v == "" {
			v = "-"
		}
		return styleKey.Render(fmt.Sprintf("%-7s", k)) + trunc(v, w-8)
	}

	out := []string{styleHeader.Render(trunc(it.repo.Rel, w)), ""}
	out = append(out, kv("path", it.repo.Path()))

	i, loaded := m.info[it.repo.Path()]
	if !loaded {
		return append(out, kv("git", "loading…"))
	}
	out = append(out,
		kv("remote", i.remote),
		kv("branch", i.branch),
		kv("commit", i.commit),
	)
	if i.dirty > 0 {
		out = append(out, styleKey.Render(fmt.Sprintf("%-7s", "status"))+styleDirty.Render(fmt.Sprintf("%d changed", i.dirty)))
	} else {
		out = append(out, kv("status", "clean"))
	}
	if it.seen.Count > 0 {
		out = append(out, kv("visits", fmt.Sprintf("%d, last %s", it.seen.Count, ago(time.Unix(it.seen.Last, 0)))))
	} else {
		out = append(out, kv("visits", "never"))
	}
	return out
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
	// Keep the tail: the repository name matters more than the host.
	return "…" + string(r[len(r)-w+1:])
}
