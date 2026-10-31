## Why

<!-- What problem does this change solve? One or two sentences. -->

## What

<!-- What changed, in one or two sentences? -->

## Reference

<!-- Issue, PR, or "N/A". -->

## Checklist

- [ ] Safe: nothing outside gm's own tree is removed — name the test that shows it (delete / remove / strays behavior).
- [ ] Tests added; say which of them fail without this change.
- [ ] `make check` clean.
- [ ] A long-running git call added in the finder runs on a task, not the UI thread.
- [ ] Any new dependency is in `Cargo.lock`, and its license is acceptable.
