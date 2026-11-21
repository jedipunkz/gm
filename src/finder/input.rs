use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use super::Key;

#[derive(Debug, Clone, Default)]
pub struct Input {
    value: Vec<char>,
    pos: usize,
    suggestions: Vec<String>,
    matched: Vec<String>,
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

    fn completion(&self) -> Option<String> {
        let s: Vec<char> = self.matched.first()?.chars().collect();
        (s.len() > self.value.len()).then(|| s[self.value.len()..].iter().collect())
    }

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
            "tab" if self.completion().is_some() => {
                self.value = self.matched[0].chars().collect();
                self.pos = self.value.len();
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

#[derive(Debug, Clone, Default)]
pub struct Spinner {
    pub frame: usize,
    pub tag: u64,
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
        assert_eq!(i.value(), "/worktrees");
        let mut e = Input::default();
        e.set_suggestions(vec!["/help".into()]);
        assert_eq!(e.completion(), None);
    }

    #[test]
    fn cuts_and_jumps_by_word() {
        let mut i = typed("a b c");
        i.update(&Key::named("alt+d"));
        assert_eq!(i.value(), "a b c");
        i.update(&Key::named("ctrl+a"));
        i.update(&Key::named("alt+d"));
        assert_eq!(i.value(), " b c");
        i.update(&Key::named("ctrl+e"));
        i.update(&Key::named("ctrl+w"));
        assert_eq!(i.value(), " b ");
        let mut j = typed("a b");
        j.update(&Key::named("alt+b"));
        j.update(&Key::named("ctrl+k"));
        assert_eq!(j.value(), "a ");
    }

    #[test]
    fn paste_keeps_printable_characters() {
        let mut i = typed("ab");
        i.update(&Key::named("left"));
        i.paste("X\t[0m\nY");
        assert_eq!(i.value(), "aX[0mYb");
        i.set_suggestions(vec!["/help".into()]);
        assert_eq!(i.completion(), None, "a match would be a mistake here");
    }

    #[test]
    fn an_unknown_key_is_a_no_op() {
        let mut i = typed("ab");
        for k in ["ctrl+meta", "f13", "media-play", "clear"] {
            i.update(&Key::named(k));
            assert_eq!(i.value(), "ab", "{k} changed the box");
        }
    }

    #[test]
    fn view_draws_the_cursor() {
        let mut i = typed("ab");
        i.set_suggestions(vec!["abc".into()]);
        i.update(&Key::named("left"));
        let s = span_text(&i.view("> ", Style::default(), Style::default(), Style::default()));
        assert_eq!(
            s,
            ["> ", "a", "b", "c"].concat(),
            "the cursor sits on b, abc trailing"
        );
        i.update(&Key::named("end"));
        let s = span_text(&i.view("> ", Style::default(), Style::default(), Style::default()));
        assert_eq!(s, "> abc", "the cursor sits on the suggestion's next c");
    }

    fn span_text(spans: &[Span<'static>]) -> String {
        spans.iter().map(|s| s.content.to_string()).collect()
    }
}
