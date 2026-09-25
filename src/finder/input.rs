//! The two widgets the finder borrowed from bubbles: a one-line text box that
//! completes what is typed from a list of suggestions, and a spinner.

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use super::Key;

/// Input is the box the query and the slash commands are typed into.
#[derive(Debug, Clone, Default)]
pub struct Input {
    value: Vec<char>,
    pos: usize, // the cursor, in chars
    suggestions: Vec<String>,
    matched: Vec<String>, // the suggestions the value is a prefix of
}

impl Input {
    pub fn value(&self) -> String {
        self.value.iter().collect()
    }

    /// set_value replaces what is in the box and puts the cursor at its end.
    pub fn set_value(&mut self, v: &str) {
        self.value = v.chars().collect();
        self.pos = self.value.len();
        self.update_matches();
    }

    pub fn set_suggestions(&mut self, s: Vec<String>) {
        self.suggestions = s;
        self.update_matches();
    }

    /// update_matches keeps the suggestions the value starts, ignoring case.
    /// An empty box suggests nothing.
    fn update_matches(&mut self) {
        let v = self.value().to_lowercase();
        self.matched = if v.is_empty() {
            Vec::new()
        } else {
            self.suggestions
                .iter()
                .filter(|s| s.to_lowercase().starts_with(&v))
                .cloned()
                .collect()
        };
    }

    /// completion is what Tab would add to the value.
    fn completion(&self) -> Option<String> {
        let s: Vec<char> = self.matched.first()?.chars().collect();
        (s.len() > self.value.len()).then(|| s[self.value.len()..].iter().collect())
    }

    /// update edits the box for one key press, the way bubbles' textinput did
    /// with its default key map.
    pub fn update(&mut self, k: &Key) {
        if let Some(c) = k.text {
            self.value.insert(self.pos, c);
            self.pos += 1;
            self.update_matches();
            return;
        }
        match k.name.as_str() {
            "left" | "ctrl+b" => self.pos = self.pos.saturating_sub(1),
            "right" | "ctrl+f" => self.pos = (self.pos + 1).min(self.value.len()),
            "home" | "ctrl+a" => self.pos = 0,
            "end" | "ctrl+e" => self.pos = self.value.len(),
            "alt+left" | "ctrl+left" | "alt+b" => self.pos = self.word_start(),
            "alt+right" | "ctrl+right" | "alt+f" => self.pos = self.word_end(),
            "backspace" | "ctrl+h" if self.pos > 0 => {
                self.value.remove(self.pos - 1);
                self.pos -= 1;
            }
            "delete" | "ctrl+d" if self.pos < self.value.len() => {
                self.value.remove(self.pos);
            }
            "alt+backspace" | "ctrl+w" => {
                let from = self.word_start();
                self.value.drain(from..self.pos);
                self.pos = from;
            }
            "alt+delete" | "alt+d" => {
                let to = self.word_end();
                self.value.drain(self.pos..to);
            }
            "ctrl+u" => {
                self.value.drain(..self.pos);
                self.pos = 0;
            }
            "ctrl+k" => self.value.truncate(self.pos),
            "tab" => {
                if let Some(rest) = self.completion() {
                    self.value.extend(rest.chars());
                    self.pos = self.value.len();
                }
            }
            _ => {}
        }
        self.update_matches();
    }

    /// paste puts text at the cursor, flattened onto the one line there is.
    pub fn paste(&mut self, text: &str) {
        for c in text.chars().filter(|c| !c.is_control()) {
            self.value.insert(self.pos, c);
            self.pos += 1;
        }
        self.update_matches();
    }

    fn word_start(&self) -> usize {
        let mut i = self.pos;
        while i > 0 && self.value[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !self.value[i - 1].is_whitespace() {
            i -= 1;
        }
        i
    }

    fn word_end(&self) -> usize {
        let mut i = self.pos;
        while i < self.value.len() && self.value[i].is_whitespace() {
            i += 1;
        }
        while i < self.value.len() && !self.value[i].is_whitespace() {
            i += 1;
        }
        i
    }

    /// view draws the prompt, the value with the cursor on it, and the rest of
    /// the command the box offers. The cursor is drawn rather than the
    /// terminal's own, and it does not blink.
    pub fn view(
        &self,
        prompt: &str,
        prompt_style: Style,
        text: Style,
        suggestion: Style,
    ) -> Vec<Span<'static>> {
        let cursor = |s: Style| s.add_modifier(Modifier::REVERSED);
        let mut spans = vec![Span::styled(prompt.to_string(), prompt_style)];
        let before: String = self.value[..self.pos].iter().collect();
        spans.push(Span::styled(before, text));
        let rest = self.completion().unwrap_or_default();
        if self.pos < self.value.len() {
            spans.push(Span::styled(self.value[self.pos].to_string(), cursor(text)));
            spans.push(Span::styled(
                self.value[self.pos + 1..].iter().collect::<String>(),
                text,
            ));
            spans.push(Span::styled(rest, suggestion));
        } else {
            let mut rest = rest.chars();
            match rest.next() {
                Some(c) => spans.push(Span::styled(c.to_string(), cursor(suggestion))),
                None => spans.push(Span::styled(" ", cursor(text))),
            }
            spans.push(Span::styled(rest.collect::<String>(), suggestion));
        }
        spans
    }
}

/// Spinner is bubbles' MiniDot: what shows gm is waiting and has not hung.
#[derive(Debug, Clone, Default)]
pub struct Spinner {
    pub frame: usize,
    pub tag: u64, // a tick carries the tag it was issued with; a stale one is dropped
}

pub const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
pub const SPINNER_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1000 / 12);

impl Spinner {
    pub fn view(&self) -> &'static str {
        SPINNER_FRAMES[self.frame % SPINNER_FRAMES.len()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str) -> Input {
        let mut i = Input::default();
        for c in s.chars() {
            i.update(&Key::char(c));
        }
        i
    }

    #[test]
    fn editing_keys() {
        let mut i = typed("hello world");
        i.update(&Key::named("alt+backspace"));
        assert_eq!(i.value(), "hello ");
        i.update(&Key::named("ctrl+a"));
        i.update(&Key::named("delete"));
        assert_eq!(i.value(), "ello ");
        i.update(&Key::named("ctrl+k"));
        assert_eq!(i.value(), "");
        let mut i = typed("ab");
        i.update(&Key::named("left"));
        i.update(&Key::char('x'));
        assert_eq!(i.value(), "axb");
        i.update(&Key::named("ctrl+u"));
        assert_eq!(i.value(), "b");
    }

    #[test]
    fn tab_accepts_the_suggestion() {
        let mut i = typed("/Wo");
        i.set_suggestions(vec!["/help".into(), "/worktrees".into()]);
        assert_eq!(i.completion().as_deref(), Some("rktrees"));
        i.update(&Key::named("tab"));
        // What was typed keeps its case; only the rest is filled in.
        assert_eq!(i.value(), "/Worktrees");
        // An empty box offers nothing.
        let mut e = Input::default();
        e.set_suggestions(vec!["/help".into()]);
        assert_eq!(e.completion(), None);
    }
}
