## Why

<!-- What problem does this change solve? One or two sentences. -->

## What

<!-- What changed, in one or two sentences? -->

## Reference

<!-- Issue, PR, or "N/A". -->

## Checklist

- [ ] Safe: nothing outside gm's own tree is removed — name the test that shows it (delete / remove / strays behavior).
- [ ] Every new `unsafe` block has a `// SAFETY:` comment.
- [ ] External input is passed through `Command` argument lists, not interpolated into shell commands.
- [ ] Tests added; say which of them fail without this change.
- [ ] `make check` clean.
- [ ] A long-running git call added in the finder runs on a task, not the UI thread.
- [ ] `repo` does not depend on `finder` or `cli`.
- [ ] No new per-keystroke allocations in the finder filter path.
- [ ] Any new dependency is in `Cargo.lock`, and its license is acceptable.
