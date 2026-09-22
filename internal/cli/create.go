package cli

import (
	"flag"
	"fmt"

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

	r, err := a.tree.Create(fs.Arg(0), *ssh)
	if err != nil {
		return err
	}
	_ = repo.Bump(r.Path())
	fmt.Println(r.Path())
	return nil
}
