package cli

import (
	"strings"
	"testing"
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
