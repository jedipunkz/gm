package cli

import (
	"flag"
	"fmt"
	"path/filepath"
	"sort"
	"strings"

	"github.com/jedipunkz/gm/internal/repo"
)

// status answers "what did I leave unfinished?" across the whole tree. It
// never fetches: ahead and behind are counted against the refs already on
// disk, so it is fast and works offline, and behind is as stale as the last
// fetch was.
func (a *app) status(args []string) error {
	fs := flag.NewFlagSet("gm status", flag.ExitOnError)
	dirtyOnly := fs.Bool("dirty", false, "only repositories with uncommitted changes")
	unpushed := fs.Bool("unpushed", false, "only repositories with commits that are not pushed")
	all := fs.Bool("a", false, "include repositories that are clean and in sync")
	full := fs.Bool("p", false, "print full paths")
	if err := fs.Parse(args); err != nil {
		return err
	}

	rows, err := a.everything(*full)
	if err != nil {
		return err
	}
	paths := make([]string, len(rows))
	for i, r := range rows {
		paths[i] = r.path
	}
	states := repo.StatusMap(paths)

	// The name column is as wide as the widest name that survives the
	// filter, so a run that prints two rows is not padded for a hundred.
	var keep []row
	width := 0
	for _, r := range rows {
		s := states[r.path]
		switch {
		case *dirtyOnly && s.Dirty == 0:
		case *unpushed && s.Ahead == 0:
		case !*all && !*dirtyOnly && !*unpushed && !s.Unfinished():
		default:
			keep = append(keep, row{r.name, r.path})
			if n := len(r.name); n > width {
				width = n
			}
		}
	}
	for _, r := range keep {
		fmt.Printf("%-*s  %s\n", width, r.name, summarize(states[r.path]))
	}
	return nil
}

// row is one line's subject: what to call it, and where to ask git.
type row struct{ name, path string }

// everything lists the repositories and the worktrees hanging off them. A
// worktree is where work in progress lives, so leaving them out would miss
// the thing the command is for; the walk skips dotted directories, so
// .worktrees has to be asked for by name.
func (a *app) everything(full bool) ([]row, error) {
	repos, err := a.tree.List()
	if err != nil {
		return nil, err
	}
	var rows []row
	seen := map[string]bool{}
	add := func(name, path string) {
		if seen[path] {
			return
		}
		seen[path] = true
		if full {
			name = path
		}
		rows = append(rows, row{name, path})
	}
	for _, r := range repos {
		add(r.Rel, r.Path())
	}
	for _, root := range a.tree.Roots {
		paths, err := repo.FindRepos(filepath.Join(root, repo.WorktreeRoot))
		if err != nil {
			return nil, err
		}
		for _, p := range paths {
			rel, err := filepath.Rel(root, p)
			if err != nil {
				continue
			}
			add(filepath.ToSlash(rel), p)
		}
	}
	sort.Slice(rows, func(i, j int) bool { return rows[i].name < rows[j].name })
	return rows, nil
}

// summarize says what is unfinished in as few words as it takes.
func summarize(s repo.State) string {
	var parts []string
	if s.Dirty > 0 {
		parts = append(parts, plural(s.Dirty, "changed file", "changed files"))
	}
	if s.Ahead > 0 {
		parts = append(parts, fmt.Sprintf("%d ahead", s.Ahead))
	}
	if s.Behind > 0 {
		parts = append(parts, fmt.Sprintf("%d behind", s.Behind))
	}
	// Worth saying once the row is printed: those commits have nowhere to go.
	// It is not a reason to print one, or every local-only repository would.
	if !s.Upstream {
		parts = append(parts, "no upstream")
	}
	if len(parts) == 0 {
		return "clean"
	}
	return strings.Join(parts, ", ")
}

// plural counts a thing in the words for it.
func plural(n int, one, many string) string {
	if n == 1 {
		return "1 " + one
	}
	return fmt.Sprintf("%d %s", n, many)
}
