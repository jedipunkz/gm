package cli

import (
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"

	"github.com/jedipunkz/gm/internal/repo"
)

func (a *app) get(args []string) error {
	fs := flag.NewFlagSet("gm get", flag.ExitOnError)
	var update, ssh, shallow, silent, look, noRecursive bool
	fs.BoolVar(&update, "u", false, "update the repository if it is already cloned")
	fs.BoolVar(&update, "update", false, "update the repository if it is already cloned")
	fs.BoolVar(&ssh, "p", false, "clone via SSH")
	fs.BoolVar(&shallow, "shallow", false, "do a shallow clone")
	fs.BoolVar(&silent, "s", false, "clone quietly")
	fs.BoolVar(&silent, "silent", false, "clone quietly")
	fs.BoolVar(&look, "l", false, "open a shell in the repository afterwards")
	fs.BoolVar(&look, "look", false, "open a shell in the repository afterwards")
	fs.BoolVar(&noRecursive, "no-recursive", false, "do not clone submodules")
	branch := fs.String("b", "", "clone a single `branch`")
	fs.StringVar(branch, "branch", "", "clone a single `branch`")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if fs.NArg() == 0 {
		return fmt.Errorf("usage: gm get [options] <url>|<user>/<repo>|<repo>")
	}

	var last string
	for _, ref := range fs.Args() {
		u, err := repo.NormalizeURL(ref, ssh)
		if err != nil {
			return err
		}
		dst := a.tree.PathFor(repo.RelPathOf(u))
		last = dst

		if repo.IsRepo(dst) {
			if !update {
				fmt.Fprintf(os.Stderr, "exists   %s\n", dst)
				_ = repo.Bump(dst)
				continue
			}
			fmt.Fprintf(os.Stderr, "update   %s\n", dst)
			if err := repo.Git("-C", dst, "remote", "update", "--prune"); err != nil {
				return err
			}
			_ = repo.Bump(dst)
			continue
		}
		if err := os.MkdirAll(filepath.Dir(dst), 0o755); err != nil {
			return err
		}

		clone := []string{"clone"}
		if shallow {
			clone = append(clone, "--depth", "1")
		}
		if *branch != "" {
			clone = append(clone, "--branch", *branch, "--single-branch")
		}
		if !noRecursive {
			clone = append(clone, "--recursive")
		}
		if silent {
			clone = append(clone, "--quiet")
		}
		clone = append(clone, u.String(), dst)

		fmt.Fprintf(os.Stderr, "clone    %s -> %s\n", u, dst)
		if err := repo.Git(clone...); err != nil {
			return err
		}
		_ = repo.Bump(dst)
	}

	if look && last != "" {
		return lookIn(last)
	}
	return nil
}

// lookIn drops the user into a shell inside the repository just cloned.
func lookIn(dir string) error {
	sh := os.Getenv("SHELL")
	if sh == "" {
		sh = "/bin/sh"
	}
	cmd := exec.Command(sh)
	cmd.Dir = dir
	cmd.Stdin, cmd.Stdout, cmd.Stderr = os.Stdin, os.Stdout, os.Stderr
	cmd.Env = append(os.Environ(), "GM_LOOK="+dir)
	return cmd.Run()
}
