package finder

import (
	"fmt"
	"sort"
	"strings"

	"charm.land/lipgloss/v2"
)

// Theme is every color the finder paints with. Adding a theme means adding one
// entry to themes; nothing else in the UI names a color.
type Theme struct {
	BgHi    string // selected row background
	Border  string // box border and the column divider
	Comment string // unselected rows, labels, dim text
	Fg      string // selected row text
	Blue    string // repository name
	Cyan    string // remote
	Magenta string // branch
	Green   string // path, clean status
	Yellow  string // commit
	Orange  string // matched characters, visit count
	Red     string // dirty status
	Light   bool   // the terminal background is light
}

// DefaultTheme is what gm wears when gm.toml says nothing.
const DefaultTheme = "tokyonight"

var themes = map[string]Theme{
	"tokyonight": {
		BgHi: "#292e42", Border: "#3b4261", Comment: "#565f89", Fg: "#c0caf5",
		Blue: "#7aa2f7", Cyan: "#7dcfff", Magenta: "#bb9af7",
		Green: "#9ece6a", Yellow: "#e0af68", Orange: "#ff9e64", Red: "#f7768e",
	},
	"solarized-dark": {
		BgHi: "#073642", Border: "#586e75", Comment: "#657b83", Fg: "#93a1a1",
		Blue: "#268bd2", Cyan: "#2aa198", Magenta: "#d33682",
		Green: "#859900", Yellow: "#b58900", Orange: "#cb4b16", Red: "#dc322f",
	},
	"solarized-light": {
		BgHi: "#eee8d5", Border: "#93a1a1", Comment: "#657b83", Fg: "#586e75",
		Blue: "#268bd2", Cyan: "#2aa198", Magenta: "#d33682",
		Green: "#859900", Yellow: "#b58900", Orange: "#cb4b16", Red: "#dc322f",
		Light: true,
	},
	"kanagawa-wave": {
		BgHi: "#363646", Border: "#54546d", Comment: "#727169", Fg: "#dcd7ba",
		Blue: "#7e9cd8", Cyan: "#7aa89f", Magenta: "#957fb8",
		Green: "#98bb6c", Yellow: "#e6c384", Orange: "#ffa066", Red: "#ff5d62",
	},
	"catppuccin-latte": {
		BgHi: "#ccd0da", Border: "#9ca0b0", Comment: "#6c6f85", Fg: "#4c4f69",
		Blue: "#1e66f5", Cyan: "#179299", Magenta: "#8839ef",
		Green: "#40a02b", Yellow: "#df8e1d", Orange: "#fe640b", Red: "#d20f39",
		Light: true,
	},
	"catppuccin-frappe": {
		BgHi: "#414559", Border: "#737994", Comment: "#a5adce", Fg: "#c6d0f5",
		Blue: "#8caaee", Cyan: "#81c8be", Magenta: "#ca9ee6",
		Green: "#a6d189", Yellow: "#e5c890", Orange: "#ef9f76", Red: "#e78284",
	},
	"catppuccin-macchiato": {
		BgHi: "#363a4f", Border: "#6e738d", Comment: "#a5adcb", Fg: "#cad3f5",
		Blue: "#8aadf4", Cyan: "#8bd5ca", Magenta: "#c6a0f6",
		Green: "#a6da95", Yellow: "#eed49f", Orange: "#f5a97f", Red: "#ed8796",
	},
	"catppuccin-mocha": {
		BgHi: "#313244", Border: "#6c7086", Comment: "#a6adc8", Fg: "#cdd6f4",
		Blue: "#89b4fa", Cyan: "#94e2d5", Magenta: "#cba6f7",
		Green: "#a6e3a1", Yellow: "#f9e2af", Orange: "#fab387", Red: "#f38ba8",
	},
	"rose-pine": {
		BgHi: "#26233a", Border: "#44415a", Comment: "#6e6a86", Fg: "#e0def4",
		Blue: "#9ccfd8", Cyan: "#ebbcba", Magenta: "#c4a7e7",
		Green: "#31748f", Yellow: "#f6c177", Orange: "#ebbcba", Red: "#eb6f92",
	},
	"dracula": {
		BgHi: "#44475a", Border: "#6272a4", Comment: "#6272a4", Fg: "#f8f8f2",
		Blue: "#bd93f9", Cyan: "#8be9fd", Magenta: "#ff79c6",
		Green: "#50fa7b", Yellow: "#f1fa8c", Orange: "#ffb86c", Red: "#ff5555",
	},
}

// LookupTheme finds a theme by name. An empty name is the default; an unknown
// one is an error listing what is on offer.
func LookupTheme(name string) (Theme, error) {
	if name == "" {
		name = DefaultTheme
	}
	t, ok := themes[name]
	if !ok {
		return Theme{}, fmt.Errorf("unknown theme %q (have %s)", name, strings.Join(ThemeNames(), ", "))
	}
	return t, nil
}

// ThemeNames lists every theme, sorted.
func ThemeNames() []string {
	names := make([]string, 0, len(themes))
	for n := range themes {
		names = append(names, n)
	}
	sort.Strings(names)
	return names
}

// Styles is the theme turned into the lipgloss styles the view draws with.
// The model owns one, so nothing about the palette is global state.
type Styles struct {
	Row       lipgloss.Style // an unselected row: the comment colour, lifted
	RowSel    lipgloss.Style // the selected row
	Hit       lipgloss.Style // matched characters
	HitSel    lipgloss.Style // ...inside the selected row
	Marker    lipgloss.Style // the ▸ cursor
	Divider   lipgloss.Style
	Label     lipgloss.Style // the field names in the details pane
	Name      lipgloss.Style
	Path      lipgloss.Style
	Remote    lipgloss.Style
	Branch    lipgloss.Style
	Commit    lipgloss.Style // a commit hash
	Subject   lipgloss.Style // a commit subject
	RefHead   lipgloss.Style // HEAD in the decorations
	RefLocal  lipgloss.Style // a local branch
	RefRemote lipgloss.Style // a remote-tracking branch
	RefTag    lipgloss.Style // a tag
	Punct     lipgloss.Style // the parentheses and commas between them
	Clean     lipgloss.Style
	Dirty     lipgloss.Style
	Visits    lipgloss.Style
	Dim       lipgloss.Style
	Box       lipgloss.Style
	Help      lipgloss.Style // the hint line's prose
	HelpKey   lipgloss.Style // the key names inside it
}

func fg(hex string) lipgloss.Style {
	return lipgloss.NewStyle().Foreground(lipgloss.Color(hex))
}

// rowLift is how far an unselected row is pulled from the comment colour
// towards the foreground: far enough to read, not so far that the selected
// row stops standing out. It works in both polarities, since a light theme's
// foreground is the darker of the two.
//
// ponytail: one global factor, give a theme its own row colour if this lands
// badly on it.
const rowLift = 0.35

// blend mixes two #rrggbb colours, t running from a (0) to b (1). An
// unparseable colour is returned untouched rather than silently becoming
// black.
func blend(a, b string, t float64) string {
	var ar, ag, ab, br, bg, bb int
	if _, err := fmt.Sscanf(a, "#%02x%02x%02x", &ar, &ag, &ab); err != nil {
		return a
	}
	if _, err := fmt.Sscanf(b, "#%02x%02x%02x", &br, &bg, &bb); err != nil {
		return a
	}
	mix := func(x, y int) int { return int(float64(x) + (float64(y)-float64(x))*t + 0.5) }
	return fmt.Sprintf("#%02x%02x%02x", mix(ar, br), mix(ag, bg), mix(ab, bb))
}

// Styles builds the drawing styles for a theme.
func (t Theme) Styles() Styles {
	on := func(hex string) lipgloss.Style {
		return fg(hex).Background(lipgloss.Color(t.BgHi)).Bold(true)
	}
	return Styles{
		Row:     fg(blend(t.Comment, t.Fg, rowLift)),
		RowSel:  on(t.Fg),
		Hit:     fg(t.Orange).Bold(true),
		HitSel:  on(t.Orange),
		Marker:  on(t.Blue),
		Divider: fg(t.Border),
		// The field names are the quiet half of the pane: the comment
		// colour, in bold. The values keep the theme's own colours, so the
		// pane alternates dim grey and colour down its length and the fields
		// come apart — a label as bright as its value left both competing and
		// neither reading as a heading. Bold, because the comment colour on
		// its own is the dimmest thing the theme has.
		Label:  fg(t.Comment).Bold(true),
		Name:   fg(t.Blue).Bold(true),
		Path:   fg(t.Green),
		Remote: fg(t.Cyan),
		Branch: fg(t.Magenta),
		// The commit block stays inside one cool family — the theme's blue
		// and cyan, lightened towards the foreground or darkened towards the
		// comment colour. Brightness carries the distinction between the
		// parts, so five different hues do not compete down the pane.
		Commit:    fg(blend(t.Blue, t.Comment, 0.35)),
		Subject:   fg(blend(t.Comment, t.Fg, rowLift)),
		RefHead:   fg(t.Cyan).Bold(true),
		RefLocal:  fg(t.Blue).Bold(true),
		RefRemote: fg(blend(t.Blue, t.Comment, 0.5)),
		RefTag:    fg(blend(t.Cyan, t.Comment, 0.35)),
		Punct:     fg(t.Comment),
		Clean:     fg(t.Green),
		Dirty:     fg(t.Red),
		Visits:    fg(t.Orange),
		Dim:       fg(t.Comment),
		Box: lipgloss.NewStyle().
			Border(lipgloss.RoundedBorder()).
			BorderForeground(lipgloss.Color(t.Border)),
		Help:    fg(t.Comment),
		HelpKey: fg(t.Blue),
	}
}
