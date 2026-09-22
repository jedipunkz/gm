package config

import (
	"fmt"
	"strings"
)

// Chord is one Ctrl-<letter> key combination, optionally with Shift, in the
// spellings the places that use it need. Ctrl is always required: a bare
// letter is text the user is typing, and Alt chords are not reported the same
// way everywhere.
type Chord struct {
	Letter  byte   // lowercase, e.g. 'w'
	Shift   bool   // Ctrl-Shift-<letter>
	Display string // "Ctrl-W", for help text and comments
}

// Key is the chord as bubbletea spells it in a key press, e.g. "ctrl+w".
// Bubble Tea always writes the modifiers in the order ctrl, alt, shift.
func (c Chord) Key() string {
	if c.Shift {
		return "ctrl+shift+" + string(c.Letter)
	}
	return "ctrl+" + string(c.Letter)
}

// Short is the chord as the finder's hint line writes it, e.g. "ctrl-w".
func (c Chord) Short() string { return strings.ToLower(c.Display) }

// ParseChord accepts ctrl-w, ctrl+w, c-w and ^w, with an optional shift-
// after the ctrl (ctrl-shift-b, ctrl+shift+b, c-s-b), in any case. The empty
// string means "not set" and returns the fallback.
func ParseChord(s, fallback string) (Chord, error) {
	if strings.TrimSpace(s) == "" {
		s = fallback
	}
	t := strings.ToLower(strings.TrimSpace(s))
	switch {
	case strings.HasPrefix(t, "ctrl-"), strings.HasPrefix(t, "ctrl+"):
		t = t[5:]
	case strings.HasPrefix(t, "c-"):
		t = t[2:]
	case strings.HasPrefix(t, "^"):
		t = t[1:]
	default:
		t = ""
	}

	shift := false
	switch {
	case strings.HasPrefix(t, "shift-"), strings.HasPrefix(t, "shift+"):
		shift, t = true, t[6:]
	case strings.HasPrefix(t, "s-"):
		shift, t = true, t[2:]
	}

	if len(t) != 1 || t[0] < 'a' || t[0] > 'z' {
		return Chord{}, fmt.Errorf("cannot bind %q: use a Ctrl chord such as %s", s, fallback)
	}
	c := Chord{Letter: t[0], Shift: shift, Display: "Ctrl-" + strings.ToUpper(t)}
	if shift {
		c.Display = "Ctrl-Shift-" + strings.ToUpper(t)
	}
	return c, nil
}
