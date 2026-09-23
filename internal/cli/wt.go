package cli

import (
	"flag"
	"fmt"
	"os"

	"github.com/jedipunkz/gm/internal/repo"
)

// wt is the command line half of the finder's worktree mode. The finder
// covers the interactive case better — the repository is the row under the
// cursor rather than something to name — so this exists for scripts: a
// bootstrap that checks out the branches you always want.
//
// The verbs are the finder's, so there is one set of names to remember:
// create and remove, spelled out, no abbreviations.
func (a *app) wt(args []string) error {
	if len(args) > 0 {
		switch args[0] {
		case "create":
			return a.wtCreate(args[1:])
		case "remove":
			return a.wtRemove(args[1:])
		}
	}
	return errWtUsage
}

var errWtUsage = fmt.Errorf("usage: gm wt <create|remove> [-y] <repo> <branch>")

// wtCreate checks a branch out beside the repository and prints where, so a
// script can cd into it the way it can with gm create.
func (a *app) wtCreate(args []string) error {
	fs := flag.NewFlagSet("gm wt create", flag.ExitOnError)
	if err := fs.Parse(args); err != nil {
		return err
	}
	if fs.NArg() != 2 {
		return errWtUsage
	}
	r, err := a.tree.Resolve(fs.Arg(0))
	if err != nil {
		return err
	}
	branch := fs.Arg(1)
	dir := a.tree.WorktreeDir(r, branch)
	if _, err := os.Stat(dir); err == nil {
		return fmt.Errorf("%s already exists", dir)
	}
	// AddWorktree starts the branch when it is new and checks it out when it
	// is not, which is the one thing worth saying before the path.
	start := "new branch"
	if repo.BranchExists(r.Path(), branch) {
		start = "existing branch"
	}
	if err := repo.AddWorktree(r.Path(), dir, branch); err != nil {
		return err
	}
	fmt.Fprintf(os.Stderr, "created  %s (%s)\n", r.Rel, start)
	_ = repo.Bump(dir)
	fmt.Println(dir)
	return nil
}

// wtRemove takes one worktree away. It is destructive, so it says what is in
// the way first and asks, the way gm remove does.
func (a *app) wtRemove(args []string) error {
	fs := flag.NewFlagSet("gm wt remove", flag.ExitOnError)
	dryRun := fs.Bool("dry-run", false, "show what would be removed")
	yes := fs.Bool("y", false, "skip the confirmation prompt")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if fs.NArg() != 2 {
		return errWtUsage
	}
	r, err := a.tree.Resolve(fs.Arg(0))
	if err != nil {
		return err
	}
	branch := fs.Arg(1)
	dir := a.tree.WorktreeDir(r, branch)

	// Asking git which worktrees there are answers two questions at once: is
	// this one of them, and is it the main one — which is the repository
	// itself and must never be removed this way.
	if !hasWorktreeAt(r, dir) {
		return fmt.Errorf("%s has no worktree for %s", r.Rel, branch)
	}
	if *dryRun {
		fmt.Fprintf(os.Stderr, "would remove %s\n", dir)
		return nil
	}
	// Work in the worktree is lost with it, so it is said before the
	// question, not discovered afterwards.
	dirty, _ := repo.IsDirty(dir)
	if dirty {
		fmt.Fprintf(os.Stderr, "warning: %s has uncommitted changes\n", dir)
	}
	if !*yes && !confirm("remove "+dir+"?") {
		fmt.Fprintln(os.Stderr, "skipped")
		return nil
	}
	if err := repo.RemoveWorktree(r.Path(), dir, dirty); err != nil {
		return err
	}
	fmt.Fprintf(os.Stderr, "removed  %s\n", dir)
	return nil
}

// hasWorktreeAt reports whether dir is one of the repository's worktrees and
// not the repository itself.
func hasWorktreeAt(r repo.Repo, dir string) bool {
	for _, w := range repo.OtherWorktrees(r) {
		if repo.SamePath(w.Path, dir) {
			return true
		}
	}
	return false
}
