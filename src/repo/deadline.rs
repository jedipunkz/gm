//! Child processes that talk to the network run under a deadline: they get a
//! session of their own, so nothing they start can prompt on the terminal, and
//! the whole process group is killed when the deadline passes. git's remote
//! calls and gh's both come through here, so one hung program cannot hang gm.

use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use crate::{Error, Result, err};

/// Timeouts for the calls that go over the network. Listing moves nothing
/// larger than names; a call that pulls objects — a fetch, a worktree checked
/// out from a pull request — gets the fetching scale, because objects are
/// where the size is.
pub(crate) const LIST_TIMEOUT: Duration = Duration::from_secs(20);
pub(crate) const FETCH_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// run_deadline runs cmd with stdin closed and its pipes captured. If the
/// deadline passes first the process group is killed, and the error names
/// what ran and for how long. spawn_error shapes the error of a program that
/// could not be started, so each caller says why in its own words ("gh is not
/// installed").
pub(super) fn run_deadline(
    cmd: &mut Command,
    timeout: Duration,
    what: &str,
    spawn_error: impl FnOnce(std::io::Error) -> Error,
) -> Result<Output> {
    // A new session has no controlling terminal, so nothing this program
    // starts can open /dev/tty to ask for a password, whatever it is.
    // SAFETY: setsid is async-signal-safe and touches nothing of the parent.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }

    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(spawn_error)?;
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

    let pgid = child.id() as libc::pid_t;
    let deadline = Instant::now() + timeout;
    let mut status = None;
    loop {
        if status.is_none() {
            status = child.try_wait()?;
            if status.is_some() {
                // Nothing the program started outlives it: a process it left
                // in the background still holds the pipes, and the reads
                // below would wait for that one instead.
                unsafe { libc::kill(-pgid, libc::SIGKILL) };
            }
        }
        // A process that left the group still has the pipes, so the reads
        // are under the deadline too.
        if let Some(status) = status
            && stdout.is_finished()
            && stderr.is_finished()
        {
            return Ok(Output {
                status,
                stdout: stdout.join().unwrap_or_default(),
                stderr: stderr.join().unwrap_or_default(),
            });
        }
        if Instant::now() >= deadline {
            if unsafe { libc::kill(-pgid, libc::SIGKILL) } != 0 && status.is_none() {
                let _ = child.kill();
            }
            if status.is_none() {
                let _ = child.wait();
            }
            return Err(err!("{what} took longer than {}", human(timeout)));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// human spells a timeout the way Go's Duration.String did: 20s, 5m0s.
pub(super) fn human(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        format!("{s}s")
    } else {
        format!("{}m{}s", d.as_secs() / 60, d.as_secs() % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The bug where the deadline stopped at the child's exit: a process it
    // left in the background, holding the pipes, kept gm waiting past it.
    #[test]
    fn a_background_process_does_not_outlast_the_deadline() {
        let mut c = Command::new("sh");
        c.args(["-c", "sleep 6 & exit 0"]);
        let t = Instant::now();
        run_deadline(&mut c, Duration::from_secs(1), "sh", Error::from).unwrap();
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    }

    // One that left the group, out of the kill's reach, still meets the
    // deadline.
    #[test]
    fn a_process_that_left_the_group_meets_the_deadline() {
        let mut c = Command::new("sh");
        c.args([
            "-c",
            "perl -MPOSIX -e 'POSIX::setsid(); sleep 6' & sleep 0.5; exit 0",
        ]);
        let t = Instant::now();
        let e = run_deadline(&mut c, Duration::from_secs(1), "sh", Error::from).unwrap_err();
        assert!(e.0.contains("sh took longer than 1s"), "{e}");
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    }
}
