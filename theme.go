package main

import (
	"fmt"
	"sort"
	"strings"

	"charm.land/lipgloss/v2"
)

// palette is every color the finder paints with. Adding a theme means adding
// one entry to themes; nothing else in the UI names a color.
type palette struct {
	bgHi    string // selected row background
	border  string // box border and the column divider
	comment string // unselected rows, labels, dim text
	fg      string // selected row text
	blue    string // repository name
	cyan    string // remote
	magenta string // branch
	green   string // path, clean status
	yellow  string // commit
	orange  string // matched characters, visit count
	red     string // dirty status
	light   bool   // the terminal background is light
}

var themes = map[string]palette{
	"tokyonight": {
		bgHi: "#292e42", border: "#3b4261", comment: "#565f89", fg: "#c0caf5",
		blue: "#7aa2f7", cyan: "#7dcfff", magenta: "#bb9af7",
		green: "#9ece6a", yellow: "#e0af68", orange: "#ff9e64", red: "#f7768e",
	},
	"solarized-dark": {
		bgHi: "#073642", border: "#586e75", comment: "#657b83", fg: "#93a1a1",
		blue: "#268bd2", cyan: "#2aa198", magenta: "#d33682",
		green: "#859900", yellow: "#b58900", orange: "#cb4b16", red: "#dc322f",
	},
	"solarized-light": {
		bgHi: "#eee8d5", border: "#93a1a1", comment: "#657b83", fg: "#586e75",
		blue: "#268bd2", cyan: "#2aa198", magenta: "#d33682",
		green: "#859900", yellow: "#b58900", orange: "#cb4b16", red: "#dc322f",
		light: true,
	},
	"kanagawa-wave": {
		bgHi: "#363646", border: "#54546d", comment: "#727169", fg: "#dcd7ba",
		blue: "#7e9cd8", cyan: "#7aa89f", magenta: "#957fb8",
		green: "#98bb6c", yellow: "#e6c384", orange: "#ffa066", red: "#ff5d62",
	},
	"catppuccin-latte": {
		bgHi: "#ccd0da", border: "#9ca0b0", comment: "#6c6f85", fg: "#4c4f69",
		blue: "#1e66f5", cyan: "#179299", magenta: "#8839ef",
		green: "#40a02b", yellow: "#df8e1d", orange: "#fe640b", red: "#d20f39",
		light: true,
	},
	"catppuccin-frappe": {
		bgHi: "#414559", border: "#737994", comment: "#a5adce", fg: "#c6d0f5",
		blue: "#8caaee", cyan: "#81c8be", magenta: "#ca9ee6",
		green: "#a6d189", yellow: "#e5c890", orange: "#ef9f76", red: "#e78284",
	},
	"catppuccin-macchiato": {
		bgHi: "#363a4f", border: "#6e738d", comment: "#a5adcb", fg: "#cad3f5",
		blue: "#8aadf4", cyan: "#8bd5ca", magenta: "#c6a0f6",
		green: "#a6da95", yellow: "#eed49f", orange: "#f5a97f", red: "#ed8796",
	},
	"catppuccin-mocha": {
		bgHi: "#313244", border: "#6c7086", comment: "#a6adc8", fg: "#cdd6f4",
		blue: "#89b4fa", cyan: "#94e2d5", magenta: "#cba6f7",
		green: "#a6e3a1", yellow: "#f9e2af", orange: "#fab387", red: "#f38ba8",
	},
	"rose-pine": {
		bgHi: "#26233a", border: "#44415a", comment: "#6e6a86", fg: "#e0def4",
		blue: "#9ccfd8", cyan: "#ebbcba", magenta: "#c4a7e7",
		green: "#31748f", yellow: "#f6c177", orange: "#ebbcba", red: "#eb6f92",
	},
	"dracula": {
		bgHi: "#44475a", border: "#6272a4", comment: "#6272a4", fg: "#f8f8f2",
		blue: "#bd93f9", cyan: "#8be9fd", magenta: "#ff79c6",
		green: "#50fa7b", yellow: "#f1fa8c", orange: "#ffb86c", red: "#ff5555",
	},
}

const defaultTheme = "tokyonight"

// theme is the palette in force; the styles below are rebuilt from it.
var theme = themes[defaultTheme]

func init() { applyTheme(defaultTheme) }

// applyTheme repoints every style at the named theme.
func applyTheme(name string) error {
	p, ok := themes[name]
	if !ok {
		return fmt.Errorf("unknown theme %q (have %s)", name, strings.Join(themeNames(), ", "))
	}
	theme = p
	styleRow = fg(p.comment)
	styleRowSel = fg(p.fg).Background(lipgloss.Color(p.bgHi)).Bold(true)
	styleHit = fg(p.orange).Bold(true)
	styleHitSel = fg(p.orange).Background(lipgloss.Color(p.bgHi)).Bold(true)
	styleMarker = fg(p.blue).Background(lipgloss.Color(p.bgHi)).Bold(true)
	styleDivider = fg(p.border)
	styleLabel = fg(p.comment)
	styleName = fg(p.blue).Bold(true)
	stylePath = fg(p.green)
	styleRemote = fg(p.cyan)
	styleBranch = fg(p.magenta)
	styleCommit = fg(p.yellow)
	styleClean = fg(p.green)
	styleDirty = fg(p.red)
	styleVisits = fg(p.orange)
	styleDim = fg(p.comment)
	styleBox = lipgloss.NewStyle().
		Border(lipgloss.RoundedBorder()).
		BorderForeground(lipgloss.Color(p.border))
	return nil
}

func themeNames() []string {
	names := make([]string, 0, len(themes))
	for n := range themes {
		names = append(names, n)
	}
	sort.Strings(names)
	return names
}
