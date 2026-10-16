//! gm keeps every repository in one predictable tree, and jumps between them.
//! The work lives in the library's modules; this is only the entry point.

fn main() {
    // `gm list | head` closes the pipe early. Go let SIGPIPE end the process
    // quietly; Rust ignores it and every write fails with "Broken pipe"
    // instead, which gm would report as an error.
    #[allow(
        unsafe_code,
        reason = "SIG_DFL is a constant, always-valid handler, and nothing else has run yet"
    )]
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
    if let Err(err) = gm::cli::run(args) {
        // A usage error has already shown the usage; nothing more to say.
        if let Some(code) = gm::cli::exit_code(&err) {
            std::process::exit(code);
        }
        eprintln!("gm: {err}");
        std::process::exit(1);
    }
}
