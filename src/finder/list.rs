use std::collections::HashMap;

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use super::command::split_input;
use super::score::{find, match_score, substring_match};
use super::worktree::Mode;
use super::{Cmd, Model, Msg};
use crate::repo::{Branch, PullRequest, Visit};

/// Item is one row: a repository in the main list, a worktree, branch or pull
/// request in the others. label is what is drawn and matched, path is what
/// Enter yields.
#[derive(Debug, Clone, Default)]
pub struct Item {
    pub label: String,
    pub path: String,
    pub score: f64,  // frecency; zero outside the repository list
    pub seen: Visit, // the visit log; empty outside the repository list
    /// branch is the branch a row of the branch list stands for. Such a row
    /// has a path only when the branch is already checked out somewhere.
    pub branch: Branch,
    /// pr is the pull request a row of the pull request list stands for, with
    /// a path the same way.
    pub pr: PullRequest,
}

impl Model {
    pub(super) fn filter(&mut self) {
        self.matched.clear();
        self.note.clear();
        // A command is not a query: the list stays as the query before it left it.
        let value = self.input.value();
        let q = split_input(value.trim())
            .map_or(value.trim().to_string(), |(q, _)| q)
            .trim()
            .to_string();
        // The selection follows the item, not the row number, whenever the
        // query is not making a new ranking statement — a command being typed,
        // or the query being cleared. Clearing it has to hold the selection
        // still, or there is no way to find a repository and then act on it.
        let hold = !self.view_stale && (q == self.query || q.is_empty());
        let held = if hold {
            self.view.get(self.cursor).copied()
        } else {
            None
        };
        self.view_stale = false;

        let view: Vec<usize> = if q.is_empty() {
            (0..self.all.len()).collect()
        } else {
            let lower = q.to_lowercase();
            let labels: Vec<&str> = self.all.iter().map(|it| it.label.as_str()).collect();
            let mut scored: Vec<(usize, f64)> = Vec::new();
            for mt in find(&q, &labels) {
                let rel = labels[mt.index];
                // fuzzy finds candidates; where it landed the characters is its
                // own guess, and a literal hit beats that guess every time.
                let pos = substring_match(rel, &lower).unwrap_or(mt.matched);
                scored.push((mt.index, match_score(rel, &pos)));
                self.matched.insert(mt.index, char_indexes(rel, &pos));
            }
            // Ascending, so the strongest match lands at the bottom; frecency
            // breaks ties between equally good matches.
            scored.sort_by(|a, b| {
                a.1.total_cmp(&b.1)
                    .then(self.all[a.0].score.total_cmp(&self.all[b.0].score))
            });
            scored.into_iter().map(|(i, _)| i).collect()
        };
        self.query = q;
        self.view = self.keep_repo_filters(view);

        if let Some(i) = held.and_then(|h| self.view.iter().position(|&x| x == h)) {
            self.cursor = i;
            return;
        }
        self.cursor = self.view.len().saturating_sub(1);
    }

    /// keep_repo_filters applies status filters to the repository list only.
    /// While the shared scan is in flight, every row stays visible.
    fn keep_repo_filters(&self, view: Vec<usize>) -> Vec<usize> {
        if self.mode != Mode::Repos || !(self.dirty_only || self.unpushed_only) {
            return view;
        }
        let Some(states) = &self.repo_states else {
            return view;
        };
        view.into_iter()
            .filter(|&i| {
                states.get(&self.all[i].path).is_some_and(|state| {
                    (!self.dirty_only || state.dirty > 0)
                        && (!self.unpushed_only || state.unpushed > 0 || state.ahead > 0)
                })
            })
            .collect()
    }

    /// scan_repo_states asks git about every repository at once, off the UI
    /// thread. The result serves both the dirty and unpushed filters.
    pub(super) fn scan_repo_states(&self, busy_tag: u64) -> Cmd {
        let paths: Vec<String> = self.all.iter().map(|it| it.path.clone()).collect();
        let scan = self.state_scan_of.clone();
        Cmd::Task(Box::new(move || {
            Some(Msg::RepoStates(busy_tag, scan(&paths)))
        }))
    }

    /// render_row draws one row, highlighting the characters the query
    /// matched and, when selected, the whole line.
    pub(super) fn render_row(&self, i: usize, selected: bool, width: usize) -> Line<'static> {
        let idx = self.view[i];
        let it = &self.all[idx];
        let (base, hit, marker) = if selected {
            (
                self.st.row_sel,
                self.st.hit_sel,
                Span::styled("▸ ", self.st.marker),
            )
        } else {
            (self.st.row, self.st.hit, Span::raw("  "))
        };
        let empty = Vec::new();
        let mut spans = vec![marker];
        spans.extend(highlight(
            &it.label,
            self.matched.get(&idx).unwrap_or(&empty),
            width.saturating_sub(2),
            base,
            hit,
        ));
        let used: usize = spans.iter().map(Span::width).sum();
        if width > used {
            spans.push(Span::styled(" ".repeat(width - used), base));
        }
        Line::from(spans)
    }
}

/// char_indexes converts byte offsets into char positions, which is what
/// matching and rendering use to track highlights.
fn char_indexes(s: &str, byte_idx: &[usize]) -> Vec<usize> {
    s.char_indices()
        .enumerate()
        .filter(|(_, (b, _))| byte_idx.contains(b))
        .map(|(i, _)| i)
        .collect()
}

/// highlight renders s truncated to width display cells, with the chars at
/// hits in their own style. The tail is kept: the repository name matters more
/// than the host.
fn highlight(s: &str, hits: &[usize], width: usize, base: Style, hit: Style) -> Vec<Span<'static>> {
    if width <= 1 {
        return Vec::new();
    }
    let chars: Vec<char> = s.chars().collect();
    let mut spans = Vec::new();
    let mut off = 0;
    let text_width: usize = chars.iter().map(|c| Span::raw(c.to_string()).width()).sum();
    if text_width > width {
        let suffix_width = width - 1;
        let mut kept_width = 0;
        off = chars.len();
        while off > 0 {
            let char_width = Span::raw(chars[off - 1].to_string()).width();
            if kept_width + char_width > suffix_width {
                break;
            }
            kept_width += char_width;
            off -= 1;
        }
        spans.push(Span::styled("…", base));
    }
    let is_hit: HashMap<usize, bool> = hits
        .iter()
        .filter(|&&h| h >= off)
        .map(|&h| (h - off, true))
        .collect();
    // Runs of same-styled chars, so one span covers many cells.
    let mut i = 0;
    while i < chars.len() - off {
        let on = is_hit.contains_key(&i);
        let mut j = i;
        while j < chars.len() - off && is_hit.contains_key(&j) == on {
            j += 1;
        }
        spans.push(Span::styled(
            chars[off + i..off + j].iter().collect::<String>(),
            if on { hit } else { base },
        ));
        i = j;
    }
    spans
}
