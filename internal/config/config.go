// Package config reads gm's own settings file, and nothing else: where the
// repositories live and which theme the finder wears.
package config

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/BurntSushi/toml"
)

// Config mirrors gm.toml.
//
//	root         = "~/ghq"     # or ["~/ghq", "~/src"], searched in order
//	theme        = "tokyonight"
//	launch_key   = "ctrl-g"    # the shell key that opens gm
//	worktree_key = "ctrl-w"    # the finder key that lists worktrees
//	branch_key   = "ctrl-l"    # the finder key that lists branches
//	pr_key       = "ctrl-m"    # the finder key that lists pull requests
//	remote_key   = "ctrl-alt-b"  # the finder key that opens the remote
//
// Root stays untyped because it takes either form; Roots resolves it.
type Config struct {
	Root        any    `toml:"root"`
	Theme       string `toml:"theme"`
	LaunchKey   string `toml:"launch_key"`
	WorktreeKey string `toml:"worktree_key"`
	BranchKey   string `toml:"branch_key"`
	PRKey       string `toml:"pr_key"`
	RemoteKey   string `toml:"remote_key"`
}

// Path is where gm looks for its settings:
//
//	$XDG_CONFIG_HOME/gm/gm.toml, else ~/.config/gm/gm.toml
func Path() (string, error) {
	dir := os.Getenv("XDG_CONFIG_HOME")
	if dir == "" {
		home, err := os.UserHomeDir()
		if err != nil {
			return "", err
		}
		dir = filepath.Join(home, ".config")
	}
	return filepath.Join(dir, "gm", "gm.toml"), nil
}

// Load reads gm.toml. A missing file is the zero Config, but a file that
// cannot be parsed is an error: silently cloning into the wrong tree is worse
// than refusing to run.
func Load() (Config, error) {
	path, err := Path()
	if err != nil {
		return Config{}, err
	}
	b, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return Config{}, nil
	}
	if err != nil {
		return Config{}, err
	}
	var c Config
	md, err := toml.Decode(string(b), &c)
	if err != nil {
		return Config{}, fmt.Errorf("%s: %w", path, err)
	}
	// A key gm does not know is a typo or a setting that has been renamed.
	// Ignoring it silently leaves the user staring at a file that says one
	// thing while gm does another.
	if left := md.Undecoded(); len(left) > 0 {
		names := make([]string, 0, len(left))
		for _, k := range left {
			names = append(names, strconv.Quote(k.String()))
		}
		return Config{}, fmt.Errorf("%s: unknown key %s", path, strings.Join(names, ", "))
	}
	return c, nil
}

// Roots normalizes the root field to a list. No root at all returns nil, which
// leaves the caller's own fallbacks in charge.
func (c Config) Roots() ([]string, error) {
	path, _ := Path()
	var roots []string
	switch v := c.Root.(type) {
	case nil:
		return nil, nil
	case string:
		roots = []string{v}
	case []any:
		for _, e := range v {
			s, ok := e.(string)
			if !ok {
				return nil, fmt.Errorf("%s: root must be a string or a list of strings", path)
			}
			roots = append(roots, s)
		}
	default:
		return nil, fmt.Errorf("%s: root must be a string or a list of strings", path)
	}
	if len(roots) == 0 {
		return nil, fmt.Errorf("%s: root is empty", path)
	}
	return roots, nil
}
