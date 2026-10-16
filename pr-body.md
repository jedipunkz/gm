## Why

Every path gm creates or removes starts as a string the user typed, a URL a remote reported, or a line in `gm.toml`. `docs/design.md` says `normalize_url` "is the one place that has to reject a reference before it becomes a path". The example-based tests cover the cases someone thought of; #244 asks for property tests so the rest is covered too — every input proptest can invent, run in plain `cargo test` on stable, so CI and the coverage job pick it up with no workflow change.

## What

`proptest = "1.12.0"` joins the dev-dependencies, and #244's invariants are now `proptest!` tests, 256 cases a head, committed beside the example tests of the modules they shield (`#[cfg(test)] mod proptests`):

| target | tests | invariants |
| --- | --- | --- |
| `repo::url::normalize_url` + `rel_path_of`, `browse_url`, `Url::parse` | `normalize_and_browse_never_panic`, `an_accepted_reference_lands_safely_under_the_root`, `parse_round_trips`, `browse_url_is_https_when_it_succeeds` | never panic; an accepted reference lands under the root with no `.` or `..` segment, no empty segment, nothing absolute, and no space or control character in the relative path; `parse` round-trips through `Display`; `browse_url` starts with `https://` |
| `config::parse`, `config::parse_chord` | `parse_never_panics_on_a_garbled_body`, `parse_never_panics_on_any_text`, `a_bound_chord_answers_to_what_it_parses_from` | never panic on arbitrary TOML or text; an Ok chord answers to the key it prints and ends on one lowercase letter |
| `finder::score::find`, `substring_match` | `find_answers_well_formed_matches`, `substring_match_points_at_the_query` | never panic on any Unicode; a match is one position per pattern character, landing on the letters it matched, in range, on char boundaries, strictly ascending; `find` sorts best first, stable |

Input strategies weight towards the forms each function is meant to accept — URL-shaped pieces for references (any garble rides in on the pieces, dot segments and stray slashes on purpose), known and unknown gm.toml keys with values of every kind (strings, lists, bare words, numbers), chord spellings with the modifiers in either order, and plain `any::<String>()` for the no-panic cases — so the Ok branches are exercised, not just the rejections.

One line of the issue's table is deliberately not a proptest: a chord that is Enter or Tab in disguise (#152) is not judged by `config::parse_chord` itself — the rejection happens above it, in `finder::check` and the shell snippets, which already hold unit tests for it. The new property documents the split: parse reads, the callers judge.

Runtime, here: 9 tests × 256 cases = 2 304 inputs, ~0.35 s of the ~4.3 s suite. Persisted failures re-run forever from the committed `proptest-regressions/`.

### Found while writing these (bug fixed in `rel_path_of`)

On the first run the property caught a real landing-place bug:

```
Test failed: empty segment in "github.com/a/" from "https://github.com/a/.git"
```

`https://github.com/a/.git` is an accepted reference, but `rel_path_of` stripped the `.git` suffix and left the trailing slash in, so the repository landed under the root in a directory spelled `github.com/a/` — a segment without a name. The same class bites a stray `//` in the middle: `https://github.com/a//r` would land as `github.com/a//r`. The fix collects the host and the non-empty path segments and joins them, so both cases land as `github.com/a/r`; nothing else changes, the example tests hold. The failing input lives in `proptest-regressions/repo/url.txt` (annotated), committed as #244 asks, and is re-tried on every run of the suite.

### Cargo-fuzz (skipped; possible follow-up)

#244's fuzzing item is optional. A `fuzz/` crate with `cargo-fuzz` targets for the same five functions would add nightly, coverage-guided runs against the same invariants; proptest covers the stable-toolchain floor we gate on.

## Checks

- `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo test --locked` clean locally.
- `cargo deny check advisories licenses sources` clean locally (cargo-deny 0.20.2); proptest's MIT/Apache licenses sit inside the allow-list, `Cargo.lock` carries it.
- Coverage locally (`cargo llvm-cov --locked --lcov`, the workflow's own summarizer): lines 92.89 (6202/6677), up from main's 92.80 (6136/6612), above the 92.79 floor; functions 91.60 — the floor gates lines only.
- Tests that fail without this change: the property `an_accepted_reference_lands_safely_under_the_root` fails without the `rel_path_of` fix.

## Reference

- Closes #244.
- Original idea: https://jedipunkz.rocks/post/fuzz-fish-guardrails/
- https://proptest-rs.github.io/proptest/
