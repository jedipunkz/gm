//! gm's command line: one table of subcommands, and the plumbing they share. A
//! new subcommand is one entry in COMMANDS plus its run function. The
//! subcommands themselves live beside it: get.rs writes to the tree, list.rs
//! and remove.rs read and take it apart.

mod get;
mod list;
mod remove;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::io::{BufRead, Write};

use crate::config::{self, Config};
use crate::finder::{self, Action, Keys};
use crate::repo::{self, History, Tree};
use crate::{Error, Result, VERSION, err};

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
