package config

import (
	"os"
	"path/filepath"
	"testing"
)

// write puts a gm.toml in a fresh XDG_CONFIG_HOME, or none at all for "".
func write(t *testing.T, body string) {
	t.Helper()
	dir := t.TempDir()
	t.Setenv("XDG_CONFIG_HOME", dir)
	if body == "" {
		return
	}
	if err := os.MkdirAll(filepath.Join(dir, "gm"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "gm", "gm.toml"), []byte(body), 0o644); err != nil {
		t.Fatal(err)
	}
}

// roots loads the config and resolves its root field in one step.
func roots(t *testing.T) ([]string, error) {
	t.Helper()
	c, err := Load()
	if err != nil {
		return nil, err
	}
	return c.Roots()
}

func TestRoots(t *testing.T) {
	t.Run("missing file is not an error", func(t *testing.T) {
		write(t, "")
		got, err := roots(t)
		if err != nil || got != nil {
			t.Errorf("Roots() = %v, %v; want nil, nil", got, err)
		}
	})

	t.Run("a single root", func(t *testing.T) {
		write(t, "root = \"~/code\"\n")
		got, err := roots(t)
		if err != nil {
			t.Fatal(err)
		}
		if len(got) != 1 || got[0] != "~/code" {
			t.Errorf("Roots() = %v, want [~/code]", got)
		}
	})

	t.Run("several roots", func(t *testing.T) {
		write(t, "root = [\"/a\", \"/b\"]\n")
		got, err := roots(t)
		if err != nil {
			t.Fatal(err)
		}
		if len(got) != 2 || got[0] != "/a" || got[1] != "/b" {
			t.Errorf("Roots() = %v, want [/a /b]", got)
		}
	})

	// A broken config must stop gm rather than silently send clones elsewhere.
	for name, body := range map[string]string{
		"malformed":  "root = \n",
		"wrong type": "root = 42\n",
		"mixed list": "root = [\"/a\", 7]\n",
		"empty list": "root = []\n",
		// A key gm does not know is a typo or a renamed setting; either way
		// the file says one thing and gm would do another.
		"unknown key": "root = \"/a\"\nkeybind = \"ctrl-g\"\n",
		"typo":        "root = \"/a\"\nthemes = \"dracula\"\n",
	} {
		t.Run(name+" is an error", func(t *testing.T) {
			write(t, body)
			if got, err := roots(t); err == nil {
				t.Errorf("Roots() = %v, nil; want an error", got)
			}
		})
	}
}

func TestTheme(t *testing.T) {
	write(t, "root = \"/a\"\ntheme = \"dracula\"\n")
	c, err := Load()
	if err != nil {
		t.Fatal(err)
	}
	if c.Theme != "dracula" {
		t.Errorf("Theme = %q, want dracula", c.Theme)
	}

	write(t, "root = \"/a\"\n")
	c, err = Load()
	if err != nil {
		t.Fatal(err)
	}
	if c.Theme != "" {
		t.Errorf("Theme = %q, want the empty default", c.Theme)
	}
}

func TestParseChord(t *testing.T) {
	for _, in := range []string{"ctrl-r", "ctrl+r", "Ctrl-R", "c-r", "^R", " ctrl-r "} {
		c, err := ParseChord(in, "ctrl-g")
		if err != nil {
			t.Errorf("ParseChord(%q) = %v", in, err)
			continue
		}
		if c.Letter != 'r' || c.Display != "Ctrl-R" || c.Key() != "ctrl+r" || c.Short() != "ctrl-r" {
			t.Errorf("ParseChord(%q) = %+v", in, c)
		}
	}

	// The empty string means "unset" and takes the fallback.
	if c, err := ParseChord("", "ctrl-g"); err != nil || c.Letter != 'g' {
		t.Errorf("ParseChord(\"\") = %+v, %v; want the fallback", c, err)
	}

	for _, bad := range []string{"r", "ctrl-", "ctrl-rr", "alt-r", "ctrl-1", "f5"} {
		if c, err := ParseChord(bad, "ctrl-g"); err == nil {
			t.Errorf("ParseChord(%q) = %+v, want an error", bad, c)
		}
	}
}

func TestKeys(t *testing.T) {
	write(t, "root = \"/a\"\nlaunch_key = \"ctrl-j\"\nworktree_key = \"ctrl-t\"\n")
	c, err := Load()
	if err != nil {
		t.Fatal(err)
	}
	if c.LaunchKey != "ctrl-j" {
		t.Errorf("LaunchKey = %q, want ctrl-j", c.LaunchKey)
	}
	if c.WorktreeKey != "ctrl-t" {
		t.Errorf("WorktreeKey = %q, want ctrl-t", c.WorktreeKey)
	}
}

func TestParseChordWithAlt(t *testing.T) {
	// The chord the remote key defaults to. Alt travels as an ESC prefix, so
	// unlike Ctrl-Shift it reaches a program on an ordinary terminal.
	for _, in := range []string{"ctrl-alt-b", "ctrl+alt+b", "Ctrl-Alt-B", "c-a-b", "ctrl-meta-b", "^alt-b"} {
		c, err := ParseChord(in, "ctrl-w")
		if err != nil {
			t.Errorf("ParseChord(%q) = %v", in, err)
			continue
		}
		if !c.Alt || c.Shift || c.Letter != 'b' || c.Plain() {
			t.Errorf("ParseChord(%q) = %+v, want alt and b", in, c)
		}
		if c.Key() != "ctrl+alt+b" || c.Display != "Ctrl-Alt-B" || c.Short() != "ctrl-alt-b" {
			t.Errorf("ParseChord(%q) spells itself %q/%q/%q", in, c.Key(), c.Display, c.Short())
		}
	}

	// Both modifiers, in either order, are the same chord.
	for _, in := range []string{"ctrl-alt-shift-b", "ctrl-shift-alt-b"} {
		c, err := ParseChord(in, "ctrl-w")
		if err != nil {
			t.Fatalf("ParseChord(%q) = %v", in, err)
		}
		if !c.Alt || !c.Shift || c.Key() != "ctrl+alt+shift+b" {
			t.Errorf("ParseChord(%q) = %+v, key %q", in, c, c.Key())
		}
	}

	// Alt without Ctrl is not a chord gm binds.
	for _, bad := range []string{"alt-b", "a-b", "ctrl-alt-", "ctrl-alt-bb"} {
		if c, err := ParseChord(bad, "ctrl-w"); err == nil {
			t.Errorf("ParseChord(%q) = %+v, want an error", bad, c)
		}
	}

	if c, _ := ParseChord("ctrl-w", "ctrl-w"); !c.Plain() {
		t.Error("a plain Ctrl chord should report itself as plain")
	}
}

func TestParseChordWithShift(t *testing.T) {
	for _, in := range []string{"ctrl-shift-b", "ctrl+shift+b", "Ctrl-Shift-B", "c-s-b"} {
		c, err := ParseChord(in, "ctrl-w")
		if err != nil {
			t.Errorf("ParseChord(%q) = %v", in, err)
			continue
		}
		if !c.Shift || c.Letter != 'b' {
			t.Errorf("ParseChord(%q) = %+v, want shift and b", in, c)
		}
		if c.Key() != "ctrl+shift+b" || c.Display != "Ctrl-Shift-B" || c.Short() != "ctrl-shift-b" {
			t.Errorf("ParseChord(%q) spells itself %q/%q/%q", in, c.Key(), c.Display, c.Short())
		}
	}

	// Shift without Ctrl is not a chord gm binds.
	for _, bad := range []string{"shift-b", "s-b", "ctrl-shift-", "ctrl-shift-bb"} {
		if c, err := ParseChord(bad, "ctrl-w"); err == nil {
			t.Errorf("ParseChord(%q) = %+v, want an error", bad, c)
		}
	}
}
