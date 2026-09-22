// Package repo owns everything about locally cloned repositories: where the
// roots are, what lives under them, and how a shorthand reference turns into a
// URL and a directory.
package repo

import (
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"strings"

	"github.com/jedipunkz/gm/internal/config"
)

// Repo is a locally cloned repository living under one of the roots.
type Repo struct {
	Root string // root directory it was found under
	Rel  string // slash-separated path relative to Root, e.g. "github.com/x-motemen/ghq"
}

func (r Repo) Path() string { return filepath.Join(r.Root, filepath.FromSlash(r.Rel)) }

// Tree is the set of roots gm keeps repositories in, resolved once and then
// carried around: every command works against the same answer, and gm.toml is
// read a single time per run.
type Tree struct {
	Roots []string // most preferred first
}

// Open resolves the roots:
//
//	$GM_ROOT > gm.toml root > git config gm.root >
//	$GHQ_ROOT > git config ghq.root > ~/ghq
//
// The ghq fallbacks make gm a drop-in replacement for an existing ghq tree.
func Open(cfg config.Config) (*Tree, error) {
	roots, err := resolveRoots(cfg)
	if err != nil {
		return nil, err
	}
	return &Tree{Roots: roots}, nil
}

func resolveRoots(cfg config.Config) ([]string, error) {
	if v := os.Getenv("GM_ROOT"); v != "" {
		return expandAll(filepath.SplitList(v))
	}
	fromFile, err := cfg.Roots()
	if err != nil {
		return nil, err
	}
	if len(fromFile) > 0 {
		return expandAll(fromFile)
	}
	if vs := GitConfigAll("gm.root"); len(vs) > 0 {
		return expandAll(vs)
	}
	if v := os.Getenv("GHQ_ROOT"); v != "" {
		return expandAll(filepath.SplitList(v))
	}
	if vs := GitConfigAll("ghq.root"); len(vs) > 0 {
		return expandAll(vs)
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return nil, err
	}
	return []string{filepath.Join(home, "ghq")}, nil
}

// Primary is the root new repositories are cloned into.
func (t *Tree) Primary() string { return t.Roots[0] }

// PathFor is where a URL lands under the primary root.
func (t *Tree) PathFor(rel string) string {
	return filepath.Join(t.Primary(), filepath.FromSlash(rel))
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

// FindRepos walks dir and returns the top directory of every working copy
// under it, never descending into one and never into a dotted directory. A
// directory that does not exist yields nothing rather than an error: a root
// gm has not cloned into yet is normal.
func FindRepos(dir string) ([]string, error) {
	var found []string
	err := filepath.WalkDir(dir, func(p string, d fs.DirEntry, err error) error {
		if err != nil || !d.IsDir() {
			return nil //nolint // unreadable entries are skipped, not fatal
		}
		if p != dir && strings.HasPrefix(d.Name(), ".") {
			return fs.SkipDir
		}
		if !IsRepo(p) {
			return nil
		}
		found = append(found, p)
		return fs.SkipDir
	})
	if err != nil && !os.IsNotExist(err) {
		return nil, err
	}
	return found, nil
}

// List walks every root and returns the repositories found.
func (t *Tree) List() ([]Repo, error) {
	var repos []Repo
	seen := map[string]bool{}
	for _, root := range t.Roots {
		paths, err := FindRepos(root)
		if err != nil {
			return nil, err
		}
		for _, p := range paths {
			rel, err := filepath.Rel(root, p)
			if err != nil || seen[p] {
				continue
			}
			seen[p] = true
			repos = append(repos, Repo{Root: root, Rel: filepath.ToSlash(rel)})
		}
	}
	return repos, nil
}

// Contains reports whether path already lives under one of the roots.
func (t *Tree) Contains(path string) bool {
	for _, root := range t.Roots {
		if path == root || strings.HasPrefix(path, root+string(filepath.Separator)) {
			return true
		}
	}
	return false
}

// At names the repository whose directory is exactly path, which is what the
// finder hands back: it picked a row, so there is nothing to resolve.
func (t *Tree) At(path string) (Repo, bool) {
	for _, root := range t.Roots {
		rel, err := filepath.Rel(root, path)
		if err != nil || rel == "." || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
			continue
		}
		return Repo{Root: root, Rel: filepath.ToSlash(rel)}, true
	}
	return Repo{}, false
}

// Resolve finds the one repository a query names, erroring on ambiguity so a
// wrong repository is never removed or moved.
func (t *Tree) Resolve(query string) (Repo, error) {
	repos, err := t.List()
	if err != nil {
		return Repo{}, err
	}
	var exact, loose []Repo
	for _, r := range repos {
		if Match(r.Rel, query, true) {
			exact = append(exact, r)
		} else if Match(r.Rel, query, false) {
			loose = append(loose, r)
		}
	}
	hits := exact
	if len(hits) == 0 {
		hits = loose
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
		return Repo{}, errors.New(b.String())
	}
}

// Match reports whether a repository answers to query. Exact requires the
// query to equal the repository name, user/repo, or the whole host/user/repo
// path.
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

// ShortestUnique gives each repository the shortest trailing path that no
// other repository in the set shares ("ghq", else "x-motemen/ghq", else the
// full path).
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

// PruneEmptyParents removes the host/user directories a deleted repository
// leaves behind, stopping at the root.
func PruneEmptyParents(root, dir string) {
	for strings.HasPrefix(dir, root+string(filepath.Separator)) {
		if os.Remove(dir) != nil {
			return
		}
		dir = filepath.Dir(dir)
	}
}

// Create makes an empty repository where ref says it belongs, with its origin
// already set so the first push needs no arguments, and returns it.
func (t *Tree) Create(ref string, ssh bool) (Repo, error) {
	u, err := NormalizeURL(ref, ssh)
	if err != nil {
		return Repo{}, err
	}
	rel := RelPathOf(u)
	dst := t.PathFor(rel)

	if entries, err := os.ReadDir(dst); err == nil && len(entries) > 0 {
		return Repo{}, fmt.Errorf("%s already exists and is not empty", dst)
	}
	if err := os.MkdirAll(dst, 0o755); err != nil {
		return Repo{}, err
	}
	// Quietly: this runs under the finder as well as from the command line.
	if err := gitQuiet("-C", dst, "init", "--quiet"); err != nil {
		return Repo{}, err
	}
	if err := gitQuiet("-C", dst, "remote", "add", "origin", u.String()); err != nil {
		return Repo{}, err
	}
	return Repo{Root: t.Primary(), Rel: rel}, nil
}

// Delete removes a repository and the host/user directories it leaves empty
// behind it. It asks nothing: the caller has already confirmed.
func Delete(r Repo) error {
	if err := os.RemoveAll(r.Path()); err != nil {
		return err
	}
	PruneEmptyParents(r.Root, filepath.Dir(r.Path()))
	return nil
}
