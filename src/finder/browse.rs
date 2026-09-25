use std::process::{Command, Stdio};

/// OS_RELEASE is where the kernel version string lives on Linux.
const OS_RELEASE: &str = "/proc/sys/kernel/osrelease";

/// is_wsl reports whether this Linux is running under WSL, where a browser is
/// a Windows program rather than something on $PATH.
fn is_wsl(distro: Option<String>, osrelease: &str) -> bool {
    if distro.is_some_and(|d| !d.is_empty()) {
        return true;
    }
    std::fs::read_to_string(osrelease).is_ok_and(|s| s.to_lowercase().contains("microsoft"))
}

/// in_path finds a program on $PATH.
fn in_path(name: &str) -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// open_url hands a URL to the platform's browser.
pub fn open_url(url: &str) -> std::io::Result<()> {
    // Detached, in every branch: the browser outlives the finder, and its
    // output must not land on the terminal the TUI is drawing on.
    let spawn = |program: &std::ffi::OsStr| {
        Command::new(program)
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|_| ())
    };
    if cfg!(target_os = "macos") {
        return spawn("open".as_ref());
    }
    if is_wsl(std::env::var("WSL_DISTRO_NAME").ok(), OS_RELEASE) {
        // A WSL distribution usually has no xdg-open. wslview is the one wslu
        // installs; explorer.exe is always there, and hands the URL to the
        // Windows default browser. Neither re-parses its argument the way
        // "cmd /c start" and "powershell -Command" would.
        if let Some(p) = ["wslview", "explorer.exe"].iter().find_map(|o| in_path(o)) {
            return spawn(p.as_os_str());
        }
    }
    spawn("xdg-open".as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn wsl_is_told_by_the_kernel_or_the_environment() {
        // A file the test controls, so the result does not depend on the
        // kernel the tests happen to run on.
        let dir = TempDir::new();
        let file = |s: &str| {
            let p = dir.join("osrelease");
            std::fs::write(&p, s).unwrap();
            p
        };
        assert!(
            is_wsl(None, &file("6.6.87.2-microsoft-standard-WSL2\n")),
            "wsl2 kernel"
        );
        assert!(!is_wsl(None, &file("6.8.0-45-generic\n")), "plain linux");
        assert!(
            is_wsl(Some("Ubuntu-26.04".into()), &file("6.8.0-45-generic\n")),
            "env alone"
        );
        assert!(!is_wsl(None, &dir.join("missing")), "no osrelease file");
    }
}
