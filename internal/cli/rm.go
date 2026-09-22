package cli

import (
	"flag"
	"fmt"
	"os"
	"path/filepath"

	"github.com/jedipunkz/gm/internal/repo"
)

func (a *app) remove(args []string) error {
	fs := flag.NewFlagSet("gm rm", flag.ExitOnError)
	dryRun := fs.Bool("dry-run", false, "show what would be removed")
	yes := fs.Bool("y", false, "skip the confirmation prompt")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if fs.NArg() == 0 {
		return fmt.Errorf("usage: gm rm [--dry-run] [-y] <repo>...")
	}

	for _, q := range fs.Args() {
		r, err := a.tree.Resolve(q)
		if err != nil {
			return err
		}
		if *dryRun {
			fmt.Fprintf(os.Stderr, "would remove %s\n", r.Path())
			continue
		}
		if dirty, _ := repo.IsDirty(r.Path()); dirty {
			fmt.Fprintf(os.Stderr, "warning: %s has uncommitted changes\n", r.Rel)
		}
		if !*yes && !confirm("remove "+r.Path()+"?") {
			fmt.Fprintln(os.Stderr, "skipped")
			continue
		}
		if err := os.RemoveAll(r.Path()); err != nil {
			return err
		}
		repo.PruneEmptyParents(r.Root, filepath.Dir(r.Path()))
		fmt.Fprintf(os.Stderr, "removed  %s\n", r.Path())
	}
	return nil
}
