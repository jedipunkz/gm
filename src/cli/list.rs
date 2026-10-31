//! The commands that only read the tree: list and status answer what is
//! where, root says where the tree is, and shell prints the key binding that
//! opens the finder.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

use ratatui::text::Span;

use crate::config;
use crate::repo::{self, Repo, State};
use crate::{Result, err, paths, plural};

use super::{App, LIST_FLAGS, ROOT_FLAGS, STATUS_FLAGS, missing_usage, parse};

impl App<'_> {
    pub(super) fn list(&mut self, args: &[String]) -> Result<()> {
        let p = parse(self, "gm list", LIST_FLAGS, args)?;
        let query = p.args.join(" ");

        let repos = self.tree.list();
        let mut hits: Vec<&Repo> = repos
            .iter()
            .filter(|r| repo::matches(&r.rel, &query, p.on("e")))
            .collect();
        hits.sort_by(|a, b| a.rel.cmp(&b.rel));

        // Uniqueness is a property of the whole tree, not of the query: a
        // name that only the hits agree on would still be ambiguous to gm rm.
        let short: HashMap<String, String> = if p.on("unique") {
            repos
                .iter()
                .map(|r| r.path())
                .zip(repo::shortest_unique(&repos))
                .collect()
        } else {
            HashMap::new()
        };
        let mut out = std::io::BufWriter::new(&mut *self.out);
        for r in hits {
            if p.on("p") {
                writeln!(out, "{}", r.path())?;
            } else if p.on("unique") {
                writeln!(out, "{}", short[&r.path()])?;
            } else {
                writeln!(out, "{}", r.rel)?;
            }
        }
        // One check for the lot: a closed pipe or a full disk shows up here.
        out.flush()?;
        Ok(())
    }

    pub(super) fn root(&mut self, args: &[String]) -> Result<()> {
        let p = parse(self, "gm root", ROOT_FLAGS, args)?;
        let n = if p.on("all") {
            self.tree.roots.len()
        } else {
            1
        };
        for r in &self.tree.roots[..n] {
            writeln!(self.out, "{r}")?;
        }
        Ok(())
    }

    /// status answers "what did I leave unfinished?" across the whole tree. It
    /// never fetches: ahead and behind are counted against the refs already on
    /// disk, so it is fast and works offline, and behind is as stale as the
    /// last fetch was.
    pub(super) fn status(&mut self, args: &[String]) -> Result<()> {
        let p = parse(self, "gm status", STATUS_FLAGS, args)?;
        let (dirty_only, unpushed, all) = (p.on("dirty"), p.on("unpushed"), p.on("a"));

        let rows = self.everything(p.on("p"));
        let states = repo::status_map(
            &rows
                .iter()
                .map(|(_, path)| path.clone())
                .collect::<Vec<_>>(),
            repo::status_of,
        );

        // The name column is as wide as the widest name that survives the
        // filter, so a run that prints two rows is not padded for a hundred.
        let keep: Vec<&(String, String)> = rows
            .iter()
            .filter(|(_, path)| {
                let s = &states[path];
                !(dirty_only && s.dirty == 0
                    || unpushed && !s.has_unpushed()
                    || !all && !dirty_only && !unpushed && !s.unfinished())
            })
            .collect();
        let width = keep
            .iter()
            .map(|(name, _)| Span::raw(name.as_str()).width())
            .max()
            .unwrap_or(0);
        for (name, path) in keep {
            let pad = " ".repeat(width.saturating_sub(Span::raw(name.as_str()).width()));
            writeln!(self.out, "{name}{pad}  {}", summarize(&states[path]))?;
        }
        Ok(())
    }

    /// everything lists repositories and the worktrees Git registers for them,
    /// as (name, path). The main checkout keeps the root's spelling; linked
    /// worktrees use Git's path, resolved against the configured roots for a
    /// short name.
    fn everything(&self, full: bool) -> Vec<(String, String)> {
        let mut rows: Vec<(String, String)> = Vec::new();
        let mut seen = std::collections::HashSet::<PathBuf>::new();
        let mut add = |name: String, path: String| {
            let key =
                std::fs::canonicalize(&path).unwrap_or_else(|_| PathBuf::from(paths::clean(&path)));
            if seen.insert(key) {
                rows.push((if full { path.clone() } else { name }, path));
            }
        };
        for r in self.tree.list() {
            let main = r.path();
            add(r.rel.clone(), main.clone());
            for w in repo::other_live_worktrees_of(&main) {
                let name = if full {
                    w.path.clone()
                } else {
                    self.worktree_name(&w.path)
                };
                add(name, w.path);
            }
        }
        rows.sort();
        rows
    }

    /// worktree_name is a root-relative name when Git's canonical path is
    /// inside one of the roots, and the full path when it is outside them.
    fn worktree_name(&self, path: &str) -> String {
        let path =
            std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(paths::clean(path)));
        self.tree
            .roots
            .iter()
            .filter_map(|root| {
                let root = std::fs::canonicalize(root)
                    .unwrap_or_else(|_| PathBuf::from(paths::clean(root)));
                path.strip_prefix(root)
                    .ok()
                    .filter(|rel| !rel.as_os_str().is_empty())
                    .map(|rel| rel.to_string_lossy().into_owned())
            })
            .next()
            .unwrap_or_else(|| path.to_string_lossy().into_owned())
    }

    pub(super) fn shell(&mut self, args: &[String]) -> Result<()> {
        let [sh] = args else {
            return Err(missing_usage("shell").into());
        };
        let tmpl = match sh.as_str() {
            "fish" => FISH,
            "zsh" => ZSH,
            "bash" => BASH,
            _ => return Err(err!("unsupported shell {sh:?} (fish, zsh, bash)")),
        };
        let k = config::parse_chord(&self.cfg.launch_key, DEFAULT_LAUNCH_KEY)?;
        // The snippets bind a control character, which is all a plain Ctrl
        // chord is. Printing a binding that cannot fire is worse than refusing.
        if !k.plain() {
            return Err(err!(
                "launch_key cannot be {}: use a plain Ctrl chord, such as ctrl-g",
                k.display
            ));
        }
        // Bound in the shell, these would take Enter or Tab over.
        if let Some((_, key)) = config::DISGUISED.iter().find(|(c, _)| *c == k.letter) {
            return Err(err!(
                "launch_key cannot be {}: a terminal sends it as {key}",
                k.display
            ));
        }
        write!(
            self.out,
            "{}",
            tmpl.replace("{{key}}", &k.letter.to_string())
                .replace("{{name}}", &k.display)
        )?;
        Ok(())
    }
}

/// summarize says what is unfinished in as few words as it takes.
fn summarize(s: &State) -> String {
    let mut parts = Vec::new();
    if s.dirty > 0 {
        parts.push(plural(s.dirty, "changed file", "changed files"));
    }
    // Unpushed is the count the default listing works from; naming it covers
    // every branch, so " ahead" is said only when there is no other count.
    if s.unpushed > 0 {
        parts.push(format!("{} unpushed", s.unpushed));
    } else if s.ahead > 0 {
        parts.push(format!("{} ahead", s.ahead));
    }
    if s.behind > 0 {
        parts.push(format!("{} behind", s.behind));
    }
    // Worth saying once the row is printed: those commits have nowhere to go.
    // It is not a reason to print one, or every local-only repository would.
    if !s.upstream {
        parts.push("no upstream".into());
    }
    if parts.is_empty() {
        return "clean".into();
    }
    parts.join(", ")
}

/// DEFAULT_LAUNCH_KEY is the shell key gm binds when gm.toml says nothing.
const DEFAULT_LAUNCH_KEY: &str = "ctrl-g";

// The bindings gm prints. {{key}} is the chord's letter and {{name}} its human
// spelling; each shell writes the Ctrl prefix its own way.
const FISH: &str = r#"# gm: {{name}} jumps to a repository. Add to ~/.config/fish/config.fish:
#   gm shell fish | source
function __gm_jump
    set -l dir (gm)
    if test -n "$dir"
        cd $dir
        commandline -f repaint
    end
end
bind \c{{key}} __gm_jump
if bind -M insert >/dev/null 2>&1
    bind -M insert \c{{key}} __gm_jump
end
"#;

const ZSH: &str = r#"# gm: {{name}} jumps to a repository. Add to ~/.zshrc:
#   eval "$(gm shell zsh)"
__gm_jump() {
  local dir
  dir=$(gm) || return
  [[ -n $dir ]] && cd -- "$dir"
  zle reset-prompt
}
zle -N __gm_jump
bindkey '^{{key}}' __gm_jump
"#;

const BASH: &str = r#"# gm: {{name}} jumps to a repository. Add to ~/.bashrc:
#   eval "$(gm shell bash)"
__gm_jump() {
  local dir
  dir=$(gm) || return
  [ -n "$dir" ] && cd -- "$dir"
}
bind -x '"\C-{{key}}": __gm_jump'
"#;
