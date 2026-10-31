# Repository instructions

- Read `docs/design.md` before touching `src/repo` or `src/cli`.
- Run `make check` before opening a PR. Never push with `--no-verify`.
- Start processes with `Command::new(program).args([...])`. Never use `sh -c` with a string built from external input such as repository names, URLs, branch names, or config values. The only shell gm starts is the user's `$SHELL` in `src/cli/get.rs` `look_in`, with no arguments.
- Remove paths only inside gm's own tree. Every new removal path must name its test in the PR.
- Use `unsafe` only with `#[allow(unsafe_code, reason = "…")]` and a `// SAFETY:` comment.
- Run long-running git calls in the finder on a task, not the UI thread. Add no per-keystroke allocations to the filter path.
- `repo` must not depend on `finder` or `cli`.
