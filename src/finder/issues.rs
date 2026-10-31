//! The issue list and the panel it opens: gh's open issues for the selected
//! repository, fuzzy filtered like every other list, and one issue's full
//! text on request.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Clear, Paragraph, Widget};

use super::info::{Seg, wrap_segs};
use super::list::Item;
use super::overlay::Overlay;
use super::worktree::Mode;
use super::{Cmd, IssueDetail, Issues, Model, Msg};
use crate::plural;
use crate::repo::{self, Issue};

impl Model {
    /// open_issues asks gh for the selected repository's open issues, off the
    /// UI thread: it goes to GitHub, and the finder stays usable while it
    /// does.
    pub(super) fn open_issues(&mut self) -> Vec<Cmd> {
        let Some(it) = self.current().cloned().filter(|_| self.mode == Mode::Repos) else {
            return vec![];
        };
        let (busy_tag, busy) = self.start_busy(&format!("asking GitHub about {}…", it.label));
        let (issues_of, path) = (self.issues_of.clone(), it.path);
        vec![
            busy,
            Cmd::Task(Box::new(move || {
                let list = issues_of(&path);
                Some(Msg::Issues(Issues {
                    path,
                    busy_tag,
                    issues: list,
                }))
            })),
        ]
    }

    /// show_issues puts gh's answer on screen, if the repository it is about
    /// is still the one selected; otherwise the user has moved on.
    pub(super) fn show_issues(&mut self, msg: Issues) -> Vec<Cmd> {
        self.finish_busy(msg.busy_tag);
        let Some(it) = self
            .current()
            .cloned()
            .filter(|it| self.mode == Mode::Repos && it.path == msg.path)
        else {
            return vec![];
        };
        let issues = match msg.issues {
            Err(e) => {
                self.note = e.0;
                return vec![];
            }
            Ok(issues) if issues.is_empty() => {
                self.note = format!("no open issues in {}", it.label);
                return vec![];
            }
            Ok(issues) => issues,
        };

        // gh lists the newest first; reversed, it lands at the bottom next to
        // the cursor.
        let items = issues
            .iter()
            .rev()
            .map(|i| Item {
                label: i.label(),
                issue: i.clone(),
                ..Default::default()
            })
            .collect();
        self.replace_list(Mode::Issues, &it, items);
        // A full page exactly means the next page cost nothing to hide:
        // #42 not on the list is not "no such issue".
        if issues.len() >= repo::ISSUE_LIMIT {
            self.note = format!("showing the newest {} open issues", repo::ISSUE_LIMIT);
        }
        self.load_status().into_iter().collect()
    }

    /// view_issue is Enter in the issue list: gh is asked for the one issue
    /// in full, off the UI thread, and the panel opens once it is back.
    pub(super) fn view_issue(&mut self) -> Vec<Cmd> {
        let Some(it) = self.current().cloned() else {
            return vec![];
        };
        let n = it.issue.number;
        let (busy_tag, busy) = self.start_busy(&format!("viewing #{n}…"));
        let (issue_of, path) = (self.issue_of.clone(), self.repo_at.clone());
        vec![
            busy,
            Cmd::Task(Box::new(move || {
                let issue = issue_of(&path, n);
                Some(Msg::IssueDetail(IssueDetail {
                    path,
                    busy_tag,
                    issue,
                }))
            })),
        ]
    }

    /// show_issue_detail holds the panel's text and puts the panel up, if the
    /// answer is still about the issue list on screen.
    pub(super) fn show_issue_detail(&mut self, msg: IssueDetail) -> Vec<Cmd> {
        self.finish_busy(msg.busy_tag);
        if self.mode != Mode::Issues || msg.path != self.repo_at {
            return vec![];
        }
        match msg.issue {
            Err(e) => self.note = e.0,
            Ok(issue) => {
                self.detail = Some(Detail { issue, scroll: 0 });
                self.over = Overlay::Issue;
                self.note.clear();
            }
        }
        vec![]
    }

    /// scroll_detail moves the panel's window, one line or a pane's worth,
    /// never further than the end of what the issue wraps to.
    pub(super) fn scroll_detail(&mut self, page: bool, down: bool) {
        let avail = self.detail_avail();
        let by = if page { avail } else { 1 };
        let max = self.issue_lines(self.panel_w()).len().saturating_sub(avail);
        let Some(d) = self.detail.as_mut() else {
            return;
        };
        if down {
            d.scroll = (d.scroll + by).min(max);
        } else {
            d.scroll = d.scroll.saturating_sub(by);
        }
    }

    /// close_detail takes the panel down, back to the issue list it came
    /// from.
    pub(super) fn close_detail(&mut self) {
        self.over = Overlay::None;
        self.detail = None;
        self.note.clear();
    }

    /// open_issue_in_browser hands the issue's URL to the platform's browser,
    /// on a thread the way the remote key does, since a browser that starts
    /// slowly must not hold the panel.
    pub(super) fn open_issue_in_browser(&mut self) -> Vec<Cmd> {
        let Some(d) = self.detail.as_ref() else {
            return vec![];
        };
        let url = d.issue.url.clone();
        if url.is_empty() {
            return vec![];
        }
        let open = self.open_url.clone();
        let n = d.issue.number;
        self.note = format!("opening #{n} in the browser…");
        vec![Cmd::Task(Box::new(move || {
            open(&url);
            None
        }))]
    }
    /// copy_issue_url puts the issue's URL on the clipboard the terminal
    /// remembers, and says so: a terminal that ignores OSC52 stays silent on
    /// its own side, so the note is the only confirmation there is.
    pub(super) fn copy_issue_url(&mut self) -> Vec<Cmd> {
        let Some(d) = self.detail.as_ref() else {
            return vec![];
        };
        let url = d.issue.url.clone();
        if url.is_empty() {
            return vec![];
        }
        let n = d.issue.number;
        self.note = match (self.copy_url)(&url) {
            Ok(()) => format!("#{n}: issue URL copied"),
            Err(e) => format!("could not copy the issue URL: {e}"),
        };
        vec![]
    }

    /// panel_w is the width the issue view's lines wrap to: the whole
    /// screen, a single cell of air either side.
    fn panel_w(&self) -> usize {
        (self.w as usize).saturating_sub(2).max(20)
    }

    /// detail_avail is how many rows of the issue show at once: the whole
    /// screen, the hint line's last row beside.
    fn detail_avail(&self) -> usize {
        (self.h as usize).saturating_sub(2).max(4)
    }

    /// issue_lines is every line the panel holds: the issue on top, then its
    /// body, then its comments after it.
    fn issue_lines(&self, w: usize) -> Vec<Line<'static>> {
        use super::md;
        let st = &self.st;
        let Some(d) = self.detail.as_ref() else {
            return vec![];
        };
        let i = &d.issue;

        let mut out = Vec::new();
        // A note — a copy's answer, a browser opening — is the one word the
        // user is waiting for, so it goes where the title usually is.
        if !self.note.is_empty() {
            out.push(Line::from(Span::styled(self.note.clone(), st.dirty)));
        } else {
            let tag = format!("#{} [{}]", i.number, i.state);
            out.extend(wrap_segs(
                &[
                    Seg::new(&i.title, st.name),
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
        field(&mut out, "author", &i.author, st.remote);
        field(&mut out, "labels", &i.labels.join(", "), st.branch);
        field(&mut out, "url", &i.url, st.remote);

        if !i.body.is_empty() {
            out.push(Line::default());
            // The description carries its own frame, the way the comments
            // each do; the screen holds no box around the lot.
            let body = md::render(&i.body, w.saturating_sub(6), st);
            out.extend(framed("description", &body, w, st));
        }
        if !i.comments.is_empty() {
            out.push(Line::from(Span::styled(
                plural(i.comments.len(), "comment", "comments"),
                st.label,
            )));
        }
        for c in &i.comments {
            out.push(Line::default());
            // Each comment comes in its own Rounded frame, the author as
            // its title.
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

    /// render_issue draws the issue full screen with no frame of its own:
    /// the text, the description's and the comments' frames inside, and one
    /// hint line naming the keys along the bottom.
    pub(super) fn render_issue(&self, buf: &mut ratatui::buffer::Buffer) {
        let area = *buf.area();
        let w = self.panel_w();
        let lines = self.issue_lines(w);
        if lines.is_empty() {
            return;
        }
        let hint = detail_hints(&self.st, w);
        let avail = self.detail_avail();
        // The text window leaves the hint line its row; it keeps to the end
        // of what fits, so a tall issue folds under the window, never the
        // hint.
        let max = lines.len().saturating_sub(avail);
        let scroll = self.detail.as_ref().map(|d| d.scroll.min(max)).unwrap_or(0);
        let shown = if lines.len() <= avail {
            lines
        } else {
            lines[scroll..(scroll + avail).min(lines.len())].to_vec()
        };

        Clear.render(area, buf);
        // One cell of air at either edge keeps the text off the very
        // corners without a frame around the lot.
        let text = Rect::new(area.x + 1, area.y, w as u16, avail as u16).intersection(area);
        Paragraph::new(shown).render(text, buf);
        let row = area.y + area.height.saturating_sub(1);
        let under = Rect::new(area.x + 1, row, w as u16, 1).intersection(area);
        Paragraph::new(hint).render(under, buf);
    }
}

/// detail_hints is the panel's hint line, one row at the bottom: its keys
/// and what each does, the tail dropped when the pane is too narrow, the
/// way the finder's own hint line is.
fn detail_hints(st: &super::theme::Styles, w: usize) -> Line<'static> {
    const SEP: &str = "  ·  ";
    let keys = [
        ("q/esc", "list"),
        ("space", "page"),
        ("b", "back"),
        ("j/k", "a line"),
        ("ctrl+b", "browse"),
        ("ctrl+y", "copy url"),
    ];
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

/// framed draws one comment in its own rounded frame: the author as its
/// title, the body's Markdown between the sides, and the pane's edge on the
/// right where the frame ends. The rows are exactly one pane wide, so the
/// frame's corners line up.
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
    // The title rides the top rail, where a named worktree row does; an
    // author too long for the rail loses the rail's decoration first.
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

/// Detail is the one issue the panel is showing, held until the key that
/// closes it, with the scroll its window is at.
#[derive(Clone)]
pub struct Detail {
    pub(super) issue: Issue,
    pub(super) scroll: usize,
}
