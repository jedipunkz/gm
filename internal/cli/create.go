package cli

import (
	"flag"
	"fmt"
	"os"

	"github.com/jedipunkz/gm/internal/repo"
)

func (a *app) create(args []string) error {
	fs := flag.NewFlagSet("gm create", flag.ExitOnError)
	ssh := fs.Bool("p", false, "set the origin remote to its SSH URL")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if fs.NArg() != 1 {
		return fmt.Errorf("usage: gm create [-p] <repo>|<user>/<repo>|<host>/<user>/<repo>")
	}

	u, err := repo.NormalizeURL(fs.Arg(0), *ssh)
	if err != nil {
		return err
	}
	dst := a.tree.PathFor(repo.RelPathOf(u))

	if entries, err := os.ReadDir(dst); err == nil && len(entries) > 0 {
		return fmt.Errorf("%s already exists and is not empty", dst)
	}
	if err := os.MkdirAll(dst, 0o755); err != nil {
		return err
	}
	if err := repo.Git("-C", dst, "init", "--quiet"); err != nil {
		return err
	}
	// Set origin up front so the first push needs no extra arguments.
	if err := repo.Git("-C", dst, "remote", "add", "origin", u.String()); err != nil {
		return err
	}
	_ = repo.Bump(dst)
	fmt.Println(dst)
	return nil
}
