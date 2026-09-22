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
