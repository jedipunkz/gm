use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::Model;
use super::worktree::Mode;
use crate::repo::{self, Commit, RefKind, Worktree};

impl Model {
    /// info_lines stacks each field's label above its value, so a long path
    /// or remote URL gets the pane's full width instead of what a label
    /// column leaves.
    pub(super) fn info_lines(&self, w: usize) -> Vec<Line<'static>> {
        let st = &self.st;
        let Some(it) = self.current() else {
            return vec![Line::from(Span::styled("no match", st.dim))];
        };

        let mut out = Vec::new();
        let field = |out: &mut Vec<Line<'static>>, k: &str, v: &str, style: Style| {
            out.push(Line::from(Span::styled(k.to_string(), st.label)));
            out.extend(wrap_segs(
                &[Seg::new(if v.is_empty() { "-" } else { v }, style)],
                w,
                "",
            ));
        };

        // What the pane is about is a field like any other: named, so the top
        // of the pane is not the one line whose meaning has to be inferred.
        match self.mode {
            Mode::Worktrees => {
                field(&mut out, "worktree", &it.label, st.name);
                field(&mut out, "repository", &self.origin, st.dim);
            }
            Mode::Branches => {
                field(&mut out, "branch", &it.label, st.name);
                field(&mut out, "repository", &self.origin, st.dim);
                if it.branch.unfetched {
                    field(
                        &mut out,
                        "worktree",
                        "not fetched yet; enter fetches it and makes one",
                        st.dim,
                    );
                    return out;
                }
                if it.path.is_empty() {
                    field(&mut out, "worktree", "none yet; enter makes one", st.dim);
                    return out;
                }
            }
            Mode::Prs => {
                let p = &it.pr;
                field(&mut out, "pull request", &it.label, st.name);
                field(&mut out, "repository", &self.origin, st.dim);
                field(&mut out, "author", &p.author, st.remote);
                let branch = if p.fork {
                    format!("{}:{}", p.head_owner, p.branch)
                } else {
                    p.branch.clone()
                };
                field(&mut out, "branch", &branch, st.branch);
                if it.path.is_empty() {
                    field(&mut out, "worktree", "none yet; enter makes one", st.dim);
                    return out;
                }
            }
            Mode::Repos => field(&mut out, "repository", &it.label, st.name),
        }
        field(&mut out, "path", &tildify(&it.path), st.path);

        let Some(s) = self.status.get(&it.path) else {
            field(&mut out, "git", "loading…", st.dim);
            return out;
        };
        if self.mode == Mode::Repos {
            field(&mut out, "remote", &s.remote, st.remote);
        }
        field(&mut out, "branch", &s.branch, st.branch);
        if s.dirty > 0 {
            field(
                &mut out,
                "status",
                &format!("{} changed", s.dirty),
                st.dirty,
            );
        } else {
            field(&mut out, "status", "clean", st.clean);
        }
        if self.mode == Mode::Repos {
            if it.seen.count > 0 {
                field(
                    &mut out,
                    "visits",
                    &format!("{}, last {}", it.seen.count, ago(it.seen.last)),
                    st.visits,
                );
            } else {
                field(&mut out, "visits", "never", st.dim);
            }
            // Only when there are some: the pane is for what is there, and
            // this is also the only way to tell whether the worktree key is
            // worth pressing on this row. The newest few, not all of them: a
            // repository with dozens would push everything below it off the
            // pane, and the whole list is one key away.
            let shown = recent_worktrees(&s.worktrees);
            if !shown.is_empty() {
                out.push(Line::from(Span::styled(
                    worktrees_label(shown.len(), s.worktrees.len()),
                    st.label,
                )));
                for wt in &shown {
                    out.extend(self.worktree_lines(wt, w));
                }
            }
        }

        // Last, because the pane is clipped from the bottom: on a short
        // terminal the commits go before the name, the path or the status do.
        if s.commits.is_empty() {
            field(&mut out, "last commit", "", st.commit);
            return out;
        }
        // The count is in the label: the lines fold, so "how many commits am I
        // looking at" is not answerable by counting rows.
        let label = match s.commits.len() {
            1 => "last commit".to_string(),
            n => format!("last {n} commits"),
        };
        out.push(Line::from(Span::styled(label, st.label)));
        for c in &s.commits {
            out.extend(self.commit_lines(c, w));
        }
        out
    }

    /// worktree_lines draws one checkout as its branch and when that branch
    /// was last committed to, which is what ranked it into this list.
    fn worktree_lines(&self, wt: &Worktree, w: usize) -> Vec<Line<'static>> {
        let mut segs = vec![Seg::new(&wt.label(), self.st.branch)];
        if wt.committed_at > 0 {
            segs.push(Seg::new(
                &format!("  {}", ago(wt.committed_at)),
                self.st.dim,
            ));
        }
        wrap_segs(&segs, w, COMMIT_INDENT)
    }

    /// commit_lines draws one commit the way `git log --oneline --decorate`
    /// does: the hash, then the refs pointing at it, then the subject. A line
    /// too long for the pane is folded, with its continuations indented so one
    /// commit still reads as one entry.
    pub(super) fn commit_lines(&self, c: &Commit, w: usize) -> Vec<Line<'static>> {
        let st = &self.st;
        let mut segs = vec![Seg::new(&c.hash, st.commit)];
        if !c.refs.is_empty() {
            segs.push(Seg::new(" (", st.punct));
            for (i, r) in c.refs.iter().enumerate() {
                if i > 0 {
                    segs.push(Seg::new(", ", st.punct));
                }
                let style = match r.kind {
                    RefKind::Head => st.ref_head,
                    RefKind::Remote => st.ref_remote,
                    RefKind::Tag => st.ref_tag,
                    RefKind::Local => st.ref_local,
                };
                segs.push(Seg::new(&r.name, style));
            }
            segs.push(Seg::new(")", st.punct));
        }
        segs.push(Seg::new(&format!(" {}", c.subject), st.subject));
        wrap_segs(&segs, w, COMMIT_INDENT)
    }
}

/// SHOWN_WORKTREES is how many checkouts the pane names. Three is what the
/// commits get, for the same reason: enough to recognise the repository, not
/// enough to become a list.
const SHOWN_WORKTREES: usize = 3;

/// recent_worktrees is the newest few checkouts, by the last commit on their
/// branch. A detached checkout has no branch to date it, so it sorts last,
/// and git's own order breaks ties.
fn recent_worktrees(all: &[Worktree]) -> Vec<Worktree> {
    let mut out = all.to_vec();
    out.sort_by_key(|w| std::cmp::Reverse(w.committed_at));
    out.truncate(SHOWN_WORKTREES);
    out
}

/// worktrees_label says how many of how many, because the rows below it are
/// the newest ones and not the whole set.
fn worktrees_label(shown: usize, total: usize) -> String {
    if total > shown {
        format!("last {shown} of {total} worktrees")
    } else if total == 1 {
        "last worktree".into()
    } else {
        format!("last {total} worktrees")
    }
}

/// COMMIT_INDENT sets the continuation lines of a folded commit in from the
/// hashes, so the eye can still count the commits.
pub(super) const COMMIT_INDENT: &str = "  ";

/// Seg is a run of text with one style, the unit wrapping counts in.
pub struct Seg {
    text: String,
    style: Style,
}

impl Seg {
    pub fn new(text: &str, style: Style) -> Seg {
        Seg {
            text: text.to_string(),
            style,
        }
    }
}

/// wrap_segs lays styled pieces out across lines of width w, breaking at a
/// space where it can and mid-word when a word is longer than the pane. Every
/// line after the first starts with indent. Styles survive the fold: a ref
/// cut across two lines keeps its colour on both.
pub fn wrap_segs(segs: &[Seg], w: usize, indent: &str) -> Vec<Line<'static>> {
    if w < 1 {
        return Vec::new();
    }
    // Flattening to chars keeps the two concerns apart: where the line breaks
    // falls out of the text, and which style each char carries is remembered
    // alongside it. Widths are terminal cells, not Unicode scalar counts.
    let (mut chars, mut widths, mut owner) = (Vec::new(), Vec::new(), Vec::new());
    for (i, s) in segs.iter().enumerate() {
        for c in s.text.chars() {
            chars.push(c);
            widths.push(Span::raw(c.to_string()).width());
            owner.push(i);
        }
    }
    let render = |pad: &str, from: usize, to: usize| {
        let mut spans = Vec::new();
        if !pad.is_empty() {
            spans.push(Span::raw(pad.to_string()));
        }
        let mut i = from;
        while i < to {
            let mut j = i;
            while j < to && owner[j] == owner[i] {
                j += 1;
            }
            spans.push(Span::styled(
                chars[i..j].iter().collect::<String>(),
                segs[owner[i]].style,
            ));
            i = j;
        }
        Line::from(spans)
    };

    let mut lines = Vec::new();
    let (mut start, mut pad) = (0, "");
    while start < chars.len() {
        let avail = w.saturating_sub(Span::raw(pad).width()).max(1);
        let mut end = start;
        let mut used = 0;
        while end < chars.len() {
            let char_width = widths[end];
            if end > start && used + char_width > avail {
                break;
            }
            // A wide character cannot fit in the remaining cells on an
            // otherwise empty line, so keep it whole on its own line.
            if end == start && char_width > avail {
                end += 1;
                break;
            }
            used += char_width;
            end += 1;
        }
        if end == chars.len() {
            lines.push(render(pad, start, chars.len()));
            break;
        }
        // Break at the last space that fits; failing that, mid-word.
        let mut next = end;
        if let Some(brk) = (start + 1..end).rev().find(|&i| chars[i] == ' ') {
            (end, next) = (brk, brk + 1);
        }
        lines.push(render(pad, start, end));
        // A fold never starts a line with the spaces it broke on.
        while next < chars.len() && chars[next] == ' ' {
            next += 1;
        }
        start = next;
        pad = indent;
    }
    lines
}

/// tildify writes a path under the home directory with a ~.
pub fn tildify(p: &str) -> String {
    match crate::paths::home() {
        Ok(home) if p.starts_with(&format!("{home}/")) => format!("~{}", &p[home.len()..]),
        _ => p.to_string(),
    }
}

/// ago says how long ago a unix time was, in the one unit that reads best.
fn ago(t: i64) -> String {
    let d = (repo::now() - t).max(0);
    match d {
        d if d < 60 => "just now".into(),
        d if d < 3600 => format!("{}m ago", d / 60),
        d if d < 24 * 3600 => format!("{}h ago", d / 3600),
        d => format!("{}d ago", d / (24 * 3600)),
    }
}
