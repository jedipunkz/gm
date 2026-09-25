//! gm's command line: one table of subcommands, and the plumbing they share. A
//! new subcommand is one entry in COMMANDS plus its run function.

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::process::{Command, Stdio};

use crate::config::{self, Config};
use crate::finder::{self, Action, Keys};
use crate::repo::{self, History, Repo, State, Tree};
use crate::{Error, Result, VERSION, err, paths, plural};

/// App is what every subcommand is handed: the settings, the repository tree
/// resolved once for the whole run, where the visit log lives, and the two
/// streams it writes to, which a test captures.
pub struct App<'a> {
    pub cfg: Config,
    pub tree: Tree,
    pub state: String, // the visit log
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
}

/// Cmd is one subcommand. usage is the argument spec shown in the help text;
/// run is the flag set and the work itself.
struct Cmd {
    name: &'static str,
    aliases: &'static [&'static str],
    usage: &'static str,
    run: fn(&mut App, &[String]) -> Result<()>,
}

const COMMANDS: &[Cmd] = &[
    Cmd {
        name: "get",
        aliases: &["clone"],
        usage: "get [-u] [-p] [--shallow] [-b <branch>] [-s] [-l] <repo>...",
        run: |a, args| a.get(args),
    },
    Cmd {
        name: "list",
        aliases: &["ls"],
        usage: "list [-p] [-e] [--unique] [<query>]",
        run: |a, args| a.list(args),
    },
    Cmd {
        name: "remove",
        aliases: &["rm"],
        usage: "remove [--dry-run] [-y] <repo>...",
        run: |a, args| a.remove(args),
    },
    Cmd {
        name: "create",
        aliases: &["new"],
        usage: "create [-p] <repo>",
        run: |a, args| a.create(args),
    },
    Cmd {
        name: "status",
        aliases: &[],
        usage: "status [--dirty] [--unpushed] [-a] [-p]",
        run: |a, args| a.status(args),
    },
    Cmd {
        name: "wt",
        aliases: &[],
        usage: "wt <create|remove> [-y] <repo> <branch>",
        run: |a, args| a.wt(args),
    },
    Cmd {
        name: "migrate",
        aliases: &[],
        usage: "migrate [--dry-run] [-y] [-r] <directory>...",
        run: |a, args| a.migrate(args),
    },
    Cmd {
        name: "root",
        aliases: &[],
        usage: "root [--all]",
        run: |a, args| a.root(args),
    },
    Cmd {
        name: "version",
        aliases: &[],
        usage: "version",
        run: |a, args| a.version(args),
    },
    Cmd {
        name: "shell",
        aliases: &[],
        usage: "shell <fish|zsh|bash>          print the Ctrl-G key binding",
        run: |a, args| a.shell(args),
    },
];

/// USAGE_ERROR asks for exit status 2 without printing anything more: the
/// usage text has already been shown. HELP asks for exit status 0 the same way.
const USAGE_ERROR: &str = "\0usage";
const HELP: &str = "\0help";

/// exit_code is the status an error ends gm with when it carries no message
/// of its own: the arguments were wrong, or help was asked for and shown.
pub fn exit_code(e: &Error) -> Option<i32> {
    match e.0.as_str() {
        USAGE_ERROR => Some(2),
        HELP => Some(0),
        _ => None,
    }
}

/// run dispatches one command line.
pub fn run(args: Vec<String>) -> Result<()> {
    let cfg = config::load()?;
    let tree = Tree::open(&cfg)?;
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
    let mut a = App {
        cfg,
        tree,
        state: repo::history_file().unwrap_or_default(),
        out: &mut out,
        err: &mut err,
    };
    let res = a.dispatch(&args);
    a.out.flush()?;
    res
}

impl App<'_> {
    fn dispatch(&mut self, args: &[String]) -> Result<()> {
        let Some(first) = args.first() else {
            return self.finder();
        };
        match first.as_str() {
            "-h" | "--help" | "help" => {
                write!(self.out, "{}", usage())?;
                return Ok(());
            }
            "-v" | "--version" => return self.version(&[]),
            _ => {}
        }
        match COMMANDS
            .iter()
            .find(|c| c.name == first || c.aliases.contains(&first.as_str()))
        {
            Some(c) => (c.run)(self, &args[1..]),
            None => {
                write!(self.err, "{}", usage())?;
                Err(USAGE_ERROR.into())
            }
        }
    }

    fn version(&mut self, _: &[String]) -> Result<()> {
        writeln!(self.out, "gm {VERSION}")?;
        Ok(())
    }

    /// bump records a visit. Losing one is not worth failing the command
    /// that made it.
    fn bump(&self, path: &str) {
        let _ = History::open(&self.state).bump(path);
    }

    /// confirm asks before anything destructive. A closed stdin answers "no".
    fn confirm(&mut self, prompt: &str) -> bool {
        let _ = write!(self.err, "{prompt} [y/N]: ");
        let _ = self.err.flush();
        let mut line = String::new();
        if std::io::stdin().lock().read_line(&mut line).unwrap_or(0) == 0 {
            return false;
        }
        matches!(line.trim().to_lowercase().as_str(), "y" | "yes")
    }
}

/// usage is gm's help text, built from the command table so it cannot drift
/// away from what actually runs.
pub fn usage() -> String {
    let mut b = String::from("gm — keep every repository in one predictable tree.\n\nusage:\n");
    b.push_str(
        "  gm                                open the fuzzy finder (prints the chosen path)\n",
    );
    for c in COMMANDS {
        b.push_str(&format!("  gm {}\n", c.usage));
    }
    b.push_str(
        "
<repo> is a URL, host/user/repo, user/repo, or just repo.
The root is $GM_ROOT, ~/.config/gm/gm.toml, git config gm.root,
$GHQ_ROOT, git config ghq.root, or ~/ghq.
The finder's colors come from theme in ~/.config/gm/gm.toml.
",
    );
    b
}

/// Flag is one option a subcommand takes. Every name answers with one dash or
/// two, the way Go's flag package read them.
struct Flag {
    names: &'static [&'static str],
    value: Option<&'static str>, // what the value is called; None for a switch
    usage: &'static str,
}

/// Parsed is a command line after its flags: the switches that were set, the
/// values given, and the arguments left over.
struct Parsed {
    set: HashMap<&'static str, String>,
    args: Vec<String>,
}

impl Parsed {
    fn on(&self, name: &str) -> bool {
        self.set.get(name).is_some_and(|v| v == "true")
    }

    fn value(&self, name: &str) -> String {
        self.set.get(name).cloned().unwrap_or_default()
    }
}

/// parse reads flags the way Go's flag package did: they come before the
/// arguments, the first argument that is not one ends them, and so does "--".
/// A flag gm does not know prints the usage and asks for exit status 2.
fn parse(a: &mut App, cmd: &str, flags: &[Flag], args: &[String]) -> Result<Parsed> {
    let mut set = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if arg == "--" {
            i += 1;
            break;
        }
        let Some(body) = arg
            .strip_prefix("--")
            .or_else(|| arg.strip_prefix('-'))
            .filter(|b| !b.is_empty())
        else {
            break;
        };
        i += 1;
        let (name, inline) = match body.split_once('=') {
            Some((n, v)) => (n, Some(v.to_string())),
            None => (body, None),
        };
        if matches!(name, "h" | "help") {
            write!(a.err, "{}", flag_usage(cmd, flags))?;
            return Err(HELP.into());
        }
        let Some(f) = flags.iter().find(|f| f.names.contains(&name)) else {
            write!(
                a.err,
                "flag provided but not defined: -{name}\n{}",
                flag_usage(cmd, flags)
            )?;
            return Err(USAGE_ERROR.into());
        };
        let v = match (f.value, inline) {
            (None, None) => "true".to_string(),
            (None, Some(v)) if parse_bool(&v).is_some() => parse_bool(&v).unwrap().to_string(),
            (Some(_), Some(v)) => v,
            (Some(_), None) if i < args.len() => {
                i += 1;
                args[i - 1].clone()
            }
            (_, v) => {
                let why = match v {
                    Some(v) => format!("invalid boolean value {v:?} for -{name}"),
                    None => format!("flag needs an argument: -{name}"),
                };
                write!(a.err, "{why}\n{}", flag_usage(cmd, flags))?;
                return Err(USAGE_ERROR.into());
            }
        };
        set.insert(f.names[0], v);
    }
    Ok(Parsed {
        set,
        args: args[i..].to_vec(),
    })
}

/// parse_bool reads a switch's value the way Go's strconv.ParseBool did.
fn parse_bool(v: &str) -> Option<bool> {
    match v {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

fn flag_usage(cmd: &str, flags: &[Flag]) -> String {
    let mut s = format!("Usage of {cmd}:\n");
    for f in flags {
        for n in f.names {
            let value = f.value.map(|v| format!(" {v}")).unwrap_or_default();
            s.push_str(&format!("  -{n}{value}\n    \t{}\n", f.usage));
        }
    }
    s
}

impl App<'_> {
    fn get(&mut self, args: &[String]) -> Result<()> {
        let flags = [
            Flag {
                names: &["u", "update"],
                value: None,
                usage: "update the repository if it is already cloned",
            },
            Flag {
                names: &["p"],
                value: None,
                usage: "clone via SSH",
            },
            Flag {
                names: &["shallow"],
                value: None,
                usage: "do a shallow clone",
            },
            Flag {
                names: &["s", "silent"],
                value: None,
                usage: "clone quietly",
            },
            Flag {
                names: &["l", "look"],
                value: None,
                usage: "open a shell in the repository afterwards",
            },
            Flag {
                names: &["no-recursive"],
                value: None,
                usage: "do not clone submodules",
            },
            Flag {
                names: &["b", "branch"],
                value: Some("branch"),
                usage: "clone a single branch",
            },
        ];
        let p = parse(self, "gm get", &flags, args)?;
        if p.args.is_empty() {
            return Err("usage: gm get [options] <url>|<user>/<repo>|<repo>".into());
        }

        let mut last = String::new();
        for reference in &p.args {
            let u = repo::normalize_url(reference, p.on("p"))?;
            let rel = repo::rel_path_of(&u);
            let existing = self.tree.existing_path(&rel);
            let dst = existing.clone().unwrap_or_else(|| self.tree.path_for(&rel));
            last = dst.clone();

            if existing.is_some() {
                if !p.on("u") {
                    writeln!(self.err, "exists   {dst}")?;
                    self.bump(&dst);
                    continue;
                }
                writeln!(self.err, "update   {dst}")?;
                repo::git(&["-C", &dst, "remote", "update", "--prune"])?;
                self.bump(&dst);
                continue;
            }
            std::fs::create_dir_all(paths::dir(&dst))?;

            let url = u.to_string();
            let branch = p.value("b");
            let mut clone = vec!["clone"];
            if p.on("shallow") {
                clone.extend(["--depth", "1"]);
            }
            if !branch.is_empty() {
                clone.extend(["--branch", &branch, "--single-branch"]);
            }
            if !p.on("no-recursive") {
                clone.push("--recursive");
            }
            if p.on("s") {
                clone.push("--quiet");
            }
            clone.extend([url.as_str(), dst.as_str()]);

            writeln!(self.err, "clone    {url} -> {dst}")?;
            if let Err(e) = repo::git(&clone) {
                repo::prune_empty_parents(self.tree.primary(), paths::dir(&dst));
                return Err(e);
            }
            self.bump(&dst);
        }

        if p.on("l") && !last.is_empty() {
            return look_in(&last);
        }
        Ok(())
    }

    fn list(&mut self, args: &[String]) -> Result<()> {
        let flags = [
            Flag {
                names: &["p", "full-path"],
                value: None,
                usage: "print full paths",
            },
            Flag {
                names: &["e", "exact"],
                value: None,
                usage: "match the query exactly",
            },
            Flag {
                names: &["unique"],
                value: None,
                usage: "print the shortest unambiguous path",
            },
        ];
        let p = parse(self, "gm list", &flags, args)?;
        let query = p.args.join(" ");

        let repos = self.tree.list();
        let mut hits: Vec<&Repo> = repos
            .iter()
            .filter(|r| repo::matches(&r.rel, &query, p.on("e")))
            .collect();
        hits.sort_by(|a, b| a.rel.cmp(&b.rel));

        // Uniqueness is a property of the whole tree, not of the query: a
        // name that only the hits agree on would still be ambiguous to gm rm.
        let short: HashMap<&str, String> = if p.on("unique") {
            repos
                .iter()
                .map(|r| r.rel.as_str())
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
                writeln!(out, "{}", short[r.rel.as_str()])?;
            } else {
                writeln!(out, "{}", r.rel)?;
            }
        }
        // One check for the lot: a closed pipe or a full disk shows up here.
        out.flush()?;
        Ok(())
    }

    fn root(&mut self, args: &[String]) -> Result<()> {
        let flags = [Flag {
            names: &["all"],
            value: None,
            usage: "show every root",
        }];
        let p = parse(self, "gm root", &flags, args)?;
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

    fn remove(&mut self, args: &[String]) -> Result<()> {
        let flags = [
            Flag {
                names: &["dry-run"],
                value: None,
                usage: "show what would be removed",
            },
            Flag {
                names: &["y"],
                value: None,
                usage: "skip the confirmation prompt",
            },
        ];
        let p = parse(self, "gm remove", &flags, args)?;
        if p.args.is_empty() {
            return Err("usage: gm remove [--dry-run] [-y] <repo>...".into());
        }
        for q in &p.args {
            let r = self.tree.resolve(q)?;
            self.remove_one(&r, p.on("dry-run"), p.on("y"))?;
        }
        Ok(())
    }

    /// remove_one deletes one repository, warning first when there is work in
    /// it and asking before anything is lost.
    fn remove_one(&mut self, r: &Repo, dry_run: bool, yes: bool) -> Result<()> {
        if dry_run {
            writeln!(self.err, "would remove {}", r.path())?;
            return Ok(());
        }
        if repo::is_dirty(&r.path()).unwrap_or(false) {
            writeln!(self.err, "warning: {} has uncommitted changes", r.rel)?;
        }
        // They go with it, so they have to be said before the question, not
        // discovered afterwards.
        let wts = repo::other_worktrees(r);
        if !wts.is_empty() {
            writeln!(self.err, "warning: {}", worktree_warning(&wts))?;
        }
        if !yes && !self.confirm(&format!("remove {}?", r.path())) {
            writeln!(self.err, "skipped")?;
            return Ok(());
        }
        repo::delete(r)?;
        writeln!(self.err, "removed  {}", r.path())?;
        Ok(())
    }

    fn create(&mut self, args: &[String]) -> Result<()> {
        let flags = [Flag {
            names: &["p"],
            value: None,
            usage: "set the origin remote to its SSH URL",
        }];
        let p = parse(self, "gm create", &flags, args)?;
        let [reference] = p.args.as_slice() else {
            return Err("usage: gm create [-p] <repo>|<user>/<repo>|<host>/<user>/<repo>".into());
        };
        let r = self.tree.create(reference, p.on("p"))?;
        self.bump(&r.path());
        writeln!(self.out, "{}", r.path())?;
        Ok(())
    }

    /// status answers "what did I leave unfinished?" across the whole tree. It
    /// never fetches: ahead and behind are counted against the refs already on
    /// disk, so it is fast and works offline, and behind is as stale as the
    /// last fetch was.
    fn status(&mut self, args: &[String]) -> Result<()> {
        let flags = [
            Flag {
                names: &["dirty"],
                value: None,
                usage: "only repositories with uncommitted changes",
            },
            Flag {
                names: &["unpushed"],
                value: None,
                usage: "only repositories with commits that are not pushed",
            },
            Flag {
                names: &["a"],
                value: None,
                usage: "include repositories that are clean and in sync",
            },
            Flag {
                names: &["p"],
                value: None,
                usage: "print full paths",
            },
        ];
        let p = parse(self, "gm status", &flags, args)?;
        let (dirty_only, unpushed, all) = (p.on("dirty"), p.on("unpushed"), p.on("a"));

        let rows = self.everything(p.on("p"));
        let states = repo::status_map(
            &rows
                .iter()
                .map(|(_, path)| path.clone())
                .collect::<Vec<_>>(),
        );

        // The name column is as wide as the widest name that survives the
        // filter, so a run that prints two rows is not padded for a hundred.
        let keep: Vec<&(String, String)> = rows
            .iter()
            .filter(|(_, path)| {
                let s = &states[path];
                !(dirty_only && s.dirty == 0
                    || unpushed && s.ahead == 0
                    || !all && !dirty_only && !unpushed && !s.unfinished())
            })
            .collect();
        let width = keep
            .iter()
            .map(|(name, _)| name.chars().count())
            .max()
            .unwrap_or(0);
        for (name, path) in keep {
            writeln!(self.out, "{name:<width$}  {}", summarize(&states[path]))?;
        }
        Ok(())
    }

    /// everything lists the repositories and the worktrees hanging off them,
    /// as (name, path). A worktree is where work in progress lives, so leaving
    /// them out would miss the thing the command is for; the walk skips dotted
    /// directories, so .worktrees has to be asked for by name.
    fn everything(&self, full: bool) -> Vec<(String, String)> {
        let mut rows: Vec<(String, String)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut add = |name: String, path: String| {
            if seen.insert(path.clone()) {
                rows.push((if full { path.clone() } else { name }, path));
            }
        };
        for r in self.tree.list() {
            add(r.rel.clone(), r.path());
        }
        for root in &self.tree.roots {
            for p in repo::find_repos(&paths::join(root, repo::WORKTREE_ROOT)) {
                if let Some(rel) = p.strip_prefix(&format!("{}/", root.trim_end_matches('/'))) {
                    add(rel.to_string(), p.clone());
                }
            }
        }
        rows.sort();
        rows
    }

    /// wt is the command line half of the finder's worktree mode. The finder
    /// covers the interactive case better — the repository is the row under
    /// the cursor rather than something to name — so this exists for scripts:
    /// a bootstrap that checks out the branches you always want.
    ///
    /// The verbs are the finder's, so there is one set of names to remember:
    /// create and remove, spelled out, no abbreviations.
    fn wt(&mut self, args: &[String]) -> Result<()> {
        match args.first().map(String::as_str) {
            Some("create") => self.wt_create(&args[1..]),
            Some("remove") => self.wt_remove(&args[1..]),
            _ => Err(WT_USAGE.into()),
        }
    }

    /// wt_create checks a branch out beside the repository and prints where,
    /// so a script can cd into it the way it can with gm create.
    fn wt_create(&mut self, args: &[String]) -> Result<()> {
        let p = parse(self, "gm wt create", &[], args)?;
        let [query, branch] = p.args.as_slice() else {
            return Err(WT_USAGE.into());
        };
        let r = self.tree.resolve(query)?;
        let dir = self.tree.worktree_dir(&r, branch);
        if paths::exists(&dir) {
            return Err(err!("{dir} already exists"));
        }
        // add_worktree starts the branch when it is new and checks it out when
        // it is not, which is the one thing worth saying before the path.
        let start = if repo::branch_exists(&r.path(), branch) {
            "existing branch"
        } else {
            "new branch"
        };
        repo::add_worktree(&r.path(), &dir, branch)?;
        writeln!(self.err, "created  {} ({start})", r.rel)?;
        self.bump(&dir);
        writeln!(self.out, "{dir}")?;
        Ok(())
    }

    /// wt_remove takes one worktree away. It is destructive, so it says what is
    /// in the way first and asks, the way gm remove does.
    fn wt_remove(&mut self, args: &[String]) -> Result<()> {
        let flags = [
            Flag {
                names: &["dry-run"],
                value: None,
                usage: "show what would be removed",
            },
            Flag {
                names: &["y"],
                value: None,
                usage: "skip the confirmation prompt",
            },
        ];
        let p = parse(self, "gm wt remove", &flags, args)?;
        let [query, branch] = p.args.as_slice() else {
            return Err(WT_USAGE.into());
        };
        let r = self.tree.resolve(query)?;
        let dir = self.tree.worktree_dir(&r, branch);

        // Asking git which worktrees there are answers two questions at once:
        // is this one of them, and is it the main one — which is the
        // repository itself and must never be removed this way.
        if !repo::other_worktrees(&r)
            .iter()
            .any(|w| repo::same_path(&w.path, &dir))
        {
            return Err(err!("{} has no worktree for {branch}", r.rel));
        }
        if p.on("dry-run") {
            writeln!(self.err, "would remove {dir}")?;
            return Ok(());
        }
        // Work in the worktree is lost with it, so it is said before the
        // question, not discovered afterwards.
        let dirty = repo::is_dirty(&dir).unwrap_or(false);
        if dirty {
            writeln!(self.err, "warning: {dir} has uncommitted changes")?;
        }
        if !p.on("y") && !self.confirm(&format!("remove {dir}?")) {
            writeln!(self.err, "skipped")?;
            return Ok(());
        }
        repo::remove_worktree_and_prune(&r, &dir, dirty)?;
        writeln!(self.err, "removed  {dir}")?;
        Ok(())
    }

    fn migrate(&mut self, args: &[String]) -> Result<()> {
        let flags = [
            Flag {
                names: &["dry-run"],
                value: None,
                usage: "show what would move",
            },
            Flag {
                names: &["y"],
                value: None,
                usage: "skip the confirmation prompt",
            },
            Flag {
                names: &["r", "recursive"],
                value: None,
                usage: "search the arguments for repositories instead of moving them",
            },
        ];
        let p = parse(self, "gm migrate", &flags, args)?;
        if p.args.is_empty() {
            return Err("usage: gm migrate [--dry-run] [-y] [-r] <directory>...".into());
        }
        let recursive = p.on("r");

        for src in self.migrate_sources(&p.args, recursive)? {
            let dst = match self.migrate_plan(&src) {
                Plan::InPlace => {
                    writeln!(self.err, "in place {src}")?;
                    continue;
                }
                // A directory named on the command line is the user's claim
                // that it should move; a directory the search turned up is
                // only a candidate, so it is skipped rather than fatal.
                Plan::Problem(why) if !recursive => return Err(err!("{src} {why}")),
                Plan::Problem(why) => {
                    writeln!(self.err, "skip     {src}: {why}")?;
                    continue;
                }
                Plan::Move(dst) => dst,
            };
            // Each checkout's .git file names the repository's path, so they
            // have to be asked about before it moves and pointed at the new
            // place after. One whose directory is already gone has nothing
            // to point.
            let wts: Vec<repo::Worktree> = repo::other_worktrees_of(&src)
                .into_iter()
                .filter(|w| paths::exists(&w.path))
                .collect();
            let along = |wts: &[repo::Worktree]| {
                let labels: Vec<String> = wts.iter().map(repo::Worktree::label).collect();
                format!(
                    "{}: {}",
                    plural(wts.len(), "worktree", "worktrees"),
                    labels.join(", ")
                )
            };
            if p.on("dry-run") {
                writeln!(self.err, "would move {src} -> {dst}")?;
                if !wts.is_empty() {
                    writeln!(self.err, "  and repair its {}", along(&wts))?;
                }
                continue;
            }
            if !wts.is_empty() {
                writeln!(self.err, "its {} will be repaired", along(&wts))?;
            }
            if !p.on("y") && !self.confirm(&format!("move {src} -> {dst}?")) {
                writeln!(self.err, "skipped")?;
                continue;
            }
            // Worked out before the move: src has to exist to be resolved.
            let dirs: Vec<String> = wts
                .iter()
                .map(|w| moved_along(&w.path, &src, &dst))
                .collect();
            move_dir(&src, &dst)?;
            self.bump(&dst);
            writeln!(self.err, "moved    {src} -> {dst}")?;
            if let Err(e) = repo::repair_worktrees(&dst, &dirs) {
                return Err(err!(
                    "moved {src} -> {dst}, but its worktrees still point at the old place ({e}); \
                     run `git -C {dst} worktree repair <worktree>...`"
                ));
            }
            if !wts.is_empty() {
                writeln!(self.err, "repaired {}", along(&wts))?;
            }
            writeln!(self.out, "{dst}")?;
        }
        Ok(())
    }

    /// migrate_sources turns the arguments into the directories to consider.
    /// Each one is a repository, or with -r a directory to search;
    /// repositories already under a root are dropped, since the whole point is
    /// to move the ones that are not.
    fn migrate_sources(&self, args: &[String], recursive: bool) -> Result<Vec<String>> {
        let mut srcs = Vec::new();
        for arg in args {
            let dir = paths::abs(arg)?;
            if !recursive {
                srcs.push(dir);
                continue;
            }
            srcs.extend(
                repo::find_repos(&dir)
                    .into_iter()
                    .filter(|p| !self.tree.contains(p)),
            );
        }
        Ok(srcs)
    }

    /// migrate_plan decides where a repository belongs, without touching
    /// anything.
    fn migrate_plan(&self, src: &str) -> Plan {
        if !repo::is_repo(src) {
            return Plan::Problem("is not a repository".into());
        }
        // A .git file means a worktree or submodule; moving it breaks the link.
        if std::fs::metadata(paths::join(src, ".git")).is_ok_and(|m| !m.is_dir()) {
            return Plan::Problem(
                "is a worktree or submodule and cannot be moved on its own".into(),
            );
        }
        let remote = match repo::git_in(src, &["remote", "get-url", "origin"]) {
            Ok(r) if !r.is_empty() => r,
            _ => return Plan::Problem("has no origin remote".into()),
        };
        let Ok(u) = repo::normalize_url(&remote, false) else {
            return Plan::Problem(format!("has an origin gm cannot read ({remote})"));
        };
        let dst = self.tree.path_for(&repo::rel_path_of(&u));
        if src == dst {
            return Plan::InPlace;
        }
        if paths::exists(&dst) {
            return Plan::Problem(format!("would move to {dst}, which already exists"));
        }
        Plan::Move(dst)
    }

    fn shell(&mut self, args: &[String]) -> Result<()> {
        let [sh] = args else {
            return Err("usage: gm shell <fish|zsh|bash>".into());
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
        write!(
            self.out,
            "{}",
            tmpl.replace("{{key}}", &k.letter.to_string())
                .replace("{{name}}", &k.display)
        )?;
        Ok(())
    }

    /// finder opens the interactive picker and prints the chosen path on
    /// stdout, so the shell binding can capture it with $(gm).
    fn finder(&mut self) -> Result<()> {
        let repos = self.tree.list();
        if repos.is_empty() {
            return Err(err!(
                "no repositories under {}; try `gm get <repo>`",
                self.tree.primary()
            ));
        }
        let theme = finder::lookup_theme(&self.cfg.theme)?;
        let keys = Keys {
            worktree: config::parse_chord(&self.cfg.worktree_key, finder::DEFAULT_WORKTREE_KEY)?,
            branch: config::parse_chord(&self.cfg.branch_key, finder::DEFAULT_BRANCH_KEY)?,
            pr: config::parse_chord(&self.cfg.pr_key, finder::DEFAULT_PR_KEY)?,
            remote: config::parse_chord(&self.cfg.remote_key, finder::DEFAULT_REMOTE_KEY)?,
        };
        let mut hist = History::open(&self.state);
        let res = finder::run(&self.tree, &repos, &hist, &theme, keys)?;
        self.act(res, &mut hist)
    }

    /// act carries out what the finder decided, now that the alternate screen
    /// is gone: a clone's progress, a password prompt and a confirmation all
    /// belong on the terminal the user can see.
    ///
    /// One rule about the visit log, so a repository is never counted twice:
    /// it is recorded by whatever fetched, made or moved the repository, and by
    /// the finder only for a row that was simply chosen. gm get has already
    /// recorded the clone by the time this sees it.
    fn act(&mut self, res: finder::Outcome, hist: &mut History) -> Result<()> {
        match res.action {
            Action::Jump => self.go_to(&res.arg, hist),
            Action::Get => {
                self.get(std::slice::from_ref(&res.arg))?;
                let u = repo::normalize_url(&res.arg, false)?;
                let rel = repo::rel_path_of(&u);
                let dst = self
                    .tree
                    .existing_path(&rel)
                    .unwrap_or_else(|| self.tree.path_for(&rel));
                writeln!(self.out, "{dst}")?;
                Ok(())
            }
            Action::None => Ok(()), // the user quit
        }
    }

    /// go_to records the visit and prints the path, which is how the shell
    /// binding learns where to cd.
    fn go_to(&mut self, path: &str, hist: &mut History) -> Result<()> {
        if path.is_empty() {
            return Ok(());
        }
        if let Err(e) = hist.bump(path) {
            writeln!(self.err, "gm: could not record visit: {e}")?;
        }
        writeln!(self.out, "{path}")?;
        Ok(())
    }
}

/// Plan is where one repository would move, or why it would not.
enum Plan {
    Move(String),    // where it goes
    InPlace,         // already where gm would put it
    Problem(String), // why it cannot move, phrased to follow the path
}

const WT_USAGE: &str = "usage: gm wt <create|remove> [-y] <repo> <branch>";

/// worktree_warning names what will be taken along with a repository, and the
/// work in each of them: they are removed with --force, so git will not stop
/// for it.
fn worktree_warning(wts: &[repo::Worktree]) -> String {
    let what = if wts.len() == 1 {
        "worktree"
    } else {
        "worktrees"
    };
    format!(
        "{} {what} will be removed too: {}",
        wts.len(),
        repo::worktree_labels(wts, repo::changed_files)
    )
}

/// summarize says what is unfinished in as few words as it takes.
fn summarize(s: &State) -> String {
    let mut parts = Vec::new();
    if s.dirty > 0 {
        parts.push(plural(s.dirty, "changed file", "changed files"));
    }
    if s.ahead > 0 {
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

/// look_in drops the user into a shell inside the repository just cloned.
fn look_in(dir: &str) -> Result<()> {
    let sh = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".into());
    let status = Command::new(sh)
        .current_dir(dir)
        .env("GM_LOOK", dir)
        .status()?;
    if !status.success() {
        return Err(repo::exit_error(&status));
    }
    Ok(())
}

/// moved_along is where a checkout will be once its repository has moved from
/// src to dst: one kept inside the repository's directory goes with it. git
/// reports the resolved path, so src is compared resolved as well, which is
/// why this has to run before the move.
fn moved_along(path: &str, src: &str, dst: &str) -> String {
    let resolved = std::fs::canonicalize(src)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    [src, resolved.as_str()]
        .iter()
        .filter(|s| !s.is_empty())
        .find_map(|s| path.strip_prefix(&format!("{}/", s.trim_end_matches('/'))))
        .map_or_else(|| path.to_string(), |rest| paths::join(dst, rest))
}

/// move_dir renames src to dst, falling back to mv across filesystems.
fn move_dir(src: &str, dst: &str) -> Result<()> {
    std::fs::create_dir_all(paths::dir(dst))?;
    if std::fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    // Rename fails across filesystems; mv copies and unlinks instead.
    let status = Command::new("mv")
        .args([src, dst])
        .stdout(Stdio::from(std::io::stderr()))
        .stderr(Stdio::from(std::io::stderr()))
        .status()?;
    if !status.success() {
        return Err(err!("moving {src} to {dst}: {}", repo::exit_error(&status)));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{TempDir, exists, git, git_repo, mkdir, write};

    /// Run is one command run against a tree, with both streams captured.
    struct Run {
        out: String,
        err: String,
        res: Result<()>,
    }

    fn run_in(
        tree: &Tree,
        state: &str,
        cfg: Config,
        f: impl FnOnce(&mut App) -> Result<()>,
    ) -> Run {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let res = {
            let mut a = App {
                cfg,
                tree: tree.clone(),
                state: state.into(),
                out: &mut out,
                err: &mut err,
            };
            f(&mut a)
        };
        Run {
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
            res,
        }
    }

    fn tree(root: &str) -> Tree {
        Tree {
            roots: vec![root.to_string()],
        }
    }

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    // The help text must not drift away from the table it is built from.
    #[test]
    fn usage_covers_every_command() {
        let u = usage();
        for c in COMMANDS {
            assert!(
                u.contains(&format!("gm {}", c.name)),
                "usage does not mention {}",
                c.name
            );
        }
    }

    // A new command must not shadow an existing name or alias.
    #[test]
    fn names_are_unique() {
        let mut seen = HashMap::new();
        for c in COMMANDS {
            for n in std::iter::once(&c.name).chain(c.aliases) {
                assert!(seen.insert(*n, c.name).is_none(), "{n} is claimed twice");
            }
        }
    }

    #[test]
    fn flags_parse_the_go_way() {
        let tmp = TempDir::new();
        let flags = [
            Flag {
                names: &["u", "update"],
                value: None,
                usage: "",
            },
            Flag {
                names: &["b", "branch"],
                value: Some("branch"),
                usage: "",
            },
        ];
        let got = |a: &[&str]| {
            let mut p = None;
            let r = run_in(&tree(&tmp.path()), "", Config::default(), |app| {
                p = Some(parse(app, "gm t", &flags, &args(a))?);
                Ok(())
            });
            r.res.map(|_| {
                let p = p.unwrap();
                (p.on("u"), p.value("b"), p.args)
            })
        };
        assert_eq!(
            got(&["-u", "--branch", "x", "a", "-b"]).unwrap(),
            (true, "x".into(), args(&["a", "-b"]))
        );
        assert_eq!(
            got(&["--update=false", "-b=y", "--", "-u"]).unwrap(),
            (false, "y".into(), args(&["-u"]))
        );
        assert!(got(&["-u=TRUE"]).unwrap().0 && !got(&["-u=F"]).unwrap().0);
        assert_eq!(exit_code(&got(&["-u=yes"]).unwrap_err()), Some(2));
        assert_eq!(exit_code(&got(&["-x"]).unwrap_err()), Some(2));
        assert_eq!(exit_code(&got(&["-b"]).unwrap_err()), Some(2));
        assert_eq!(exit_code(&got(&["-h"]).unwrap_err()), Some(0));
    }

    // The spelling each shell needs, and no template placeholder left over.
    #[test]
    fn shell_snippets_bind_the_configured_key() {
        let tmp = TempDir::new();
        let cfg = |key: &str| Config {
            launch_key: key.into(),
            ..Default::default()
        };
        for (sh, want) in [
            ("fish", r"bind \cr __gm_jump"),
            ("zsh", "bindkey '^r' __gm_jump"),
            ("bash", r#"bind -x '"\C-r": __gm_jump'"#),
        ] {
            let r = run_in(&tree(&tmp.path()), "", cfg("ctrl-r"), |a| {
                a.shell(&args(&[sh]))
            });
            r.res.unwrap();
            assert!(
                r.out.contains(want),
                "gm shell {sh} does not bind Ctrl-R:\n{}",
                r.out
            );
            assert!(
                r.out.contains("Ctrl-R jumps"),
                "gm shell {sh} does not name the key:\n{}",
                r.out
            );
            assert!(
                !r.out.contains("{{"),
                "gm shell {sh} left a placeholder:\n{}",
                r.out
            );
        }
        // An unusable key must stop gm rather than print a binding that
        // silently does nothing.
        for bad in ["alt-r", "ctrl-shift-b", "ctrl-alt-b"] {
            let r = run_in(&tree(&tmp.path()), "", cfg(bad), |a| {
                a.shell(&args(&["zsh"]))
            });
            assert!(r.res.is_err(), "gm shell accepted {bad}");
        }
    }

    // --unique is a property of the whole tree: a query must not shorten a
    // name down to one that gm rm would call ambiguous.
    #[test]
    fn list_unique_stays_unique_under_a_query() {
        let root = TempDir::new();
        for rel in [
            "github.com/alice/gm",
            "github.com/alice/tools",
            "github.com/bob/gm",
        ] {
            mkdir(&root.join(&format!("{rel}/.git")));
        }
        let t = tree(&root.path());
        let r = run_in(&t, "", Config::default(), |a| {
            a.list(&args(&["--unique", "alice"]))
        });
        r.res.unwrap();
        let got: Vec<&str> = r.out.split_whitespace().collect();
        assert_eq!(got, vec!["alice/gm", "tools"]);
        for name in got {
            assert!(
                t.resolve(name).is_ok(),
                "printed name {name:?} does not resolve"
            );
        }
    }

    // Recording the visit and printing the path is the whole contract between
    // the finder and the shell binding, and one jump is one visit.
    #[test]
    fn act_jump_records_once_and_prints() {
        let root = TempDir::new();
        let dst = root.join("github.com/acme/alpha");
        mkdir(&dst);
        let mut hist = History::open(&root.join("frecency.json"));
        let r = run_in(&tree(&root.path()), "", Config::default(), |a| {
            a.act(
                finder::Outcome {
                    action: Action::Jump,
                    arg: dst.clone(),
                },
                &mut hist,
            )
        });
        r.res.unwrap();
        assert_eq!(r.out.trim(), dst);
        assert_eq!(hist.visit(&dst).count, 1);
    }

    // Quitting the finder leaves no trace.
    #[test]
    fn act_none_does_nothing() {
        let root = TempDir::new();
        let mut hist = History::open(&root.join("frecency.json"));
        let r = run_in(&tree(&root.path()), "", Config::default(), |a| {
            a.act(finder::Outcome::default(), &mut hist)
        });
        r.res.unwrap();
        assert_eq!(r.out + &r.err, "");
    }

    // gm get writes the visit log itself, in the middle of act(); the copy the
    // finder was handed was read before that. Saving the finder's copy
    // afterwards would throw that write away, and anything else written to the
    // log in that window.
    #[test]
    fn act_get_keeps_what_get_recorded() {
        let root = TempDir::new();
        let state = root.join("state/frecency.json");
        let dst = root.join("example.com/acme/alpha");
        git_repo(&dst); // already cloned: gm get reports it without the network
        let other = root.join("example.com/acme/other");
        git_repo(&other);

        // The finder reads the log when it opens, and something writes to it
        // afterwards.
        let mut hist = History::open(&state);
        History::open(&state).bump(&other).unwrap();

        let r = run_in(&tree(&root.path()), &state, Config::default(), |a| {
            a.act(
                finder::Outcome {
                    action: Action::Get,
                    arg: "example.com/acme/alpha".into(),
                },
                &mut hist,
            )
        });
        r.res.unwrap();
        assert!(
            r.out.contains(&dst),
            "the path was not printed: {:?}",
            r.out
        );

        let after = History::open(&state);
        assert_eq!(after.visit(&dst).count, 1, "the repository gm get reported");
        assert_eq!(
            after.visit(&other).count,
            1,
            "a visit written while the finder was open was discarded"
        );
    }

    // get searches every configured root before cloning into the primary one,
    // and the finder returns the root where the repository was found.
    #[test]
    fn get_finds_repository_under_a_secondary_root() {
        let base = TempDir::new();
        let blocked = base.join("blocked");
        write(&blocked, "not a directory");
        let primary = paths::join(&blocked, "root");
        let secondary = base.join("secondary");
        let dst = paths::join(&secondary, "example.com/acme/alpha");
        git_repo(&dst);
        let t = Tree {
            roots: vec![primary.clone(), secondary],
        };
        let state = base.join("state/frecency.json");

        let already_present = run_in(&t, &state, Config::default(), |a| {
            a.get(&args(&["example.com/acme/alpha"]))
        });
        already_present.res.unwrap();
        assert!(
            already_present.err.contains(&format!("exists   {dst}")),
            "{}",
            already_present.err
        );
        assert!(!exists(&paths::join(&primary, "example.com/acme/alpha")));
        assert_eq!(History::open(&state).visit(&dst).count, 1);

        let update = run_in(&t, &state, Config::default(), |a| {
            a.get(&args(&["-u", "example.com/acme/alpha"]))
        });
        update.res.unwrap();
        assert!(
            update.err.contains(&format!("update   {dst}")),
            "{}",
            update.err
        );
        assert!(!exists(&paths::join(&primary, "example.com/acme/alpha")));
        assert_eq!(History::open(&state).visit(&dst).count, 2);

        let mut hist = History::open(&state);
        let finder_get = run_in(&t, &state, Config::default(), |a| {
            a.act(
                finder::Outcome {
                    action: Action::Get,
                    arg: "example.com/acme/alpha".into(),
                },
                &mut hist,
            )
        });
        finder_get.res.unwrap();
        assert_eq!(finder_get.out.trim(), dst);
        assert!(finder_get.err.contains(&format!("exists   {dst}")));
        assert!(!exists(&paths::join(&primary, "example.com/acme/alpha")));
        assert_eq!(History::open(&state).visit(&dst).count, 3);
    }

    // A reference with ".." in it is refused before anything is made, so no
    // directory appears outside the root.
    #[test]
    fn get_and_create_refuse_to_leave_the_root() {
        let base = TempDir::new();
        let root = base.join("root");
        mkdir(&root);
        for cmd in ["get", "create"] {
            let r = run_in(&tree(&root), "", Config::default(), |a| {
                let reference = args(&["example.com/../../evil"]);
                if cmd == "get" {
                    a.get(&reference)
                } else {
                    a.create(&reference)
                }
            });
            assert!(r.res.is_err(), "gm {cmd} accepted it");
            assert!(
                !exists(&base.join("evil")),
                "gm {cmd} made a directory outside the root"
            );
        }
    }

    #[test]
    fn get_prunes_host_after_failed_clone() {
        let base = TempDir::new();
        let root = base.join("root");
        mkdir(&root);
        let source = base.join("missing-repository");
        let reference = format!("file://localhost{source}");

        let r = run_in(&tree(&root), "", Config::default(), |a| {
            a.get(&args(&[&reference]))
        });

        let host = paths::join(&root, "localhost");
        let err = r.res.expect_err("gm get succeeded for a missing source");
        assert!(!exists(&host), "failed clone left host path {host}");
        assert_eq!(err.0, "exit status 128", "gm get changed the clone error");
    }

    fn init_repo(dir: &str, origin: &str) -> String {
        mkdir(dir);
        git(dir, &["init", "-q"]);
        if !origin.is_empty() {
            git(dir, &["remote", "add", "origin", origin]);
        }
        dir.to_string()
    }

    // What -r is for: find the working copies under a directory, leave the
    // ones already in the tree alone, and report the ones that cannot move
    // instead of abandoning the run.
    #[test]
    fn migrate_recursive() {
        let base = TempDir::new();
        let root = base.join("tree");
        mkdir(&root);
        let good = init_repo(&base.join("src/good"), "https://github.com/acme/good");
        let also = init_repo(
            &base.join("src/deeper/also"),
            "git@github.com:acme/also.git",
        );
        let no_remote = init_repo(&base.join("src/noremote"), "");
        // An origin that would put it outside the root is not a place to go.
        let escaping = init_repo(&base.join("src/escaping"), "https://github.com/../../evil");
        init_repo(
            &paths::join(&root, "github.com/acme/already"),
            "https://github.com/acme/already",
        );
        // A .git file is a worktree or a submodule, and moving it breaks the link.
        let linked = base.join("src/linked");
        mkdir(&linked);
        write(&paths::join(&linked, ".git"), "gitdir: /elsewhere\n");

        let r = run_in(&tree(&root), "", Config::default(), |a| {
            a.migrate(&args(&["-r", "--dry-run", &base.path()]))
        });
        r.res.unwrap();
        for want in [
            format!("{good} -> {}", paths::join(&root, "github.com/acme/good")),
            format!("{also} -> {}", paths::join(&root, "github.com/acme/also")),
            format!("{no_remote}: has no origin remote"),
            format!("{escaping}: has an origin gm cannot read"),
            format!("{linked}: is a worktree or submodule"),
        ] {
            assert!(
                r.err.contains(&want),
                "the output is missing {want:?}:\n{}",
                r.err
            );
        }
        // A repository already under the root is not a candidate at all.
        assert!(!r.err.contains("already"), "{}", r.err);
        // --dry-run moves nothing.
        assert!(exists(&good));
    }

    // A worktree's .git file names the repository's path, so moving the
    // repository alone leaves every checkout answering "not a git repository".
    // Both kinds are repaired: one elsewhere, and one kept inside the
    // repository's directory, which moves with it.
    #[test]
    fn migrate_repairs_the_worktrees() {
        let base = TempDir::new();
        let root = base.join("tree");
        mkdir(&root);
        let src = base.join("src/alpha");
        git_repo(&src);
        git(
            &src,
            &["remote", "add", "origin", "https://github.com/acme/alpha"],
        );
        let outside = base.join("wt/login");
        repo::add_worktree(&src, &outside, "feat/login").unwrap();
        repo::add_worktree(&src, &paths::join(&src, "inner"), "fix/inner").unwrap();

        // --dry-run says so, and touches nothing.
        let dry = run_in(&tree(&root), "", Config::default(), |a| {
            a.migrate(&args(&["--dry-run", &src]))
        });
        dry.res.unwrap();
        // In git's order, which is not the order they were made in.
        for want in ["and repair its 2 worktrees: ", "feat/login", "fix/inner"] {
            assert!(dry.err.contains(want), "{want:?}:\n{}", dry.err);
        }
        assert!(exists(&src));

        let r = run_in(&tree(&root), "", Config::default(), |a| {
            a.migrate(&args(&["-y", &src]))
        });
        r.res.unwrap();
        let dst = paths::join(&root, "github.com/acme/alpha");
        assert_eq!(r.out.trim(), dst);
        assert!(r.err.contains("repaired 2 worktrees"), "{}", r.err);
        for wt in [outside, paths::join(&dst, "inner")] {
            assert!(
                repo::git_in(&wt, &["status", "--porcelain"]).is_ok(),
                "{wt} is still broken"
            );
        }
        assert_eq!(repo::other_worktrees_of(&dst).len(), 2);
    }

    // Naming a directory is a claim that it should move, so a problem with it
    // is fatal rather than a skip.
    #[test]
    fn migrate_named_directory_still_errors() {
        let base = TempDir::new();
        let no_remote = init_repo(&base.join("src/noremote"), "");
        let r = run_in(&tree(&base.join("tree")), "", Config::default(), |a| {
            a.migrate(&args(&["--dry-run", &no_remote]))
        });
        assert!(r.res.unwrap_err().0.contains("has no origin remote"));
    }

    // Each reason a repository is listed, and the one reason it is not.
    #[test]
    fn status_finds_unfinished_work() {
        let tmp = TempDir::new();
        let root = tmp.join("root");
        let t = tree(&root);

        // clean: one commit, nothing changed, and an upstream it matches.
        let origin = tmp.join("origin.git");
        git(
            &tmp.path(),
            &["init", "-q", "--bare", "-b", "main", &origin],
        );
        let clean = Repo {
            root: root.clone(),
            rel: "github.com/acme/clean".into(),
        };
        mkdir(&paths::dir(&clean.path()));
        git(&tmp.path(), &["clone", "-q", &origin, &clean.path()]);
        git(
            &clean.path(),
            &["commit", "-q", "--allow-empty", "-m", "one"],
        );
        git(&clean.path(), &["push", "-q", "-u", "origin", "main"]);

        // ahead: the same, with a commit that was never pushed.
        let ahead = Repo {
            root: root.clone(),
            rel: "github.com/acme/ahead".into(),
        };
        git(&tmp.path(), &["clone", "-q", &origin, &ahead.path()]);
        git(
            &ahead.path(),
            &["commit", "-q", "--allow-empty", "-m", "two"],
        );

        // dirty: a repository with no remote at all and an uncommitted file.
        let dirty = Repo {
            root: root.clone(),
            rel: "github.com/acme/dirty".into(),
        };
        git_repo(&dirty.path());
        write(&paths::join(&dirty.path(), "wip.txt"), "x\n");

        // A worktree holds work in progress by definition, and lives under a
        // dotted directory the repository walk never descends into.
        let wt = t.worktree_dir(&dirty, "feat/login");
        repo::add_worktree(&dirty.path(), &wt, "feat/login").unwrap();
        write(&paths::join(&wt, "scratch.txt"), "y\n");

        let status = |a: &[&str]| {
            let r = run_in(&t, "", Config::default(), |app| app.status(&args(a)));
            r.res.unwrap();
            r.out
        };
        let out = status(&[]);
        assert!(
            !out.contains("acme/clean"),
            "a clean repository in sync was listed:\n{out}"
        );
        for want in [
            "github.com/acme/ahead",
            "1 ahead",
            "github.com/acme/dirty",
            "1 changed file",
            "no upstream",
            ".worktrees/github.com/acme/dirty/feat/login",
        ] {
            assert!(
                out.contains(want),
                "status did not mention {want:?}:\n{out}"
            );
        }

        // --dirty drops the repository whose only news is an unpushed commit.
        let out = status(&["--dirty"]);
        assert!(
            !out.contains("acme/ahead") && out.contains("acme/dirty"),
            "{out}"
        );
        // --unpushed is the other way round.
        let out = status(&["--unpushed"]);
        assert!(
            out.contains("acme/ahead") && !out.contains("acme/dirty"),
            "{out}"
        );
        // -a says so about the ones that are fine, rather than staying silent.
        let out = status(&["-a"]);
        assert!(out.contains("acme/clean") && out.contains("clean"), "{out}");
    }

    // The round trip a script does: make a worktree, read where it went off
    // stdout, then take it away again.
    #[test]
    fn wt_create_and_remove() {
        let root = TempDir::new();
        let r = Repo {
            root: root.path(),
            rel: "github.com/acme/alpha".into(),
        };
        git_repo(&r.path());
        let t = tree(&root.path());
        let state = root.join("state/frecency.json");

        let made = run_in(&t, &state, Config::default(), |a| {
            a.wt(&args(&["create", "acme/alpha", "feat/login"]))
        });
        made.res.unwrap();
        let dir = made.out.trim().to_string();
        assert_eq!(dir, t.worktree_dir(&r, "feat/login"));
        assert!(exists(&paths::join(&dir, ".git")));

        // The same branch twice is a mistake worth stopping at, not a silent
        // no-op: the second call would otherwise look like it worked.
        let again = run_in(&t, &state, Config::default(), |a| {
            a.wt(&args(&["create", "acme/alpha", "feat/login"]))
        });
        assert!(again.res.is_err());

        let gone = run_in(&t, &state, Config::default(), |a| {
            a.wt(&args(&["remove", "-y", "acme/alpha", "feat/login"]))
        });
        gone.res.unwrap();
        assert!(!exists(&dir));
        assert!(!exists(&root.join(".worktrees/github.com")));
    }

    // gm rm takes the worktrees along with --force, so the work in each has
    // to be named before the question, not only the repository's own.
    #[test]
    fn remove_names_the_work_in_the_worktrees() {
        let root = TempDir::new();
        let r = Repo {
            root: root.path(),
            rel: "github.com/acme/alpha".into(),
        };
        git_repo(&r.path());
        let t = tree(&root.path());
        let login = t.worktree_dir(&r, "feat/login");
        let timeout = t.worktree_dir(&r, "fix/timeout");
        repo::add_worktree(&r.path(), &login, "feat/login").unwrap();
        repo::add_worktree(&r.path(), &timeout, "fix/timeout").unwrap();
        write(&paths::join(&login, "wip.txt"), "wip\n");

        let res = run_in(&t, "", Config::default(), |a| {
            a.remove(&args(&["-y", "acme/alpha"]))
        });
        res.res.unwrap();
        assert!(
            res.err
                .contains("2 worktrees will be removed too: feat/login (1 changed), fix/timeout"),
            "{}",
            res.err
        );
        // The repository itself is clean, so it is not called dirty.
        assert!(!res.err.contains("alpha has uncommitted"), "{}", res.err);
        assert!(!exists(&r.path()) && !exists(&login));
    }

    // The main worktree is the repository itself, and not in the list this
    // checks against.
    #[test]
    fn wt_remove_refuses_what_is_not_a_worktree() {
        let root = TempDir::new();
        let r = Repo {
            root: root.path(),
            rel: "github.com/acme/alpha".into(),
        };
        git_repo(&r.path());
        let res = run_in(&tree(&root.path()), "", Config::default(), |a| {
            a.wt(&args(&["remove", "-y", "acme/alpha", "main"]))
        });
        assert!(res.res.unwrap_err().0.contains("no worktree"));
        assert!(exists(&r.path()));
    }

    // A missing or unknown verb says what the verbs are rather than doing one.
    #[test]
    fn wt_usage() {
        let root = TempDir::new();
        for a in [&[][..], &["add"], &["create", "only-one-arg"]] {
            let r = run_in(&tree(&root.path()), "", Config::default(), |app| {
                app.wt(&args(a))
            });
            assert!(r.res.is_err(), "gm wt {a:?} was accepted");
        }
    }
}
