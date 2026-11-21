fn main() {
    // `gm list | head` closes the pipe early. Rust ignores SIGPIPE and every
    // write fails with "Broken pipe" instead, which gm would report as an error.
    #[allow(
        unsafe_code,
        reason = "SIG_DFL is a constant, always-valid handler, and nothing else has run yet"
    )]
    // SAFETY: called before any thread exists, and SIG_DFL is always valid.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let args = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    if let Err(err) = gm::cli::run(args) {
        if let Some(code) = gm::cli::exit_code(&err) {
            std::process::exit(code);
        }
        eprintln!("gm: {err}");
        std::process::exit(1);
    }
}
