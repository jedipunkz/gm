package cli

import (
	"flag"
	"fmt"
	"os"
	"strings"

	"github.com/jedipunkz/gm/internal/repo"
)

func (a *app) remove(args []string) error {
	fs := flag.NewFlagSet("gm remove", flag.ExitOnError)
	dryRun := fs.Bool("dry-run", false, "show what would be removed")
	yes := fs.Bool("y", false, "skip the confirmation prompt")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if fs.NArg() == 0 {
		return fmt.Errorf("usage: gm remove [--dry-run] [-y] <repo>...")
	}

	for _, q := range fs.Args() {
		r, err := a.tree.Resolve(q)
		if err != nil {
			return err
		}
		if err := removeOne(r, *dryRun, *yes); err != nil {
			return err
		}
	}
	return nil
}

// removeOne deletes one repository, warning first when there is work in it
// and asking before anything is lost.
// worktreeWarning names what will be taken along with a repository.
func worktreeWarning(wts []repo.Worktree) string {
	labels := make([]string, 0, len(wts))
	for _, w := range wts {
		labels = append(labels, w.Label())
	}
	what := "worktrees"
	if len(wts) == 1 {
		what = "worktree"
	}
	return fmt.Sprintf("%d %s will be removed too: %s", len(wts), what, strings.Join(labels, ", "))
}

func removeOne(r repo.Repo, dryRun, yes bool) error {
	if dryRun {
		fmt.Fprintf(os.Stderr, "would remove %s\n", r.Path())
		return nil
	}
	if dirty, _ := repo.IsDirty(r.Path()); dirty {
		fmt.Fprintf(os.Stderr, "warning: %s has uncommitted changes\n", r.Rel)
	}
	// They go with it, so they have to be said before the question, not
	// discovered afterwards.
	if wts := repo.OtherWorktrees(r); len(wts) > 0 {
		fmt.Fprintf(os.Stderr, "warning: %s\n", worktreeWarning(wts))
	}
	if !yes && !confirm("remove "+r.Path()+"?") {
		fmt.Fprintln(os.Stderr, "skipped")
		return nil
	}
	if err := repo.Delete(r); err != nil {
		return err
	}
	fmt.Fprintf(os.Stderr, "removed  %s\n", r.Path())
	return nil
}
