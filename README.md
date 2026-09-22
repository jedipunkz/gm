# gm

A [ghq](https://github.com/x-motemen/ghq)-style repository manager with a
built-in fuzzy finder. Clones land in one predictable `host/user/repo` tree,
and `Ctrl-G` jumps to any of them — or to any of their git worktrees.

## ✨ Advantages over ghq

- **The finder is built in** — no `ghq list | fzf | cd` pipeline to assemble,
  and the best match is the row at the bottom, next to the prompt where the
  cursor already rests, so the usual choice costs zero keystrokes.
- **It ranks matches itself** — a fuzzy score first, then frecency (how
  recently and how often you opened a repository) to break ties between equally
  good matches. See [Ranking](#-ranking).
- **The selected repository is described on screen** — path, remote, branch,
  working-tree status, visit count, and the last five commits with their
  branch and tag decorations — so you can tell two similarly named clones
  apart before jumping.
- **Worktrees are first-class** — `Ctrl-W` swaps the list for the git worktrees
  of the repository under the cursor; ghq only knows about clones.
- **Settings live in `gm.toml`** — roots, theme, key bindings; ghq configures
  itself only through `git config`. `$GHQ_ROOT` and `ghq.root` are still
  honored, so an existing ghq tree needs no migration.
- **`gm create` sets up `origin`**, which ghq leaves to you.

Not implemented, deliberately: Mercurial/Subversion/Darcs cloning (they are
still *listed*), bare clones, partial clones, parallel import, `--vcs`, and
`ghq.<url>.root` per-URL roots.

## 📋 Requirements

- Go 1.25 or newer, to build or `go install`
- `git` on `$PATH`
- A true-color terminal, for the finder's themes to look as intended

## 📦 Install

```sh
go install github.com/jedipunkz/gm@latest
```

## 🐚 Shell integration

`gm` with no arguments opens the finder and prints the chosen path, so it
composes with `$(...)`. One line in your rc file turns that into a `cd` on
`Ctrl-G`, or on whatever `launch_key` says.

```sh
gm shell fish | source    # ~/.config/fish/config.fish
eval "$(gm shell zsh)"    # ~/.zshrc
eval "$(gm shell bash)"   # ~/.bashrc
```

## ⌨️ Keys

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
| `Ctrl-Alt-B` (`remote_key`) | Open the remote in a browser | Open the remote in a browser |
| `Ctrl-G` | — | Back to the repositories |
| `Esc` | Quit without printing | Back to the repositories |
| `Ctrl-C` | Quit without printing | Quit without printing |

The line under the prompt lists the keys for whichever list is up, naming the
chords you configured. Going back keeps the query, the cursor and the
highlights as they were. The best match is the row at the bottom; in the
worktree list that row is the main worktree.

Everything else is ordinary text editing (`Ctrl-A`, `Ctrl-E`, `Ctrl-U` and so
on), except `Ctrl-W`, which no longer deletes the word before the cursor.

## 🧰 Commands

| Command | What it does |
|---|---|
| `gm` | Open the fuzzy finder; print the selected path |
| `gm get [-u] [-p] [--shallow] [-b <branch>] [-s] [-l] <repo>...` | Clone into the tree; `-u` updates an existing clone |
| `gm list [-p] [-e] [--unique] [<query>]` | List repositories (`-p` full paths, `-e` exact match, `--unique` shortest unambiguous name) |
| `gm rm [--dry-run] [-y] <repo>...` | Remove a repository after confirming, pruning empty parents |
| `gm create [-p] <repo>` | Create and `git init` a repository with `origin` already set |
| `gm migrate [--dry-run] [-y] [-r] <dir>...` | Move an existing clone into the tree, using its `origin` remote; `-r` searches the directory for them |
| `gm root [--all]` | Print the root directory |
| `gm shell <fish\|zsh\|bash>` | Print the `Ctrl-G` binding |

`<repo>` accepts a full URL, `git@host:user/repo.git`, `host/user/repo`,
`user/repo`, or a bare `repo` (resolved against `git config github.user`).

## ⚙️ Configuration

`~/.config/gm/gm.toml` (or `$XDG_CONFIG_HOME/gm/gm.toml`) is optional. A file
that cannot be parsed, or that holds a key `gm` does not know, is an error
rather than a file half ignored.

```toml
root         = "~/ghq"       # or ["~/ghq", "~/src"], searched in order
theme        = "tokyonight"
launch_key   = "ctrl-g"        # the shell key that opens gm
worktree_key = "ctrl-w"        # the finder key that lists worktrees
remote_key   = "ctrl-alt-b"    # the finder key that opens the remote
```

The root is resolved in this order, so an existing ghq tree works untouched:

1. `$GM_ROOT`
2. `root` in `gm.toml`
3. `git config --get-all gm.root`
4. `$GHQ_ROOT`
5. `git config --get-all ghq.root`
6. `~/ghq`

### Themes

`tokyonight` (default), `solarized-dark`, `solarized-light`, `kanagawa-wave`,
`catppuccin-latte`, `catppuccin-frappe`, `catppuccin-macchiato`,
`catppuccin-mocha`, `rose-pine`, `dracula`. An unknown name is an error listing
the valid ones.

### Key bindings

`launch_key` is the chord `gm shell` binds; `worktree_key` and `remote_key` are
the finder's own. Ctrl is written `ctrl-`, `ctrl+`, `c-` or `^`, and the
finder's two also take `alt` and `shift` after it, in any order —
`ctrl-alt-b`, `c-a-b`, `ctrl-shift-b`, `ctrl-alt-shift-b`. Anything else is an
error rather than a binding that quietly does nothing.

After changing `launch_key`, re-run `gm shell <shell>` (or restart the shell,
if you source it from your rc file). Some chords are already taken: `ctrl-r` is
reverse history search, and `ctrl-c`, `ctrl-d` and `ctrl-z` are terminal
signals. `worktree_key` and `remote_key` cannot be `ctrl-c`, `ctrl-n` or
`ctrl-p`, which the finder uses to quit and to move, and cannot both be the
same chord, and `launch_key` must be a plain Ctrl chord.

How far a chord travels depends on the terminal:

| Chord | Reaches `gm` |
|---|---|
| `ctrl-<letter>` | Everywhere |
| `ctrl-alt-<letter>` | Nearly everywhere: Alt is sent as an ESC prefix |
| `ctrl-shift-<letter>` | Only with the Kitty keyboard protocol — Ghostty, kitty, WezTerm, foot, recent Alacritty. Elsewhere it arrives as plain `ctrl-<letter>` |

## 🎯 Ranking

Typing filters by fuzzy match, scored so that a literal substring beats a
subsequence pieced together from elsewhere, and the repository name counts for
more than the user name.

Frecency only breaks ties between equally good matches. Visits are recorded in
`$XDG_STATE_HOME/gm/frecency.json` (default
`~/.local/state/gm/frecency.json`); delete it to start over.

## 📄 License

MIT
