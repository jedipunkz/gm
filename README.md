# gm

[![CI](https://github.com/jedipunkz/gm/actions/workflows/pr.yaml/badge.svg)](https://github.com/jedipunkz/gm/actions/workflows/pr.yaml)

A [ghq](https://github.com/x-motemen/ghq)-style repository manager with a
built-in fuzzy finder.

- Clones land in one predictable `host/user/repo` tree.
- `Ctrl-G` jumps to any of them, or to any of their git worktrees.

## ✨ Advantages over ghq

- **Built-in finder** — no `ghq list | fzf | cd` pipeline. The best match is
  the bottom row, next to the prompt, so the usual pick is just `Enter`.
- **Own ranking** — fuzzy score first; frecency (how recently and how often you
  opened a repository) breaks ties. See [Ranking](#-ranking).
- **Details pane** — path, remote, branch, working-tree status, visit count and
  the last three commits (with branch and tag decorations) of the selected
  repository, to tell similarly named clones apart.
- **First-class worktrees** — `Ctrl-W` lists worktrees, `Ctrl-L` branches,
  `Ctrl-J` open pull requests. `Enter` checks one out as a worktree and goes
  there. ghq only knows about clones.
- **`gm.toml`** — roots, theme and key bindings. `$GHQ_ROOT` and `ghq.root` are
  still honored, so an existing ghq tree needs no migration.
- **`gm create` sets up `origin`**, which ghq leaves to you.

Deliberately not implemented:

- Cloning Mercurial / Subversion / Darcs (existing clones are still listed)
- Bare clones and partial clones
- Parallel import
- `--vcs`
- Per-URL roots (`ghq.<url>.root`)

## 📋 Requirements

- `git` on `$PATH`
- Rust 1.95 or newer, only to build it yourself
- A true-color terminal, for the finder's themes to look as intended

## 📦 Install

```sh
brew install jedipunkz/gm/gm                                  # macOS / Linux, prebuilt binary, no Rust needed
cargo install --locked --git https://github.com/jedipunkz/gm  # from source, needs Rust
```

- The Homebrew formula comes from the [tap](https://github.com/jedipunkz/homebrew-gm).
- `gm version` prints the installed version.

## 🐚 Shell integration

Add one line to your rc file. `Ctrl-G` (or your `launch_key`) then opens the
finder and `cd`s to the pick.

```sh
gm shell fish | source    # ~/.config/fish/config.fish
eval "$(gm shell zsh)"    # ~/.zshrc
eval "$(gm shell bash)"   # ~/.bashrc
```

Without the binding, `gm` opens the finder and prints the chosen path, so it
also works inside `$(...)`.

## ⌨️ Keys

The finder has four lists. `Ctrl-W`, `Ctrl-L` and `Ctrl-J` switch to one;
pressing the same key again returns to the repositories. Filtering and the
details pane work the same in all four.

| Key | Repository list | Worktree list | Branch list | Pull request list |
|---|---|---|---|---|
| any character | Filter | Filter | Filter | Filter |
| `↑` / `Ctrl-P` | Move up | Move up | Move up | Move up |
| `↓` / `Ctrl-N` | Move down | Move down | Move down | Move down |
| `Enter` | Print the repository path and exit | Print the worktree path and exit | Check the branch out as a worktree, print its path and exit | Check the pull request out as a worktree, print its path and exit |
| `Ctrl-W` (`worktree_key`) | Show the worktrees of the selected repository | Back to the repositories | Show the worktrees | Show the worktrees |
| `Ctrl-L` (`branch_key`) | Show the branches of the selected repository | Show the branches | Back to the repositories | Show the branches |
| `Ctrl-J` (`pr_key`) | Show the open pull requests of the selected repository | Show the pull requests | Show the pull requests | Back to the repositories |
| `Ctrl-Alt-B` (`remote_key`) | Open the remote in a browser | Open the remote in a browser | Open the remote in a browser | Open the remote in a browser |
| `Ctrl-G` | — | Back to the repositories | Back to the repositories | Back to the repositories |
| `Esc` | Clear the query, then the filter, then quit | Back to the repositories | Back to the repositories | Back to the repositories |
| `Ctrl-C` | Quit without printing | Quit without printing | Quit without printing | Quit without printing |

Other keys are ordinary text editing (`Ctrl-A`, `Ctrl-E`, `Ctrl-U`, …), except
`Ctrl-W`, which no longer deletes the previous word.

### Finder behavior

- The best match is the bottom row. In the worktree list, that row is the main
  worktree.
- The hint line under the prompt lists the keys of the current list, using the
  chords you configured.
- Going back to a list keeps its query, cursor and highlights.
- `Esc` undoes one layer at a time (worktree list → query → filter) and quits
  only when nothing is left. The hint line says what it will do next.
- While `gm` waits on git or GitHub (loading pull requests, checking out, a
  `/dirty` or `/unpushed` scan), the hint line shows a spinner, what it waits
  on and the elapsed seconds. The finder stays usable meanwhile.

### Branch list

- Shows local branches, plus remote branches with no local branch of the same
  name (e.g. `origin/feat/login`). Newest commit at the bottom.
- Branches a remote has that the last `git fetch` did not bring are added at
  the top once the remotes answer (`git ls-remote`, in the background). Nothing
  is fetched until you pick one. No `gh` is needed: git uses its own
  credentials (ssh agent, credential helper). `gm` never prompts for a
  password; a remote that needs one is skipped and named on the hint line.
- `Enter` on a branch that is already checked out goes to its worktree.
- `Enter` on any other branch creates the worktree without asking, then goes
  there. A remote branch becomes a local branch that tracks it; an unfetched
  one is fetched first (that branch only).

### Pull request list

- Requires the [GitHub CLI](https://cli.github.com/) (`gh`), logged in, and new
  enough to have `gh pr checkout --worktree`.
- Shows open pull requests, newest at the bottom. Drafts are marked `[draft]`.
- Type a number such as `#42` to find one.
- `Enter` goes to the pull request's worktree. If there is none, `gh pr
  checkout` creates it first; `gh` names the branch and, for a fork, sets up
  where it pushes.
- The worktree is named after the head branch. A fork's is prefixed with its
  owner (`bob/main`), so a fork's `main` does not collide with the
  repository's own.

### Slash commands

A `/` at the start of the input types a command instead of a filter. The box
completes it as you type; `Tab` accepts the completion.

| Command | What it does |
|---|---|
| `/help` | Show the command list; `q` or `Esc` closes it |
| `/dirty` | Show only repositories with uncommitted work |
| `/unpushed` | Show only repositories with unpushed commits |
| `/create <repo>` | Create a repository, after asking; adds it to the list |
| `/create <branch>` | In the worktree list: check that branch out as a worktree |
| `/get <repo>` | Same as `gm get`, then go to the clone |
| `/remove` | Remove the selected repository, or worktree, after asking |
| `/worktrees` | Same as `Ctrl-W` |
| `/branches` | Same as `Ctrl-L` |
| `/prs` | Same as `Ctrl-J` |
| `/remote` | Same as `Ctrl-Alt-B` |

- Only a leading `/` starts a command. `acme/alpha` still filters.
- To act on a repository you searched for, end the query with `;` and type the
  command: `gm;/remove` selects `gm` and removes it. Alternatively, press `Esc`
  to empty the box (the selection is kept), then type the command.
- `/create` and `/remove`:
  - Ask first in a panel over the list; answer `y` or `n`.
  - Run without leaving the finder. The removed row disappears, the created
    one is added and selected, and the hint line reports the result.
  - The question warns about uncommitted changes that would be lost.
  - Removing a repository also removes its worktrees; the question says how
    many.
  - In the worktree list, `/create <branch>` checks the branch out, starting it
    from `HEAD` if it does not exist yet (the question tells you).
    `/remove` removes the selected worktree; the main worktree is refused (use
    `/remove` in the repository list instead).
- `/get` closes the finder and clones in the visible terminal, since cloning
  may need a progress bar or a passphrase. It then prints the clone's path, so
  the shell binding `cd`s there.
- `/dirty` and `/unpushed` share one Git status scan on first use, in parallel
  and in the background. The hint line shows the active filter; using both
  keeps repositories that satisfy both. `Esc` clears the query first, then all
  active status filters.

### Worktree location

You type only the branch name; `gm` picks the path:

```
~/gm/github.com/jedipunkz/gm/            the repository
~/gm/.worktrees/github.com/jedipunkz/gm/feat/login
```

- The leading dot in `.worktrees` is required. A worktree has a `.git` file, and
  `gm` skips dotted directories, so worktrees are not listed as repositories
  and `gm migrate` does not refuse them.
- For scripts (e.g. a dotfiles bootstrap), `gm wt create <repo> <branch>` and
  `gm wt remove <repo> <branch>` do the same without the finder.
  - `create` prints the new path: `cd (gm wt create gm feat/login)`.
  - `remove` warns about uncommitted work and asks first, unless given `-y`.

## 🧰 Sub Commands

| Command | What it does |
|---|---|
| `gm` | Open the fuzzy finder; print the selected path |
| `gm get [-u] [-p] [--shallow] [-b <branch>] [-s] [-l] <repo>...` | Clone into the tree; `-u` updates an existing clone |
| `gm list [-p] [-e] [--unique] [<query>]` | List repositories (`-p` full paths, `-e` exact match, `--unique` shortest unambiguous name) |
| `gm status [--dirty] [--unpushed] [-a] [-p]` | List unfinished work across repositories and worktrees (uncommitted changes or unpushed commits); never fetches, so it is fast and works offline |
| `gm remove [--dry-run] [-y] <repo>...` | Remove a repository and its worktrees after confirming, pruning empty parents (`gm rm` also works) |
| `gm create [-p] <repo>` | Create and `git init` a repository with `origin` already set |
| `gm wt <create\|remove> [-y] <repo> <branch>` | Add or remove a worktree from a script; the finder is better for doing it by hand |
| `gm migrate [--dry-run] [-y] [-r] <dir>...` | Move an existing clone into the tree, using its `origin` remote; `-r` searches the directory for them |
| `gm root [--all]` | Print the root directory |
| `gm shell <fish\|zsh\|bash>` | Print the `Ctrl-G` binding |

`<repo>` accepts:

- A full URL
- `git@host:user/repo.git`
- `host/user/repo`
- `user/repo`
- `repo` (the user comes from `git config github.user`)

## ⚙️ Configuration

- File: `~/.config/gm/gm.toml` (or `$XDG_CONFIG_HOME/gm/gm.toml`). Optional.
- A parse error or an unknown key is an error; the file is never half ignored.

```toml
root         = "~/ghq"       # or ["~/ghq", "~/src"], searched in order
theme        = "tokyonight"
launch_key   = "ctrl-g"        # the shell key that opens gm
worktree_key = "ctrl-w"        # the finder key that lists worktrees
branch_key   = "ctrl-l"        # the finder key that lists branches
pr_key       = "ctrl-j"        # the finder key that lists pull requests
remote_key   = "ctrl-alt-b"    # the finder key that opens the remote
```

The root is resolved in this order, so an existing ghq tree works untouched:

1. `$GM_ROOT`
2. `root` in `gm.toml`
3. `git config --get-all gm.root`
4. `$GHQ_ROOT`
5. `git config --get-all ghq.root`
6. `~/ghq`

Each root must be absolute; `~` and `~/...` expand to your home directory.
Relative roots are rejected instead of being resolved from the current working
directory. Empty entries in `$GM_ROOT` and `$GHQ_ROOT` colon-separated lists
are ignored.

### Themes

`tokyonight` (default), `solarized-dark`, `solarized-light`, `kanagawa-wave`,
`catppuccin-latte`, `catppuccin-frappe`, `catppuccin-macchiato`,
`catppuccin-mocha`, `rose-pine`, `dracula`. An unknown name is an error listing
the valid ones.

### Key bindings

| Setting | Where it works | Allowed chords | Rejected |
|---|---|---|---|
| `launch_key` | The shell, via `gm shell` | Plain Ctrl chord | Anything with `alt` or `shift` |
| `worktree_key`, `branch_key`, `pr_key`, `remote_key` | The finder | Ctrl, optionally with `alt` and `shift` | `ctrl-c`, `ctrl-n`, `ctrl-p` (quit and move); the same chord as another of the four |

- Ctrl is written `ctrl-`, `ctrl+`, `c-` or `^`. `alt` and `shift` follow in
  any order: `ctrl-alt-b`, `c-a-b`, `ctrl-shift-b`, `ctrl-alt-shift-b`.
- An invalid chord is an error, not a binding that silently does nothing.
- After changing `launch_key`, re-run `gm shell <shell>`, or restart the shell
  if your rc file sources it.
- Avoid chords the shell or terminal already uses: `ctrl-r` (reverse history
  search), `ctrl-c`, `ctrl-d`, `ctrl-z` (terminal signals).

Whether a chord reaches `gm` depends on the terminal:

| Chord | Reaches `gm` |
|---|---|
| `ctrl-<letter>` | Everywhere |
| `ctrl-alt-<letter>` | Nearly everywhere: Alt is sent as an ESC prefix |
| `ctrl-shift-<letter>` | Only with the Kitty keyboard protocol — Ghostty, kitty, WezTerm, foot, recent Alacritty. Elsewhere it arrives as plain `ctrl-<letter>` |

## 🎯 Ranking

- Typing filters by fuzzy match.
- A literal substring beats a subsequence pieced together from elsewhere.
- The repository name counts for more than the user name.
- Frecency only breaks ties between equally good matches.
- Visits are recorded in `$XDG_STATE_HOME/gm/frecency.json` (default
  `~/.local/state/gm/frecency.json`). Delete it to start over.

## 📄 License

MIT
