mod deadline;
mod gh;
mod git;
mod history;
mod remote;
mod removal;
mod url;
mod worktree;

pub use gh::{
    Comment, FORK_DIR, Issue, PullRequest, check_out_pull_request, check_out_pull_request_in,
    issue, issues, parse_issues, parse_pull_requests, pull_request, pull_requests,
};
pub(crate) use gh::{ISSUE_LIMIT, PR_LIMIT};
#[cfg(test)]
pub use git::git_spawns_reset;
pub use git::{
    Branch, Commit, Ref, RefKind, State, Status, branch_exists, branches, changed_files, describe,
    git, git_config_all, git_in, git_message, git_quiet, is_dirty, origin_url, parse_branches,
    parse_commits, parse_status, remotes, status_map, status_of, status_with_worktrees,
    valid_branch,
};
pub(crate) use git::{exit_error, git_command};
pub use history::{History, Visit, history_file, now};
pub use remote::{fetch_branch, parse_ls_remote, remote_branches};
pub use removal::{
    ExpiryPlan, RemovalPlan, WorktreeImpact, WorktreeRemovalPlan, expiry_plan, removal_plan,
    worktree_removal_plan,
};
pub use url::{Url, browse_url, normalize_url, rel_path_of};
pub use worktree::{
    Expiry, WORKTREE_ROOT, Worktree, add_worktree, add_worktree_from, expire, expired,
    live_worktrees_of, other_live_worktrees_of, other_worktrees, other_worktrees_of, parse_days,
    parse_worktrees, registered_worktrees, remove_worktree, remove_worktree_and_branch,
    repair_worktrees, strays, worktree_labels, worktree_labels_in, worktrees, worktrees_dir,
};

use crate::config::Config;
use crate::{Result, err, paths};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Repo {
    pub root: String,
    pub rel: String, // slash-separated path relative to root, e.g. "github.com/x-motemen/ghq"
}

impl Repo {
    pub fn path(&self) -> String {
        paths::join(&self.root, &self.rel)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Tree {
    pub roots: Vec<String>, // most preferred first
}

impl Tree {
    /// open resolves the roots:
    ///
    ///   $GM_ROOT > gm.toml root > git config gm.root >
    ///   $GHQ_ROOT > git config ghq.root > ~/ghq
    pub fn open(cfg: &Config) -> Result<Tree> {
        Tree::open_with(cfg, |k| std::env::var(k).ok())
    }

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

    pub fn primary(&self) -> &str {
        &self.roots[0]
    }

    pub fn path_for(&self, rel: &str) -> String {
        paths::join(self.primary(), rel)
    }

    /// existing_paths finds every root a repository at rel already lives
    /// under, most preferred first.
    pub fn existing_paths(&self, rel: &str) -> Vec<String> {
        self.roots
            .iter()
            .map(|root| paths::join(root, rel))
            .filter(|path| is_repo(path))
            .collect()
    }

    pub fn refuse_existing(&self, rel: &str) -> Result<()> {
        let existing = self.existing_paths(rel);
        if existing.is_empty() {
            return Ok(());
        }
        Err(err!("{rel} already exists: {}", existing.join(", ")))
    }

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

    pub fn contains(&self, path: &str) -> bool {
        self.roots
            .iter()
            .any(|root| path == root || path.starts_with(&under(root)))
    }

    /// at names the repository whose directory is exactly path.
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
        let query_path = paths::clean(query);
        let exact: Vec<&Repo> = repos
            .iter()
            .filter(|r| matches(&r.rel, query, true) || r.path() == query_path)
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
                    msg.push_str(&format!("\n  {}", r.path()));
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

        self.refuse_existing(&rel)?;
        if std::fs::read_dir(&dst).is_ok_and(|mut d| d.next().is_some()) {
            return Err(err!("{dst} already exists and is not empty"));
        }
        std::fs::create_dir_all(&dst)?;
        if let Err(e) = git_quiet(&["-C", &dst, "init", "--quiet"]) {
            prune_empty_parents(self.primary(), paths::dir(&dst));
            return Err(e);
        }
        if let Err(e) = git_quiet(&["-C", &dst, "remote", "add", "origin", &u.to_string()]) {
            remove_all(&dst)?;
            prune_empty_parents(self.primary(), paths::dir(&dst));
            return Err(e);
        }
        Ok(Repo {
            root: self.primary().to_string(),
            rel,
        })
    }
}

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
    if std::fs::metadata(dir).is_ok_and(|m| m.is_dir()) {
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
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    for n in names {
        let child = if dir.ends_with('/') {
            format!("{dir}{n}")
        } else {
            format!("{dir}/{n}")
        };
        walk(&child, false, found);
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
        if !query.chars().any(char::is_uppercase) {
            return rel.to_lowercase().contains(&query.to_lowercase());
        }
        return rel.contains(query);
    }
    let parts: Vec<&str> = rel.split('/').collect();
    (0..parts.len()).any(|i| parts[i..].join("/") == query)
}

/// shortest_unique gives each repository the shortest trailing path that no
/// other repository in the set shares ("ghq", else "x-motemen/ghq", else the
/// full path).
pub fn shortest_unique(repos: &[Repo]) -> Vec<String> {
    let mut rel_count = std::collections::HashMap::<String, usize>::new();
    let mut count = std::collections::HashMap::<String, usize>::new();
    for r in repos {
        *rel_count.entry(r.rel.clone()).or_default() += 1;
        let starts: Vec<usize> = std::iter::once(0)
            .chain(r.rel.match_indices('/').map(|(i, _)| i + 1))
            .collect();
        for s in starts {
            if let Some(c) = count.get_mut(&r.rel[s..]) {
                *c += 1;
            } else {
                count.insert(r.rel[s..].to_string(), 1);
            }
        }
    }
    repos
        .iter()
        .map(|r| {
            if rel_count[&r.rel] > 1 {
                return r.path();
            }
            let starts: Vec<usize> = std::iter::once(0)
                .chain(r.rel.match_indices('/').map(|(i, _)| i + 1))
                .collect();
            starts
                .iter()
                .map(|&s| &r.rel[s..])
                .rev()
                .find(|&suffix| count[suffix] == 1)
                .map(str::to_string)
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

fn remove_all(p: &str) -> Result<()> {
    match std::fs::remove_dir_all(p) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(err!("remove {p}: {e}")),
        _ => Ok(()),
    }
}

/// delete removes a repository, every worktree checked out of it, and the
/// directories that are left empty above them. It asks nothing: the caller has
/// already confirmed. It answers with the checkouts it has left where they
/// were, because the repository's own files named a directory outside gm's
/// worktree directory and git refused to remove them.
///
/// The worktrees go first. Their administrative files live inside the
/// repository, so once it is gone git can no longer remove them and they are
/// left as directories whose .git points at nothing.
pub fn delete(r: &Repo) -> Result<Vec<String>> {
    let mut left = vec![];
    for w in other_worktrees(r) {
        if remove_worktree(&r.path(), &w.path, true).is_err() {
            if is_under(&w.path, &worktrees_dir(r)) {
                remove_all(&w.path)?;
            } else {
                left.push(w.path.clone());
            }
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
    Ok(left)
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

pub(crate) fn is_under(p: &str, dir: &str) -> bool {
    match (std::fs::canonicalize(p), std::fs::canonicalize(dir)) {
        (Ok(rp), Ok(rd)) => rp != rd && rp.starts_with(&rd),
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
            ("motemen/ghq", true, false),
            ("motemen", false, true),
            ("nope", false, false),
            ("", false, true),
        ] {
            assert_eq!(matches(rel, query, exact), want, "{query:?} exact={exact}");
        }
    }

    #[test]
    fn matches_lowercase_queries_without_case() {
        for (rel, query, want) in [
            ("github.com/acme/MyRepo", "myrepo", true),
            ("github.com/acme/myrepo", "myrepo", true),
            ("github.com/acme/MyRepo", "acme/myrepo", true),
        ] {
            assert_eq!(matches(rel, query, false), want, "{query:?} in {rel:?}");
        }
    }

    #[test]
    fn matches_query_case_as_written_when_it_carries_an_uppercase() {
        for (rel, query, want) in [
            ("github.com/acme/MyRepo", "MyRepo", true),
            ("github.com/acme/MyRepo", "MYREPO", false),
            ("github.com/acme/myrepo", "MyRepo", false),
        ] {
            assert_eq!(matches(rel, query, false), want, "{query:?} in {rel:?}");
        }
    }

    #[test]
    fn matches_exact_queries_by_case() {
        for (rel, query, want) in [
            ("github.com/acme/MyRepo", "myrepo", false),
            ("github.com/acme/MyRepo", "MyRepo", true),
            ("github.com/acme/myrepo", "myrepo", true),
        ] {
            assert_eq!(matches(rel, query, true), want, "{query:?} in {rel:?}");
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
    fn shortest_unique_uses_paths_for_duplicate_rels() {
        let repo = |root: &str, rel: &str| Repo {
            root: root.into(),
            rel: rel.into(),
        };
        assert_eq!(
            shortest_unique(&[
                repo("/r1", "github.com/acme/alpha"),
                repo("/r2", "github.com/acme/alpha"),
                repo("/r1", "github.com/acme/bravo"),
            ]),
            vec![
                "/r1/github.com/acme/alpha",
                "/r2/github.com/acme/alpha",
                "bravo"
            ]
        );
    }

    #[test]
    fn open_resolves_roots_in_order() {
        let home = paths::home().unwrap();
        let cfg = |body: &str| crate::config::parse(body).unwrap();
        let none = |_: &str| None;

        let tree = Tree::open_with(&cfg("root = \"~/code\""), none).unwrap();
        assert_eq!(tree.roots, vec![paths::join(&home, "code")]);

        let env = |k: &str| (k == "GM_ROOT").then(|| "/from-env".to_string());
        assert_eq!(
            Tree::open_with(&cfg("root = \"/from-file\""), env)
                .unwrap()
                .primary(),
            "/from-env"
        );

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

    #[test]
    fn list_and_resolve() {
        let root = TempDir::new();
        for rel in [
            "github.com/acme/alpha",
            "github.com/acme/bravo",
            "github.com/other/alpha",
            "github.com/acme/alpha/vendor/nested",
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

    #[test]
    fn resolve_accepts_full_paths_and_reports_them_on_ambiguity() {
        let root = TempDir::new();
        let r1 = root.join("r1");
        let r2 = root.join("r2");
        for dir in [&r1, &r2] {
            mkdir(&format!("{dir}/github.com/acme/alpha/.git"));
        }
        let tree = Tree {
            roots: vec![r1.clone(), r2.clone()],
        };
        let err = tree.resolve("alpha").unwrap_err();
        assert!(err.0.contains(&format!("\n  {r1}/github.com/acme/alpha")));
        assert!(err.0.contains(&format!("\n  {r2}/github.com/acme/alpha")));

        assert_eq!(
            tree.resolve(&format!("{r1}/github.com/acme/alpha"))
                .unwrap()
                .path(),
            format!("{r1}/github.com/acme/alpha")
        );
    }

    #[test]
    fn find_repos_and_contains() {
        let dir = TempDir::new();
        for rel in [
            "projects/alpha",
            "projects/nested/bravo",
            "projects/alpha/vendor/inner",
            ".cache/charlie",
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
            (dir.join("projects-other"), false),
        ] {
            assert_eq!(tree.contains(&path), want, "{path}");
        }
    }

    #[test]
    fn find_repos_follows_a_symlinked_root() {
        let dir = TempDir::new();
        mkdir(&dir.join("real/github.com/acme/alpha/.git"));
        std::os::unix::fs::symlink(dir.join("real"), dir.join("link")).unwrap();
        assert_eq!(
            find_repos(&dir.join("link")),
            vec![dir.join("link/github.com/acme/alpha")]
        );

        std::os::unix::fs::symlink(
            dir.join("real/github.com/acme"),
            dir.join("real/github.com/elsewhere"),
        )
        .unwrap();
        assert_eq!(
            find_repos(&dir.join("real")),
            vec![dir.join("real/github.com/acme/alpha")]
        );
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
        add_worktree(&base.path(), &r.path(), &login, "feat/login").unwrap();
        add_worktree(&base.path(), &r.path(), &timeout, "fix/timeout").unwrap();
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
        add_worktree(&base.path(), &r.path(), &dir, "feat/login").unwrap();
        std::fs::remove_file(paths::join(&dir, ".git")).unwrap();

        delete(&r).unwrap();
        assert!(!exists(&r.path()) && !exists(&dir));
    }

    #[test]
    fn delete_leaves_a_checkout_outside_the_worktree_dir() {
        let base = TempDir::new();
        let other = TempDir::new();
        let r = Repo {
            root: base.path(),
            rel: "github.com/acme/alpha".into(),
        };
        git_repo(&r.path());
        let tree = Tree {
            roots: vec![base.path()],
        };
        let login = tree.worktree_dir(&r, "feat/login");
        add_worktree(&base.path(), &r.path(), &login, "feat/login").unwrap();
        let victim = paths::join(&other.path(), "victim");
        mkdir(&victim);
        write(&paths::join(&victim, "keep.txt"), "keep\n");
        retarget_worktree(&r.path(), &victim);
        assert_eq!(
            other_worktrees(&r).first().unwrap().path,
            victim,
            "git now reports the victim checkout"
        );

        let left = delete(&r).unwrap();
        assert!(!exists(&r.path()), "the repository went");
        assert!(exists(&victim), "an outside checkout survives delete");
        assert!(exists(&paths::join(&victim, "keep.txt")));
        assert_eq!(
            left,
            vec![victim],
            "the left checkout is named for the note"
        );
    }

    #[test]
    fn delete_leaves_a_symlink_that_resolves_outwards() {
        let base = TempDir::new();
        let other = TempDir::new();
        let r = Repo {
            root: base.path(),
            rel: "github.com/acme/alpha".into(),
        };
        git_repo(&r.path());
        let tree = Tree {
            roots: vec![base.path()],
        };
        let login = tree.worktree_dir(&r, "feat/login");
        add_worktree(&base.path(), &r.path(), &login, "feat/login").unwrap();
        let victim = paths::join(&other.path(), "victim");
        mkdir(&victim);
        write(&paths::join(&victim, "keep.txt"), "keep\n");
        let evil = paths::join(&tree.worktree_dir(&r, "feat"), "evil");
        mkdir(&paths::dir(&evil));
        std::os::unix::fs::symlink(&victim, &evil).unwrap();
        retarget_worktree(&r.path(), &evil);

        let left = delete(&r).unwrap();
        assert!(!exists(&r.path()), "the repository went");
        assert!(exists(&victim) && exists(&paths::join(&victim, "keep.txt")));
        assert_eq!(left, vec![evil]);
    }

    fn retarget_worktree(repo_dir: &str, dir: &str) {
        let admin = paths::join(repo_dir, ".git/worktrees");
        let mut names: Vec<String> = std::fs::read_dir(&admin)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 1, "one entry to retarget");
        names.sort();
        let entry = paths::join(&admin, &names.remove(0));
        write(&paths::join(&entry, "gitdir"), &format!("{dir}/.git\n"));
    }

    #[test]
    fn create_refuses_a_repository_under_another_root() {
        let base = TempDir::new();
        let (r1, r2) = (base.join("r1"), base.join("r2"));
        let there = paths::join(&r2, "github.com/acme/bravo");
        git_repo(&there);
        let tree = Tree {
            roots: vec![r1.clone(), r2],
        };
        let e = tree.create("acme/bravo", false).unwrap_err();
        assert!(e.0.contains(&there), "{}", e.0);
        assert!(!exists(&paths::join(&r1, "github.com")));
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
            &base.path(),
            &r.path(),
            &tree.worktree_dir(&r, "feat/login"),
            "feat/login",
        )
        .unwrap();
        let got = describe(&r.path()).worktrees;
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].label(), "feat/login");
        assert!(got[0].committed_at > 0);
    }
}
