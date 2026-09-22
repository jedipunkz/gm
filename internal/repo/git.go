package repo

import (
	"os"
	"os/exec"
	"strconv"
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
	Remote  string
	Branch  string
	Commits []Commit // newest first
	Dirty   int      // changed files
}

// recentCommits is how many commits Describe collects. Three answers "is this
// the repository I mean?" without turning the details pane into a log; the
// pane draws fewer when it is short of height.
const recentCommits = 3

// Describe collects the status of one working copy. It shells out four times,
// so callers keep it off any hot path.
func Describe(dir string) Status {
	var s Status
	s.Branch, _ = GitIn(dir, "rev-parse", "--abbrev-ref", "HEAD")
	s.Remote, _ = GitIn(dir, "remote", "get-url", "origin")
	s.Commits = commits(dir)
	if out, err := GitIn(dir, "status", "--porcelain"); err == nil && out != "" {
		s.Dirty = len(strings.Split(out, "\n"))
	}
	return s
}

// Commit is one line of the log, split so the details pane can colour its
// parts the way `git log --oneline --decorate` does.
type Commit struct {
	Hash    string
	Refs    []Ref // the decorations, in the order git printed them
	Subject string
}

// RefKind says how a decoration reads. git gives each kind its own colour,
// and so does the finder.
type RefKind int

const (
	RefLocal  RefKind = iota // a branch in this repository
	RefHead                  // HEAD itself
	RefRemote                // a remote-tracking branch
	RefTag                   // a tag
)

// Ref is one decoration, named as git prints it ("main", "origin/main",
// "tag: v1.0").
type Ref struct {
	Name string
	Kind RefKind
}

// commits reads the newest commits and their decorations.
func commits(dir string) []Commit {
	out, err := GitIn(dir, "log", "-n", strconv.Itoa(recentCommits),
		"--format=%h"+commitFormatSep+"%D"+commitFormatSep+"%s")
	if err != nil {
		return nil
	}
	return parseCommits(out, remoteNames(dir))
}

// remoteNames lists the configured remotes. Without them "origin/main" and a
// local branch called "release/main" look alike: both are a name with a
// slash in it.
func remoteNames(dir string) []string {
	out, err := GitIn(dir, "remote")
	if err != nil || out == "" {
		return nil
	}
	return strings.Split(out, "\n")
}

// The fields of one log line are separated by a NUL, which cannot appear in a
// hash, a ref name or a subject. commitFormatSep is git's own escape for it,
// written in the --format argument; a real NUL there would truncate the
// argument on the way to exec. commitSep is the byte git then emits.
const (
	commitFormatSep = "%x00"
	commitSep       = "\x00"
)

// parseCommits reads the lines commits asked git for. A line git could not
// format is dropped rather than shown half-parsed.
func parseCommits(out string, remotes []string) []Commit {
	if out == "" {
		return nil
	}
	var list []Commit
	for _, line := range strings.Split(out, "\n") {
		f := strings.SplitN(line, commitSep, 3)
		if len(f) != 3 {
			continue
		}
		list = append(list, Commit{
			Hash:    f[0],
			Refs:    parseRefs(f[1], remotes),
			Subject: f[2],
		})
	}
	return list
}

// parseRefs reads git's %D decoration list: comma-separated names, where the
// checked-out branch appears as "HEAD -> name" and a tag as "tag: name".
func parseRefs(d string, remotes []string) []Ref {
	d = strings.TrimSpace(d)
	if d == "" {
		return nil
	}
	var refs []Ref
	for _, part := range strings.Split(d, ",") {
		part = strings.TrimSpace(part)
		if part == "" {
			continue
		}
		if head, branch, found := strings.Cut(part, " -> "); found {
			refs = append(refs, Ref{Name: strings.TrimSpace(head), Kind: RefHead})
			part = strings.TrimSpace(branch)
		}
		refs = append(refs, Ref{Name: part, Kind: refKind(part, remotes)})
	}
	return refs
}

func refKind(name string, remotes []string) RefKind {
	switch {
	case name == "HEAD":
		return RefHead
	case strings.HasPrefix(name, "tag: "):
		return RefTag
	}
	for _, r := range remotes {
		if r != "" && strings.HasPrefix(name, r+"/") {
			return RefRemote
		}
	}
	return RefLocal
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
