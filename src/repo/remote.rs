use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::git::{Branch, exit_error, git_in, git_message, remotes, valid_branch};
use crate::{Error, Result, err};

/// Timeouts for the calls that go to a remote. Listing moves only ref names;
/// fetching one branch moves its objects, which can take a while.
const LIST_TIMEOUT: Duration = Duration::from_secs(20);
const FETCH_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// git_remote runs git for a call that talks to a remote, with every way it
/// could ask for a password shut. The finder owns the terminal: a prompt would
/// be drawn over it and wait for input that never comes. What does not need
/// typing — an ssh agent, a credential helper — still works.
fn git_remote(timeout: Duration, dir: &str, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // ssh prompts on the controlling terminal, not on stdin; batch mode makes
    // it fail instead. A user's own ssh command is left alone.
    if std::env::var_os("GIT_SSH_COMMAND").is_none()
        && git_config(dir, "core.sshCommand").is_empty()
    {
        cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    // A new session has no controlling terminal, so nothing git starts can
    // open /dev/tty to ask, whatever it is.
    // SAFETY: setsid is async-signal-safe and touches nothing of the parent.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    let mut child = cmd.spawn()?;
    let drain = |r: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut buf);
            }
            buf
        })
    };
    let stdout = drain(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let stderr = drain(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );

    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(err!("git {} took longer than {}", args[0], human(timeout)));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let (out, errout) = (
        stdout.join().unwrap_or_default(),
        stderr.join().unwrap_or_default(),
    );
    if !status.success() {
        return match git_message(&errout) {
            msg if !msg.is_empty() => Err(Error(msg)),
            _ => Err(exit_error(&status)),
        };
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// human spells a timeout the way Go's Duration.String did: 20s, 5m0s.
fn human(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        format!("{s}s")
    } else {
        format!("{}m{}s", s / 60, s % 60)
    }
}

fn git_config(dir: &str, key: &str) -> String {
    git_in(dir, &["config", "--get", key]).unwrap_or_default()
}

/// remote_branches asks every remote which branches it has right now, without
/// fetching anything. The answer holds only what the last fetch did not bring:
/// a branch already in refs/remotes is in branches, with its date. A remote
/// that cannot be reached is skipped, and the first such error is returned
/// beside what the others said.
pub fn remote_branches(dir: &str) -> (Vec<Branch>, Option<Error>) {
    let (names, _) = remotes(dir);
    let fetched: Vec<String> = git_in(
        dir,
        &["for-each-ref", "--format=%(refname:short)", "refs/remotes"],
    )
    .map(|out| out.split('\n').map(str::to_string).collect())
    .unwrap_or_default();
    let mut list = Vec::new();
    let mut first = None;
    for r in &names {
        match git_remote(LIST_TIMEOUT, dir, &["ls-remote", "--heads", r]) {
            Ok(out) => list.extend(
                parse_ls_remote(&out, r)
                    .into_iter()
                    .filter(|b| !fetched.contains(&b.remote)),
            ),
            Err(e) => {
                first.get_or_insert(err!("{r}: {e}"));
            }
        }
    }
    (list, first)
}

/// parse_ls_remote reads "<hash>\trefs/heads/<name>" lines into branches of
/// the remote r that have not been fetched.
pub fn parse_ls_remote(out: &str, r: &str) -> Vec<Branch> {
    out.split('\n')
        .filter_map(|l| {
            let (_, reference) = l.split_once('\t')?;
            let name = reference
                .strip_prefix("refs/heads/")
                .filter(|n| !n.is_empty())?;
            Some(Branch {
                name: name.to_string(),
                remote: format!("{r}/{name}"),
                unfetched: true,
            })
        })
        .collect()
}

/// fetch_branch brings one branch of a remote into refs/remotes, so a worktree
/// can start from it. The name must pass valid_branch: it goes into a refspec.
pub fn fetch_branch(dir: &str, b: &Branch) -> Result<()> {
    let r = b.remote.strip_suffix(&format!("/{}", b.name));
    let Some(r) = r.filter(|_| valid_branch(&b.name)) else {
        return Err(err!("{} is not a remote branch", b.remote));
    };
    let refspec = format!("refs/heads/{}:refs/remotes/{}", b.name, b.remote);
    git_remote(FETCH_TIMEOUT, dir, &["fetch", "--no-tags", r, &refspec]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::git::{add_worktree_from, git_quiet};
    use crate::testutil::{TempDir, git_repo};

    #[test]
    fn parse_ls_remote_keeps_the_heads() {
        let out = "abc\trefs/heads/main\nabd\trefs/heads/feat/x\nabe\trefs/tags/v1\ngarbage\n";
        let b = |name: &str, remote: &str| Branch {
            name: name.into(),
            remote: remote.into(),
            unfetched: true,
        };
        assert_eq!(
            parse_ls_remote(out, "up"),
            vec![b("main", "up/main"), b("feat/x", "up/feat/x")]
        );
    }

    // What a remote has that the last fetch did not bring is found, and one of
    // them is fetched into a worktree that tracks it.
    #[test]
    fn remote_branches_finds_the_unfetched() {
        let tmp = TempDir::new();
        let upstream = tmp.join("upstream");
        git_repo(&upstream);
        git_in(&upstream, &["branch", "old"]).unwrap();
        let clone = tmp.join("clone");
        git_quiet(&["clone", "-q", &upstream, &clone]).unwrap();
        // Pushed after the clone: only the remote knows about it.
        git_in(&upstream, &["branch", "feat/new"]).unwrap();

        let (bs, e) = remote_branches(&clone);
        assert_eq!(e, None);
        assert_eq!(
            bs,
            vec![Branch {
                name: "feat/new".into(),
                remote: "origin/feat/new".into(),
                unfetched: true
            }]
        );

        fetch_branch(&clone, &bs[0]).unwrap();
        let dir = tmp.join("wt");
        add_worktree_from(&clone, &dir, "feat/new", "origin/feat/new").unwrap();
        assert_eq!(
            git_in(&dir, &["rev-parse", "--abbrev-ref", "feat/new@{upstream}"]).unwrap(),
            "origin/feat/new"
        );

        // A refspec is never built out of a name git would not take.
        let bad = Branch {
            name: "a:b".into(),
            remote: "origin/a:b".into(),
            unfetched: true,
        };
        assert!(fetch_branch(&clone, &bad).is_err());
    }

    // The remote it could not reach is named, and nothing is asked on the way.
    #[test]
    fn remote_branches_names_the_unreachable() {
        let tmp = TempDir::new();
        let clone = tmp.join("clone");
        git_repo(&clone);
        git_in(&clone, &["remote", "add", "origin", &tmp.join("gone")]).unwrap();
        let (bs, e) = remote_branches(&clone);
        assert!(
            bs.is_empty() && e.as_ref().is_some_and(|e| e.0.starts_with("origin: ")),
            "{bs:?} {e:?}"
        );
    }
}
