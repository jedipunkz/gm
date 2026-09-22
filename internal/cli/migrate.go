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
	if err := fs.Parse(args); err != nil {
		return err
	}
	if fs.NArg() == 0 {
		return fmt.Errorf("usage: gm migrate [--dry-run] [-y] <directory>...")
	}

	for _, arg := range fs.Args() {
		src, err := filepath.Abs(arg)
		if err != nil {
			return err
		}
		if !repo.IsRepo(src) {
			return fmt.Errorf("%s is not a repository", src)
		}
		// A .git file means a worktree or submodule; moving it breaks the link.
		if st, err := os.Stat(filepath.Join(src, ".git")); err == nil && !st.IsDir() {
			return fmt.Errorf("%s is a worktree or submodule and cannot be moved on its own", src)
		}
		remote, err := repo.GitIn(src, "remote", "get-url", "origin")
		if err != nil || remote == "" {
			return fmt.Errorf("%s has no origin remote", src)
		}
		u, err := repo.NormalizeURL(remote, false)
		if err != nil {
			return err
		}
		dst := a.tree.PathFor(repo.RelPathOf(u))
		if src == dst {
			fmt.Fprintf(os.Stderr, "already in place: %s\n", dst)
			continue
		}
		if _, err := os.Stat(dst); err == nil {
			return fmt.Errorf("%s already exists", dst)
		}
		if *dryRun {
			fmt.Fprintf(os.Stderr, "would move %s -> %s\n", src, dst)
			continue
		}
		if !*yes && !confirm(fmt.Sprintf("move %s -> %s?", src, dst)) {
			fmt.Fprintln(os.Stderr, "skipped")
			continue
		}
		if err := os.MkdirAll(filepath.Dir(dst), 0o755); err != nil {
			return err
		}
		if err := os.Rename(src, dst); err != nil {
			// Rename fails across filesystems; mv copies and unlinks instead.
			mv := exec.Command("mv", src, dst)
			mv.Stdout, mv.Stderr = os.Stderr, os.Stderr
			if err := mv.Run(); err != nil {
				return fmt.Errorf("moving %s to %s: %w", src, dst, err)
			}
		}
		_ = repo.Bump(dst)
		fmt.Fprintf(os.Stderr, "moved    %s -> %s\n", src, dst)
		fmt.Println(dst)
	}
	return nil
}
