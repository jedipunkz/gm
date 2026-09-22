package cli

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"sort"
	"strings"

	"github.com/jedipunkz/gm/internal/repo"
)

func (a *app) list(args []string) error {
	fs := flag.NewFlagSet("gm list", flag.ExitOnError)
	var full, exact, unique bool
	fs.BoolVar(&full, "p", false, "print full paths")
	fs.BoolVar(&full, "full-path", false, "print full paths")
	fs.BoolVar(&exact, "e", false, "match the query exactly")
	fs.BoolVar(&exact, "exact", false, "match the query exactly")
	fs.BoolVar(&unique, "unique", false, "print the shortest unambiguous path")
	if err := fs.Parse(args); err != nil {
		return err
	}
	query := strings.Join(fs.Args(), " ")

	repos, err := a.tree.List()
	if err != nil {
		return err
	}
	var hits []repo.Repo
	for _, r := range repos {
		if repo.Match(r.Rel, query, exact) {
			hits = append(hits, r)
		}
	}
	sort.Slice(hits, func(i, j int) bool { return hits[i].Rel < hits[j].Rel })

	out := bufio.NewWriter(os.Stdout)
	var short []string
	if unique {
		short = repo.ShortestUnique(hits)
	}
	for i, r := range hits {
		// The writes cannot be checked one by one without drowning the loop;
		// out.Flush below reports whatever went wrong.
		switch {
		case full:
			_, _ = fmt.Fprintln(out, r.Path())
		case unique:
			_, _ = fmt.Fprintln(out, short[i])
		default:
			_, _ = fmt.Fprintln(out, r.Rel)
		}
	}
	// One check for the lot: a closed pipe or a full disk shows up here.
	return out.Flush()
}

func (a *app) root(args []string) error {
	fs := flag.NewFlagSet("gm root", flag.ExitOnError)
	all := fs.Bool("all", false, "show every root")
	if err := fs.Parse(args); err != nil {
		return err
	}
	roots := a.tree.Roots
	if !*all {
		roots = roots[:1]
	}
	for _, r := range roots {
		fmt.Println(r)
	}
	return nil
}
