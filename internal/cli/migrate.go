package cli

import (
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"

	"github.com/jedipunkz/gm/internal/repo"
)

func (a *app) migrate(args []string) error {
	fs := flag.NewFlagSet("gm migrate", flag.ExitOnError)
	dryRun := fs.Bool("dry-run", false, "show what would move")
	yes := fs.Bool("y", false, "skip the confirmation prompt")
	scan := fs.Bool("scan", false, "treat the arguments as directories to search for repositories")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if fs.NArg() == 0 {
		return fmt.Errorf("usage: gm migrate [--dry-run] [-y] [--scan] <directory>...")
	}

	srcs, err := a.migrateSources(fs.Args(), *scan)
	if err != nil {
		return err
	}
	for _, src := range srcs {
		p, err := a.migratePlan(src)
		if err != nil {
			return err
		}
		switch {
		case p.inPlace:
			fmt.Fprintf(os.Stderr, "in place %s\n", src)
			continue
		case p.problem != "":
			// A directory named on the command line is the user's claim that
			// it should move; a directory the scan turned up is only a
			// candidate, so it is skipped rather than fatal.
			if !*scan {
				return fmt.Errorf("%s %s", src, p.problem)
			}
			fmt.Fprintf(os.Stderr, "skip     %s: %s\n", src, p.problem)
			continue
		}

		if *dryRun {
			fmt.Fprintf(os.Stderr, "would move %s -> %s\n", src, p.dst)
			continue
		}
		if !*yes && !confirm(fmt.Sprintf("move %s -> %s?", src, p.dst)) {
			fmt.Fprintln(os.Stderr, "skipped")
			continue
		}
		if err := move(src, p.dst); err != nil {
			return err
		}
		_ = repo.Bump(p.dst)
		fmt.Fprintf(os.Stderr, "moved    %s -> %s\n", src, p.dst)
		fmt.Println(p.dst)
	}
	return nil
}

// migrateSources turns the arguments into the directories to consider. Each
// one is a repository, or with --scan a directory to search; repositories
// already under a root are dropped, since the whole point is to move the ones
// that are not.
func (a *app) migrateSources(args []string, scan bool) ([]string, error) {
	var srcs []string
	for _, arg := range args {
		dir, err := filepath.Abs(arg)
		if err != nil {
			return nil, err
		}
		if !scan {
			srcs = append(srcs, dir)
			continue
		}
		found, err := repo.FindRepos(dir)
		if err != nil {
			return nil, err
		}
		for _, p := range found {
			if a.tree.Contains(p) {
				continue
			}
			srcs = append(srcs, p)
		}
	}
	return srcs, nil
}

// plan is where one repository would move, or why it would not.
type plan struct {
	dst     string // where it goes
	inPlace bool   // already where gm would put it
	problem string // why it cannot move, phrased to follow the path
}

// migratePlan decides where a repository belongs, without touching anything.
func (a *app) migratePlan(src string) (plan, error) {
	if !repo.IsRepo(src) {
		return plan{problem: "is not a repository"}, nil
	}
	// A .git file means a worktree or submodule; moving it breaks the link.
	if st, err := os.Stat(filepath.Join(src, ".git")); err == nil && !st.IsDir() {
		return plan{problem: "is a worktree or submodule and cannot be moved on its own"}, nil
	}
	remote, err := repo.GitIn(src, "remote", "get-url", "origin")
	if err != nil || remote == "" {
		return plan{problem: "has no origin remote"}, nil
	}
	u, err := repo.NormalizeURL(remote, false)
	if err != nil {
		return plan{problem: fmt.Sprintf("has an origin gm cannot read (%s)", remote)}, nil
	}
	dst := a.tree.PathFor(repo.RelPathOf(u))
	if src == dst {
		return plan{inPlace: true}, nil
	}
	if _, err := os.Stat(dst); err == nil {
		return plan{problem: fmt.Sprintf("would move to %s, which already exists", dst)}, nil
	}
	return plan{dst: dst}, nil
}

// move renames src to dst, falling back to mv across filesystems.
func move(src, dst string) error {
	if err := os.MkdirAll(filepath.Dir(dst), 0o755); err != nil {
		return err
	}
	if err := os.Rename(src, dst); err == nil {
		return nil
	}
	// Rename fails across filesystems; mv copies and unlinks instead.
	cmd := exec.Command("mv", src, dst)
	cmd.Stdout, cmd.Stderr = os.Stderr, os.Stderr
	if err := cmd.Run(); err != nil {
		return fmt.Errorf("moving %s to %s: %w", src, dst, err)
	}
	return nil
}
