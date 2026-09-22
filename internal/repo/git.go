package repo

import (
	"os"
	"os/exec"
	"strings"
)

// Git runs git with its output on the terminal, for the commands whose
// progress the user wants to watch.
func Git(args ...string) error {
	cmd := exec.Command("git", args...)
	cmd.Stdout, cmd.Stderr, cmd.Stdin = os.Stderr, os.Stderr, os.Stdin
	return cmd.Run()
}

// GitIn runs git inside dir and returns its trimmed output.
func GitIn(dir string, args ...string) (string, error) {
	cmd := exec.Command("git", args...)
	cmd.Dir = dir
	out, err := cmd.Output()
	return strings.TrimSpace(string(out)), err
}

// GitConfigAll reads every value of a git config key, nil when it is unset.
func GitConfigAll(key string) []string {
	out, err := exec.Command("git", "config", "--path", "--get-all", key).Output()
	if err != nil {
		return nil
	}
	var vs []string
	for _, l := range strings.Split(string(out), "\n") {
		if l = strings.TrimSpace(l); l != "" {
			vs = append(vs, l)
		}
	}
	return vs
}

// Status is what git says about a working copy. Every field is best-effort:
// a repository git cannot read still has to be listed and jumped to.
type Status struct {
	Remote string
	Branch string
	Commit string // "<hash>  <relative date>  <subject>"
	Dirty  int    // changed files
}

// Describe collects the status of one working copy. It shells out four times,
// so callers keep it off any hot path.
func Describe(dir string) Status {
	var s Status
	s.Branch, _ = GitIn(dir, "rev-parse", "--abbrev-ref", "HEAD")
	s.Remote, _ = GitIn(dir, "remote", "get-url", "origin")
	s.Commit, _ = GitIn(dir, "log", "-1", "--format=%h  %cr  %s")
	if out, err := GitIn(dir, "status", "--porcelain"); err == nil && out != "" {
		s.Dirty = len(strings.Split(out, "\n"))
	}
	return s
}

// IsDirty reports whether the working copy has uncommitted changes.
func IsDirty(dir string) (bool, error) {
	out, err := GitIn(dir, "status", "--porcelain")
	return out != "", err
}

// Worktree is one checkout of a repository: the main one, plus whatever
// `git worktree add` created.
type Worktree struct {
	Path   string
	Branch string // short name, empty when HEAD is detached
	Head   string // commit hash
	Bare   bool
}

// Label names a worktree in one word: its branch, or a detached HEAD's hash.
func (w Worktree) Label() string {
	switch {
	case w.Bare:
		return "(bare)"
	case w.Branch != "":
		return w.Branch
	case len(w.Head) >= 7:
		return w.Head[:7]
	default:
		return "(detached)"
	}
}

// Worktrees lists the checkouts of the repository at dir, the main one first,
// which is the order git itself reports.
func Worktrees(dir string) ([]Worktree, error) {
	out, err := GitIn(dir, "worktree", "list", "--porcelain")
	if err != nil {
		return nil, err
	}
	return parseWorktrees(out), nil
}

// parseWorktrees reads `git worktree list --porcelain`: records separated by a
// blank line, each a "worktree <path>" line followed by attributes.
func parseWorktrees(out string) []Worktree {
	var (
		list []Worktree
		w    Worktree
	)
	flush := func() {
		if w.Path != "" {
			list = append(list, w)
		}
		w = Worktree{}
	}
	for _, line := range strings.Split(out, "\n") {
		line = strings.TrimRight(line, "\r")
		key, val, _ := strings.Cut(line, " ")
		switch key {
		case "worktree":
			flush()
			w.Path = val
		case "HEAD":
			w.Head = val
		case "branch":
			w.Branch = strings.TrimPrefix(val, "refs/heads/")
		case "bare":
			w.Bare = true
		}
	}
	flush()
	return list
}
