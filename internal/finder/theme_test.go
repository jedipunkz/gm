package finder

import (
	"strings"
	"testing"
)

func TestThemes(t *testing.T) {
	for _, name := range ThemeNames() {
		th, err := LookupTheme(name)
		if err != nil {
			t.Fatalf("LookupTheme(%q) = %v", name, err)
		}
		// A zero field renders as the terminal default, which reads as a hole
		// in the palette.
		for field, v := range map[string]string{
			"BgHi": th.BgHi, "Border": th.Border, "Comment": th.Comment, "Fg": th.Fg,
			"Blue": th.Blue, "Cyan": th.Cyan, "Magenta": th.Magenta,
			"Green": th.Green, "Yellow": th.Yellow, "Orange": th.Orange, "Red": th.Red,
		} {
			if !strings.HasPrefix(v, "#") || len(v) != 7 {
				t.Errorf("theme %s: %s = %q, want a #rrggbb color", name, field, v)
			}
		}
		th.Styles() // must not panic on any theme
	}

	if _, err := LookupTheme(""); err != nil {
		t.Errorf("the empty name must fall back to %s: %v", DefaultTheme, err)
	}
	if _, err := LookupTheme("nope"); err == nil {
		t.Error("LookupTheme(\"nope\") = nil, want an error")
	}
}

func TestBlendRejectsJunk(t *testing.T) {
	if got := blend("not a colour", "#ffffff", 0.5); got != "not a colour" {
		t.Errorf("blend() = %q, want the input back untouched", got)
	}
	if got := blend("#000000", "#ffffff", 1); got != "#ffffff" {
		t.Errorf("blend(black, white, 1) = %q, want #ffffff", got)
	}
}
