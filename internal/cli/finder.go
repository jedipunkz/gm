package cli

import (
	"fmt"
	"os"

	"github.com/jedipunkz/gm/internal/config"
	"github.com/jedipunkz/gm/internal/finder"
	"github.com/jedipunkz/gm/internal/repo"
)

// finder opens the interactive picker and prints the chosen path on stdout,
// so the shell binding can capture it with $(gm).
func (a *app) finder() error {
	repos, err := a.tree.List()
	if err != nil {
		return err
	}
	if len(repos) == 0 {
		return fmt.Errorf("no repositories under %s; try `gm get <repo>`", a.tree.Primary())
	}
	theme, err := finder.LookupTheme(a.cfg.Theme)
	if err != nil {
		return err
	}
	worktreeKey, err := config.ParseChord(a.cfg.WorktreeKey, finder.DefaultWorktreeKey)
	if err != nil {
		return err
	}
	branchKey, err := config.ParseChord(a.cfg.BranchKey, finder.DefaultBranchKey)
	if err != nil {
		return err
	}
	prKey, err := config.ParseChord(a.cfg.PRKey, finder.DefaultPRKey)
	if err != nil {
		return err
	}
	remoteKey, err := config.ParseChord(a.cfg.RemoteKey, finder.DefaultRemoteKey)
	if err != nil {
		return err
	}
	keys := finder.Keys{Worktree: worktreeKey, Branch: branchKey, PR: prKey, Remote: remoteKey}

	hist := repo.LoadHistory()
	res, err := finder.Run(a.tree, repos, hist, theme, keys)
	if err != nil {
		return err
	}
	return a.act(res, hist)
}

// act carries out what the finder decided, now that the alternate screen is
// gone: a clone's progress, a password prompt and a confirmation all belong
// on the terminal the user can see.
//
// One rule about the visit log, so a repository is never counted twice: it is
// recorded by whatever fetched, made or moved the repository, and by the
// finder only for a row that was simply chosen. gm get has already recorded
// the clone by the time this sees it.
func (a *app) act(res finder.Result, hist *repo.History) error {
	switch res.Action {
	case finder.ActionJump:
		return a.goTo(res.Arg, hist)

	case finder.ActionGet:
		if err := a.get([]string{res.Arg}); err != nil {
			return err
		}
		u, err := repo.NormalizeURL(res.Arg, false)
		if err != nil {
			return err
		}
		return printPath(a.tree.PathFor(repo.RelPathOf(u)))

	}
	return nil // the user quit
}

// goTo records the visit and prints the path, which is how the shell binding
// learns where to cd.
func (a *app) goTo(path string, hist *repo.History) error {
	if path == "" {
		return nil
	}
	if err := hist.Bump(path); err != nil {
		fmt.Fprintf(os.Stderr, "gm: could not record visit: %v\n", err)
	}
	return printPath(path)
}

// printPath hands the path to the shell binding and records nothing: the
// subcommand that just fetched the repository has already done that.
func printPath(path string) error {
	if path == "" {
		return nil
	}
	fmt.Println(path)
	return nil
}
