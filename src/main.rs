//! gm keeps every repository in one predictable tree, and jumps between them.
//! The work lives in the modules; this is only the entry point and the few
//! helpers every one of them shares.

mod cli;
mod config;
mod finder;
mod repo;

use std::fmt;

/// VERSION is stamped in at release time through the GM_VERSION environment
/// variable of the build.
const VERSION: &str = match option_env!("GM_VERSION") {
    Some(v) => v,
    None => "dev",
};

fn main() {
    // `gm list | head` closes the pipe early. Go let SIGPIPE end the process
    // quietly; Rust ignores it and every write fails with "Broken pipe"
    // instead, which gm would report as an error.
    // SAFETY: called before any thread exists, and SIG_DFL is always valid.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    // Arguments are read lossily: a name that is not UTF-8 reaches the command
    // mangled rather than stopping gm before it starts.
    let args = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if let Err(err) = cli::run(args) {
        // A usage error has already shown the usage; nothing more to say.
        if let Some(code) = cli::exit_code(&err) {
            std::process::exit(code);
        }
        eprintln!("gm: {err}");
        std::process::exit(1);
    }
}

/// Error is what gm reports: one line, already worded for the user. Every
/// failure is a message, as it was in the Go version, so there is no error
/// hierarchy to walk.
#[derive(Debug, Clone, PartialEq)]
pub struct Error(pub String);

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for Error {
    fn from(s: String) -> Self {
        Error(s)
    }
}

impl From<&str> for Error {
    fn from(s: &str) -> Self {
        Error(s.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error(e.to_string())
    }
}

/// err builds an Error in place, the way fmt.Errorf did.
macro_rules! err {
    ($($arg:tt)*) => { $crate::Error(format!($($arg)*)) };
}
pub(crate) use err;

/// plural counts a thing in the words for it.
pub fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// Paths are carried as strings, the way the Go version carried them: every
/// one of them is compared, printed and stored in JSON, and a lossy name is the
/// price of a directory that is not UTF-8.
pub mod paths {
    /// home is $HOME, which is what os.UserHomeDir read on Unix.
    pub fn home() -> crate::Result<String> {
        match std::env::var("HOME") {
            Ok(h) if !h.is_empty() => Ok(h),
            _ => Err("$HOME is not defined".into()),
        }
    }

    /// join puts two paths together and cleans the result, like filepath.Join.
    pub fn join(a: &str, b: &str) -> String {
        if a.is_empty() {
            return clean(b);
        }
        if b.is_empty() {
            return clean(a);
        }
        clean(&format!("{a}/{b}"))
    }

    /// dir is everything but the last element, like filepath.Dir.
    pub fn dir(p: &str) -> String {
        match p.rfind('/') {
            Some(i) => clean(&p[..=i]),
            None => ".".to_string(),
        }
    }

    /// clean is filepath.Clean: the shortest lexical equivalent of a path.
    pub fn clean(p: &str) -> String {
        if p.is_empty() {
            return ".".to_string();
        }
        let rooted = p.starts_with('/');
        let mut out: Vec<&str> = Vec::new();
        for part in p.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    if out.last().is_some_and(|l| *l != "..") {
                        out.pop();
                    } else if !rooted {
                        out.push("..");
                    }
                }
                _ => out.push(part),
            }
        }
        let body = out.join("/");
        match (rooted, body.is_empty()) {
            (true, _) => format!("/{body}"),
            (false, true) => ".".to_string(),
            (false, false) => body,
        }
    }

    /// abs makes a path absolute against the working directory.
    pub fn abs(p: &str) -> crate::Result<String> {
        if p.starts_with('/') {
            return Ok(clean(p));
        }
        let wd = std::env::current_dir()?;
        Ok(join(&wd.to_string_lossy(), p))
    }

    /// exists reports whether anything is at p, the way os.Stat succeeding did.
    pub fn exists(p: &str) -> bool {
        std::fs::metadata(p).is_ok()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn clean_matches_filepath_clean() {
            for (in_, want) in [
                ("", "."),
                ("/", "/"),
                ("a/b/../c", "a/c"),
                ("/a//b/./c/", "/a/b/c"),
                ("/../x", "/x"),
                ("../../x", "../../x"),
                ("a/..", "."),
            ] {
                assert_eq!(clean(in_), want, "clean({in_:?})");
            }
            assert_eq!(
                join("/r/.worktrees/h/u/r", "../../escaped"),
                "/r/.worktrees/h/escaped"
            );
            assert_eq!(dir("/a/b/c"), "/a/b");
            assert_eq!(dir("/a"), "/");
        }
    }
}

#[cfg(test)]
pub mod testutil {
    //! What the tests share: temporary directories and real repositories.

    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// TempDir is a directory removed when the test is done with it.
    #[derive(Debug)]
    pub struct TempDir(PathBuf);

    impl Default for TempDir {
        fn default() -> Self {
            Self::new()
        }
    }

    impl TempDir {
        #[allow(clippy::new_without_default)]
        pub fn new() -> TempDir {
            static N: AtomicUsize = AtomicUsize::new(0);
            let base = std::env::temp_dir().join(format!(
                "gm-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&base).unwrap();
            TempDir(base)
        }

        pub fn path(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }

        pub fn join(&self, rel: &str) -> String {
            crate::paths::join(&self.path(), rel)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// git runs one git command in dir and fails the test if it does not work.
    pub fn git(dir: &str, args: &[&str]) -> String {
        let out = crate::repo::git_command()
            .args(["-c", "user.email=t@e.x", "-c", "user.name=t"])
            .args(args)
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// git_repo makes a repository with one commit, which is what git wants
    /// before it hands out a worktree.
    pub fn git_repo(dir: &str) {
        std::fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-q", "-b", "main"]);
        git(dir, &["commit", "-q", "--allow-empty", "-m", "init"]);
    }

    /// mkdir makes a directory and its parents.
    pub fn mkdir(p: &str) {
        std::fs::create_dir_all(p).unwrap();
    }

    /// write puts a file at p.
    pub fn write(p: &str, body: &str) {
        std::fs::write(p, body).unwrap();
    }

    /// exists reports whether anything is at p.
    pub fn exists(p: &str) -> bool {
        std::path::Path::new(p).symlink_metadata().is_ok()
    }
}
