# gm

A [ghq](https://github.com/x-motemen/ghq)-style repository manager with a
built-in fuzzy finder. Clones land in one predictable `host/user/repo` tree,
and `Ctrl-G` jumps to any of them, the most likely one already selected.

## Requirements

- Go 1.25 or newer, to build or `go install`
- `git` on `$PATH`
- A true-color terminal, for the finder's themes to look as intended

## Install

```sh
go install github.com/jedipunkz/gm@latest
```

## Shell integration

`gm` with no arguments opens the finder and prints the chosen path on stdout;
the TUI draws on stderr, so it composes with `$(...)`. One binding per shell
turns that into a `cd` on `Ctrl-G`, or on whatever `launch_key` says.

### fish

```fish
# ~/.config/fish/config.fish
gm shell fish | source
```

### zsh

```sh
# ~/.zshrc
eval "$(gm shell zsh)"
```

### bash

```sh
# ~/.bashrc
eval "$(gm shell bash)"
```

## Keys

`Ctrl-W` swaps the repository list for the git worktrees of the repository
under the cursor, and swaps it back. Filtering, the details pane and `Enter`
work the same in both.

| Key | Repository list | Worktree list |
|---|---|---|
| any character | Filter | Filter |
| `↑` / `Ctrl-P` | Move up | Move up |
| `↓` / `Ctrl-N` | Move down | Move down |
| `Enter` | Print the repository path and exit | Print the worktree path and exit |
| `Ctrl-W` (`worktree_key`) | Show the worktrees of the selected repository | Back to the repositories |
| `Ctrl-G` | — | Back to the repositories |
| `Esc` | Quit without printing | Back to the repositories |
| `Ctrl-C` | Quit without printing | Quit without printing |

The line under the prompt lists the keys for whichever list is up. Going back
keeps the query, the cursor and the highlights as they were. The
best match is the row at the bottom, next to the prompt; in the worktree list
that row is the main worktree.

Everything else is ordinary text editing (`Ctrl-A`, `Ctrl-E`, `Ctrl-U` and so
on), except `Ctrl-W`, which no longer deletes the word before the cursor.

## Commands

| Command | What it does |
|---|---|
| `gm` | Open the fuzzy finder; print the selected path |
| `gm get [-u] [-p] [--shallow] [-b <branch>] [-s] [-l] <repo>...` | Clone into the tree; `-u` updates an existing clone |
| `gm list [-p] [-e] [--unique] [<query>]` | List repositories (`-p` full paths, `-e` exact match, `--unique` shortest unambiguous name) |
| `gm rm [--dry-run] [-y] <repo>...` | Remove a repository after confirming, pruning empty parents |
| `gm create [-p] <repo>` | Create and `git init` a repository with `origin` already set |
| `gm migrate [--dry-run] [-y] <dir>...` | Move an existing clone into the tree, using its `origin` remote |
| `gm root [--all]` | Print the root directory |
| `gm shell <fish\|zsh\|bash>` | Print the `Ctrl-G` binding |

`<repo>` accepts a full URL, `git@host:user/repo.git`, `host/user/repo`,
`user/repo`, or a bare `repo` (resolved against `git config github.user`).

## Configuration

`~/.config/gm/gm.toml` (or `$XDG_CONFIG_HOME/gm/gm.toml`) is optional; a file
that cannot be parsed — or that holds a key `gm` does not know — stops `gm`
rather than letting it clone somewhere unexpected or quietly ignore half the
file.

```toml
root         = "~/ghq"       # or ["~/ghq", "~/src"], searched in order
theme        = "tokyonight"
launch_key   = "ctrl-g"      # the shell key that opens gm
worktree_key = "ctrl-w"      # the finder key that lists worktrees
```

The root is resolved in this order, so an existing ghq tree works untouched:

1. `$GM_ROOT`
2. `root` in `gm.toml`
3. `git config --get-all gm.root`
4. `$GHQ_ROOT`
5. `git config --get-all ghq.root`
6. `~/ghq`

### Themes

- `tokyonight` (default)
- `solarized-dark`
- `solarized-light`
- `kanagawa-wave`
- `catppuccin-latte`
- `catppuccin-frappe`
- `catppuccin-macchiato`
- `catppuccin-mocha`
- `rose-pine`
- `dracula`

An unknown name is an error listing the valid ones.

### Key bindings

`launch_key` is the chord `gm shell` binds, and `worktree_key` the one that
opens the worktree list inside the finder. Both are written as `ctrl-g`,
`ctrl+g`, `c-g` or `^g`; anything else is an error rather than a binding that
quietly does nothing.

After changing `launch_key`, re-run `gm shell <shell>` (or restart the shell,
if you source it from your rc file). Some chords are already taken: `ctrl-r` is
reverse history search, and `ctrl-c`, `ctrl-d` and `ctrl-z` are terminal
signals.

`worktree_key` cannot be `ctrl-c`, `ctrl-n` or `ctrl-p`, which the finder uses
to quit and to move. The hint line under the prompt always names the chord you
configured.

## Ranking

Typing filters by fuzzy match: word boundaries and consecutive characters earn
points, gaps cost them, the repository name outweighs the user name, and the
shared `host` segment counts against a match. A literal substring always beats
a subsequence pieced together from elsewhere.

Visits are recorded in `$XDG_STATE_HOME/gm/frecency.json` (default
`~/.local/state/gm/frecency.json`) and weighted by recency the way `z` and
`zoxide` do. Frecency only breaks ties between equally good matches. Entries
for deleted repositories are pruned on write.

ghq has nothing like this: `ghq list` prints the tree in directory order and
leaves the choosing to whatever you pipe it into, so the repository you open
every day is as far from the cursor as the one you cloned once and forgot.

## Differences from ghq

- **A finder is built in.** No `ghq list | fzf | cd` pipeline to assemble, and
  the ranking knows which repositories you actually use, not just which ones
  match what you typed (see [Ranking](#ranking)).
- **The best match is at the bottom**, next to the prompt where the cursor
  already rests, so the usual choice costs zero keystrokes.
- **The details of the selected repository are on screen** — path, remote,
  branch, last commit, working-tree status, visit count — so you can tell two
  similarly named clones apart before jumping.
- **Settings live in a file.** `gm.toml` holds the roots and the theme; ghq
  configures itself only through `git config`.
- **It reads ghq's own settings.** `$GHQ_ROOT` and `ghq.root` are honored, so
  an existing tree needs no migration.
- **`gm create` sets up `origin`**, which ghq leaves to you.

Not implemented, deliberately: Mercurial/Subversion/Darcs cloning (they are
still *listed*), bare clones, partial clones, parallel import, `--vcs`, and
`ghq.<url>.root` per-URL roots.

## License

MIT
