use ratatui::style::{Color, Modifier, Style};

use crate::{Result, err};

/// Theme is every color the finder paints with. Adding a theme means adding
/// one entry to THEMES; nothing else in the UI names a color. The site reads
/// this table at build time, so keep one theme per entry in this shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub bg_hi: &'static str,   // selected row background
    pub border: &'static str,  // box border and the column divider
    pub comment: &'static str, // unselected rows, labels, dim text
    pub fg: &'static str,      // selected row text
    pub blue: &'static str,    // repository name
    pub cyan: &'static str,    // remote
    pub magenta: &'static str, // branch
    pub green: &'static str,   // path, clean status
    pub yellow: &'static str,  // commit
    pub orange: &'static str,  // matched characters, visit count
    pub red: &'static str,     // dirty status
    pub light: bool,           // the terminal background is light
}

/// DEFAULT_THEME is what gm wears when gm.toml says nothing.
pub const DEFAULT_THEME: &str = "tokyonight";

const DARK: bool = false;
const LIGHT: bool = true;

#[rustfmt::skip]
pub const THEMES: &[(&str, Theme)] = &[
    ("tokyonight", Theme {
        bg_hi: "#292e42", border: "#3b4261", comment: "#565f89", fg: "#c0caf5",
        blue: "#7aa2f7", cyan: "#7dcfff", magenta: "#bb9af7",
        green: "#9ece6a", yellow: "#e0af68", orange: "#ff9e64", red: "#f7768e",
        light: DARK,
    }),
    ("solarized-dark", Theme {
        bg_hi: "#073642", border: "#586e75", comment: "#657b83", fg: "#93a1a1",
        blue: "#268bd2", cyan: "#2aa198", magenta: "#d33682",
        green: "#859900", yellow: "#b58900", orange: "#cb4b16", red: "#dc322f",
        light: DARK,
    }),
    ("solarized-light", Theme {
        bg_hi: "#eee8d5", border: "#93a1a1", comment: "#657b83", fg: "#586e75",
        blue: "#268bd2", cyan: "#2aa198", magenta: "#d33682",
        green: "#859900", yellow: "#b58900", orange: "#cb4b16", red: "#dc322f",
        light: LIGHT,
    }),
    ("kanagawa-wave", Theme {
        bg_hi: "#363646", border: "#54546d", comment: "#727169", fg: "#dcd7ba",
        blue: "#7e9cd8", cyan: "#7aa89f", magenta: "#957fb8",
        green: "#98bb6c", yellow: "#e6c384", orange: "#ffa066", red: "#ff5d62",
        light: DARK,
    }),
    ("catppuccin-latte", Theme {
        bg_hi: "#ccd0da", border: "#9ca0b0", comment: "#6c6f85", fg: "#4c4f69",
        blue: "#1e66f5", cyan: "#179299", magenta: "#8839ef",
        green: "#40a02b", yellow: "#df8e1d", orange: "#fe640b", red: "#d20f39",
        light: LIGHT,
    }),
    ("catppuccin-frappe", Theme {
        bg_hi: "#414559", border: "#737994", comment: "#a5adce", fg: "#c6d0f5",
        blue: "#8caaee", cyan: "#81c8be", magenta: "#ca9ee6",
        green: "#a6d189", yellow: "#e5c890", orange: "#ef9f76", red: "#e78284",
        light: DARK,
    }),
    ("catppuccin-macchiato", Theme {
        bg_hi: "#363a4f", border: "#6e738d", comment: "#a5adcb", fg: "#cad3f5",
        blue: "#8aadf4", cyan: "#8bd5ca", magenta: "#c6a0f6",
        green: "#a6da95", yellow: "#eed49f", orange: "#f5a97f", red: "#ed8796",
        light: DARK,
    }),
    ("catppuccin-mocha", Theme {
        bg_hi: "#313244", border: "#6c7086", comment: "#a6adc8", fg: "#cdd6f4",
        blue: "#89b4fa", cyan: "#94e2d5", magenta: "#cba6f7",
        green: "#a6e3a1", yellow: "#f9e2af", orange: "#fab387", red: "#f38ba8",
        light: DARK,
    }),
    ("rose-pine", Theme {
        bg_hi: "#26233a", border: "#44415a", comment: "#6e6a86", fg: "#e0def4",
        blue: "#9ccfd8", cyan: "#ebbcba", magenta: "#c4a7e7",
        green: "#31748f", yellow: "#f6c177", orange: "#ebbcba", red: "#eb6f92",
        light: DARK,
    }),
    ("dracula", Theme {
        bg_hi: "#44475a", border: "#6272a4", comment: "#6272a4", fg: "#f8f8f2",
        blue: "#bd93f9", cyan: "#8be9fd", magenta: "#ff79c6",
        green: "#50fa7b", yellow: "#f1fa8c", orange: "#ffb86c", red: "#ff5555",
        light: DARK,
    }),
];

/// lookup_theme finds a theme by name. An empty name is the default; an
/// unknown one is an error listing what is on offer.
pub fn lookup_theme(name: &str) -> Result<Theme> {
    let name = if name.is_empty() { DEFAULT_THEME } else { name };
    THEMES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, t)| *t)
        .ok_or_else(|| err!("unknown theme {name:?} (have {})", theme_names().join(", ")))
}

/// theme_names lists every theme, sorted.
pub fn theme_names() -> Vec<&'static str> {
    let mut names: Vec<&str> = THEMES.iter().map(|(n, _)| *n).collect();
    names.sort();
    names
}

/// Styles is the theme turned into the styles the view draws with. The model
/// owns one, so nothing about the palette is global state.
#[derive(Debug, Clone, Copy, Default)]
pub struct Styles {
    pub row: Style,     // an unselected row: the comment colour, lifted
    pub row_sel: Style, // the selected row
    pub hit: Style,     // matched characters
    pub hit_sel: Style, // ...inside the selected row
    pub marker: Style,  // the ▸ cursor
    pub divider: Style,
    pub label: Style, // the field names in the details pane
    pub name: Style,
    pub path: Style,
    pub remote: Style,
    pub branch: Style,
    pub commit: Style,     // a commit hash
    pub subject: Style,    // a commit subject
    pub ref_head: Style,   // HEAD in the decorations
    pub ref_local: Style,  // a local branch
    pub ref_remote: Style, // a remote-tracking branch
    pub ref_tag: Style,    // a tag
    pub punct: Style,      // the parentheses and commas between them
    pub clean: Style,
    pub dirty: Style,
    pub visits: Style,
    pub dim: Style,
    pub border: Style,   // the rounded boxes
    pub help: Style,     // the hint line's prose
    pub help_key: Style, // the key names inside it
    pub prompt: Style,
    pub text: Style,       // what is typed into the box
    pub suggestion: Style, // the rest of a command the box offers
}

/// rgb reads a #rrggbb colour. An unparseable one is the terminal's own.
pub fn rgb(hex: &str) -> Color {
    parse_hex(hex)
        .map(|(r, g, b)| Color::Rgb(r, g, b))
        .unwrap_or(Color::Reset)
}

fn parse_hex(hex: &str) -> Option<(u8, u8, u8)> {
    let h = hex
        .strip_prefix('#')
        .filter(|h| h.len() == 6 && h.is_ascii())?;
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some((c(0)?, c(2)?, c(4)?))
}

fn fg(hex: &str) -> Style {
    Style::new().fg(rgb(hex))
}

/// ROW_LIFT is how far an unselected row is pulled from the comment colour
/// towards the foreground: far enough to read, not so far that the selected
/// row stops standing out. It works in both polarities, since a light theme's
/// foreground is the darker of the two.
///
/// ponytail: one global factor, give a theme its own row colour if this lands
/// badly on it.
pub const ROW_LIFT: f64 = 0.35;

/// blend mixes two #rrggbb colours, t running from a (0) to b (1). An
/// unparseable colour is returned untouched rather than silently becoming
/// black.
pub fn blend(a: &str, b: &str, t: f64) -> String {
    let (Some(x), Some(y)) = (parse_hex(a), parse_hex(b)) else {
        return a.to_string();
    };
    let mix = |p: u8, q: u8| (p as f64 + (q as f64 - p as f64) * t + 0.5) as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        mix(x.0, y.0),
        mix(x.1, y.1),
        mix(x.2, y.2)
    )
}

impl Theme {
    /// styles builds the drawing styles for a theme.
    pub fn styles(&self) -> Styles {
        let bold = Modifier::BOLD;
        let on = |hex: &str| fg(hex).bg(rgb(self.bg_hi)).add_modifier(bold);
        Styles {
            row: fg(&blend(self.comment, self.fg, ROW_LIFT)),
            row_sel: on(self.fg),
            hit: fg(self.orange).add_modifier(bold),
            hit_sel: on(self.orange),
            marker: on(self.blue),
            divider: fg(self.border),
            // The field names are the quiet half of the pane: the comment
            // colour, in bold. The values keep the theme's own colours, so the
            // pane alternates dim grey and colour down its length and the
            // fields come apart — a label as bright as its value left both
            // competing and neither reading as a heading. Bold, because the
            // comment colour on its own is the dimmest thing the theme has.
            label: fg(self.comment).add_modifier(bold),
            name: fg(self.blue).add_modifier(bold),
            path: fg(self.green),
            remote: fg(self.cyan),
            branch: fg(self.magenta),
            // The commit block stays inside one cool family — the theme's blue
            // and cyan, lightened towards the foreground or darkened towards
            // the comment colour. Brightness carries the distinction between
            // the parts, so five different hues do not compete down the pane.
            commit: fg(&blend(self.blue, self.comment, 0.35)),
            subject: fg(&blend(self.comment, self.fg, ROW_LIFT)),
            ref_head: fg(self.cyan).add_modifier(bold),
            ref_local: fg(self.blue).add_modifier(bold),
            ref_remote: fg(&blend(self.blue, self.comment, 0.5)),
            ref_tag: fg(&blend(self.cyan, self.comment, 0.35)),
            punct: fg(self.comment),
            clean: fg(self.green),
            dirty: fg(self.red),
            visits: fg(self.orange),
            dim: fg(self.comment),
            border: fg(self.border),
            help: fg(self.comment),
            help_key: fg(self.blue),
            prompt: fg(self.blue),
            text: fg(self.fg),
            suggestion: fg(self.comment),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_is_complete() {
        for name in theme_names() {
            let t = lookup_theme(name).unwrap();
            // A missing colour renders as the terminal default, which reads as
            // a hole in the palette.
            for (field, v) in [
                ("bg_hi", t.bg_hi),
                ("border", t.border),
                ("comment", t.comment),
                ("fg", t.fg),
                ("blue", t.blue),
                ("cyan", t.cyan),
                ("magenta", t.magenta),
                ("green", t.green),
                ("yellow", t.yellow),
                ("orange", t.orange),
                ("red", t.red),
            ] {
                assert!(
                    parse_hex(v).is_some(),
                    "theme {name}: {field} = {v:?}, want a #rrggbb color"
                );
            }
            t.styles();
        }
        assert!(
            lookup_theme("").is_ok(),
            "the empty name must fall back to {DEFAULT_THEME}"
        );
        assert!(lookup_theme("nope").is_err());
    }

    #[test]
    fn blend_rejects_junk() {
        assert_eq!(blend("not a colour", "#ffffff", 0.5), "not a colour");
        assert_eq!(blend("#000000", "#ffffff", 1.0), "#ffffff");
    }

    // The contrast ladder in the list: an unselected row sits between the
    // comment colour and the foreground, closer to the comment colour, so the
    // selected row stays the brightest thing in the list.
    #[test]
    fn unselected_rows_are_lifted() {
        let dist = |a: &str, b: &str| {
            let (x, y) = (parse_hex(a).unwrap(), parse_hex(b).unwrap());
            (x.0 as i32 - y.0 as i32).abs()
                + (x.1 as i32 - y.1 as i32).abs()
                + (x.2 as i32 - y.2 as i32).abs()
        };
        for name in theme_names() {
            let t = lookup_theme(name).unwrap();
            let row = blend(t.comment, t.fg, ROW_LIFT);
            assert_ne!(row, t.comment, "theme {name}");
            assert!(
                dist(&row, t.comment) < dist(&row, t.fg),
                "theme {name}: {row} drifted past halfway"
            );
        }
    }
}
