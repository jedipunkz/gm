package cli

import (
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/jedipunkz/gm/internal/config"
	"github.com/jedipunkz/gm/internal/repo"
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
	_ = w.Close()
	var b strings.Builder
	if _, err := io.Copy(&b, r); err != nil {
		t.Fatal(err)
	}
	return b.String()
}

// TestListUniqueStaysUniqueUnderAQuery pins --unique to the whole tree: a
// query must not shorten a name down to one that gm rm would call ambiguous.
func TestListUniqueStaysUniqueUnderAQuery(t *testing.T) {
	root := t.TempDir()
	for _, rel := range []string{"github.com/alice/gm", "github.com/alice/tools", "github.com/bob/gm"} {
		if err := os.MkdirAll(filepath.Join(root, rel, ".git"), 0o755); err != nil {
			t.Fatal(err)
		}
	}
	tree := &repo.Tree{Roots: []string{root}}

	out := captureStdout(t, func() {
		a := &app{tree: tree}
		if err := a.list([]string{"--unique", "alice"}); err != nil {
			t.Fatal(err)
		}
	})
	got := strings.Fields(out)
	want := []string{"alice/gm", "tools"}
	if !slices.Equal(got, want) {
		t.Fatalf("gm list --unique alice printed %v, want %v", got, want)
	}
	for _, name := range got {
		if _, err := tree.Resolve(name); err != nil {
			t.Errorf("printed name %q does not resolve: %v", name, err)
		}
	}
}
