use std::collections::HashMap;
use std::ffi::OsStr;
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{Repo, Tree};
use crate::{Error, Result, err, paths};

/// git_command makes a Git command with a predictable configuration in tests.
/// The environment belongs to this child only: tests run in parallel without
/// changing the process environment they share.
pub(crate) fn git_command() -> Command {
    git_command_with(OsStr::new("git"))
}

/// git_command_with is git_command for a caller that supplies the executable.
pub(crate) fn git_command_with(program: &OsStr) -> Command {
    let command = Command::new(program);
    #[cfg(test)]
    let command = {
        let mut command = command;
        isolate_git_config(&mut command);
        command
    };
    command
}

#[cfg(test)]
pub(super) fn isolate_git_config(command: &mut Command) {
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("GIT_CONFIG_") {
            command.env_remove(key);
        }
    }
    command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1");
}

/// exit_error words a process that ran and failed the way Go's ExitError did.
pub(crate) fn exit_error(out: &std::process::ExitStatus) -> Error {
    match out.code() {
        Some(c) => err!("exit status {c}"),
        None => "signal: killed".into(),
    }
}

/// git runs git with its output on the terminal, for the commands whose
/// progress the user wants to watch.
pub fn git(args: &[&str]) -> Result<()> {
    let status = git_command()
        .args(args)
        .stdout(Stdio::from(std::io::stderr()))
        .stderr(Stdio::from(std::io::stderr()))
        .status()?;
    if !status.success() {
        return Err(exit_error(&status));
    }
    Ok(())
}

/// git_quiet runs git without letting it near the terminal: the finder may own
/// the screen, and a stray "Preparing worktree" line would land on top of it.
/// Whatever git printed comes back in the error instead of being shown.
pub fn git_quiet(args: &[&str]) -> Result<()> {
    let out = git_command().args(args).stdin(Stdio::null()).output()?;
    quiet_result(out)
}

/// quiet_result turns a finished quiet command into the one line worth
/// reading when it failed.
pub(crate) fn quiet_result(out: Output) -> Result<()> {
    if out.status.success() {
        return Ok(());
    }
    let mut all = out.stdout;
    all.extend_from_slice(&out.stderr);
    match git_message(&all) {
        msg if !msg.is_empty() => Err(Error(msg)),
        _ => Err(exit_error(&out.status)),
    }
}

/// git_message picks the line worth repeating out of git's output: what it
/// complained about, or failing that the first thing it said.
pub fn git_message(out: &[u8]) -> String {
    let text = String::from_utf8_lossy(out);
    let mut first = "";
    for line in text.split('\n') {
        let line = line.trim();
        if line.is_empty() || line.starts_with("hint:") {
            continue;
        }
        if let Some(rest) = line
            .strip_prefix("fatal: ")
            .or_else(|| line.strip_prefix("error: "))
        {
            return rest.trim().to_string();
        }
        if first.is_empty() {
            first = line;
        }
    }
    first.to_string()
}

/// valid_branch reports whether git would take this as a branch name. gm asks
/// before it builds a path out of one: a name like "../.." would otherwise
/// make a directory outside the tree before git ever saw it.
pub fn valid_branch(name: &str) -> bool {
    if name.is_empty() || name.starts_with('-') || name.contains("..") {
        return false;
    }
    git_command()
        .args(["check-ref-format", "--branch", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// git_in runs git inside dir and returns its trimmed output.
pub fn git_in(dir: &str, args: &[&str]) -> Result<String> {
    let out = git_command()
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()?;
    if !out.status.success() {
        return Err(exit_error(&out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// git_out is git_in for a caller that takes whatever git printed, failed or
/// not: a repository with no commits still names its branch.
fn git_out(dir: &str, args: &[&str]) -> String {
    git_command()
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// git_config_all reads every value of a git config key, empty when unset.
pub fn git_config_all(key: &str) -> Vec<String> {
    let Ok(out) = git_command()
        .args(["config", "--path", "--get-all", key])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Status is what git says about a working copy. Every field is best-effort:
/// a repository git cannot read still has to be listed and jumped to.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub remote: String,
    pub branch: String,
    pub commits: Vec<Commit>,     // newest first
    pub dirty: usize,             // changed files
    pub worktrees: Vec<Worktree>, // the other checkouts, without the main one
}

/// RECENT_COMMITS is how many commits describe collects. Three answers "is
/// this the repository I mean?" without turning the details pane into a log;
/// the pane draws fewer when it is short of height.
const RECENT_COMMITS: &str = "3";

/// describe collects the status of one working copy. It still shells out, so
/// callers keep it off any hot path and off rows the cursor only passed over.
pub fn describe(dir: &str) -> Status {
    let (names, origin) = remotes(dir);
    let dirty = match git_in(dir, &["status", "--porcelain"]) {
        Ok(out) if !out.is_empty() => out.split('\n').count(),
        _ => 0,
    };
    Status {
        branch: git_out(dir, &["rev-parse", "--abbrev-ref", "HEAD"]),
        remote: origin,
        commits: commits(dir, &names),
        worktrees: super::other_worktrees_of(dir),
        dirty,
    }
}

/// Commit is one line of the log, split so the details pane can colour its
/// parts the way `git log --oneline --decorate` does.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Commit {
    pub hash: String,
    pub refs: Vec<Ref>, // the decorations, in the order git printed them
    pub subject: String,
}

/// RefKind says how a decoration reads. git gives each kind its own colour,
/// and so does the finder.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum RefKind {
    #[default]
    Local, // a branch in this repository
    Head,   // HEAD itself
    Remote, // a remote-tracking branch
    Tag,    // a tag
}

/// Ref is one decoration, named as git prints it ("main", "origin/main",
/// "tag: v1.0").
#[derive(Debug, Clone, PartialEq)]
pub struct Ref {
    pub name: String,
    pub kind: RefKind,
}

/// The fields of one log line are separated by a NUL, which cannot appear in
/// a hash, a ref name or a subject. %x00 is git's own escape for it, written
/// in the --format argument; a real NUL there could not be passed to exec.
const COMMIT_FORMAT: &str = "--format=%h%x00%D%x00%s";

/// commits reads the newest commits and their decorations. The remote names
/// come from the caller: without them "origin/main" and a local branch called
/// "release/main" look alike, both being a name with a slash in it.
fn commits(dir: &str, remotes: &[String]) -> Vec<Commit> {
    match git_in(dir, &["log", "-n", RECENT_COMMITS, COMMIT_FORMAT]) {
        Ok(out) => parse_commits(&out, remotes),
        Err(_) => Vec::new(),
    }
}

/// origin_url is origin's URL on its own, for the caller that wants nothing
/// else about the repository and should not pay for a whole describe.
pub fn origin_url(dir: &str) -> String {
    remotes(dir).1
}

/// remotes reads the configured remotes in one call: their names, and
/// origin's URL. Both halves come out of `git remote -v`, so asking git
/// separately for each of them is one process more than the answer costs.
pub fn remotes(dir: &str) -> (Vec<String>, String) {
    let out = match git_in(dir, &["remote", "-v"]) {
        Ok(out) if !out.is_empty() => out,
        _ => return (Vec::new(), String::new()),
    };
    // Each remote gets a fetch line and a push line: "origin<TAB>URL (fetch)".
    let (mut names, mut origin) = (Vec::<String>::new(), String::new());
    for l in out.split('\n') {
        let Some((name, rest)) = l.split_once('\t') else {
            continue;
        };
        if !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
        if name == "origin" && origin.is_empty() {
            origin = rest.split(' ').next().unwrap_or("").to_string();
        }
    }
    (names, origin)
}

/// parse_commits reads the lines commits asked git for. A line git could not
/// format is dropped rather than shown half-parsed.
pub fn parse_commits(out: &str, remotes: &[String]) -> Vec<Commit> {
    if out.is_empty() {
        return Vec::new();
    }
    out.split('\n')
        .filter_map(|line| {
            let f: Vec<&str> = line.splitn(3, '\0').collect();
            (f.len() == 3).then(|| Commit {
                hash: f[0].to_string(),
                refs: parse_refs(f[1], remotes),
                subject: f[2].to_string(),
            })
        })
        .collect()
}

/// parse_refs reads git's %D decoration list: comma-separated names, where the
/// checked-out branch appears as "HEAD -> name" and a tag as "tag: name".
fn parse_refs(d: &str, remotes: &[String]) -> Vec<Ref> {
    let mut refs = Vec::new();
    for part in d.trim().split(',') {
        let mut part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((head, branch)) = part.split_once(" -> ") {
            refs.push(Ref {
                name: head.trim().to_string(),
                kind: RefKind::Head,
            });
            part = branch.trim();
        }
        refs.push(Ref {
            name: part.to_string(),
            kind: ref_kind(part, remotes),
        });
    }
    refs
}

fn ref_kind(name: &str, remotes: &[String]) -> RefKind {
    if name == "HEAD" {
        return RefKind::Head;
    }
    if name.starts_with("tag: ") {
        return RefKind::Tag;
    }
    if remotes
        .iter()
        .any(|r| !r.is_empty() && name.starts_with(&format!("{r}/")))
    {
        return RefKind::Remote;
    }
    RefKind::Local
}

/// is_dirty reports whether the working copy has uncommitted changes.
pub fn is_dirty(dir: &str) -> Result<bool> {
    Ok(!git_in(dir, &["status", "--porcelain"])?.is_empty())
}

/// changed_files counts what a removal of dir would lose: the files git status
/// reports. It asks git now, never a cache, because it is only asked right
/// before something is deleted. A checkout git cannot answer for counts as
/// none.
pub fn changed_files(dir: &str) -> usize {
    status_of(dir).dirty
}

/// worktree_labels names checkouts for a warning, each with the work that
/// would go with it: "feat/login (3 changed), fix/timeout". changed is
/// changed_files, or what a test puts in its place.
pub fn worktree_labels(wts: &[Worktree], changed: impl Fn(&str) -> usize) -> String {
    wts.iter()
        .map(|w| match changed(&w.path) {
            0 => w.label(),
            n => format!("{} ({n} changed)", w.label()),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Worktree is one checkout of a repository: the main one, plus whatever
/// `git worktree add` created.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Worktree {
    pub path: String,
    pub branch: String, // short name, empty when HEAD is detached
    pub head: String,   // commit hash
    pub bare: bool,
    pub committed_at: i64, // unix seconds of the branch tip; 0 when unknown
}

impl Worktree {
    /// label names a worktree in one word: its branch, or a detached HEAD's
    /// hash.
    pub fn label(&self) -> String {
        if self.bare {
            "(bare)".into()
        } else if !self.branch.is_empty() {
            self.branch.clone()
        } else if self.head.len() >= 7 {
            self.head[..7].to_string()
        } else {
            "(detached)".into()
        }
    }
}

/// worktrees lists the checkouts of the repository at dir, the main one
/// first, which is the order git itself reports.
pub fn worktrees(dir: &str) -> Result<Vec<Worktree>> {
    let out = git_in(dir, &["worktree", "list", "--porcelain"])?;
    let mut list = parse_worktrees(&out);
    let dates = branch_dates(dir, &list);
    for w in &mut list {
        w.committed_at = dates.get(&w.branch).copied().unwrap_or(0);
    }
    Ok(list)
}

/// branch_dates is when each checkout's branch was last committed to. One
/// call for all of them, so dating the checkouts costs a process and not one
/// per checkout, and it asks only for the branches that are checked out: a
/// repository with thousands of branches is no more work than one with three.
/// A detached HEAD has no branch to ask about and stays undated.
fn branch_dates(dir: &str, list: &[Worktree]) -> HashMap<String, i64> {
    let refs: Vec<String> = list
        .iter()
        .filter(|w| !w.branch.is_empty())
        .map(|w| format!("refs/heads/{}", w.branch))
        .collect();
    // Without a pattern for-each-ref lists every ref there is, which is the
    // one call this must never make.
    if refs.is_empty() {
        return HashMap::new();
    }
    let mut args = vec![
        "for-each-ref",
        "--format=%(committerdate:unix) %(refname:short)",
    ];
    args.extend(refs.iter().map(String::as_str));
    let Ok(out) = git_in(dir, &args) else {
        return HashMap::new();
    };
    out.split('\n')
        .filter_map(|line| {
            let (ts, name) = line.trim().split_once(' ')?;
            Some((name.to_string(), ts.parse().ok()?))
        })
        .collect()
}

/// parse_worktrees reads `git worktree list --porcelain`: records separated by
/// a blank line, each a "worktree <path>" line followed by attributes.
pub fn parse_worktrees(out: &str) -> Vec<Worktree> {
    let mut list = Vec::new();
    let mut w = Worktree::default();
    for line in out.split('\n') {
        let line = line.trim_end_matches('\r');
        let (key, val) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "worktree" => {
                if !w.path.is_empty() {
                    list.push(std::mem::take(&mut w));
                }
                w = Worktree {
                    path: val.to_string(),
                    ..Default::default()
                };
            }
            "HEAD" => w.head = val.to_string(),
            "branch" => w.branch = val.strip_prefix("refs/heads/").unwrap_or(val).to_string(),
            "bare" => w.bare = true,
            _ => {}
        }
    }
    if !w.path.is_empty() {
        list.push(w);
    }
    list
}

/// State is what a repository holds that nothing else does: work not
/// committed, and commits not pushed. It comes out of one `git status
/// --porcelain -b`, which reads the refs already on disk and never fetches, so
/// the answer is as fresh as the last time something did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub branch: String,  // as git names it, "HEAD" when detached
    pub upstream: bool,  // the branch tracks something
    pub ahead: usize,    // commits here the upstream does not have
    pub behind: usize,   // and the other way round
    pub dirty: usize,    // changed files
    pub unpushed: usize, // commits on branches no remote has, 0 without remotes
}

impl State {
    /// unfinished reports whether this is work someone walked away from. Being
    /// behind is the remote's news rather than the user's, so it alone does
    /// not make a repository worth listing.
    pub fn unfinished(&self) -> bool {
        self.dirty > 0 || self.ahead > 0 || self.unpushed > 0
    }
}

/// STATUS_WORKERS bounds the Git processes in a bulk status scan. More
/// workers than this do not help once they compete for the same disk.
const STATUS_WORKERS: usize = 8;

/// status_of reads one repository's state.
pub fn status_of(dir: &str) -> State {
    let mut s = git_in(dir, &["status", "--porcelain", "-b"])
        .map(|out| parse_status(&out))
        .unwrap_or_default();
    // The commits on branches no remote has are unpushed work, as much as
    // being ahead of the upstream is — more, since they name every branch,
    // not just the checked-out one. Only a repository with remotes counts:
    // one without would list every commit it ever made.
    if !remotes(dir).0.is_empty() {
        s.unpushed = unpushed_commits(dir);
    }
    s
}

/// unpushed_commits counts the commits reachable from local branches and from
/// no remote-tracking ref. Never a fetch: the answer is as fresh as the last
/// fetch was.
fn unpushed_commits(dir: &str) -> usize {
    git_in(
        dir,
        &["rev-list", "--count", "--branches", "--not", "--remotes"],
    )
    .ok()
    .and_then(|n| n.trim().parse().ok())
    .unwrap_or(0)
}

/// status_map reads every path, a few at a time: git is the slow part and the
/// disk is shared, so more workers than this buys nothing.
pub fn status_map(paths: &[String]) -> HashMap<String, State> {
    let out = Mutex::new(HashMap::with_capacity(paths.len()));
    let next = AtomicUsize::new(0);
    std::thread::scope(|s| {
        for _ in 0..STATUS_WORKERS.min(paths.len()) {
            s.spawn(|| {
                while let Some(p) = paths.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let st = status_of(p);
                    out.lock().unwrap().insert(p.clone(), st);
                }
            });
        }
    });
    out.into_inner().unwrap()
}

/// parse_status reads `git status --porcelain -b`: a "## " header naming the
/// branch and how far it has drifted, then a line per changed file.
pub fn parse_status(out: &str) -> State {
    let mut s = State::default();
    for (i, l) in out.split('\n').enumerate() {
        if i == 0
            && let Some(header) = l.strip_prefix("## ")
        {
            s.parse_branch(header);
            continue;
        }
        if !l.is_empty() {
            s.dirty += 1;
        }
    }
    s
}

impl State {
    /// parse_branch reads the header, which git writes as one of:
    ///
    ///   main...origin/main [ahead 1, behind 2]
    ///   main...origin/main
    ///   main
    ///   HEAD (no branch)
    ///   No commits yet on main
    fn parse_branch(&mut self, mut l: &str) {
        if let (Some(i), true) = (l.rfind(" ["), l.ends_with(']')) {
            for part in l[i + 2..l.len() - 1].split(", ") {
                // "gone" has no number and means the upstream was deleted.
                let Some((kind, num)) = part.split_once(' ') else {
                    continue;
                };
                let Ok(n) = num.parse() else { continue };
                match kind {
                    "ahead" => self.ahead = n,
                    "behind" => self.behind = n,
                    _ => {}
                }
            }
            l = &l[..i];
        }
        let l = l.strip_prefix("No commits yet on ").unwrap_or(l);
        if let Some((b, up)) = l.split_once("...") {
            self.branch = b.to_string();
            self.upstream = !up.is_empty();
            return;
        }
        self.branch = l.strip_suffix(" (no branch)").unwrap_or(l).to_string();
    }
}

/// WORKTREE_ROOT is where gm keeps the checkouts it creates:
///
///   <root>/.worktrees/<host>/<user>/<repo>/<branch>
///
/// The dot matters. A worktree has a .git file, so is_repo answers yes for
/// one, and a worktree in the open would be listed as a repository and
/// refused by gm migrate. find_repos never descends into a dotted directory,
/// so this one stays out of the way.
pub const WORKTREE_ROOT: &str = ".worktrees";

/// worktrees_dir is where gm keeps every checkout of one repository.
pub fn worktrees_dir(r: &Repo) -> String {
    paths::join(&paths::join(&r.root, WORKTREE_ROOT), &r.rel)
}

impl Tree {
    /// worktree_dir is where a branch's checkout of r belongs.
    pub fn worktree_dir(&self, r: &Repo, branch: &str) -> String {
        paths::join(&worktrees_dir(r), branch)
    }
}

/// branch_exists reports whether the repository already has this branch,
/// which decides whether a worktree starts one or checks one out.
pub fn branch_exists(dir: &str, branch: &str) -> bool {
    git_command()
        .args([
            "-C",
            dir,
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// add_worktree checks branch out at dir, starting the branch from HEAD when
/// it does not exist yet.
pub fn add_worktree(worktree_root: &str, repo_dir: &str, dir: &str, branch: &str) -> Result<()> {
    add_worktree_from(worktree_root, repo_dir, dir, branch, "")
}

/// add_worktree_from is add_worktree for a branch that may live only on a
/// remote: a new branch starts at start, "origin/feature", and tracks it. An
/// empty start is HEAD.
pub fn add_worktree_from(
    worktree_root: &str,
    repo_dir: &str,
    dir: &str,
    branch: &str,
    start: &str,
) -> Result<()> {
    if !valid_branch(branch) {
        return Err(err!("{branch:?} is not a branch name"));
    }
    std::fs::create_dir_all(paths::dir(dir))?;
    let mut args = vec!["-C", repo_dir, "worktree", "add"];
    if branch_exists(repo_dir, branch) {
        args.extend([dir, branch]);
    } else if !start.is_empty() {
        args.extend(["--track", "-b", branch, dir, start]);
    } else {
        args.extend(["-b", branch, dir]);
    }
    match git_quiet(&args) {
        Ok(()) => Ok(()),
        Err(e) => {
            super::prune_empty_parents(worktree_root, paths::dir(dir));
            Err(e)
        }
    }
}

/// Branch is one branch a worktree can be made from. remote is set when only a
/// remote has it, "origin/feature", and is where the local branch will start.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Branch {
    pub name: String,
    pub remote: String,
    /// unfetched is a branch the remote has that the last fetch did not
    /// bring: it is known by name only, and has to be fetched to be used.
    pub unfetched: bool,
}

impl Branch {
    /// label is how the branch reads in a list: by where it lives when that
    /// is only a remote.
    pub fn label(&self) -> String {
        if self.remote.is_empty() {
            self.name.clone()
        } else {
            self.remote.clone()
        }
    }
}

/// branches lists the local branches, and the remote ones with no local branch
/// of the same name, newest commit first. The remote ones are what the last
/// fetch left: opening a list is not the moment to go to the network.
pub fn branches(dir: &str) -> Result<Vec<Branch>> {
    let (names, _) = remotes(dir);
    let out = git_in(
        dir,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes",
        ],
    )?;
    Ok(parse_branches(&out, &names))
}

/// parse_branches reads full ref names. The remote names come from the caller,
/// for the reason commits needs them: "origin/main" is a remote's branch only
/// because origin is a remote.
pub fn parse_branches(out: &str, remotes: &[String]) -> Vec<Branch> {
    let lines: Vec<&str> = out.split('\n').collect();
    let local: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.strip_prefix("refs/heads/"))
        .collect();
    let mut list = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for l in &lines {
        if let Some(name) = l.strip_prefix("refs/heads/") {
            list.push(Branch {
                name: name.to_string(),
                ..Default::default()
            });
            continue;
        }
        let Some(short) = l.strip_prefix("refs/remotes/") else {
            continue;
        };
        for r in remotes {
            let Some(name) = short.strip_prefix(&format!("{r}/")) else {
                continue;
            };
            // HEAD is a pointer at the remote's default branch, not a branch.
            if name == "HEAD" || local.contains(&name) || seen.iter().any(|s| s == name) {
                continue;
            }
            seen.push(name.to_string());
            list.push(Branch {
                name: name.to_string(),
                remote: short.to_string(),
                unfetched: false,
            });
        }
    }
    list
}

/// repair_worktrees links the checkouts at dirs back to the repository at
/// repo_dir after the repository has moved: each checkout's .git file holds
/// the repository's old path, and git answers "not a git repository" in it
/// until this runs.
pub fn repair_worktrees(repo_dir: &str, dirs: &[String]) -> Result<()> {
    if dirs.is_empty() {
        return Ok(());
    }
    let mut args = vec!["-C", repo_dir, "worktree", "repair"];
    args.extend(dirs.iter().map(String::as_str));
    git_quiet(&args)
}

/// remove_worktree takes a checkout away. force is what the caller has already
/// confirmed: git refuses on its own when there is work in it.
pub fn remove_worktree(repo_dir: &str, dir: &str, force: bool) -> Result<()> {
    let mut args = vec!["-C", repo_dir, "worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(dir);
    git_quiet(&args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{TempDir, git_repo, mkdir, write};

    #[test]
    fn parse_worktrees_reads_porcelain() {
        let out = "worktree /home/u/ghq/github.com/u/agx
HEAD 0123456789abcdef0123456789abcdef01234567
branch refs/heads/main

worktree /home/u/.worktrees/agx-login
HEAD 89abcdef0123456789abcdef0123456789abcdef
branch refs/heads/feat/login

worktree /home/u/.worktrees/agx-detached
HEAD fedcba9876543210fedcba9876543210fedcba98
detached
";
        let got = parse_worktrees(out);
        assert_eq!(got.len(), 3, "{got:?}");
        // The main worktree comes first, the way git reports it.
        assert_eq!(
            (got[0].path.as_str(), got[0].branch.as_str()),
            ("/home/u/ghq/github.com/u/agx", "main")
        );
        assert_eq!(
            (got[1].branch.as_str(), got[1].label()),
            ("feat/login", "feat/login".to_string())
        );
        assert_eq!(
            (got[2].branch.as_str(), got[2].label()),
            ("", "fedcba9".to_string())
        );
        assert!(parse_worktrees("").is_empty());
        assert_eq!(
            Worktree {
                path: "/x".into(),
                bare: true,
                ..Default::default()
            }
            .label(),
            "(bare)"
        );
    }

    #[test]
    fn parse_commits_reads_decorations() {
        let out = [
            "b1b7b91\0HEAD -> feat/migrate-scan, origin/feat/migrate-scan\0refactor: name the flag -r",
            "06f966e\0\0feat: add gm migrate -r",
            "d7f3a4c\0tag: v1.2.0, origin/main, main, release/main\0Merge pull request #24",
        ]
        .join("\n");
        let got = parse_commits(&out, &["origin".to_string()]);
        assert_eq!(got.len(), 3);
        assert_eq!(
            (got[0].hash.as_str(), got[0].subject.as_str()),
            ("b1b7b91", "refactor: name the flag -r")
        );
        // "HEAD -> branch" is two decorations, coloured differently.
        let r = |name: &str, kind| Ref {
            name: name.into(),
            kind,
        };
        assert_eq!(
            got[0].refs,
            vec![
                r("HEAD", RefKind::Head),
                r("feat/migrate-scan", RefKind::Local),
                r("origin/feat/migrate-scan", RefKind::Remote)
            ]
        );
        assert!(got[1].refs.is_empty());
        // A local branch whose name has a slash in it is not a remote one:
        // only the configured remotes make it remote.
        assert_eq!(
            got[2].refs,
            vec![
                r("tag: v1.2.0", RefKind::Tag),
                r("origin/main", RefKind::Remote),
                r("main", RefKind::Local),
                r("release/main", RefKind::Local)
            ]
        );
        assert!(parse_commits("", &[]).is_empty());
        // A line git could not format is dropped, not turned into a bad row.
        assert!(parse_commits("no separators here", &[]).is_empty());
    }

    #[test]
    fn parse_status_reads_every_header() {
        let st = |branch: &str, upstream, ahead, behind, dirty| State {
            branch: branch.into(),
            upstream,
            ahead,
            behind,
            dirty,
            unpushed: 0,
        };
        for (name, out, want) in [
            (
                "tracking and drifted",
                "## main...origin/main [ahead 1, behind 2]",
                st("main", true, 1, 2, 0),
            ),
            (
                "ahead only",
                "## main...origin/main [ahead 3]",
                st("main", true, 3, 0, 0),
            ),
            (
                "in sync",
                "## main...origin/main",
                st("main", true, 0, 0, 0),
            ),
            ("no upstream", "## main", st("main", false, 0, 0, 0)),
            (
                "detached",
                "## HEAD (no branch)",
                st("HEAD", false, 0, 0, 0),
            ),
            (
                "empty repository",
                "## No commits yet on main",
                st("main", false, 0, 0, 0),
            ),
            // The upstream branch was deleted: there is no count to read, and
            // the branch still tracks something as far as the config is concerned.
            (
                "gone upstream",
                "## main...origin/main [gone]",
                st("main", true, 0, 0, 0),
            ),
            (
                "changes counted",
                "## main...origin/main\n M a.go\n?? b.go\nD  c.go",
                st("main", true, 0, 0, 3),
            ),
        ] {
            assert_eq!(parse_status(out), want, "{name}");
        }
        // Being behind is the remote's news, not work anyone left behind.
        assert!(!st("", true, 0, 4, 0).unfinished());
        assert!(st("", true, 1, 0, 0).unfinished());
    }

    #[test]
    fn parse_branches_keeps_one_row_per_branch() {
        let out = [
            "refs/heads/main",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            "refs/remotes/origin/feat/login",
            "refs/heads/release/main",
            "refs/remotes/upstream/feat/login",
            "refs/remotes/upstream/fix",
        ]
        .join("\n");
        let b = |name: &str, remote: &str| Branch {
            name: name.into(),
            remote: remote.into(),
            unfetched: false,
        };
        assert_eq!(
            parse_branches(&out, &["origin".into(), "upstream".into()]),
            vec![
                b("main", ""),
                b("feat/login", "origin/feat/login"),
                b("release/main", ""),
                b("fix", "upstream/fix")
            ]
        );
    }

    #[test]
    fn status_map_counts_untracked_files() {
        let dir = TempDir::new();
        let clean = dir.join("clean");
        let dirty = dir.join("dirty");
        for p in [&clean, &dirty] {
            mkdir(p);
            crate::testutil::git(p, &["init", "-q"]);
        }
        write(&paths::join(&dirty, "scratch.txt"), "wip\n");
        // Not a repository at all; git cannot answer, which counts as clean.
        let plain = dir.join("plain");
        mkdir(&plain);

        let got = status_map(&[clean.clone(), dirty.clone(), plain.clone()]);
        assert_eq!(got.len(), 3, "{got:?}");
        assert!(got[&dirty].dirty > 0);
        assert_eq!(got[&clean].dirty, 0);
        assert_eq!(got[&plain].dirty, 0);
        assert!(status_map(&[]).is_empty());
    }

    #[test]
    fn remotes_reads_names_and_origin_in_one_call() {
        let tmp = TempDir::new();
        let dir = tmp.join("repo");
        git_repo(&dir);
        crate::testutil::git(
            &dir,
            &[
                "remote",
                "add",
                "upstream",
                "https://example.com/upstream.git",
            ],
        );
        crate::testutil::git(
            &dir,
            &["remote", "add", "origin", "git@github.com:acme/alpha.git"],
        );

        let (names, origin) = remotes(&dir);
        assert_eq!(origin, "git@github.com:acme/alpha.git");
        // Each remote is listed twice, for fetch and for push, and has to be
        // counted once: the names tell a remote ref from a local branch.
        assert_eq!(names, vec!["origin".to_string(), "upstream".to_string()]);

        // A repository with no remotes is normal, not an error.
        let bare = tmp.join("bare");
        git_repo(&bare);
        assert_eq!(remotes(&bare), (Vec::new(), String::new()));
    }

    #[test]
    fn inherited_git_configuration_does_not_rewrite_remote_urls() {
        const CHILD: &str = "GM_TEST_GIT_CONFIG_ISOLATION_CHILD";
        const REPO: &str = "GM_TEST_GIT_CONFIG_ISOLATION_REPO";
        const GH: &str = "GM_TEST_GIT_CONFIG_ISOLATION_GH";
        const OUTPUT: &str = "GM_TEST_GIT_CONFIG_ISOLATION_OUTPUT";
        if std::env::var_os(CHILD).is_some() {
            let repo = std::env::var(REPO).unwrap();
            assert_eq!(remotes(&repo).1, "git@github.com:acme/alpha.git");
            let gh = std::env::var(GH).unwrap();
            crate::repo::check_out_pull_request(&gh, &repo, &repo, 1).unwrap();
            let output = std::fs::read_to_string(std::env::var(OUTPUT).unwrap()).unwrap();
            assert_eq!(output.lines().count(), 2);
            assert!(
                output
                    .lines()
                    .all(|line| line.contains("git@github.com:acme/alpha.git"))
            );
            return;
        }

        let tmp = TempDir::new();
        let repo = tmp.join("repo");
        git_repo(&repo);
        crate::testutil::git(
            &repo,
            &["remote", "add", "origin", "git@github.com:acme/alpha.git"],
        );
        let hostile = tmp.join("hostile.gitconfig");
        write(
            &hostile,
            "[url \"https://rewritten.example/\"]\n\tinsteadOf = git@github.com:\n",
        );
        let gh = tmp.join("gh");
        write(
            &gh,
            "#!/bin/sh\ngit -C \"$GM_TEST_GIT_CONFIG_ISOLATION_REPO\" remote -v > \"$GM_TEST_GIT_CONFIG_ISOLATION_OUTPUT\"\n",
        );
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
        let output = tmp.join("remote-output");

        let executable = std::env::current_exe().unwrap();
        for mode in ["global", "system", "count"] {
            let mut child = Command::new(&executable);
            child
                .args([
                    "--exact",
                    "repo::git::tests::inherited_git_configuration_does_not_rewrite_remote_urls",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .env(REPO, &repo)
                .env(GH, &gh)
                .env(OUTPUT, &output);
            for (key, _) in std::env::vars_os() {
                if key.to_string_lossy().starts_with("GIT_CONFIG_") {
                    child.env_remove(key);
                }
            }
            child.env("GIT_CONFIG_NOSYSTEM", "1");
            match mode {
                "global" => {
                    child.env("GIT_CONFIG_GLOBAL", &hostile);
                }
                "system" => {
                    child
                        .env("GIT_CONFIG_GLOBAL", "/dev/null")
                        .env("GIT_CONFIG_SYSTEM", &hostile)
                        .env("GIT_CONFIG_NOSYSTEM", "0");
                }
                "count" => {
                    child
                        .env("GIT_CONFIG_GLOBAL", "/dev/null")
                        .env("GIT_CONFIG_COUNT", "1")
                        .env(
                            "GIT_CONFIG_KEY_0",
                            "url.https://rewritten.example/.insteadOf",
                        )
                        .env("GIT_CONFIG_VALUE_0", "git@github.com:");
                }
                _ => unreachable!(),
            }
            let out = child.output().unwrap();
            assert!(
                out.status.success(),
                "{mode} configuration changed Git behavior:\n{}\n{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }

    #[test]
    fn valid_branch_refuses_what_git_would() {
        for ok in ["main", "feat/login", "release-1.2"] {
            assert!(valid_branch(ok), "{ok}");
        }
        for bad in [
            "",
            "-force",
            "../../escaped",
            "a..b",
            "feat login",
            "~x",
            "x:y",
        ] {
            assert!(!valid_branch(bad), "{bad}");
        }

        // And nothing is created on the way to finding out.
        let base = TempDir::new();
        let main = base.join("github.com/acme/alpha");
        git_repo(&main);
        let tree = Tree {
            roots: vec![base.path()],
        };
        let r = Repo {
            root: base.path(),
            rel: "github.com/acme/alpha".into(),
        };
        assert!(
            add_worktree(
                &base.path(),
                &main,
                &tree.worktree_dir(&r, "../../escaped"),
                "../../escaped"
            )
            .is_err()
        );
        assert!(!crate::testutil::exists(&base.join(WORKTREE_ROOT)));
    }

    // These run while the finder owns the terminal, so git must print nothing
    // and put what it had to say in the error instead. Every one of them
    // captures git's output, which a failure here would show as a message
    // that lost git's explanation.
    #[test]
    fn git_output_goes_into_the_error() {
        let base = TempDir::new();
        let main = base.join("github.com/acme/alpha");
        git_repo(&main);
        let tree = Tree {
            roots: vec![base.path()],
        };
        let r = Repo {
            root: base.path(),
            rel: "github.com/acme/alpha".into(),
        };
        let dir = tree.worktree_dir(&r, "feat/login");

        add_worktree(&base.path(), &main, &dir, "feat/login").unwrap();
        let e = add_worktree(&base.path(), &main, &dir, "feat/login").unwrap_err();
        assert!(
            e.0.contains("already exists"),
            "the error lost git's explanation: {e}"
        );
        tree.create("acme/bravo", false).unwrap();
        remove_worktree(&main, &dir, false).unwrap();
    }

    #[test]
    fn failed_worktree_add_prunes_its_empty_parents() {
        let base = TempDir::new();
        let main = base.join("github.com/acme/alpha");
        git_repo(&main);
        let root = base.join(WORKTREE_ROOT);
        let first = base.join("other/feat/login");
        add_worktree(&root, &main, &first, "feat/login").unwrap();

        let failed = paths::join(&root, "github.com/acme/alpha/fix/login");
        assert!(add_worktree(&root, &main, &failed, "feat/login").is_err());
        assert!(!crate::testutil::exists(&paths::join(&root, "github.com")));

        remove_worktree(&main, &first, false).unwrap();
    }

    #[test]
    fn add_worktree_from_a_remote_branch_tracks_it() {
        let tmp = TempDir::new();
        let upstream = tmp.join("upstream");
        git_repo(&upstream);
        git_in(&upstream, &["branch", "feat/login"]).unwrap();
        let clone = tmp.join("clone");
        git_quiet(&["clone", "-q", &upstream, &clone]).unwrap();

        let bs = branches(&clone).unwrap();
        assert!(
            bs.contains(&Branch {
                name: "feat/login".into(),
                remote: "origin/feat/login".into(),
                unfetched: false
            }),
            "{bs:?}"
        );
        let dir = tmp.join("wt");
        add_worktree_from(&tmp.path(), &clone, &dir, "feat/login", "origin/feat/login").unwrap();
        assert_eq!(
            git_in(
                &dir,
                &["rev-parse", "--abbrev-ref", "feat/login@{upstream}"]
            )
            .unwrap(),
            "origin/feat/login"
        );
    }
}
