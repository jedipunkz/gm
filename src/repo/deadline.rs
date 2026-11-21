use std::io::Read;
use std::os::unix::process::CommandExt;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use crate::{Error, Result, err};

pub(crate) const LIST_TIMEOUT: Duration = Duration::from_secs(20);
pub(crate) const FETCH_TIMEOUT: Duration = Duration::from_secs(5 * 60);

pub(super) fn run_deadline(
    cmd: &mut Command,
    timeout: Duration,
    what: &str,
    spawn_error: impl FnOnce(std::io::Error) -> Error,
) -> Result<Output> {
    // A new session has no controlling terminal, so nothing this program
    // starts can open /dev/tty to ask for a password, whatever it is.
    #[allow(
        unsafe_code,
        reason = "runs in the child between fork and exec; setsid is async-signal-safe and touches nothing of the parent"
    )]
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
                #[allow(
                    unsafe_code,
                    reason = "kill only signals the child's own process group"
                )]
                // SAFETY: kill only signals the child's own process group.
                unsafe {
                    libc::kill(-pgid, libc::SIGKILL)
                };
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
            #[allow(
                unsafe_code,
                reason = "kill only signals the child's own process group"
            )]
            // SAFETY: kill only signals the child's own process group.
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

    #[test]
    fn a_background_process_does_not_outlast_the_deadline() {
        let mut c = Command::new("sh");
        c.args(["-c", "sleep 6 & exit 0"]);
        let t = Instant::now();
        run_deadline(&mut c, Duration::from_secs(1), "sh", Error::from).unwrap();
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    }

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
