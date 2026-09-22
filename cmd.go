package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
)

func run(name string, args ...string) error {
	cmd := exec.Command(name, args...)
	cmd.Stdout, cmd.Stderr, cmd.Stdin = os.Stderr, os.Stderr, os.Stdin
	return cmd.Run()
}

func runSilent(name string, args ...string) error {
	return exec.Command(name, args...).Run()
}

func capture(dir, name string, args ...string) (string, error) {
	cmd := exec.Command(name, args...)
	cmd.Dir = dir
	out, err := cmd.Output()
	return strings.TrimSpace(string(out)), err
}

// confirm asks before anything destructive. A non-tty answers "no".
func confirm(prompt string) bool {
	fmt.Fprintf(os.Stderr, "%s [y/N]: ", prompt)
	s := bufio.NewScanner(os.Stdin)
	if !s.Scan() {
		return false
	}
	a := strings.ToLower(strings.TrimSpace(s.Text()))
	return a == "y" || a == "yes"
}

func cmdGet(args []string) error {
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

	root, err := PrimaryRoot()
	if err != nil {
		return err
	}
	var last string
	for _, ref := range fs.Args() {
		u, err := NormalizeURL(ref, ssh)
		if err != nil {
			return err
		}
		dst := filepath.Join(root, filepath.FromSlash(RelPathOf(u)))
		last = dst

		if IsRepo(dst) {
			if !update {
				fmt.Fprintf(os.Stderr, "exists   %s\n", dst)
				_ = Bump(dst)
				continue
			}
			fmt.Fprintf(os.Stderr, "update   %s\n", dst)
			if err := run("git", "-C", dst, "remote", "update", "--prune"); err != nil {
				return err
			}
			_ = Bump(dst)
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
		if err := run("git", clone...); err != nil {
			return err
		}
		_ = Bump(dst)
	}

	if look && last != "" {
		return lookIn(last)
	}
	return nil
}

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

func cmdList(args []string) error {
	fs := flag.NewFlagSet("gm list", flag.ExitOnError)
	var full, exact, unique bool
	fs.BoolVar(&full, "p", false, "print full paths")
	fs.BoolVar(&full, "full-path", false, "print full paths")
	fs.BoolVar(&exact, "e", false, "match the query exactly")
	fs.BoolVar(&exact, "exact", false, "match the query exactly")
	fs.BoolVar(&unique, "unique", false, "print the shortest unambiguous path")
	if err := fs.Parse(args); err != nil {
		return err
	}
	query := strings.Join(fs.Args(), " ")

	repos, err := List()
	if err != nil {
		return err
	}
	var hits []Repo
	for _, r := range repos {
		if Match(r.Rel, query, exact) {
			hits = append(hits, r)
		}
	}
	sort.Slice(hits, func(i, j int) bool { return hits[i].Rel < hits[j].Rel })

	out := bufio.NewWriter(os.Stdout)
	defer out.Flush()
	var short []string
	if unique {
		short = ShortestUnique(hits)
	}
	for i, r := range hits {
		switch {
		case full:
			fmt.Fprintln(out, r.Path())
		case unique:
			fmt.Fprintln(out, short[i])
		default:
			fmt.Fprintln(out, r.Rel)
		}
	}
	return nil
}

// resolve finds the one repository a query names, erroring on ambiguity so a
// wrong repo is never removed.
func resolve(query string) (Repo, error) {
	repos, err := List()
	if err != nil {
		return Repo{}, err
	}
	var exactHits, looseHits []Repo
	for _, r := range repos {
		if Match(r.Rel, query, true) {
			exactHits = append(exactHits, r)
		} else if Match(r.Rel, query, false) {
			looseHits = append(looseHits, r)
		}
	}
	hits := exactHits
	if len(hits) == 0 {
		hits = looseHits
	}
	switch len(hits) {
	case 0:
		return Repo{}, fmt.Errorf("no repository matches %q", query)
	case 1:
		return hits[0], nil
	default:
		var b strings.Builder
		fmt.Fprintf(&b, "%q matches %d repositories:", query, len(hits))
		for _, r := range hits {
			fmt.Fprintf(&b, "\n  %s", r.Rel)
		}
		return Repo{}, fmt.Errorf("%s", b.String())
	}
}

func cmdRm(args []string) error {
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
		r, err := resolve(q)
		if err != nil {
			return err
		}
		if *dryRun {
			fmt.Fprintf(os.Stderr, "would remove %s\n", r.Path())
			continue
		}
		if dirty, _ := isDirty(r.Path()); dirty {
			fmt.Fprintf(os.Stderr, "warning: %s has uncommitted changes\n", r.Rel)
		}
		if !*yes && !confirm("remove "+r.Path()+"?") {
			fmt.Fprintln(os.Stderr, "skipped")
			continue
		}
		if err := os.RemoveAll(r.Path()); err != nil {
			return err
		}
		pruneEmptyParents(r.Root, filepath.Dir(r.Path()))
		fmt.Fprintf(os.Stderr, "removed  %s\n", r.Path())
	}
	return nil
}

// pruneEmptyParents removes the host/user directories a deleted repo leaves
// behind, stopping at the root.
func pruneEmptyParents(root, dir string) {
	for strings.HasPrefix(dir, root+string(filepath.Separator)) {
		if os.Remove(dir) != nil {
			return
		}
		dir = filepath.Dir(dir)
	}
}

func isDirty(dir string) (bool, error) {
	out, err := capture(dir, "git", "status", "--porcelain")
	return out != "", err
}

func cmdCreate(args []string) error {
	fs := flag.NewFlagSet("gm create", flag.ExitOnError)
	ssh := fs.Bool("p", false, "set the origin remote to its SSH URL")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if fs.NArg() != 1 {
		return fmt.Errorf("usage: gm create [-p] <repo>|<user>/<repo>|<host>/<user>/<repo>")
	}

	u, err := NormalizeURL(fs.Arg(0), *ssh)
	if err != nil {
		return err
	}
	root, err := PrimaryRoot()
	if err != nil {
		return err
	}
	dst := filepath.Join(root, filepath.FromSlash(RelPathOf(u)))

	if entries, err := os.ReadDir(dst); err == nil && len(entries) > 0 {
		return fmt.Errorf("%s already exists and is not empty", dst)
	}
	if err := os.MkdirAll(dst, 0o755); err != nil {
		return err
	}
	if err := run("git", "-C", dst, "init", "--quiet"); err != nil {
		return err
	}
	// Set origin up front so the first push needs no extra arguments.
	if err := run("git", "-C", dst, "remote", "add", "origin", u.String()); err != nil {
		return err
	}
	_ = Bump(dst)
	fmt.Println(dst)
	return nil
}

func cmdMigrate(args []string) error {
	fs := flag.NewFlagSet("gm migrate", flag.ExitOnError)
	dryRun := fs.Bool("dry-run", false, "show what would move")
	yes := fs.Bool("y", false, "skip the confirmation prompt")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if fs.NArg() == 0 {
		return fmt.Errorf("usage: gm migrate [--dry-run] [-y] <directory>...")
	}

	root, err := PrimaryRoot()
	if err != nil {
		return err
	}
	for _, arg := range fs.Args() {
		src, err := filepath.Abs(arg)
		if err != nil {
			return err
		}
		if !IsRepo(src) {
			return fmt.Errorf("%s is not a repository", src)
		}
		// A .git file means a worktree or submodule; moving it breaks the link.
		if st, err := os.Stat(filepath.Join(src, ".git")); err == nil && !st.IsDir() {
			return fmt.Errorf("%s is a worktree or submodule and cannot be moved on its own", src)
		}
		remote, err := capture(src, "git", "remote", "get-url", "origin")
		if err != nil || remote == "" {
			return fmt.Errorf("%s has no origin remote", src)
		}
		u, err := NormalizeURL(remote, false)
		if err != nil {
			return err
		}
		dst := filepath.Join(root, filepath.FromSlash(RelPathOf(u)))
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
			if err := run("mv", src, dst); err != nil {
				return fmt.Errorf("moving %s to %s: %w", src, dst, err)
			}
		}
		_ = Bump(dst)
		fmt.Fprintf(os.Stderr, "moved    %s -> %s\n", src, dst)
		fmt.Println(dst)
	}
	return nil
}

func cmdRoot(args []string) error {
	fs := flag.NewFlagSet("gm root", flag.ExitOnError)
	all := fs.Bool("all", false, "show every root")
	if err := fs.Parse(args); err != nil {
		return err
	}
	roots, err := Roots()
	if err != nil {
		return err
	}
	if !*all {
		roots = roots[:1]
	}
	for _, r := range roots {
		fmt.Println(r)
	}
	return nil
}
