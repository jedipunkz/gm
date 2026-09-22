package main

import (
	"errors"
	"fmt"
	"io/fs"
	"net/url"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
)

// Repo is a locally cloned repository living under one of the roots.
type Repo struct {
	Root string // root directory it was found under
	Rel  string // slash-separated path relative to Root, e.g. "github.com/x-motemen/ghq"
}

func (r Repo) Path() string { return filepath.Join(r.Root, filepath.FromSlash(r.Rel)) }

// Roots returns the directories gm keeps repositories in, most preferred first.
//
//	$GM_ROOT > gm.toml root > git config gm.root >
//	$GHQ_ROOT > git config ghq.root > ~/ghq
//
// The ghq fallbacks make gm a drop-in replacement for an existing ghq tree.
func Roots() ([]string, error) {
	if v := os.Getenv("GM_ROOT"); v != "" {
		return expandAll(filepath.SplitList(v))
	}
	cfg, err := configRoots()
	if err != nil {
		return nil, err
	}
	if len(cfg) > 0 {
		return expandAll(cfg)
	}
	if vs := gitConfigAll("gm.root"); len(vs) > 0 {
		return expandAll(vs)
	}
	if v := os.Getenv("GHQ_ROOT"); v != "" {
		return expandAll(filepath.SplitList(v))
	}
	if vs := gitConfigAll("ghq.root"); len(vs) > 0 {
		return expandAll(vs)
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return nil, err
	}
	return []string{filepath.Join(home, "ghq")}, nil
}

// PrimaryRoot is the root new repositories are cloned into.
func PrimaryRoot() (string, error) {
	roots, err := Roots()
	if err != nil {
		return "", err
	}
	return roots[0], nil
}

func expandAll(paths []string) ([]string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return nil, err
	}
	out := make([]string, 0, len(paths))
	seen := map[string]bool{}
	for _, p := range paths {
		if p == "~" || strings.HasPrefix(p, "~/") {
			p = filepath.Join(home, strings.TrimPrefix(p, "~"))
		}
		p = filepath.Clean(p)
		if !seen[p] {
			seen[p] = true
			out = append(out, p)
		}
	}
	if len(out) == 0 {
		return nil, errors.New("no repository root configured")
	}
	return out, nil
}

func gitConfigAll(key string) []string {
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

var (
	schemeRe  = regexp.MustCompile(`^[A-Za-z][A-Za-z0-9+.\-]*://`)
	scpLikeRe = regexp.MustCompile(`^(?:([^@/]+)@)?([^:/]+):(/?.+)$`)
	hostRe    = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9.\-]*\.[A-Za-z]{2,}(?::\d{1,5})?$`)
)

// NormalizeURL turns every shorthand gm accepts into a real URL:
//
//	https://github.com/u/r  -> as-is
//	git@github.com:u/r.git  -> ssh://git@github.com/u/r.git
//	example.com/u/r         -> https://example.com/u/r
//	u/r                     -> https://github.com/u/r
//	r                       -> https://github.com/<you>/r
//
// With ssh set, https refs are rewritten to ssh://git@...
func NormalizeURL(ref string, ssh bool) (*url.URL, error) {
	ref = strings.TrimRight(strings.TrimSpace(ref), "/")
	if ref == "" {
		return nil, errors.New("empty repository reference")
	}

	if !schemeRe.MatchString(ref) {
		if m := scpLikeRe.FindStringSubmatch(ref); m != nil && hostRe.MatchString(m[2]) {
			user := ""
			if m[1] != "" {
				user = m[1] + "@"
			}
			ref = "ssh://" + user + m[2] + "/" + strings.TrimPrefix(m[3], "/")
		} else {
			parts := strings.Split(ref, "/")
			switch {
			case len(parts) >= 3 && hostRe.MatchString(parts[0]):
				ref = "https://" + ref
			case len(parts) == 2:
				ref = "https://github.com/" + ref
			case len(parts) == 1:
				me, err := selfName()
				if err != nil {
					return nil, err
				}
				ref = "https://github.com/" + me + "/" + ref
			default:
				return nil, fmt.Errorf("cannot make a repository URL out of %q", ref)
			}
		}
	}

	u, err := url.Parse(ref)
	if err != nil {
		return nil, err
	}
	if u.Hostname() == "" || strings.Trim(u.Path, "/") == "" {
		return nil, fmt.Errorf("cannot make a repository URL out of %q", ref)
	}
	if ssh && u.Scheme == "https" {
		u.Scheme = "ssh"
		u.User = url.User("git")
	}
	return u, nil
}

// selfName is the GitHub account used for one-word refs like "gm create foo".
func selfName() (string, error) {
	if out, err := exec.Command("git", "config", "--get", "github.user").Output(); err == nil {
		if s := strings.TrimSpace(string(out)); s != "" {
			return s, nil
		}
	}
	if u := os.Getenv("USER"); u != "" {
		return u, nil
	}
	return "", errors.New("cannot guess your account name; set `git config --global github.user <name>` or use <user>/<repo>")
}

// RelPathOf is where a URL lands under a root: host/path, minus any .git suffix.
func RelPathOf(u *url.URL) string {
	p := strings.Trim(u.Path, "/")
	p = strings.TrimSuffix(p, ".git")
	return strings.Join(append([]string{u.Hostname()}, strings.Split(p, "/")...), "/")
}

var vcsDirs = []string{".git", ".hg", ".svn"}

// IsRepo reports whether dir is the top of a working copy.
func IsRepo(dir string) bool {
	for _, d := range vcsDirs {
		// .git is a file, not a directory, inside a worktree or submodule.
		if _, err := os.Lstat(filepath.Join(dir, d)); err == nil {
			return true
		}
	}
	return false
}

// List walks every root and returns the repositories found, never descending
// into one.
func List() ([]Repo, error) {
	roots, err := Roots()
	if err != nil {
		return nil, err
	}
	var repos []Repo
	seen := map[string]bool{}
	for _, root := range roots {
		err := filepath.WalkDir(root, func(p string, d fs.DirEntry, err error) error {
			if err != nil || !d.IsDir() {
				return nil //nolint // unreadable entries are skipped, not fatal
			}
			if p != root && strings.HasPrefix(d.Name(), ".") {
				return fs.SkipDir
			}
			if !IsRepo(p) {
				return nil
			}
			rel, err := filepath.Rel(root, p)
			if err != nil {
				return nil
			}
			if !seen[p] {
				seen[p] = true
				repos = append(repos, Repo{Root: root, Rel: filepath.ToSlash(rel)})
			}
			return fs.SkipDir
		})
		if err != nil && !os.IsNotExist(err) {
			return nil, err
		}
	}
	return repos, nil
}

// Match reports whether a repository answers to query. Exact requires the
// query to equal the repo name, user/repo, or the whole host/user/repo path.
func Match(rel, query string, exact bool) bool {
	if query == "" {
		return true
	}
	if !exact {
		return strings.Contains(rel, query)
	}
	parts := strings.Split(rel, "/")
	for i := range parts {
		if strings.Join(parts[i:], "/") == query {
			return true
		}
	}
	return false
}

// ShortestUnique gives each repo the shortest trailing path that no other repo
// in the set shares ("ghq", else "x-motemen/ghq", else the full path).
func ShortestUnique(repos []Repo) []string {
	count := map[string]int{}
	for _, r := range repos {
		parts := strings.Split(r.Rel, "/")
		for i := range parts {
			count[strings.Join(parts[i:], "/")]++
		}
	}
	out := make([]string, len(repos))
	for i, r := range repos {
		parts := strings.Split(r.Rel, "/")
		out[i] = r.Rel
		for j := len(parts) - 1; j >= 0; j-- {
			if s := strings.Join(parts[j:], "/"); count[s] == 1 {
				out[i] = s
				break
			}
		}
	}
	return out
}
