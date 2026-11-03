//! Starting git: one gate every process passes, and the words a failed
//! one leaves behind.

mod branch;
mod describe;
mod status;

pub use branch::{Branch, branch_exists, branches, parse_branches, valid_branch};
pub use describe::{Commit, Ref, RefKind, Status, describe, parse_commits};
pub use status::{
    State, changed_files, is_dirty, parse_status, status_map, status_of, status_with_worktrees,
};

use std::ffi::OsStr;
use std::process::{Command, Output, Stdio};
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{Error, Result, err};

/// git_command makes a Git command with a predictable configuration in tests.
/// The environment belongs to this child only: tests run in parallel without
/// changing the process environment they share.
pub(crate) fn git_command() -> Command {
    git_command_with(OsStr::new("git"))
}

/// git_command_with is git_command for a caller that supplies the executable.
/// Disabling fsmonitor keeps commands gm starts from executing a repository's
/// own hook while reading its status.
pub(crate) fn git_command_with(program: &OsStr) -> Command {
    #[cfg(test)]
    if program == OsStr::new("git") {
        GIT_SPAWNS.fetch_add(1, Ordering::Relaxed);
    }
    let mut command = Command::new(program);
    command.args(["-c", "core.fsmonitor=false"]);
    #[cfg(test)]
    isolate_git_config(&mut command);
    command
}

/// GIT_SPAWNS counts the git processes started through git_command, the one
/// gate every git spawn in production code passes. Counting is a test-build
/// concern: the release binary carries no counter and reads no knob for it.
#[cfg(test)]
static GIT_SPAWNS: AtomicUsize = AtomicUsize::new(0);

/// git_spawns_reset returns how many git processes have been started since
/// the last call and counts from zero again.
#[cfg(test)]
pub fn git_spawns_reset() -> usize {
    GIT_SPAWNS.swap(0, Ordering::Relaxed)
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
    Err(failure(&all, &out.status))
}

/// failure is what a git or gh that ran and failed said about it, out being
/// whichever of its streams the caller trusts to hold the complaint. A process
/// that said nothing is worded by its exit status.
pub(crate) fn failure(out: &[u8], status: &std::process::ExitStatus) -> Error {
    match git_message(out) {
        msg if !msg.is_empty() => Error(msg),
        _ => exit_error(status),
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

/// git_in runs git inside dir and returns its trimmed output.
pub fn git_in(dir: &str, args: &[&str]) -> Result<String> {
    let out = git_command()
        .args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()?;
    if !out.status.success() {
        return Err(failure(&out.stderr, &out.status));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{Repo, Tree, add_worktree, remove_worktree};
    use crate::testutil::{TempDir, git_repo, write};

    #[test]
    fn read_only_status_does_not_run_a_repository_fsmonitor() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = TempDir::new();
        let dir = tmp.join("repo");
        let marker = tmp.join("ran-fsmonitor");
        let hook = tmp.join("fsmonitor");
        git_repo(&dir);
        write(
            &hook,
            &format!("#!/bin/sh\n/usr/bin/touch '{}'\nexit 1\n", marker),
        );
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        crate::testutil::git(&dir, &["config", "core.fsmonitor", &hook]);

        assert!(!crate::testutil::exists(&marker));
        let _ = status_of(&dir);
        assert!(
            !crate::testutil::exists(&marker),
            "status_of ran the repository's fsmonitor"
        );

        let _ = super::describe(&dir);
        assert!(
            !crate::testutil::exists(&marker),
            "describe ran the repository's fsmonitor"
        );
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

    // git_in once kept git's complaint to itself and said only "exit status
    // 128": the user is told what git said instead.
    #[test]
    fn git_in_says_what_git_said() {
        let tmp = TempDir::new();
        let e = git_in(&tmp.path(), &["rev-parse", "HEAD"]).unwrap_err();
        assert!(e.0.contains("not a git repository"), "{e}");
    }
}
