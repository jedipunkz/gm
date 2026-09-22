package cli

import (
	"io"
	"os"
	"strings"
	"testing"

	"github.com/jedipunkz/gm/internal/config"
)

// TestUsageCoversEveryCommand guards the help text against drifting away from
// the table it is built from.
func TestUsageCoversEveryCommand(t *testing.T) {
	u := Usage()
	for _, c := range commands {
		if !strings.Contains(u, "gm "+c.name) {
			t.Errorf("usage does not mention %q:\n%s", c.name, u)
		}
		if c.run == nil {
			t.Errorf("command %q has no run function", c.name)
		}
	}
}

// TestNamesAreUnique keeps a new command from shadowing an existing name or
// alias, which would make it unreachable.
func TestNamesAreUnique(t *testing.T) {
	seen := map[string]string{}
	for _, c := range commands {
		for _, n := range append([]string{c.name}, c.aliases...) {
			if prev, dup := seen[n]; dup {
				t.Errorf("%q is claimed by both %s and %s", n, prev, c.name)
			}
			seen[n] = c.name
		}
	}
}

// TestShellSnippetsBindTheConfiguredKey checks the spelling each shell needs,
// and that no template placeholder survives into the output.
func TestShellSnippetsBindTheConfiguredKey(t *testing.T) {
	cases := map[string]string{
		"fish": `bind \cr __gm_jump`,
		"zsh":  `bindkey '^r' __gm_jump`,
		"bash": `bind -x '"\C-r": __gm_jump'`,
	}
	for sh, want := range cases {
		out := captureStdout(t, func() {
			a := &app{cfg: config.Config{LaunchKey: "ctrl-r"}}
			if err := a.shell([]string{sh}); err != nil {
				t.Fatal(err)
			}
		})
		if !strings.Contains(out, want) {
			t.Errorf("gm shell %s does not bind Ctrl-R (%q):\n%s", sh, want, out)
		}
		if !strings.Contains(out, "Ctrl-R jumps") {
			t.Errorf("gm shell %s does not name the key in its comment:\n%s", sh, out)
		}
		if strings.Contains(out, "{{") {
			t.Errorf("gm shell %s left a placeholder unreplaced:\n%s", sh, out)
		}
	}

	// An unusable key must stop gm rather than print a binding that silently
	// does nothing.
	for _, bad := range []string{"alt-r", "ctrl-shift-b", "ctrl-alt-b"} {
		a := &app{cfg: config.Config{LaunchKey: bad}}
		if err := a.shell([]string{"zsh"}); err == nil {
			t.Errorf("gm shell accepted %s", bad)
		}
	}
}

// captureStdout runs f with stdout redirected and returns what it wrote.
func captureStdout(t *testing.T, f func()) string {
	t.Helper()
	r, w, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	old := os.Stdout
	os.Stdout = w
	defer func() { os.Stdout = old }()

	f()
	w.Close()
	var b strings.Builder
	if _, err := io.Copy(&b, r); err != nil {
		t.Fatal(err)
	}
	return b.String()
}
