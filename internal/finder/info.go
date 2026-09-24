package finder

import (
	"cmp"
	"fmt"
	"os"
	"slices"
	"strings"
	"time"

	"charm.land/lipgloss/v2"

	"github.com/jedipunkz/gm/internal/repo"
)

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

	// What the pane is about is a field like any other: named, so the top of
	// the pane is not the one line whose meaning has to be inferred.
	if m.mode == modeWorktrees {
		field("worktree", it.label, m.st.Name)
		field("repository", m.origin, m.st.Dim)
	} else {
		field("repository", it.label, m.st.Name)
	}
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
		// Only when there are some: the pane is for what is there, and this
		// is also the only way to tell whether the worktree key is worth
		// pressing on this row. The newest few, not all of them: a repository
		// with dozens would push everything below it off the pane, and the
		// whole list is one ctrl-t away.
		if shown := recentWorktrees(s.Worktrees); len(shown) > 0 {
			out = append(out, m.st.Label.Render(worktreesLabel(len(shown), len(s.Worktrees))))
			for _, wt := range shown {
				out = append(out, m.worktreeLines(wt, w)...)
			}
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

// shownWorktrees is how many checkouts the pane names. Three is what the
// commits get, for the same reason: enough to recognise the repository,
// not enough to become a list.
const shownWorktrees = 3

// recentWorktrees is the newest few checkouts, by the last commit on their
// branch. A detached checkout has no branch to date it, so it sorts last,
// and git's own order breaks ties.
func recentWorktrees(all []repo.Worktree) []repo.Worktree {
	out := slices.Clone(all)
	slices.SortStableFunc(out, func(a, b repo.Worktree) int {
		return cmp.Compare(b.CommittedAt, a.CommittedAt)
	})
	return out[:min(len(out), shownWorktrees)]
}

// worktreesLabel says how many of how many, because the rows below it are
// the newest ones and not the whole set.
func worktreesLabel(shown, total int) string {
	switch {
	case total > shown:
		return fmt.Sprintf("last %d of %d worktrees", shown, total)
	case total == 1:
		return "last worktree"
	default:
		return fmt.Sprintf("last %d worktrees", total)
	}
}

// worktreeLines draws one checkout as its branch and when that branch was
// last committed to, which is what ranked it into this list.
func (m model) worktreeLines(wt repo.Worktree, w int) []string {
	segs := []seg{{wt.Label(), m.st.Branch}}
	if wt.CommittedAt > 0 {
		segs = append(segs, seg{"  " + ago(time.Unix(wt.CommittedAt, 0)), m.st.Dim})
	}
	return wrapSegs(segs, w, commitIndent)
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
