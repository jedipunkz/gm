//! Everything about locally cloned repositories: where the roots are, what
//! lives under them, and how a shorthand reference turns into a URL and a
//! directory.

mod git;
mod history;
mod pr;
mod remote;
mod url;

pub use git::*;
pub use history::*;
pub use pr::*;
pub use remote::*;
pub use url::*;

use crate::config::Config;
use crate::{Result, err, paths};

/// Repo is a locally cloned repository living under one of the roots.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Repo {
    pub root: String, // root directory it was found under
    pub rel: String,  // slash-separated path relative to root, e.g. "github.com/x-motemen/ghq"
}

impl Repo {
    pub fn path(&self) -> String {
        paths::join(&self.root, &self.rel)
    }
}

/// Tree is the set of roots gm keeps repositories in, resolved once and then
/// carried around: every command works against the same answer, and gm.toml
/// is read a single time per run.
#[derive(Debug, Clone, Default)]
pub struct Tree {
    pub roots: Vec<String>, // most preferred first
}

impl Tree {
    /// open resolves the roots:
    ///
    ///   $GM_ROOT > gm.toml root > git config gm.root >
    ///   $GHQ_ROOT > git config ghq.root > ~/ghq
    ///
    /// The ghq fallbacks make gm a drop-in replacement for an existing ghq
    /// tree.
    pub fn open(cfg: &Config) -> Result<Tree> {
        Tree::open_with(cfg, |k| std::env::var(k).ok())
    }

    /// open_with is open reading the environment through env, so a test can
    /// hand it one.
    pub fn open_with(cfg: &Config, env: impl Fn(&str) -> Option<String>) -> Result<Tree> {
        let set = |k: &str| env(k).filter(|v| !v.is_empty());
        let split = |v: String| {
            v.split(':')
                .filter(|root| !root.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        };

        let roots = if let Some(v) = set("GM_ROOT") {
            split(v)
        } else if let Some(r) = cfg.roots()? {
            r
        } else if let vs @ [_, ..] = git_config_all("gm.root").as_slice() {
            vs.to_vec()
        } else if let Some(v) = set("GHQ_ROOT") {
            split(v)
        } else if let vs @ [_, ..] = git_config_all("ghq.root").as_slice() {
            vs.to_vec()
        } else {
            vec!["~/ghq".to_string()]
        };
        Ok(Tree {
            roots: expand_all(&roots)?,
        })
    }

    /// primary is the root new repositories are cloned into.
    pub fn primary(&self) -> &str {
        &self.roots[0]
    }

    /// path_for is where a URL lands under the primary root.
    pub fn path_for(&self, rel: &str) -> String {
        paths::join(self.primary(), rel)
    }

    /// existing_path finds a repository at rel under the first matching root.
    pub fn existing_path(&self, rel: &str) -> Option<String> {
        self.roots
            .iter()
            .map(|root| paths::join(root, rel))
            .find(|path| is_repo(path))
    }

    /// list walks every root and returns the repositories found.
    pub fn list(&self) -> Vec<Repo> {
        let mut repos = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for root in &self.roots {
            for p in find_repos(root) {
                if let Some(rel) = p.strip_prefix(&under(root))
                    && seen.insert(p.clone())
                {
                    repos.push(Repo {
                        root: root.clone(),
                        rel: rel.to_string(),
                    });
                }
            }
        }
        repos
    }

    /// contains reports whether path already lives under one of the roots.
    pub fn contains(&self, path: &str) -> bool {
        self.roots
            .iter()
            .any(|root| path == root || path.starts_with(&under(root)))
    }

    /// at names the repository whose directory is exactly path, which is what
    /// the finder hands back: it picked a row, so there is nothing to resolve.
    pub fn at(&self, path: &str) -> Option<Repo> {
        let path = paths::clean(path);
        self.roots.iter().find_map(|root| {
            let root = paths::clean(root);
            let rel = path.strip_prefix(&under(&root))?;
            Some(Repo {
                root,
                rel: rel.to_string(),
            })
        })
    }

    /// resolve finds the one repository a query names, erroring on ambiguity so
    /// a wrong repository is never removed or moved.
    pub fn resolve(&self, query: &str) -> Result<Repo> {
        let repos = self.list();
        let exact: Vec<&Repo> = repos
            .iter()
            .filter(|r| matches(&r.rel, query, true))
            .collect();
        let hits = if exact.is_empty() {
            repos
                .iter()
                .filter(|r| matches(&r.rel, query, false))
                .collect()
        } else {
            exact
        };
        match hits.as_slice() {
            [] => Err(err!("no repository matches {query:?}")),
            [one] => Ok((*one).clone()),
            many => {
                let mut msg = format!("{query:?} matches {} repositories:", many.len());
                for r in many {
                    msg.push_str(&format!("\n  {}", r.rel));
                }
                Err(msg.into())
            }
        }
    }

    /// create makes an empty repository where reference says it belongs, with
    /// its origin already set so the first push needs no arguments.
    pub fn create(&self, reference: &str, ssh: bool) -> Result<Repo> {
        let u = normalize_url(reference, ssh)?;
        let rel = rel_path_of(&u);
        let dst = self.path_for(&rel);

        if std::fs::read_dir(&dst).is_ok_and(|mut d| d.next().is_some()) {
            return Err(err!("{dst} already exists and is not empty"));
        }
        std::fs::create_dir_all(&dst)?;
        // Quietly: this runs under the finder as well as from the command line.
        git_quiet(&["-C", &dst, "init", "--quiet"])?;
        git_quiet(&["-C", &dst, "remote", "add", "origin", &u.to_string()])?;
        Ok(Repo {
            root: self.primary().to_string(),
            rel,
        })
    }
}

/// under is the prefix every path inside dir starts with.
fn under(dir: &str) -> String {
    format!("{}/", dir.trim_end_matches('/'))
}

fn expand_all(roots: &[String]) -> Result<Vec<String>> {
    let home = paths::home()?;
    let mut out: Vec<String> = Vec::new();
    for p in roots {
        if p.is_empty() {
            return Err("repository root is empty".into());
        }
        let p = match p.strip_prefix('~') {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => paths::join(&home, rest),
            _ => paths::clean(p),
        };
        if !p.starts_with('/') {
            return Err(err!("repository root must be absolute: {p:?}"));
        }
        if !out.contains(&p) {
            out.push(p);
        }
    }
    if out.is_empty() {
        return Err("no repository root configured".into());
    }
    Ok(out)
}

const VCS_DIRS: [&str; 3] = [".git", ".hg", ".svn"];

/// is_repo reports whether dir is the top of a working copy.
pub fn is_repo(dir: &str) -> bool {
    // .git is a file, not a directory, inside a worktree or submodule.
    VCS_DIRS
        .iter()
        .any(|d| std::fs::symlink_metadata(paths::join(dir, d)).is_ok())
}

/// find_repos walks dir and returns the top directory of every working copy
/// under it, never descending into one and never into a dotted directory. A
/// directory that does not exist yields nothing rather than an error: a root gm
/// has not cloned into yet is normal, and an unreadable entry is skipped.
pub fn find_repos(dir: &str) -> Vec<String> {
    let mut found = Vec::new();
    // The walk does not follow symlinks, the root included, as WalkDir did.
    if std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir()) {
        walk(&paths::clean(dir), true, &mut found);
    }
    found
}

fn walk(dir: &str, top: bool, found: &mut Vec<String>) {
    if !top && dir.rsplit('/').next().is_some_and(|n| n.starts_with('.')) {
        return;
    }
    if is_repo(dir) {
        found.push(dir.to_string());
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        // ponytail: a name that is not UTF-8 is carried lossily and then not
        // found again; convert paths to OsString throughout if that matters.
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for n in names {
        walk(&paths::join(dir, &n), false, found);
    }
}

/// matches reports whether a repository answers to query. exact requires the
/// query to equal the repository name, user/repo, or the whole host/user/repo
/// path.
pub fn matches(rel: &str, query: &str, exact: bool) -> bool {
    if query.is_empty() {
        return true;
    }
    if !exact {
        return rel.contains(query);
    }
    let parts: Vec<&str> = rel.split('/').collect();
    (0..parts.len()).any(|i| parts[i..].join("/") == query)
}

/// shortest_unique gives each repository the shortest trailing path that no
/// other repository in the set shares ("ghq", else "x-motemen/ghq", else the
/// full path).
pub fn shortest_unique(repos: &[Repo]) -> Vec<String> {
    let mut count = std::collections::HashMap::<String, usize>::new();
    for r in repos {
        let parts: Vec<&str> = r.rel.split('/').collect();
        for i in 0..parts.len() {
            *count.entry(parts[i..].join("/")).or_default() += 1;
        }
    }
    repos
        .iter()
        .map(|r| {
            let parts: Vec<&str> = r.rel.split('/').collect();
            (0..parts.len())
                .rev()
                .map(|j| parts[j..].join("/"))
                .find(|s| count[s] == 1)
                .unwrap_or_else(|| r.rel.clone())
        })
        .collect()
}

/// prune_empty_parents removes the host/user directories a deleted repository
/// leaves behind, stopping at the root.
pub fn prune_empty_parents(root: &str, mut dir: String) {
    while dir.starts_with(&under(root)) {
        if std::fs::remove_dir(&dir).is_err() {
            return;
        }
        dir = paths::dir(&dir);
    }
}

/// remove_worktree_and_prune takes a checkout away, then removes any empty
/// directories it leaves under the root's worktree directory.
pub fn remove_worktree_and_prune(r: &Repo, dir: &str, force: bool) -> Result<()> {
    remove_worktree(&r.path(), dir, force)?;
    prune_empty_parents(&paths::join(&r.root, WORKTREE_ROOT), paths::dir(dir));
    Ok(())
}

/// remove_all is os.RemoveAll: what is not there is already removed.
fn remove_all(p: &str) -> Result<()> {
    match std::fs::remove_dir_all(p) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(err!("remove {p}: {e}")),
        _ => Ok(()),
    }
}

/// delete removes a repository, every worktree checked out of it, and the
/// directories that are left empty above them. It asks nothing: the caller has
/// already confirmed.
///
/// The worktrees go first. Their administrative files live inside the
/// repository, so once it is gone git can no longer remove them and they are
/// left as directories whose .git points at nothing.
pub fn delete(r: &Repo) -> Result<()> {
    for w in other_worktrees(r) {
        if remove_worktree(&r.path(), &w.path, true).is_err() {
            // The repository is going anyway, so git's bookkeeping about this
            // checkout does not need to survive; the directory does not
            // either. This is the path a worktree git has lost track of
            // takes, and it must not block the removal.
            remove_all(&w.path)?;
        }
    }
    // The branch directories the checkouts hung under are gm's own, and
    // pruning them by the paths git reported would not work: git resolves
    // symlinks, so on macOS its /private/var is not the /var the tree was
    // walked as.
    let _ = remove_all(&worktrees_dir(r));
    prune_empty_parents(&r.root, paths::dir(&worktrees_dir(r)));

    remove_all(&r.path())?;
    prune_empty_parents(&r.root, paths::dir(&r.path()));
    Ok(())
}

/// other_worktrees are the checkouts of r that are not r itself. A repository
/// git cannot answer for has none, which is the right answer for a directory
/// that is about to be deleted.
pub fn other_worktrees(r: &Repo) -> Vec<Worktree> {
    other_worktrees_of(&r.path())
}

/// other_worktrees_of is the same thing for a path, which is all describe has.
pub fn other_worktrees_of(dir: &str) -> Vec<Worktree> {
    worktrees(dir)
        .unwrap_or_default()
        .into_iter()
        .filter(|w| !same_path(&w.path, dir)) // the main worktree is the repository
        .collect()
}

/// same_path reports whether two paths name the same directory. git prints
/// the resolved path, which on macOS is not the one gm walked to find it: /var
/// is a symlink to /private/var.
pub fn same_path(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ra), Ok(rb)) => ra == rb,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{TempDir, exists, git_repo, mkdir, write};

    #[test]
    fn matches_whole_segments_when_exact() {
        let rel = "github.com/x-motemen/ghq";
        for (query, exact, want) in [
            ("ghq", false, true),
            ("ghq", true, true),
            ("x-motemen/ghq", true, true),
            ("github.com/x-motemen/ghq", true, true),
            ("motemen/ghq", true, false), // exact means whole segments
            ("motemen", false, true),
            ("nope", false, false),
            ("", false, true),
        ] {
            assert_eq!(matches(rel, query, exact), want, "{query:?} exact={exact}");
        }
    }

    #[test]
    fn shortest_unique_names() {
        let r = |rel: &str| Repo {
            root: String::new(),
            rel: rel.into(),
        };
        assert_eq!(
            shortest_unique(&[
                r("github.com/a/ghq"),
                r("github.com/b/ghq"),
                r("github.com/a/gm")
            ]),
            vec!["a/ghq", "b/ghq", "gm"]
        );
    }

    #[test]
    fn open_resolves_roots_in_order() {
        let home = paths::home().unwrap();
        let cfg = |body: &str| crate::config::parse(body).unwrap();
        let none = |_: &str| None;

        // The config file, with ~ expanded.
        let tree = Tree::open_with(&cfg("root = \"~/code\""), none).unwrap();
        assert_eq!(tree.roots, vec![paths::join(&home, "code")]);

        // GM_ROOT still wins.
        let env = |k: &str| (k == "GM_ROOT").then(|| "/from-env".to_string());
        assert_eq!(
            Tree::open_with(&cfg("root = \"/from-file\""), env)
                .unwrap()
                .primary(),
            "/from-env"
        );

        // A broken config stops gm.
        assert!(Tree::open_with(&cfg("root = 42"), none).is_err());
    }

    #[test]
    fn roots_skip_empty_environment_entries_and_reject_invalid_config_roots() {
        let cfg = |body: &str| crate::config::parse(body).unwrap();
        for (key, roots) in [
            ("GM_ROOT", "/a:"),
            ("GM_ROOT", ":/a"),
            ("GHQ_ROOT", "/a:"),
            ("GHQ_ROOT", ":/a"),
        ] {
            let env = |k: &str| (k == key).then(|| roots.to_string());
            assert_eq!(
                Tree::open_with(&Config::default(), env).unwrap().roots,
                vec!["/a"]
            );
        }

        let empty = Tree::open_with(&cfg("root = \"\""), |_| None).unwrap_err();
        assert!(empty.0.contains("gm.toml"), "{empty}");
        let empty_entry = Tree::open_with(&cfg("root = [\"~/ghq\", \"\"]"), |_| None).unwrap_err();
        assert!(empty_entry.0.contains("gm.toml"), "{empty_entry}");
        assert!(Tree::open_with(&cfg("root = \"src\""), |_| None).is_err());
    }

    // A real directory tree: repositories are found, nested ones are not
    // descended into, and an ambiguous query is refused.
    #[test]
    fn list_and_resolve() {
        let root = TempDir::new();
        for rel in [
            "github.com/acme/alpha",
            "github.com/acme/bravo",
            "github.com/other/alpha",
            "github.com/acme/alpha/vendor/nested", // inside a repository: skipped
        ] {
            mkdir(&root.join(&format!("{rel}/.git")));
        }
        let tree = Tree {
            roots: vec![root.path()],
        };
        assert_eq!(tree.list().len(), 3, "{:?}", tree.list());
        assert_eq!(tree.resolve("bravo").unwrap().rel, "github.com/acme/bravo");
        assert!(
            tree.resolve("alpha").is_err(),
            "an ambiguous query must be refused"
        );
        assert!(tree.resolve("nothing").is_err());
    }

    // The walk gm migrate -r relies on: every working copy is found once,
    // nothing inside one is descended into, dotted directories are left alone,
    // and a missing directory is not an error.
    #[test]
    fn find_repos_and_contains() {
        let dir = TempDir::new();
        for rel in [
            "projects/alpha",
            "projects/nested/bravo",
            "projects/alpha/vendor/inner", // inside a repository: not reported
            ".cache/charlie",              // dotted: not descended into
        ] {
            mkdir(&dir.join(&format!("{rel}/.git")));
        }
        assert_eq!(
            find_repos(&dir.path()),
            vec![
                dir.join("projects/alpha"),
                dir.join("projects/nested/bravo")
            ]
        );
        assert!(find_repos(&dir.join("nope")).is_empty());

        let tree = Tree {
            roots: vec![dir.join("projects")],
        };
        for (path, want) in [
            (dir.join("projects"), true),
            (dir.join("projects/alpha"), true),
            (dir.join(".cache/charlie"), false),
            (dir.join("projects-other"), false), // a prefix, not a parent
        ] {
            assert_eq!(tree.contains(&path), want, "{path}");
        }
    }

    #[test]
    fn tree_at() {
        let root = TempDir::new();
        let tree = Tree {
            roots: vec![root.path()],
        };
        let r = tree.at(&root.join("github.com/acme/alpha")).unwrap();
        assert_eq!(
            (r.rel.as_str(), r.root.as_str()),
            ("github.com/acme/alpha", root.path().as_str())
        );
        assert_eq!(
            tree.at(&paths::join(&paths::dir(&root.path()), "elsewhere")),
            None
        );
        assert_eq!(
            tree.at(&root.path()),
            None,
            "the root itself is not a repository in it"
        );
    }

    // The bug where a removed repository left its checkouts behind, pointing
    // at a .git that was gone.
    #[test]
    fn delete_takes_the_worktrees() {
        let base = TempDir::new();
        let r = Repo {
            root: base.path(),
            rel: "github.com/acme/alpha".into(),
        };
        git_repo(&r.path());
        let tree = Tree {
            roots: vec![base.path()],
        };
        let login = tree.worktree_dir(&r, "feat/login");
        let timeout = tree.worktree_dir(&r, "fix/timeout");
        add_worktree(&r.path(), &login, "feat/login").unwrap();
        add_worktree(&r.path(), &timeout, "fix/timeout").unwrap();
        // One of them has work in it: the repository is going regardless, so
        // this must not stop the removal half way.
        write(&paths::join(&login, "scratch.txt"), "wip\n");
        assert_eq!(other_worktrees(&r).len(), 2);

        delete(&r).unwrap();
        for p in [
            r.path(),
            login,
            timeout,
            base.join(WORKTREE_ROOT),
            base.join("github.com"),
        ] {
            assert!(!exists(&p), "{p} survived");
        }
    }

    // A checkout git can no longer remove must not block the removal, or the
    // repository can never be deleted.
    #[test]
    fn delete_with_a_worktree_git_lost_track_of() {
        let base = TempDir::new();
        let r = Repo {
            root: base.path(),
            rel: "github.com/acme/alpha".into(),
        };
        git_repo(&r.path());
        let dir = Tree {
            roots: vec![base.path()],
        }
        .worktree_dir(&r, "feat/login");
        add_worktree(&r.path(), &dir, "feat/login").unwrap();
        // Break git's link to it, the way deleting .git by hand would.
        std::fs::remove_file(paths::join(&dir, ".git")).unwrap();

        delete(&r).unwrap();
        assert!(!exists(&r.path()) && !exists(&dir));
    }

    #[test]
    fn delete_without_worktrees() {
        let base = TempDir::new();
        let r = Repo {
            root: base.path(),
            rel: "github.com/acme/alpha".into(),
        };
        git_repo(&r.path());
        assert!(other_worktrees(&r).is_empty());
        delete(&r).unwrap();
        assert!(!exists(&r.path()));
    }

    // The field the details pane draws. The main worktree is the repository
    // itself and must not be in it, which on macOS means comparing
    // /private/var with /var.
    #[test]
    fn describe_reports_the_other_worktrees() {
        let base = TempDir::new();
        let r = Repo {
            root: base.path(),
            rel: "github.com/acme/alpha".into(),
        };
        git_repo(&r.path());
        assert!(describe(&r.path()).worktrees.is_empty());

        let tree = Tree {
            roots: vec![base.path()],
        };
        add_worktree(
            &r.path(),
            &tree.worktree_dir(&r, "feat/login"),
            "feat/login",
        )
        .unwrap();
        let got = describe(&r.path()).worktrees;
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].label(), "feat/login");
        // The date is what ranks the checkouts in the details pane, so an
        // undated worktree would silently sort to the bottom.
        assert!(got[0].committed_at > 0);
    }
}
