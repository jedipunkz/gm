//! The terminal the finder owns while it is up: /dev/tty in raw mode and
//! the alternate screen, put back however the finder ends.

use super::*;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind, KeyModifiers,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::crossterm::{cursor, execute, terminal};

pub struct Term {
    term: ratatui::Terminal<CrosstermBackend<Box<dyn Write>>>,
}

/// tty is the terminal to draw on: /dev/tty, so $(gm) can capture stdout,
/// or stderr when there is no controlling terminal.
fn tty() -> Box<dyn Write> {
    match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
    {
        Ok(f) => Box::new(f),
        Err(_) => Box::new(std::io::stderr()),
    }
}

impl Term {
    pub fn open() -> Result<Term> {
        terminal::enable_raw_mode()?;
        // Until a Term exists nothing will put the terminal back, so a
        // failure on the way there has to do it here.
        let opened = (|| -> std::io::Result<Term> {
            let mut out = tty();
            // The Kitty keyboard protocol is what lets Ctrl-Shift chords
            // through. It is asked for without asking whether the
            // terminal speaks it: the question is answered on stdout,
            // which belongs to the shell binding, and a terminal that
            // does not ignores it.
            execute!(
                out,
                EnterAlternateScreen,
                EnableBracketedPaste,
                cursor::Hide,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )?;
            Ok(Term {
                term: ratatui::Terminal::new(CrosstermBackend::new(out))?,
            })
        })();
        opened.map_err(|e| {
            restore(&mut tty());
            e.into()
        })
    }

    pub fn size(&self) -> Result<(u16, u16)> {
        let s = self.term.size()?;
        Ok((s.width, s.height))
    }

    pub fn draw(&mut self, m: &Model) -> Result<()> {
        self.term.draw(|f| m.render(f.buffer_mut()))?;
        Ok(())
    }

    /// poll waits up to wait for something from the terminal.
    pub fn poll(&mut self, wait: Duration) -> Result<Option<Msg>> {
        if !event::poll(wait)? {
            return Ok(None);
        }
        Ok(match event::read()? {
            Event::Key(k) if k.kind != KeyEventKind::Release => {
                key_of(k.code, k.modifiers).map(Msg::Key)
            }
            Event::Paste(s) => Some(Msg::Paste(s)),
            Event::Resize(w, h) => {
                self.term.autoresize()?;
                Some(Msg::Resize(w, h))
            }
            _ => None,
        })
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        restore(self.term.backend_mut());
    }
}

/// restore undoes what open did, whichever part of it happened.
fn restore(out: &mut impl Write) {
    let _ = execute!(
        out,
        PopKeyboardEnhancementFlags,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        cursor::Show
    );
    let _ = terminal::disable_raw_mode();
}

/// key_of spells a key press the way Bubble Tea did, modifiers in the
/// order ctrl, alt, shift.
pub fn key_of(code: KeyCode, m: KeyModifiers) -> Option<Key> {
    let (ctrl, alt) = (
        m.contains(KeyModifiers::CONTROL),
        m.contains(KeyModifiers::ALT),
    );
    let mut shift = m.contains(KeyModifiers::SHIFT);
    let base = match code {
        KeyCode::Char(c) if !ctrl && !alt => return Some(Key::char(c)),
        KeyCode::Char(c) => {
            shift |= c.is_uppercase();
            c.to_lowercase().to_string()
        }
        KeyCode::Enter => "enter".into(),
        KeyCode::Esc => "esc".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::BackTab => {
            shift = true;
            "tab".into()
        }
        KeyCode::Backspace => "backspace".into(),
        KeyCode::Delete => "delete".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        KeyCode::PageUp => "pgup".into(),
        KeyCode::PageDown => "pgdown".into(),
        KeyCode::F(n) => format!("f{n}"),
        _ => return None,
    };
    let mut name = String::new();
    if ctrl {
        name.push_str("ctrl+");
    }
    if alt {
        name.push_str("alt+");
    }
    if shift {
        name.push_str("shift+");
    }
    name.push_str(&base);
    Some(Key { name, text: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_spelled_the_bubble_tea_way() {
        let k = |c, m| key_of(c, m).unwrap().name;
        assert_eq!(
            k(
                KeyCode::Char('b'),
                KeyModifiers::CONTROL | KeyModifiers::ALT
            ),
            "ctrl+alt+b"
        );
        assert_eq!(k(KeyCode::Char('B'), KeyModifiers::CONTROL), "ctrl+shift+b");
        assert_eq!(k(KeyCode::Char('w'), KeyModifiers::CONTROL), "ctrl+w");
        assert_eq!(k(KeyCode::Esc, KeyModifiers::NONE), "esc");
        assert_eq!(
            key_of(KeyCode::Char('Y'), KeyModifiers::SHIFT),
            Some(Key::char('Y'))
        );
    }
}
