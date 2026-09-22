package config

import (
	"fmt"
	"strings"
)

// Chord is one Ctrl-<letter> key combination, in the spellings the places
// that use it need. Only Ctrl chords are supported: they are what a shell can
// bind to a widget, and what a terminal reports unambiguously.
type Chord struct {
	Letter  byte   // lowercase, e.g. 'w'
	Display string // "Ctrl-W", for help text and comments
}

// Key is the chord as bubbletea spells it in a key press, e.g. "ctrl+w".
func (c Chord) Key() string { return "ctrl+" + string(c.Letter) }

// Short is the chord as the finder's hint line writes it, e.g. "ctrl-w".
func (c Chord) Short() string { return strings.ToLower(c.Display) }

// ParseChord accepts ctrl-w, ctrl+w, c-w and ^w, in any case. The empty
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
	if len(t) != 1 || t[0] < 'a' || t[0] > 'z' {
		return Chord{}, fmt.Errorf("cannot bind %q: use a Ctrl chord such as %s", s, fallback)
	}
	return Chord{Letter: t[0], Display: "Ctrl-" + strings.ToUpper(t)}, nil
}
