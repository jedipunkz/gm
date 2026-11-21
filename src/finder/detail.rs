use std::cell::RefCell;
use std::sync::Arc;

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Widget};

use super::info::{Seg, tildify, wrap_segs};
use super::overlay::Overlay;
use super::{Cmd, Model};
use crate::plural;
use crate::repo::{Comment, Issue, PullRequest};

pub(super) type CachedLines = (usize, String, Arc<Vec<Line<'static>>>);

impl Model {
    pub(super) fn scroll_detail(&mut self, page: bool, down: bool) {
        let avail = self.detail_avail();
        let by = if page { avail } else { 1 };
        let max = self
            .detail_lines(self.panel_w())
            .len()
            .saturating_sub(avail);
        let Some(d) = self.detail.as_mut() else {
            return;
        };
        if down {
            d.scroll = (d.scroll + by).min(max);
        } else {
            d.scroll = d.scroll.saturating_sub(by);
        }
    }

    pub(super) fn close_detail(&mut self) {
        self.over = Overlay::None;
        self.detail = None;
        self.note.clear();
    }

    pub(super) fn open_detail_in_browser(&mut self) -> Vec<Cmd> {
        let Some(d) = self.detail.as_ref() else {
            return vec![];
        };
        let url = d.subject.url().to_string();
        if url.is_empty() {
            return vec![];
        }
        let open = self.backend.open_url.clone();
        let n = d.subject.number();
        self.note = format!("opening #{n} in the browser…");
        vec![Cmd::Task(Box::new(move || {
            open(&url);
            None
        }))]
    }
    pub(super) fn copy_detail_url(&mut self) -> Vec<Cmd> {
        let Some(d) = self.detail.as_ref() else {
            return vec![];
        };
        let url = d.subject.url().to_string();
        if url.is_empty() {
            return vec![];
        }
        let (n, what) = (d.subject.number(), d.subject.noun());
        self.note = match (self.backend.copy_url)(&url) {
            Ok(()) => format!("#{n}: {what} URL copied"),
            Err(e) => format!("could not copy the {what} URL: {e}"),
        };
        vec![]
    }

    fn panel_w(&self) -> usize {
        (self.w as usize).saturating_sub(2).max(20)
    }

    fn detail_avail(&self) -> usize {
        (self.h as usize).saturating_sub(2).max(4)
    }

    fn detail_lines(&self, w: usize) -> Arc<Vec<Line<'static>>> {
        let Some(detail) = self.detail.as_ref() else {
            return Arc::new(vec![]);
        };
        let note = &self.note;
        if let Some((cached_w, cached_note, lines)) = detail.lines.borrow().as_ref()
            && *cached_w == w
            && cached_note == note
        {
            return Arc::clone(lines);
        }

        let lines = Arc::new(self.build_detail_lines(w));
        if let Some(detail) = self.detail.as_ref() {
            *detail.lines.borrow_mut() = Some((w, note.clone(), Arc::clone(&lines)));
        }
        lines
    }

    fn build_detail_lines(&self, w: usize) -> Vec<Line<'static>> {
        #[cfg(test)]
        DETAIL_LINE_BUILDS.with(|count| count.set(count.get() + 1));
        use super::md;
        let st = &self.st;
        let Some(d) = self.detail.as_ref() else {
            return vec![];
        };
        let subject = &d.subject;

        let mut out = Vec::new();
        if !self.note.is_empty() {
            out.push(Line::from(Span::styled(self.note.clone(), st.dirty)));
        } else {
            let tag = format!("#{} [{}]", subject.number(), subject.state());
            out.extend(wrap_segs(
                &[
                    Seg::new(subject.title(), st.name),
                    Seg::new("  ", st.dim),
                    Seg::new(&tag, st.visits),
                ],
                w,
                "",
            ));
        }
        let field = |out: &mut Vec<Line<'static>>, k: &str, v: &str, style| {
            if !v.is_empty() {
                out.push(Line::from(Span::styled(k.to_string(), st.label)));
                out.extend(wrap_segs(&[Seg::new(v, style)], w, "  "));
            }
        };
        field(&mut out, "repository", &self.origin, st.dim);
        match subject {
            Subject::Issue(i) => {
                field(&mut out, "author", &i.author, st.remote);
                field(&mut out, "labels", &i.labels.join(", "), st.branch);
                field(&mut out, "url", &i.url, st.remote);
            }
            Subject::Pr(p) => {
                field(&mut out, "author", &p.author, st.remote);
                field(&mut out, "labels", &p.labels.join(", "), st.branch);
                field(&mut out, "branch", &super::pr::pr_branch(p), st.branch);
                let at = self
                    .pr_row(p.number)
                    .map(|it| it.path.clone())
                    .unwrap_or_default();
                if at.is_empty() {
                    field(&mut out, "worktree", "none yet; enter makes one", st.dim);
                } else {
                    field(&mut out, "worktree", &tildify(&at), st.path);
                }
                field(&mut out, "url", &p.url, st.remote);
            }
        }

        if !subject.body().is_empty() {
            out.push(Line::default());
            let body = md::render(subject.body(), w.saturating_sub(6), st);
            out.extend(framed("description", &body, w, st));
        }
        let comments = subject.comments();
        if !comments.is_empty() {
            out.push(Line::from(Span::styled(
                plural(comments.len(), "comment", "comments"),
                st.label,
            )));
        }
        for c in comments {
            out.push(Line::default());
            let who = if c.author.is_empty() {
                "comment"
            } else {
                &c.author
            };
            let body = if c.body.is_empty() {
                vec![Line::from(Span::styled("-", st.dim))]
            } else {
                md::render(&c.body, w.saturating_sub(6), st)
            };
            out.extend(framed(who, &body, w, st));
        }
        out
    }

    pub(super) fn render_detail(&self, buf: &mut ratatui::buffer::Buffer) {
        let area = *buf.area();
        let w = self.panel_w();
        let lines = self.detail_lines(w);
        if lines.is_empty() {
            return;
        }
        let is_pr = matches!(
            self.detail.as_ref().map(|d| &d.subject),
            Some(Subject::Pr(_))
        );
        let hint = detail_hints(&self.st, w, is_pr);
        let avail = self.detail_avail();
        let max = lines.len().saturating_sub(avail);
        let scroll = self.detail.as_ref().map(|d| d.scroll.min(max)).unwrap_or(0);
        let shown = lines[scroll..(scroll + avail).min(lines.len())].to_vec();

        Clear.render(area, buf);
        let text = Rect::new(area.x + 1, area.y, w as u16, avail as u16).intersection(area);
        Paragraph::new(shown).render(text, buf);
        let row = area.y + area.height.saturating_sub(1);
        let under = Rect::new(area.x + 1, row, w as u16, 1).intersection(area);
        Paragraph::new(hint).render(under, buf);
    }
}

fn detail_hints(st: &super::theme::Styles, w: usize, is_pr: bool) -> Line<'static> {
    const SEP: &str = "  ·  ";
    let mut keys = Vec::with_capacity(7);
    if is_pr {
        keys.push(("enter", "check out"));
    }
    keys.extend([
        ("q/esc", "list"),
        ("space", "page"),
        ("b", "back"),
        ("j/k", "a line"),
        ("ctrl+b", "browse"),
        ("ctrl+y", "copy url"),
    ]);
    let mut spans = Vec::new();
    let mut used = 0usize;
    for (i, (key, what)) in keys.into_iter().enumerate() {
        let lead = if i > 0 { SEP } else { "" };
        let cell = Span::raw(lead).width() + Span::raw(key).width() + 1 + Span::raw(what).width();
        if used + cell > w {
            break;
        }
        used += cell;
        spans.push(Span::styled(lead, st.help));
        spans.push(Span::styled(key, st.help_key));
        spans.push(Span::styled(format!(" {what}"), st.help));
    }
    Line::from(spans)
}

pub(super) fn framed(
    title: &str,
    body: &[Line<'static>],
    w: usize,
    st: &super::theme::Styles,
) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    if w < 8 {
        return rows;
    }
    let mut top = vec![Span::styled("╭", st.border)];
    let title_w = Span::raw(title).width();
    if w >= title_w + 6 {
        let dash = w - title_w - 6;
        top.extend([
            Span::styled("─ ", st.border),
            Span::styled(title.to_string(), st.remote),
            Span::styled(format!(" {}╮", "─".repeat(dash)), st.border),
        ]);
    } else {
        top.push(Span::styled(
            format!("{}╮", "─".repeat(w.saturating_sub(3))),
            st.border,
        ));
    }
    rows.push(Line::from(top));
    for l in body {
        let pad = w.saturating_sub(l.width() + 4);
        let mut spans = vec![Span::styled("│ ", st.border)];
        spans.extend(l.spans.iter().cloned());
        spans.push(Span::styled(" ".repeat(pad), st.row));
        spans.push(Span::styled(" │", st.border));
        rows.push(Line::from(spans));
    }
    rows.push(Line::from(Span::styled(
        format!("╰{}╯", "─".repeat(w.saturating_sub(2))),
        st.border,
    )));
    rows
}

#[derive(Clone)]
pub struct Detail {
    pub(super) subject: Subject,
    pub(super) scroll: usize,
    pub(super) lines: RefCell<Option<CachedLines>>,
}

#[cfg(test)]
thread_local! {
    pub(super) static DETAIL_LINE_BUILDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone)]
pub enum Subject {
    Issue(Issue),
    Pr(PullRequest),
}

impl Subject {
    pub(super) fn number(&self) -> u64 {
        match self {
            Subject::Issue(i) => i.number,
            Subject::Pr(p) => p.number,
        }
    }

    fn title(&self) -> &str {
        match self {
            Subject::Issue(i) => &i.title,
            Subject::Pr(p) => &p.title,
        }
    }

    fn state(&self) -> &str {
        match self {
            Subject::Issue(i) => &i.state,
            Subject::Pr(p) if p.draft => "DRAFT",
            Subject::Pr(p) => &p.state,
        }
    }

    fn url(&self) -> &str {
        match self {
            Subject::Issue(i) => &i.url,
            Subject::Pr(p) => &p.url,
        }
    }

    fn body(&self) -> &str {
        match self {
            Subject::Issue(i) => &i.body,
            Subject::Pr(p) => &p.body,
        }
    }

    fn comments(&self) -> &[Comment] {
        match self {
            Subject::Issue(i) => &i.comments,
            Subject::Pr(p) => &p.comments,
        }
    }

    fn noun(&self) -> &'static str {
        match self {
            Subject::Issue(_) => "issue",
            Subject::Pr(_) => "pull request",
        }
    }
}
