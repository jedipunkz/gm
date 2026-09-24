package repo

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
)

// Git runs git with its output on the terminal, for the commands whose
// progress the user wants to watch.
func Git(args ...string) error {
	cmd := exec.Command("git", args...)
	cmd.Stdout, cmd.Stderr, cmd.Stdin = os.Stderr, os.Stderr, os.Stdin
	return cmd.Run()
}

// gitQuiet runs git without letting it near the terminal: the finder may own
// the screen, and a stray "Preparing worktree" line would land on top of it.
// Whatever git printed comes back in the error instead of being shown.
func gitQuiet(args ...string) error {
	out, err := exec.Command("git", args...).CombinedOutput()
	if err == nil {
		return nil
	}
	if msg := gitMessage(out); msg != "" {
		return errors.New(msg)
	}
	return err
}

// gitMessage picks the line worth repeating out of git's output: what it
// complained about, or failing that the first thing it said.
func gitMessage(out []byte) string {
	var first string
	for _, line := range strings.Split(string(out), "\n") {
		line = strings.TrimSpace(line)
		switch {
		case line == "" || strings.HasPrefix(line, "hint:"):
			continue
		case strings.HasPrefix(line, "fatal: "), strings.HasPrefix(line, "error: "):
			return strings.TrimSpace(line[strings.Index(line, " ")+1:])
		case first == "":
			first = line
		}
	}
	return first
}

// ValidBranch reports whether git would take this as a branch name. gm asks
// before it builds a path out of one: a name like "../.." would otherwise
// make a directory outside the tree before git ever saw it.
func ValidBranch(name string) bool {
	if name == "" || strings.HasPrefix(name, "-") || strings.Contains(name, "..") {
		return false
	}
	return exec.Command("git", "check-ref-format", "--branch", name).Run() == nil
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
	Remote    string
	Branch    string
	Commits   []Commit   // newest first
	Dirty     int        // changed files
	Worktrees []Worktree // the other checkouts, without the main one
}

// recentCommits is how many commits Describe collects. Three answers "is this
// the repository I mean?" without turning the details pane into a log; the
// pane draws fewer when it is short of height.
const recentCommits = 3

// Describe collects the status of one working copy. It still shells out, so
// callers keep it off any hot path and off rows the cursor only passed over.
func Describe(dir string) Status {
	var s Status
	s.Branch, _ = GitIn(dir, "rev-parse", "--abbrev-ref", "HEAD")
	names, origin := remotes(dir)
	s.Remote = origin
	s.Commits = commits(dir, names)
	s.Worktrees = otherWorktrees(dir)
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

// commits reads the newest commits and their decorations. The remote names
// come from the caller: without them "origin/main" and a local branch called
// "release/main" look alike, both being a name with a slash in it.
func commits(dir string, remotes []string) []Commit {
	out, err := GitIn(dir, "log", "-n", strconv.Itoa(recentCommits),
		"--format=%h"+commitFormatSep+"%D"+commitFormatSep+"%s")
	if err != nil {
		return nil
	}
	return parseCommits(out, remotes)
}

// remotes reads the configured remotes in one call: their names, and origin's
// URL. Both halves come out of `git remote -v`, so asking git separately for
// each of them is one process more than the answer costs.
// OriginURL is origin's URL on its own, for the caller that wants nothing
// else about the repository and should not pay for a whole Describe.
func OriginURL(dir string) string {
	_, origin := remotes(dir)
	return origin
}

func remotes(dir string) (names []string, origin string) {
	out, err := GitIn(dir, "remote", "-v")
	if err != nil || out == "" {
		return nil, ""
	}
	// Each remote gets a fetch line and a push line: "origin<TAB>URL (fetch)".
	seen := map[string]bool{}
	for _, l := range strings.Split(out, "\n") {
		name, rest, ok := strings.Cut(l, "\t")
		if !ok {
			continue
		}
		if !seen[name] {
			seen[name] = true
			names = append(names, name)
		}
		if name == "origin" && origin == "" {
			origin, _, _ = strings.Cut(rest, " ")
		}
	}
	return names, origin
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
	Path        string
	Branch      string // short name, empty when HEAD is detached
	Head        string // commit hash
	Bare        bool
	CommittedAt int64 // unix seconds of the branch tip; 0 when unknown
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
	list := parseWorktrees(out)
	dates := branchDates(dir)
	for i := range list {
		list[i].CommittedAt = dates[list[i].Branch]
	}
	return list, nil
}

// branchDates is when each branch was last committed to. One call for the
// whole repository, so dating the checkouts costs a process and not one per
// checkout; a detached HEAD has no branch here and stays undated.
func branchDates(dir string) map[string]int64 {
	out, err := GitIn(dir, "for-each-ref", "--format=%(committerdate:unix) %(refname:short)", "refs/heads")
	if err != nil || out == "" {
		return nil
	}
	dates := map[string]int64{}
	for _, line := range strings.Split(out, "\n") {
		ts, name, ok := strings.Cut(strings.TrimSpace(line), " ")
		if !ok {
			continue
		}
		if n, err := strconv.ParseInt(ts, 10, 64); err == nil {
			dates[name] = n
		}
	}
	return dates
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

// dirtyWorkers bounds the git processes a scan runs at once. Each one is a
// short-lived process doing a little disk work, so a handful keeps the disk
// busy without forking hundreds of them on a large tree.
//
// ponytail: a fixed number, tune it if a big tree on a slow disk says so.
const dirtyWorkers = 8

// DirtyMap reports which of these working copies have uncommitted changes.
// A repository git cannot answer for counts as clean: the point is to find
// work in progress, not to report on git's health.
func DirtyMap(paths []string) map[string]bool {
	out := make(map[string]bool, len(paths))
	for p, s := range StatusMap(paths) {
		out[p] = s.Dirty > 0
	}
	return out
}

// State is what a repository holds that nothing else does: work not
// committed, and commits not pushed. It comes out of one `git status
// --porcelain -b`, which reads the refs already on disk and never fetches, so
// the answer is as fresh as the last time something did.
type State struct {
	Branch   string // as git names it, "HEAD" when detached
	Upstream bool   // the branch tracks something
	Ahead    int    // commits here the upstream does not have
	Behind   int    // and the other way round
	Dirty    int    // changed files
}

// Unfinished reports whether this is work someone walked away from. Being
// behind is the remote's news rather than the user's, so it alone does not
// make a repository worth listing.
func (s State) Unfinished() bool { return s.Dirty > 0 || s.Ahead > 0 }

// StatusOf reads one repository's state.
func StatusOf(dir string) State {
	out, err := GitIn(dir, "status", "--porcelain", "-b")
	if err != nil {
		return State{}
	}
	return parseStatus(out)
}

// StatusMap reads every path, a few at a time: git is the slow part and the
// disk is shared, so more workers than this buys nothing.
func StatusMap(paths []string) map[string]State {
	out := make(map[string]State, len(paths))
	var (
		mu sync.Mutex
		wg sync.WaitGroup
	)
	sem := make(chan struct{}, dirtyWorkers)
	for _, p := range paths {
		wg.Add(1)
		go func() {
			defer wg.Done()
			sem <- struct{}{}
			defer func() { <-sem }()

			s := StatusOf(p)
			mu.Lock()
			out[p] = s
			mu.Unlock()
		}()
	}
	wg.Wait()
	return out
}

// parseStatus reads `git status --porcelain -b`: a "## " header naming the
// branch and how far it has drifted, then a line per changed file.
func parseStatus(out string) State {
	var s State
	for i, l := range strings.Split(out, "\n") {
		if i == 0 && strings.HasPrefix(l, "## ") {
			s.parseBranch(strings.TrimPrefix(l, "## "))
			continue
		}
		if l != "" {
			s.Dirty++
		}
	}
	return s
}

// parseBranch reads the header, which git writes as one of:
//
//	main...origin/main [ahead 1, behind 2]
//	main...origin/main
//	main
//	HEAD (no branch)
//	No commits yet on main
func (s *State) parseBranch(l string) {
	if i := strings.LastIndex(l, " ["); i >= 0 && strings.HasSuffix(l, "]") {
		for _, part := range strings.Split(l[i+2:len(l)-1], ", ") {
			// "gone" has no number and means the upstream was deleted.
			kind, num, ok := strings.Cut(part, " ")
			n, err := strconv.Atoi(num)
			if !ok || err != nil {
				continue
			}
			switch kind {
			case "ahead":
				s.Ahead = n
			case "behind":
				s.Behind = n
			}
		}
		l = l[:i]
	}
	l = strings.TrimPrefix(l, "No commits yet on ")
	if b, up, ok := strings.Cut(l, "..."); ok {
		s.Branch, s.Upstream = b, up != ""
		return
	}
	s.Branch = strings.TrimSuffix(l, " (no branch)")
}

// WorktreeRoot is where gm keeps the checkouts it creates:
//
//	<root>/.worktrees/<host>/<user>/<repo>/<branch>
//
// The dot matters. A worktree has a .git file, so IsRepo answers yes for one,
// and a worktree in the open would be listed as a repository and refused by
// gm migrate. FindRepos never descends into a dotted directory, so this one
// stays out of the way.
const WorktreeRoot = ".worktrees"

// WorktreesDir is where gm keeps every checkout of one repository.
func WorktreesDir(r Repo) string {
	return filepath.Join(r.Root, WorktreeRoot, filepath.FromSlash(r.Rel))
}

// WorktreeDir is where a branch's checkout of r belongs.
func (t *Tree) WorktreeDir(r Repo, branch string) string {
	return filepath.Join(WorktreesDir(r), filepath.FromSlash(branch))
}

// BranchExists reports whether the repository already has this branch, which
// decides whether a worktree starts one or checks one out.
func BranchExists(dir, branch string) bool {
	err := exec.Command("git", "-C", dir, "show-ref", "--verify", "--quiet", "refs/heads/"+branch).Run()
	return err == nil
}

// AddWorktree checks branch out at dir, starting the branch from HEAD when it
// does not exist yet.
func AddWorktree(repoDir, dir, branch string) error {
	if !ValidBranch(branch) {
		return fmt.Errorf("%q is not a branch name", branch)
	}
	if err := os.MkdirAll(filepath.Dir(dir), 0o755); err != nil {
		return err
	}
	args := []string{"-C", repoDir, "worktree", "add"}
	if BranchExists(repoDir, branch) {
		args = append(args, dir, branch)
	} else {
		args = append(args, "-b", branch, dir)
	}
	return gitQuiet(args...)
}

// RemoveWorktree takes a checkout away. force is what the caller has already
// confirmed: git refuses on its own when there is work in it.
func RemoveWorktree(repoDir, dir string, force bool) error {
	args := []string{"-C", repoDir, "worktree", "remove"}
	if force {
		args = append(args, "--force")
	}
	return gitQuiet(append(args, dir)...)
}
