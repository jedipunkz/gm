package config

import (
	"fmt"
	"strings"
)

// Chord is one key combination, in the spellings the places that use it need.
// Ctrl is always required — a bare letter is text the user is typing — and
// Alt and Shift may be added on top of it.
//
// Which combinations actually reach a program depends on the terminal. A
// plain Ctrl chord arrives everywhere. Ctrl-Alt travels as an ESC prefix and
// arrives nearly everywhere. Ctrl-Shift needs the Kitty keyboard protocol,
// since the legacy encoding sends the same byte for Ctrl-B and Ctrl-Shift-B.
type Chord struct {
	Letter  byte   // lowercase, e.g. 'w'
	Alt     bool   // Ctrl-Alt-<letter>
	Shift   bool   // Ctrl-Shift-<letter>
	Display string // "Ctrl-Alt-B", for help text and comments
}

// Key is the chord as bubbletea spells it in a key press, e.g. "ctrl+alt+b".
// Bubble Tea always writes the modifiers in the order ctrl, alt, shift.
func (c Chord) Key() string {
	var b strings.Builder
	b.WriteString("ctrl+")
	if c.Alt {
		b.WriteString("alt+")
	}
	if c.Shift {
		b.WriteString("shift+")
	}
	b.WriteByte(c.Letter)
	return b.String()
}

// Short is the chord as the finder's hint line writes it, e.g. "ctrl-alt-b".
func (c Chord) Short() string { return strings.ToLower(c.Display) }

// Plain reports whether the chord is Ctrl and a letter, with no other
// modifier. Only plain chords can collide with the finder's fixed keys, and
// only plain chords can be handed to a shell.
func (c Chord) Plain() bool { return !c.Alt && !c.Shift }

// modifiers are the optional prefixes accepted after the required ctrl, in
// any order: ctrl-alt-shift-b and ctrl-shift-alt-b are the same chord.
var modifiers = []struct {
	names []string
	set   func(*Chord)
}{
	{[]string{"alt-", "alt+", "a-", "meta-", "meta+", "m-"}, func(c *Chord) { c.Alt = true }},
	{[]string{"shift-", "shift+", "s-"}, func(c *Chord) { c.Shift = true }},
}

// ParseChord reads a chord out of gm.toml. Ctrl is written ctrl-, ctrl+, c-
// or ^; alt and shift may follow it in any order:
//
//	ctrl-w  ctrl+w  c-w  ^w
//	ctrl-alt-b  ctrl+alt+b  c-a-b  ctrl-meta-b
//	ctrl-shift-b  ctrl-alt-shift-b
//
// Case does not matter, and the empty string means "not set" and takes the
// fallback.
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
		t = "" // no ctrl, so nothing left that could be a chord
	}

	var c Chord
	for more := true; more; {
		more = false
		for _, m := range modifiers {
			for _, name := range m.names {
				if strings.HasPrefix(t, name) {
					m.set(&c)
					t, more = t[len(name):], true
					break
				}
			}
		}
	}

	if len(t) != 1 || t[0] < 'a' || t[0] > 'z' {
		return Chord{}, fmt.Errorf("cannot bind %q: use a Ctrl chord such as %s", s, fallback)
	}
	c.Letter = t[0]

	c.Display = "Ctrl-"
	if c.Alt {
		c.Display += "Alt-"
	}
	if c.Shift {
		c.Display += "Shift-"
	}
	c.Display += strings.ToUpper(t)
	return c, nil
}
