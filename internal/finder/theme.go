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
	Row     lipgloss.Style // an unselected row
	RowSel  lipgloss.Style // the selected row
	Hit     lipgloss.Style // matched characters
	HitSel  lipgloss.Style // ...inside the selected row
	Marker  lipgloss.Style // the ▸ cursor
	Divider lipgloss.Style
	Label   lipgloss.Style
	Name    lipgloss.Style
	Path    lipgloss.Style
	Remote  lipgloss.Style
	Branch  lipgloss.Style
	Commit  lipgloss.Style
	Clean   lipgloss.Style
	Dirty   lipgloss.Style
	Visits  lipgloss.Style
	Dim     lipgloss.Style
	Box     lipgloss.Style
}

func fg(hex string) lipgloss.Style {
	return lipgloss.NewStyle().Foreground(lipgloss.Color(hex))
}

// Styles builds the drawing styles for a theme.
func (t Theme) Styles() Styles {
	on := func(hex string) lipgloss.Style {
		return fg(hex).Background(lipgloss.Color(t.BgHi)).Bold(true)
	}
	return Styles{
		Row:     fg(t.Comment),
		RowSel:  on(t.Fg),
		Hit:     fg(t.Orange).Bold(true),
		HitSel:  on(t.Orange),
		Marker:  on(t.Blue),
		Divider: fg(t.Border),
		Label:   fg(t.Comment),
		Name:    fg(t.Blue).Bold(true),
		Path:    fg(t.Green),
		Remote:  fg(t.Cyan),
		Branch:  fg(t.Magenta),
		Commit:  fg(t.Yellow),
		Clean:   fg(t.Green),
		Dirty:   fg(t.Red),
		Visits:  fg(t.Orange),
		Dim:     fg(t.Comment),
		Box: lipgloss.NewStyle().
			Border(lipgloss.RoundedBorder()).
			BorderForeground(lipgloss.Color(t.Border)),
	}
}
