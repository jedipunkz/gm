//! A small Markdown renderer for the issue panel. GitHub bodies lean on a
//! short list of the language — headings, fenced code, lists, block quotes,
//! rules, bold, italic, inline code and links — and that short list is what
//! this answers in: every line of the body takes its own row, because a
//! newline is a Markdown structure, not a character to fold.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::info::{Seg, wrap_segs};
use super::theme::Styles;

/// render lays a Markdown body out across a pane w cells wide. Styling is
/// Markdown's own meaning pulled out of the text: headings in the issue's
/// name colour, code in the commit colour, links in the remote colour, and
/// emphasis as emphasis. Everything else is the body's subject colour.
pub fn render(body: &str, w: usize, st: &Styles) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut code = false;
    for src in body.split('\n') {
        let line = src.strip_suffix('\r').unwrap_or(src);
        let trimmed = line.trim();

        // A fence opens and closes a verbatim stretch between blank rules.
        // The marker line itself is nothing to show, so it opens or closes
        // in place.
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            code = !code;
            continue;
        }
        if code {
            out.extend(wrap_segs(
                &[Seg::new(line, st.commit)],
                w.saturating_sub(2),
                "",
            ));
            continue;
        }

        if trimmed.is_empty() {
            out.push(Line::default());
            continue;
        }

        // Headings: bold and separate, the way a heading both reads and
        // stands apart.
        if let Some(rest) = heading_of(trimmed) {
            out.extend(wrap_segs(&[Seg::new(rest, st.name)], w, ""));
            out.push(Line::default());
            continue;
        }

        // Rules: a drawn line, not three dashes.
        if is_rule(trimmed) {
            out.push(Line::from(Span::styled("─".repeat(w.min(40)), st.punct)));
            continue;
        }

        // Block quotes read as someone else's words, dimmed.
        if let Some(rest) = quote_of(line) {
            out.extend(wrap_segs(
                &[Seg::new(&rest, st.dim)],
                w.saturating_sub(2),
                "",
            ));
            continue;
        }

        // Lists carry their nesting and a real bullet; a numbered one keeps
        // its number.
        if let Some((pad, segs)) = list_of(trimmed, st) {
            let pad = " ".repeat(pad);
            let mut indented = vec![Seg::new(&pad, st.dim)];
            indented.extend(segs);
            out.extend(wrap_segs(
                &indented,
                w,
                &format!("{}    ", " ".repeat(pad.len())),
            ));
            continue;
        }

        // Tables and any other row the renderer does not answer to keep
        // their text as it was typed, as code: a table's pipes belong to a
        // grid the list pane has no room to draw.
        if trimmed.starts_with('|') {
            out.extend(wrap_segs(&[Seg::new(trimmed, st.commit)], w, ""));
            continue;
        }

        out.extend(wrap_segs(&inline(trimmed, st), w, ""));
    }
    if !code {
        return out;
    }
    // An unclosed fence keeps rendering verbatim to the end rather than
    // letting the rest of the body merge into it.
    out
}

/// heading_of is a `#` run's text, if it is a heading.
fn heading_of(t: &str) -> Option<&str> {
    let hashes = t.char_indices().take_while(|(_, c)| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &t[hashes..];
    if !rest.starts_with(' ') && !rest.is_empty() {
        return None;
    }
    Some(rest.trim_start())
}

/// is_rule is a horizontal rule: nothing but three or more, three kinds.
fn is_rule(t: &str) -> bool {
    for c in ['-', '_', '*'] {
        let run = t.chars().all(|x| x == c || x == ' ');
        if run && t.chars().filter(|x| *x == c).count() >= 3 {
            return true;
        }
    }
    false
}

/// quote_of strips a `>` and one space, without stacking the levels.
fn quote_of(l: &str) -> Option<String> {
    let obs = l.trim_start();
    let n = obs.chars().take_while(|c| *c == '>').count();
    (n > 0).then(|| {
        let after = &obs[n..];
        let after = after.strip_prefix(' ').unwrap_or(after);
        after.trim_start_matches('>').to_string()
    })
}

/// list_of is a list row's nesting and marked text: bullets keep their
/// nesting by pairs of spaces, numbers keep the number they had.
fn list_of(t: &str, st: &Styles) -> Option<(usize, Vec<Seg>)> {
    let level = t.chars().take_while(|c| *c == ' ').count() / 2;
    let t = t.trim_start();
    if let Some(rest) = t
        .strip_prefix("- ")
        .or_else(|| t.strip_prefix("* "))
        .or_else(|| t.strip_prefix("+ "))
    {
        return Some((
            2 * level,
            vec![Seg::new("• ", st.dim), Seg::new(rest, st.subject)],
        ));
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &t[digits..];
        if let Some(rest) = rest.strip_prefix(". ") {
            return Some((
                2 * level,
                vec![
                    Seg::new(&format!("{:>3}. ", &t[..digits]), st.punct),
                    Seg::new(rest, st.subject),
                ],
            ));
        }
    }
    None
}

/// inline styles one plain-text row by its emphasis: inline code, bold,
/// italic and links. An unbalanced marker stays literal rather than turning
/// half a paragraph into code.
fn inline(t: &str, st: &Styles) -> Vec<Seg> {
    let chars: Vec<char> = t.chars().collect();
    let mut shown: Vec<(char, Style)> = Vec::new();
    let mut i = 0;

    let push = |shown: &mut Vec<(char, Style)>, text: &str, style: Style| {
        for c in text.chars() {
            shown.push((c, style));
        }
    };
    let at = |i: usize| chars.get(i).copied().unwrap_or('\0');

    while i < chars.len() {
        let c = chars[i];
        if c == '\\' {
            // An escape hands the next character through, whatever it is.
            i += 1;
            if let Some(c) = chars.get(i) {
                shown.push((*c, st.subject));
            }
            i += 1;
            continue;
        }
        if c == '`' {
            if let Some(j) = (i + 1..chars.len()).find(|&j| chars[j] == '`') {
                push(
                    &mut shown,
                    &chars[i + 1..j].iter().collect::<String>(),
                    st.commit,
                );
                i = j + 1;
                continue;
            }
            shown.push((c, st.subject));
            i += 1;
            continue;
        }
        if c == '[' {
            // A link reads as its text in the remote colour; the URL it
            // points at is too long to sit in the pane twice.
            if let Some(url) = &link_tail(i, &chars) {
                push(&mut shown, url, st.remote);
                i = link_len(i, &chars);
                continue;
            }
        }
        if (c == '*' || c == '_') && at(i + 1) == c {
            // Bold, when it closes; the markers go, the emphasis stays.
            if let Some(j) =
                (i + 2..chars.len()).find(|&j| chars[j] == c && chars.get(j + 1) == Some(&c))
            {
                push(
                    &mut shown,
                    &chars[i + 2..j].iter().collect::<String>(),
                    st.subject.add_modifier(Modifier::BOLD),
                );
                i = j + 2;
                continue;
            }
            shown.push((c, st.subject));
            shown.push((c, st.subject));
            i += 2;
            continue;
        }
        if c == '*' || c == '_' {
            // Italic, the single marker, again when it closes.
            if let Some(j) = (i + 1..chars.len()).find(|&j| chars[j] == c) {
                push(
                    &mut shown,
                    &chars[i + 1..j].iter().collect::<String>(),
                    st.subject.add_modifier(Modifier::ITALIC),
                );
                i = j + 1;
                continue;
            }
            shown.push((c, st.subject));
            i += 1;
            continue;
        }
        shown.push((c, st.subject));
        i += 1;
    }

    let mut segs: Vec<Seg> = Vec::new();
    for (run, style) in runs(&shown) {
        segs.push(Seg::new(&run, style));
    }
    segs
}

/// link_tail is the `[text](url)` text when one begins at i, so a link is
/// the text it names and nothing else; the length of the whole it took is
/// slightly different.
fn link_tail(i: usize, chars: &[char]) -> Option<String> {
    // Runs from a '[' out through "](...)" and replies with the text.
    let close_br = (i + 1..chars.len()).find(|&j| chars[j] == ']')?;
    if chars.get(close_br + 1) != Some(&'(') {
        return None;
    }
    if !(close_br + 2..chars.len()).any(|j| chars[j] == ')') {
        return None;
    }
    let text: String = chars[i + 1..close_br].iter().collect();
    (!text.is_empty()).then_some(text)
}

/// link_len says how far the `[text](url)` from i reaches.
fn link_len(i: usize, chars: &[char]) -> usize {
    let close_br = (i + 1..chars.len()).find(|&j| chars[j] == ']').unwrap_or(i);
    match (close_br + 1..chars.len()).find(|&j| chars[j] == ')') {
        Some(j) => j + 1,
        None => i + 1,
    }
}

/// runs squeezes adjacent same-styled characters into one run each, so the
/// spans stay legible in the terminal's own accounting.
fn runs(shown: &[(char, Style)]) -> Vec<(String, Style)> {
    let mut out: Vec<(String, Style)> = Vec::new();
    for (c, s) in shown {
        match out.last_mut() {
            Some((last, last_style)) if *last_style == *s => last.push(*c),
            _ => out.push((c.to_string(), *s)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finder::theme::lookup_theme;

    fn styles() -> Styles {
        lookup_theme("").unwrap().styles()
    }

    fn text(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    // A newline is a row: paragraphs stay apart no matter how long they run.
    #[test]
    fn newlines_make_rows() {
        let st = styles();
        let out = render("first paragraph\nsecond paragraph", 40, &st);
        let t = text(&out);
        assert!(t.starts_with("first paragraph\nsecond paragraph"), "{t}");
    }

    // Headings, code fences, lists and quotes take the shapes the pane can
    // draw, and the words all stay on the page.
    #[test]
    fn markdown_shapes() {
        let st = styles();
        let body =
            "# Head\n> quoted\n\n- one\n- two\n\n```\ncode; lines\n```\n\n| a | b |\n| 1 | 2 |\n";
        let out = render(body, 30, &st);
        let t = text(&out);
        assert!(t.contains("Head\n"), "{t}");
        assert!(t.contains("quoted"), "{t}");
        assert!(t.contains("• one") && t.contains("• two"), "{t}");
        assert!(t.contains("code; lines"), "{t}");
        assert!(t.contains("| a | b |"), "{t}");
        // Closing fences end the verbatim stretch.
        assert!(!t.contains("```"), "{t}");
    }

    // Emphasis keeps its meaning and stays literal when it never rolled up.
    #[test]
    fn emphasis_and_links() {
        let st = styles();
        let out = render(
            "**bold** *lt* `mono` [dist](https://x.y/*) and ** unclosed",
            40,
            &st,
        );
        let t = text(&out);
        assert!(t.contains("bold") && !t.contains("**bold"), "{t}");
        assert!(t.contains("lt") && !t.contains("*lt"), "{t}");
        assert!(t.contains("mono") && !t.contains("`mono`"), "{t}");
        assert!(t.contains("dist") && !t.contains("[dist]"), "{t}");
        // The unclosed pair stays the characters that were typed.
        assert!(t.contains("** unclosed"), "{t}");
    }

    // A rule draws; the text under it is its own.
    #[test]
    fn rules_draw() {
        let st = styles();
        let out = render("above\n---\nbelow", 20, &st);
        let t = text(&out);
        assert!(t.contains("above"), "{t}");
        assert!(t.contains("below"), "{t}");
        assert!(t.contains("─"), "{t}");
    }

    // Numbered rows keep their numbers, every bullet mark draws, and a `#`
    // with no space after it is the word it was typed as.
    #[test]
    fn numbers_bullets_and_bare_hashes() {
        let st = styles();
        let out = render("1. first\n12. twelfth\n  + inner\n#tag\n7.no", 30, &st);
        let t = text(&out);
        assert!(t.contains("  1. first"), "{t}");
        assert!(t.contains(" 12. twelfth"), "{t}");
        assert!(t.contains("• inner"), "{t}");
        assert!(t.contains("#tag"), "{t}");
        assert!(t.contains("7.no"), "{t}");
    }

    // Escapes hand the next character through, and a marker that never
    // closes stays the characters that were typed.
    #[test]
    fn escapes_and_unclosed_markers() {
        let st = styles();
        let out = render(
            "\\*not italic\\* `open *half [](w) [x] [y](z end\\",
            80,
            &st,
        );
        let t = text(&out);
        assert!(t.contains("*not italic*"), "{t}");
        assert!(t.contains("`open"), "{t}");
        assert!(t.contains("*half"), "{t}");
        assert!(t.contains("[x]"), "{t}");
        assert!(t.contains("[y](z"), "{t}");
        assert!(t.contains("end"), "{t}");
    }

    // An unclosed fence renders verbatim to the end of the body.
    #[test]
    fn unclosed_fence_runs_to_the_end() {
        let st = styles();
        let out = render("before\n```\n**kept** as typed", 40, &st);
        let t = text(&out);
        assert!(t.contains("before"), "{t}");
        assert!(t.contains("**kept** as typed"), "{t}");
    }
}
