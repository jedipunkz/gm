package cli

import (
	"fmt"
	"os"

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

	hist := repo.LoadHistory()
	chosen, err := finder.Run(repos, hist, theme)
	if err != nil || chosen == "" {
		return err
	}
	if err := hist.Bump(chosen); err != nil {
		fmt.Fprintf(os.Stderr, "gm: could not record visit: %v\n", err)
	}
	fmt.Println(chosen)
	return nil
}
