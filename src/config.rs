//! gm's own settings file, and nothing else: where the repositories live,
//! which theme the finder wears, and which keys it answers to.

use crate::{Error, Result, err, paths};

/// Config mirrors gm.toml.
///
/// ```toml
/// root         = "~/ghq"       # or ["~/ghq", "~/src"], searched in order
/// theme        = "tokyonight"
/// launch_key   = "ctrl-g"      # the shell key that opens gm
/// worktree_key = "ctrl-w"      # the finder key that lists worktrees
/// branch_key   = "ctrl-l"      # the finder key that lists branches
/// pr_key       = "ctrl-j"      # the finder key that lists pull requests
/// remote_key   = "ctrl-alt-b"  # the finder key that opens the remote
/// ```
///
/// Root stays untyped because it takes either form; roots resolves it.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub root: Option<toml::Value>,
    pub theme: String,
    pub launch_key: String,
    pub worktree_key: String,
    pub branch_key: String,
    pub pr_key: String,
    pub remote_key: String,
}

/// KEYS are every setting gm.toml may hold.
const KEYS: [&str; 7] = [
    "root",
    "theme",
    "launch_key",
    "worktree_key",
    "branch_key",
    "pr_key",
    "remote_key",
];

/// path is where gm looks for its settings:
///
///   $XDG_CONFIG_HOME/gm/gm.toml, else ~/.config/gm/gm.toml
pub fn path() -> Result<String> {
    let dir = match std::env::var("XDG_CONFIG_HOME") {
        Ok(d) if !d.is_empty() => d,
        _ => paths::join(&paths::home()?, ".config"),
    };
    Ok(paths::join(&dir, "gm/gm.toml"))
}

/// load reads gm.toml. A missing file is the default Config, but a file that
/// cannot be parsed is an error: silently cloning into the wrong tree is worse
/// than refusing to run.
pub fn load() -> Result<Config> {
    load_from(&path()?)
}

pub fn load_from(path: &str) -> Result<Config> {
    match std::fs::read_to_string(path) {
        Ok(body) => parse(&body).map_err(|e| err!("{path}: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(err!("{path}: {e}")),
    }
}

/// parse reads the body of a gm.toml.
pub fn parse(body: &str) -> Result<Config> {
    let table: toml::Table = body
        .parse()
        .map_err(|e: toml::de::Error| Error(e.to_string().trim_end().to_string()))?;

    // A key gm does not know is a typo or a setting that has been renamed.
    // Ignoring it silently leaves the user staring at a file that says one
    // thing while gm does another.
    let unknown: Vec<String> = table
        .keys()
        .filter(|k| !KEYS.contains(&k.as_str()))
        .map(|k| format!("{k:?}"))
        .collect();
    if !unknown.is_empty() {
        return Err(err!("unknown key {}", unknown.join(", ")));
    }

    let string = |key: &str| -> Result<String> {
        match table.get(key) {
            None => Ok(String::new()),
            Some(toml::Value::String(s)) => Ok(s.clone()),
            Some(_) => Err(err!("{key} must be a string")),
        }
    };
    Ok(Config {
        root: table.get("root").cloned(),
        theme: string("theme")?,
        launch_key: string("launch_key")?,
        worktree_key: string("worktree_key")?,
        branch_key: string("branch_key")?,
        pr_key: string("pr_key")?,
        remote_key: string("remote_key")?,
    })
}

impl Config {
    /// roots normalizes the root field to a list. No root at all is None,
    /// which leaves the caller's own fallbacks in charge.
    pub fn roots(&self) -> Result<Option<Vec<String>>> {
        let path = path().unwrap_or_default();
        let wrong = || err!("{path}: root must be a string or a list of strings");
        let roots = match &self.root {
            None => return Ok(None),
            Some(toml::Value::String(s)) => vec![s.clone()],
            Some(toml::Value::Array(a)) => a
                .iter()
                .map(|v| v.as_str().map(str::to_string).ok_or_else(wrong))
                .collect::<Result<Vec<_>>>()?,
            Some(_) => return Err(wrong()),
        };
        if roots.is_empty() || roots.iter().any(String::is_empty) {
            return Err(err!("{path}: root is empty"));
        }
        Ok(Some(roots))
    }
}

/// Chord is one key combination, in the spellings the places that use it
/// need. Ctrl is always required — a bare letter is text the user is typing —
/// and Alt and Shift may be added on top of it.
///
/// Which combinations actually reach a program depends on the terminal. A
/// plain Ctrl chord arrives everywhere. Ctrl-Alt travels as an ESC prefix and
/// arrives nearly everywhere. Ctrl-Shift needs the Kitty keyboard protocol,
/// since the legacy encoding sends the same byte for Ctrl-B and Ctrl-Shift-B.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Chord {
    pub letter: char,    // lowercase, e.g. 'w'
    pub alt: bool,       // Ctrl-Alt-<letter>
    pub shift: bool,     // Ctrl-Shift-<letter>
    pub display: String, // "Ctrl-Alt-B", for help text and comments
}

impl Chord {
    /// key is the chord as the finder spells a key press, e.g. "ctrl+alt+b",
    /// always with the modifiers in the order ctrl, alt, shift.
    pub fn key(&self) -> String {
        let mut s = String::from("ctrl+");
        if self.alt {
            s.push_str("alt+");
        }
        if self.shift {
            s.push_str("shift+");
        }
        s.push(self.letter);
        s
    }

    /// short is the chord as the finder's hint line writes it, e.g. "ctrl-alt-b".
    pub fn short(&self) -> String {
        self.display.to_lowercase()
    }

    /// plain reports whether the chord is Ctrl and a letter, with no other
    /// modifier. Only plain chords can collide with the finder's fixed keys,
    /// and only plain chords can be handed to a shell.
    pub fn plain(&self) -> bool {
        !self.alt && !self.shift
    }
}

/// MODIFIERS are the optional prefixes accepted after the required ctrl, in
/// any order: ctrl-alt-shift-b and ctrl-shift-alt-b are the same chord.
const ALT: [&str; 6] = ["alt-", "alt+", "a-", "meta-", "meta+", "m-"];
const SHIFT: [&str; 3] = ["shift-", "shift+", "s-"];

/// parse_chord reads a chord out of gm.toml. Ctrl is written ctrl-, ctrl+, c-
/// or ^; alt and shift may follow it in any order:
///
///   ctrl-w  ctrl+w  c-w  ^w
///   ctrl-alt-b  ctrl+alt+b  c-a-b  ctrl-meta-b
///   ctrl-shift-b  ctrl-alt-shift-b
///
/// Case does not matter, and the empty string means "not set" and takes the
/// fallback.
pub fn parse_chord(s: &str, fallback: &str) -> Result<Chord> {
    let s = if s.trim().is_empty() { fallback } else { s };
    let lower = s.trim().to_lowercase();

    let mut t = ["ctrl-", "ctrl+", "c-", "^"]
        .iter()
        .find_map(|p| lower.strip_prefix(p))
        .unwrap_or(""); // no ctrl, so nothing left that could be a chord

    let mut c = Chord::default();
    loop {
        if let Some(rest) = ALT.iter().find_map(|p| t.strip_prefix(p)) {
            c.alt = true;
            t = rest;
        } else if let Some(rest) = SHIFT.iter().find_map(|p| t.strip_prefix(p)) {
            c.shift = true;
            t = rest;
        } else {
            break;
        }
    }

    let mut chars = t.chars();
    let (Some(letter), None) = (chars.next(), chars.next()) else {
        return Err(err!(
            "cannot bind {s:?}: use a Ctrl chord such as {fallback}"
        ));
    };
    if !letter.is_ascii_lowercase() {
        return Err(err!(
            "cannot bind {s:?}: use a Ctrl chord such as {fallback}"
        ));
    }
    c.letter = letter;
    c.display = format!(
        "Ctrl-{}{}{}",
        if c.alt { "Alt-" } else { "" },
        if c.shift { "Shift-" } else { "" },
        letter.to_ascii_uppercase()
    );
    Ok(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    fn roots(body: &str) -> Result<Option<Vec<String>>> {
        parse(body)?.roots()
    }

    #[test]
    fn roots_are_read_in_either_form() {
        let dir = TempDir::new();
        let missing = load_from(&dir.join("gm.toml")).unwrap();
        assert_eq!(
            missing.roots().unwrap(),
            None,
            "a missing file is not an error"
        );

        assert_eq!(
            roots("root = \"~/code\"\n").unwrap(),
            Some(vec!["~/code".to_string()])
        );
        assert_eq!(
            roots("root = [\"/a\", \"/b\"]\n").unwrap(),
            Some(vec!["/a".to_string(), "/b".to_string()])
        );
    }

    // A broken config must stop gm rather than silently send clones elsewhere.
    #[test]
    fn a_broken_config_is_an_error() {
        for (name, body) in [
            ("malformed", "root = \n"),
            ("wrong type", "root = 42\n"),
            ("mixed list", "root = [\"/a\", 7]\n"),
            ("empty list", "root = []\n"),
            // A key gm does not know is a typo or a renamed setting; either
            // way the file says one thing and gm would do another.
            ("unknown key", "root = \"/a\"\nkeybind = \"ctrl-g\"\n"),
            ("typo", "root = \"/a\"\nthemes = \"dracula\"\n"),
            ("a key that is not a string", "theme = 3\n"),
        ] {
            assert!(roots(body).is_err(), "{name}: {:?}", roots(body));
        }
        assert!(
            parse("keybind = \"x\"\n")
                .unwrap_err()
                .0
                .contains("\"keybind\""),
            "the error names the key"
        );
    }

    #[test]
    fn theme_and_keys_are_read() {
        let c = parse("root = \"/a\"\ntheme = \"dracula\"\n").unwrap();
        assert_eq!(c.theme, "dracula");
        assert_eq!(parse("root = \"/a\"\n").unwrap().theme, "");

        let c = parse(
            "launch_key = \"ctrl-j\"\nworktree_key = \"ctrl-t\"\nbranch_key = \"ctrl-b\"\npr_key = \"ctrl-o\"\nremote_key = \"ctrl-alt-r\"\n",
        )
        .unwrap();
        assert_eq!(
            (
                c.launch_key.as_str(),
                c.worktree_key.as_str(),
                c.branch_key.as_str(),
                c.pr_key.as_str(),
                c.remote_key.as_str()
            ),
            ("ctrl-j", "ctrl-t", "ctrl-b", "ctrl-o", "ctrl-alt-r")
        );
    }

    #[test]
    fn parse_chord_reads_every_spelling() {
        for s in ["ctrl-r", "ctrl+r", "Ctrl-R", "c-r", "^R", " ctrl-r "] {
            let c = parse_chord(s, "ctrl-g").unwrap();
            assert_eq!(
                (
                    c.letter,
                    c.display.as_str(),
                    c.key().as_str(),
                    c.short().as_str()
                ),
                ('r', "Ctrl-R", "ctrl+r", "ctrl-r"),
                "{s:?}"
            );
        }
        // The empty string means "unset" and takes the fallback.
        assert_eq!(parse_chord("", "ctrl-g").unwrap().letter, 'g');

        for bad in ["r", "ctrl-", "ctrl-rr", "alt-r", "ctrl-1", "f5"] {
            assert!(parse_chord(bad, "ctrl-g").is_err(), "{bad:?}");
        }
    }

    #[test]
    fn parse_chord_with_alt() {
        // The chord the remote key defaults to. Alt travels as an ESC prefix,
        // so unlike Ctrl-Shift it reaches a program on an ordinary terminal.
        for s in [
            "ctrl-alt-b",
            "ctrl+alt+b",
            "Ctrl-Alt-B",
            "c-a-b",
            "ctrl-meta-b",
            "^alt-b",
        ] {
            let c = parse_chord(s, "ctrl-w").unwrap();
            assert!(
                c.alt && !c.shift && c.letter == 'b' && !c.plain(),
                "{s:?}: {c:?}"
            );
            assert_eq!(
                (c.key().as_str(), c.display.as_str(), c.short().as_str()),
                ("ctrl+alt+b", "Ctrl-Alt-B", "ctrl-alt-b"),
                "{s:?}"
            );
        }
        // Both modifiers, in either order, are the same chord.
        for s in ["ctrl-alt-shift-b", "ctrl-shift-alt-b"] {
            let c = parse_chord(s, "ctrl-w").unwrap();
            assert!(
                c.alt && c.shift && c.key() == "ctrl+alt+shift+b",
                "{s:?}: {c:?}"
            );
        }
        // Alt without Ctrl is not a chord gm binds.
        for bad in ["alt-b", "a-b", "ctrl-alt-", "ctrl-alt-bb"] {
            assert!(parse_chord(bad, "ctrl-w").is_err(), "{bad:?}");
        }
        assert!(parse_chord("ctrl-w", "ctrl-w").unwrap().plain());
    }

    #[test]
    fn parse_chord_with_shift() {
        for s in ["ctrl-shift-b", "ctrl+shift+b", "Ctrl-Shift-B", "c-s-b"] {
            let c = parse_chord(s, "ctrl-w").unwrap();
            assert!(c.shift && c.letter == 'b', "{s:?}: {c:?}");
            assert_eq!(
                (c.key().as_str(), c.display.as_str(), c.short().as_str()),
                ("ctrl+shift+b", "Ctrl-Shift-B", "ctrl-shift-b"),
                "{s:?}"
            );
        }
        for bad in ["shift-b", "s-b", "ctrl-shift-", "ctrl-shift-bb"] {
            assert!(parse_chord(bad, "ctrl-w").is_err(), "{bad:?}");
        }
    }
}
