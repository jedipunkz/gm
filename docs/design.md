# Design principles

These are the rules gm's own code follows, written down so a new contributor
does not have to read every comment to find them. Each principle names the
module where it lives today.

## One tree, predictable places

Every repository lives at a single shape: `<root>/<host>/<user>/<repo>`, and
the checkouts gm makes for it live at `<root>/.worktrees/<host>/<user>/<repo>/<branch>`
(`repo/git.rs`, `WORKTREE_ROOT`). The leading dot matters: the walk that lists
repositories never descends into a dotted directory, so gm's own worktree
storage stays out of the repository list. Where a URL lands under a root is
decided by exactly one function, `repo/url.rs`'s `rel_path_of`, and every
shorthand (`git@…:`, `u/r`, bare `r`) is turned into a URL by exactly one
normalizer, `normalize_url`, whose dot-segment and control-character checks are
the one place that has to reject a reference before it becomes a path.

## gm only ever removes what it owns

`repo/mod.rs`'s `delete` has one story: it removes the repository, every
worktree checked out of it, and the empty directories above them — and only
inside gm's tree. A worktree entry points wherever the repository's own `.git`
names, and nothing ties that to gm's tree, so a checkout outside the worktree
directory is left where it is: `delete` returns it, and `gm remove` prints
where it stayed. Ownership is checked by resolving symlinks on both sides
(`is_under`, `same_path`), so a path inside the tree that resolves outside is
outside, and deleted paths land where they resolve — that resolution is also
why the macOS `/var` versus `/private/var` difference is handled in one place
rather than assumed away. Anything under the worktree directory that is not a
checkout git knows about, `strays` names first; `delete` then takes the whole
directory, so nothing there disappears unannounced. Git refusing one checkout
must not block a removal: the repository is going anyway, so git's lost
bookkeeping does not survive, but only inside gm's own worktree directory.

## Refuse, never guess

When the answer is ambiguous, gm stops instead of picking. `Tree::resolve`
errors on a query matching more than one repository, so a wrong repository is
never removed or moved. A repository that already lives under a second root is
not cloned again as an empty copy — `refuse_existing` and `existing_paths`
demand that the caller say which root it means. A path that no longer resolves
counts as outside: `is_under` treats a missing directory as "outside" and
leaves the one worktree git refused rather than deleting it on a guess.

## The machine does what is confirmable; the human confirms what is not

The split runs through the whole of `repo` and `cli`: `repo::delete` asks
nothing and checks nothing — "It asks nothing: the caller has already
confirmed." The CLI side owns the question. `gm remove`, `gm wt remove`,
`gm wt expire` and `gm wt mv` all print what they are about to take —
warnings for uncommitted work, for the worktrees going along, for strays —
before asking, and `-y` ("skip the confirmation prompt") is the only way
around the question. A closed stdin answers "no" (`App::confirm`,
`cli/mod.rs`). The finder's own create/remove sit behind the same pattern:
they confirm before they act, and their result is carried out after the
alternate screen is gone.

## The finder shows state; the CLI mutates it

The finder is a viewer. It ranks, filters and describes; the changes it
carries out under confirmation (create, remove, check out) end in an
`Outcome` (`finder/mod.rs`), and `cli/mod.rs`'s `act` mutates the tree once
the terminal is back — which is also where clone progress, password prompts
and confirmations belong. A visitor's repository history is recorded by
whatever fetched, made or moved it, and by the finder only for a chosen row,
so a visit is never counted twice.

## Nothing long-running on the UI thread

The finder draws every frame itself; git and gh never run in it. Updates are
messages; work that talks to git is a `Task` (`finder/mod.rs`) spawning off
the thread and coming back as a `Msg`. Asking git about the selected
repository is armed behind a delay (`STATUS_DELAY`) and re-checked against
the current selection before the call:
`probe` will not fire for a row the cursor has already left, so scrolling
costs nothing. A new long-running git call in the finder must take the same
shape.

## Tests run real git

`testutil` in `lib.rs` makes repositories with real `git init` and real
commits (`git_repo`), and state questions — worktrees, reflogs, dirty
status — are answered by real git in a temporary directory, not by stubs.
Where git itself is the seam, the environment is isolated per child process
(`repo/git.rs`'s `git_command`) so tests run in parallel without touching
each other's configuration. Run-state means something here; a fake would
test the fake.

## One of everything

One URL normalizer (`repo/url.rs`). One rel-path form, the slash-separated
`rel` on `Repo`. One `Error` type whose value is the message the user reads,
already worded at the point of failure (`lib.rs`) — no error hierarchy to
walk, no wrapping. Paths are carried as plain strings with `paths::join` and
`paths::clean` doing the joining; a lossy name for a non-UTF-8 directory is
the recorded price, and conversion to `OsString` waits for a case where it
matters.

## How the code explains itself

Comments here do not narrate; they state rules. A doc comment on a function
carries the invariant the function keeps and the bug it keeps it against —
"`delete` removes a repository, every worktree checked out of it…", "The
expensive parts happen once", "A symlinked checkout resolves to wherever it
points, which is the honest answer about where deleting it would reach."
Multi-line, English, and placed where the rule is decided, not where it is
felt. A change that changes a rule changes the comment.
