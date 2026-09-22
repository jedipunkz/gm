package main

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"github.com/BurntSushi/toml"
)

// ConfigFile is where gm looks for its own settings.
//
//	$XDG_CONFIG_HOME/gm/gm.toml, else ~/.config/gm/gm.toml
func ConfigFile() (string, error) {
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

// config mirrors gm.toml. root takes either a string or a list of them:
//
//	root = "~/ghq"
//	root = ["~/ghq", "~/src"]
//	theme = "tokyonight"
type config struct {
	Root  any    `toml:"root"`
	Theme string `toml:"theme"`
}

// loadConfig reads gm.toml. A missing file returns the zero config, but a file
// that cannot be parsed is an error: silently cloning into the wrong tree is
// worse than refusing to run.
func loadConfig() (config, string, error) {
	path, err := ConfigFile()
	if err != nil {
		return config{}, "", err
	}
	b, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return config{}, path, nil
	}
	if err != nil {
		return config{}, path, err
	}
	var c config
	if _, err := toml.Decode(string(b), &c); err != nil {
		return config{}, path, fmt.Errorf("%s: %w", path, err)
	}
	return c, path, nil
}

// configTheme reads the theme name out of gm.toml, defaulting to tokyonight.
func configTheme() (string, error) {
	c, _, err := loadConfig()
	if err != nil {
		return "", err
	}
	if c.Theme == "" {
		return defaultTheme, nil
	}
	return c.Theme, nil
}

// configRoots reads the roots out of gm.toml.
func configRoots() ([]string, error) {
	c, path, err := loadConfig()
	if err != nil {
		return nil, err
	}

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
