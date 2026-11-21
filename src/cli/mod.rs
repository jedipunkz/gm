mod get;
mod list;
mod migrate;
mod remove;
mod wt;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::io::{BufRead, Write};

use crate::config::{self, Config};
use crate::finder::{self, Action, Keys};
use crate::repo::{self, History, Tree};
use crate::{Error, Result, VERSION, err};

pub struct App<'a> {
    pub cfg: Config,
    pub tree: Tree,
    pub state: String,
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
}

struct Cmd {
    name: &'static str,
    aliases: &'static [&'static str],
    flags: &'static [Flag],
    args: &'static str,
    description: &'static str,
    verbs: &'static [Verb],
    run: fn(&mut App, &[String]) -> Result<()>,
}

struct Verb {
    name: &'static str,
    flags: &'static [Flag],
    args: &'static str,
}

struct Flag {
    names: &'static [&'static str],
    value: Option<&'static str>,
    usage: &'static str,
}

const GET_FLAGS: &[Flag] = &[
    Flag {
        names: &["u", "update"],
        value: None,
        usage: "update the repository if it is already cloned",
    },
    Flag {
        names: &["ssh"],
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

const CREATE_FLAGS: &[Flag] = &[Flag {
    names: &["ssh"],
    value: None,
    usage: "set the origin remote to its SSH URL",
}];

const LIST_FLAGS: &[Flag] = &[
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

const REMOVE_FLAGS: &[Flag] = &[
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

const STATUS_FLAGS: &[Flag] = &[
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
        names: &["p", "full-path"],
        value: None,
        usage: "print full paths",
    },
];

const WT_REMOVE_FLAGS: &[Flag] = &[
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

const WT_VERBS: &[Verb] = &[
    Verb {
        name: "create",
        flags: &[],
        args: "<repo> <branch>",
    },
    Verb {
        name: "remove",
        flags: WT_REMOVE_FLAGS,
        args: "<repo> <branch>",
    },
    Verb {
        name: "expire",
        flags: WT_REMOVE_FLAGS,
        args: "<repo> <days>d",
    },
];

const MIGRATE_FLAGS: &[Flag] = &[
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

const ROOT_FLAGS: &[Flag] = &[Flag {
    names: &["all"],
    value: None,
    usage: "show every root",
}];

const COMMANDS: &[Cmd] = &[
    Cmd {
        name: "get",
        aliases: &["clone"],
        flags: GET_FLAGS,
        args: "<repo>...",
        description: "",
        verbs: &[],
        run: |a, args| a.get(args),
    },
    Cmd {
        name: "list",
        aliases: &["ls"],
        flags: LIST_FLAGS,
        args: "[<query>]",
        description: "",
        verbs: &[],
        run: |a, args| a.list(args),
    },
    Cmd {
        name: "remove",
        aliases: &["rm"],
        flags: REMOVE_FLAGS,
        args: "<repo>...",
        description: "",
        verbs: &[],
        run: |a, args| a.remove(args),
    },
    Cmd {
        name: "create",
        aliases: &["new"],
        flags: CREATE_FLAGS,
        args: "<repo>|<user>/<repo>|<host>/<user>/<repo>",
        description: "",
        verbs: &[],
        run: |a, args| a.create(args),
    },
    Cmd {
        name: "status",
        aliases: &[],
        flags: STATUS_FLAGS,
        args: "",
        description: "",
        verbs: &[],
        run: |a, args| a.status(args),
    },
    Cmd {
        name: "wt",
        aliases: &[],
        flags: &[],
        args: "",
        description: "",
        verbs: WT_VERBS,
        run: |a, args| a.wt(args),
    },
    Cmd {
        name: "migrate",
        aliases: &[],
        flags: MIGRATE_FLAGS,
        args: "<directory>...",
        description: "",
        verbs: &[],
        run: |a, args| a.migrate(args),
    },
    Cmd {
        name: "root",
        aliases: &[],
        flags: ROOT_FLAGS,
        args: "",
        description: "",
        verbs: &[],
        run: |a, args| a.root(args),
    },
    Cmd {
        name: "version",
        aliases: &[],
        flags: &[],
        args: "",
        description: "",
        verbs: &[],
        run: |a, args| a.version(args),
    },
    Cmd {
        name: "shell",
        aliases: &[],
        flags: &[],
        args: "<fish|zsh|bash>",
        description: "print the launch_key binding (Ctrl-G by default)",
        verbs: &[],
        run: |a, args| a.shell(args),
    },
];

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

pub fn run(args: Vec<String>) -> Result<()> {
    let (mut out, mut err) = (std::io::stdout(), std::io::stderr());
    let res = run_with(
        &args,
        || {
            let cfg = config::load()?;
            let tree = Tree::open(&cfg)?;
            Ok((cfg, tree))
        },
        &mut out,
        &mut err,
    );
    out.flush()?;
    res
}

fn run_with(
    args: &[String],
    open: impl FnOnce() -> Result<(Config, Tree)>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("-h" | "--help" | "help") => {
            write!(out, "{}", usage())?;
            return Ok(());
        }
        Some("-v" | "--version" | "version") => {
            writeln!(out, "gm {VERSION}")?;
            return Ok(());
        }
        _ => {}
    }
    let (cfg, tree) = open()?;
    App {
        cfg,
        tree,
        state: repo::history_file().unwrap_or_default(),
        out,
        err,
    }
    .dispatch(args)
}

impl App<'_> {
    fn dispatch(&mut self, args: &[String]) -> Result<()> {
        let Some(first) = args.first() else {
            return self.finder();
        };
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

    fn history(&self) -> History {
        let mut h = History::open(&self.state);
        h.tree = self.tree.clone();
        h
    }

    fn bump(&self, path: &str) {
        let _ = self.history().bump(path);
    }

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

pub fn usage() -> String {
    let mut b = String::from("gm — keep every repository in one predictable tree.\n\nusage:\n");
    b.push_str(
        "  gm                                open the fuzzy finder (prints the chosen path)\n",
    );
    for c in COMMANDS {
        b.push_str(&format!("  gm {}", c.syntax()));
        if !c.description.is_empty() {
            b.push_str(&format!("          {}", c.description));
        }
        b.push('\n');
    }
    b.push_str(
        "
<repo> is a URL, host/user/repo, user/repo, or just repo.
The root is $GM_ROOT, gm.toml, git config gm.root, $GHQ_ROOT,
git config ghq.root, or ~/ghq.
The finder's colors come from theme in gm.toml.
gm.toml is $XDG_CONFIG_HOME/gm/gm.toml, or ~/.config/gm/gm.toml.
",
    );
    b
}

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

impl Cmd {
    fn syntax(&self) -> String {
        if !self.verbs.is_empty() {
            return self
                .verbs
                .iter()
                .map(|verb| format!("{} {}", self.name, verb.syntax()))
                .collect::<Vec<_>>()
                .join(" | ");
        }
        simple_syntax(self.name, self.flags, self.args)
    }
}

impl Verb {
    fn syntax(&self) -> String {
        simple_syntax(self.name, self.flags, self.args)
    }
}

fn simple_syntax(name: &str, flags: &[Flag], args: &str) -> String {
    let options = flags
        .iter()
        .map(|flag| {
            let spellings = flag
                .names
                .iter()
                .map(|name| {
                    let prefix = if name.len() == 1 { "-" } else { "--" };
                    let value = flag
                        .value
                        .map(|value| format!(" <{value}>"))
                        .unwrap_or_default();
                    format!("{prefix}{name}{value}")
                })
                .collect::<Vec<_>>()
                .join("|");
            format!("[{spellings}]")
        })
        .collect::<Vec<_>>()
        .join(" ");
    [name, options.as_str(), args]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn missing_usage(name: &str) -> String {
    let command = COMMANDS
        .iter()
        .find(|command| command.name == name)
        .expect("every usage error names a command");
    if command.verbs.is_empty() {
        return format!("usage: gm {}", command.syntax());
    }
    format!(
        "usage: {}",
        command
            .verbs
            .iter()
            .map(|verb| format!("gm {} {}", command.name, verb.syntax()))
            .collect::<Vec<_>>()
            .join("\n       ")
    )
}

impl App<'_> {
    fn finder(&mut self) -> Result<()> {
        let repos = self.tree.list();
        if repos.is_empty() {
            return Err(err!(
                "no repositories under {}; try `gm get <repo>`",
                self.tree.primary()
            ));
        }
        let theme = finder::lookup_theme(&self.cfg.theme)?;
        let keys = Keys::from_config(&self.cfg)?;
        let mut hist = self.history();
        let res = finder::run(&self.tree, &repos, &hist, &theme, keys)?;
        self.act(res, &mut hist)
    }

    fn act(&mut self, res: finder::Outcome, hist: &mut History) -> Result<()> {
        match res.action {
            Action::Jump => self.go_to(&res.arg, hist),
            Action::Get => {
                let args: Vec<String> = res.arg.split_whitespace().map(String::from).collect();
                let dst = self.get_last(&args)?;
                writeln!(self.out, "{dst}")?;
                Ok(())
            }
            Action::None => Ok(()),
        }
    }

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
