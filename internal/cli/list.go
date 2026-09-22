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
	defer out.Flush()
	var short []string
	if unique {
		short = repo.ShortestUnique(hits)
	}
	for i, r := range hits {
		switch {
		case full:
			fmt.Fprintln(out, r.Path())
		case unique:
			fmt.Fprintln(out, short[i])
		default:
			fmt.Fprintln(out, r.Rel)
		}
	}
	return nil
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
