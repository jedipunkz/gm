# gm

[![CI](https://github.com/jedipunkz/gm/actions/workflows/pr.yaml/badge.svg)](https://github.com/jedipunkz/gm/actions/workflows/pr.yaml)

A [ghq](https://github.com/x-motemen/ghq)-style repository manager with a
built-in fuzzy finder.

- Clones land in one predictable `host/user/repo` tree.
- `Ctrl-G` jumps to any of them, or to any of their git worktrees.
- Branches and pull requests become worktrees with one `Enter`.

**Contents**

1. [Quick start](#-quick-start)
2. [What you can do](#-what-you-can-do)
3. [The finder](#-the-finder)
4. [Worktrees, branches and pull requests](#-worktrees-branches-and-pull-requests)
5. [Commands](#-commands)
6. [Configuration](#-configuration)
7. [Ranking](#-ranking)
8. [Compared with ghq](#-compared-with-ghq)

## 🚀 Quick start

1. Install:

   ```sh
   brew install jedipunkz/gm/gm                                  # macOS / Linux, prebuilt binary, no Rust needed
   cargo install --locked --git https://github.com/jedipunkz/gm  # from source, needs Rust
   ```

2. Add one line to your rc file:

   ```sh
   gm shell fish | source    # ~/.config/fish/config.fish
   eval "$(gm shell zsh)"    # ~/.zshrc
   eval "$(gm shell bash)"   # ~/.bashrc
   ```

3. Clone something: `gm get jedipunkz/gm`
4. Press `Ctrl-G`, type part of a name, press `Enter`. The shell `cd`s there.

An existing ghq tree needs no migration: `gm` reads `$GHQ_ROOT` and `ghq.root`.

### Requirements

- macOS / Linux: the finder and git's plumbing run against Unix only, so it
  cannot be built or run on Windows
- `git` on `$PATH`
- [GitHub CLI](https://cli.github.com/) (`gh`), only for the
  [pull request list](#pull-request-list)
- Rust 1.95 or newer, only to build it yourself
- A true-color terminal, for the finder's themes to look as intended

### Install notes

- The Homebrew formula comes from the [tap](https://github.com/jedipunkz/homebrew-gm).
- `gm version` prints the installed version.
- The shell binding opens the finder with `Ctrl-G` (or your
  [`launch_key`](#key-bindings)) and `cd`s to the pick. Without it, `gm` opens
  the finder and prints the chosen path, so it also works inside `$(...)`.

## 🧭 What you can do

| I want to… | In the finder | From the command line |
|---|---|---|
| Go to a repository | `Ctrl-G`, type, `Enter` | `cd (gm)` |
| Clone a repository | `/get <repo>` | `gm get <repo>` |
| Update a clone | `/get -u <repo>` | `gm get -u <repo>` |
| Create a new repository with `origin` set | `/create <repo>` | `gm create <repo>` |
| Remove a repository and its worktrees | `/remove` | `gm remove <repo>` |
| Move an existing clone into the tree | — | `gm migrate <dir>` |
| List repositories | — | `gm list` |
| Find uncommitted or unpushed work | `/dirty`, `/unpushed` | `gm status` |
| Go to a worktree | `Ctrl-W`, `Enter` | — |
| Check out a branch as a worktree | `Ctrl-L`, `Enter` | `gm wt create <repo> <branch>` |
| Check out a pull request as a worktree | `Ctrl-J`, `Enter` | — |
| Remove a worktree | `Ctrl-W`, `/remove` | `gm wt remove <repo> <branch>` |
| Open the remote in a browser | `Ctrl-Alt-B` | — |
| Print the root directory | — | `gm root` |

`<repo>` accepts:

- A full URL
- `git@host:user/repo.git`
- `host/user/repo`
- `user/repo`
- `repo` (the user comes from `git config github.user`)

## 🔎 The finder

The finder has four lists. Filtering and the details pane work the same in all
four.

| List | Opened with | Shows | `Enter` |
|---|---|---|---|
| Repositories | `gm` / `Ctrl-G` | Every clone under the roots | Print the repository path and exit |
| Worktrees | `Ctrl-W` | Worktrees of the selected repository | Print the worktree path and exit |
| Branches | `Ctrl-L` | Branches of the selected repository | Check the branch out as a worktree, print its path and exit |
| Pull requests | `Ctrl-J` | Open pull requests of the selected repository | Check the pull request out as a worktree, print its path and exit |

The details pane shows the path, remote, branch, working-tree status, visit
count and the last three commits (with branch and tag decorations) of the
selected repository, to tell similarly named clones apart.

### Keys

`Ctrl-W`, `Ctrl-L` and `Ctrl-J` switch to a list; pressing the same key again
returns to the repositories. The names in parentheses are the
[settings](#key-bindings) that rebind them.

| Key | Repository list | Worktree, branch and pull request lists |
|---|---|---|
| any character | Filter | Filter |
| `↑` / `Ctrl-P` | Move up | Move up |
| `↓` / `Ctrl-N` | Move down | Move down |
| `PgUp` / `PgDn` | A page up / down | A page up / down |
| `Enter` | Print the repository path and exit | See the table above |
| `Ctrl-W` (`worktree_key`) | Show the worktrees | Show the worktrees; in the worktree list, back to the repositories |
| `Ctrl-L` (`branch_key`) | Show the branches | Show the branches; in the branch list, back to the repositories |
| `Ctrl-J` (`pr_key`) | Show the open pull requests | Show the pull requests; in the pull request list, back to the repositories |
| `Ctrl-Alt-B` (`remote_key`) | Open the remote in a browser | Open the remote in a browser |
| `Ctrl-G` | Clear the query, then the filter | Back to the repositories |
| `Esc` | Clear the query, then the filter, then quit | Back to the repositories |
| `Ctrl-C` | Quit without printing | Quit without printing |

Other keys are ordinary text editing (`Ctrl-A`, `Ctrl-E`, `Ctrl-U`, …), except
`Ctrl-W`, which no longer deletes the previous word.

### Behavior

- The best match is the bottom row, next to the prompt, so the usual pick is
  just `Enter`. In the worktree list, that row is the main worktree.
- The hint line under the prompt lists the keys of the current list, using the
  chords you configured.
- Going back to the repository list keeps its query, cursor and highlights; the
  worktree, branch and pull request lists are rebuilt each time they are
  entered.
- `Esc` undoes one layer at a time (worktree list → query → filter) and quits
  only when nothing is left. The hint line says what it will do next.
- While `gm` waits on git or GitHub (loading pull requests, checking out, a
  `/dirty` or `/unpushed` scan), the hint line shows a spinner, what it waits
  on and the elapsed seconds. The finder stays usable meanwhile.

### Slash commands

A `/` at the start of the input types a command instead of a filter. The box
completes it as you type; `Tab` accepts the completion. `/help` lists them all.

| Command | What it does |
|---|---|
| `/help` | Show the command list; `q` or `Esc` closes it |
| `/dirty` | Show only repositories with uncommitted work |
| `/unpushed` | Show only repositories with unpushed commits |
| `/create <repo>` | Create a repository, after asking; adds it to the list |
| `/create <branch>` | In the worktree list: check that branch out as a worktree |
| `/get [flags] <repo>` | Same as [`gm get`](#gm-get), then go to the clone |
| `/remove` | Remove the selected repository, or worktree, after asking |
| `/worktrees` | Same as `Ctrl-W` |
| `/branches` | Same as `Ctrl-L` |
| `/prs` | Same as `Ctrl-J` |
| `/remote` | Same as `Ctrl-Alt-B` |

Selecting the target:

- Only a leading `/` starts a command. `acme/alpha` still filters.
- To act on a repository you searched for, end the query with `;` and type the
  command: `gm;/remove` selects `gm` and removes it. Alternatively, press `Esc`
  to empty the box (the selection is kept), then type the command.

`/create` and `/remove`:

- Ask first in a panel over the list; answer `y` or `n`.
- Run without leaving the finder. The removed row disappears, the created one
  is added and selected, and the hint line reports the result.
- The question warns about uncommitted changes that would be lost.
- Removing a repository also removes its worktrees; the question says how many.
- In the worktree list, `/create <branch>` checks the branch out, starting it
  from `HEAD` if it does not exist yet (the question tells you). `/remove`
  removes the selected worktree; the main worktree is refused (use `/remove` in
  the repository list instead).

`/get`:

- Closes the finder and clones in the visible terminal, since cloning may need
  a progress bar or a passphrase.
- Then prints the clone's path, so the shell binding `cd`s there.

`/dirty` and `/unpushed`:

- Share one Git status scan on first use, in parallel and in the background.
- The hint line shows the active filter.
- Using both keeps repositories that satisfy both.
- `Esc` clears the query first, then all active status filters.

## 🌳 Worktrees, branches and pull requests

### Worktree location

You type only the branch name; `gm` picks the path:

```
~/gm/github.com/jedipunkz/gm/            the repository
~/gm/.worktrees/github.com/jedipunkz/gm/feat/login
```

- The leading dot in `.worktrees` is required. A worktree has a `.git` file, and
  `gm` skips dotted directories, so worktrees are not listed as repositories
  and `gm migrate` does not refuse them.

### Branch list

- Shows local branches, plus remote branches with no local branch of the same
  name (e.g. `origin/feat/login`). Newest commit at the bottom.
- Branches a remote has that the last `git fetch` did not bring are added at
  the top once the remotes answer (`git ls-remote`, in the background). Nothing
  is fetched until you pick one.
- No `gh` is needed: git uses its own credentials (ssh agent, credential
  helper). `gm` never prompts for a password; a remote that needs one is
  skipped and named on the hint line.
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
- The worktree is named after the head branch. A fork's goes under
  `.forks/<owner>/` (`.forks/bob/main`), apart from the repository's own
  branches: no branch name can start with a dot, so a fork's `main` collides
  neither with the repository's `main` nor with a local branch `bob/main`.

### From scripts

For scripts (e.g. a dotfiles bootstrap), [`gm wt`](#gm-wt) does the same
without the finder:

- `gm wt create <repo> <branch>` prints the new path: `cd (gm wt create gm feat/login)`.
- `gm wt remove <repo> <branch>` warns about uncommitted work and asks first,
  unless given `-y`.

## 🧰 Commands

| Command | What it does |
|---|---|
| `gm` | Open the finder; print the selected path |
| [`gm get`](#gm-get) | Clone into the tree, or update a clone |
| [`gm list`](#gm-list) | List repositories |
| [`gm status`](#gm-status) | List unfinished work across repositories and worktrees |
| [`gm create`](#gm-create) | Create and `git init` a repository with `origin` set |
| [`gm remove`](#gm-remove) | Remove a repository and its worktrees (`gm rm` also works) |
| [`gm wt`](#gm-wt) | Add or remove a worktree from a script |
| [`gm migrate`](#gm-migrate) | Move an existing clone into the tree |
| [`gm root`](#gm-root) | Print the root directory |
| `gm shell <fish\|zsh\|bash>` | Print the `Ctrl-G` binding (see [Quick start](#-quick-start)) |
| `gm version` | Print the version (also `-v`, `--version`) |
| `gm help` | Print usage (also `-h`, `--help`) |

### gm get

`gm get [-u] [-p] [--shallow] [--no-recursive] [-b <branch>] [-s] [-l] <repo>...`

| Flag | Effect |
|---|---|
| `-u`, `--update` | Update an existing clone instead of skipping it (see below) |
| `-p` | Clone via SSH |
| `--shallow` | Shallow clone (`--depth 1`) |
| `--no-recursive` | Do not clone submodules |
| `-b`, `--branch <branch>` | Clone a single branch |
| `-s`, `--silent` | Clone quietly |
| `-l`, `--look` | Open a shell in the repository afterwards |

- Without `-u`, an existing clone is left alone.
- `-u` fetches, then fast-forwards the checked-out branch when the working tree
  is clean. A dirty tree or a diverged branch says so and stays put. It then
  updates submodules.
- If the same repository exists under two roots, `gm get` names both and stops.

### gm list

`gm list [-p] [-e] [--unique] [<query>]`

| Flag | Effect |
|---|---|
| `-p`, `--full-path` | Print full paths |
| `-e`, `--exact` | Match the query exactly |
| `--unique` | Print the shortest unambiguous name |

### gm status

`gm status [--dirty] [--unpushed] [-a] [-p]`

Lists unfinished work across repositories and worktrees: uncommitted changes,
or commits on local branches no remote has. Every branch counts, not just the
checked-out one. It never fetches, so it is fast and works offline.

| Flag | Effect |
|---|---|
| `--dirty` | Only repositories with uncommitted changes |
| `--unpushed` | Only repositories with unpushed commits |
| `-a` | Also include repositories that are clean and in sync |
| `-p` | Print full paths |

### gm create

`gm create [-p] <repo>` creates the directory, runs `git init` and sets
`origin`, then prints the path. `-p` sets `origin` to the SSH URL.

### gm remove

`gm remove [--dry-run] [-y] <repo>...` removes a repository and its worktrees
after confirming, and prunes empty parent directories. `gm rm` also works.

| Flag | Effect |
|---|---|
| `--dry-run` | Show what would be removed |
| `-y` | Skip the confirmation prompt |

### gm wt

`gm wt <create|remove> [-y] <repo> <branch>` adds or removes a worktree from a
script; the finder is better for doing it by hand. See
[From scripts](#from-scripts).

### gm migrate

`gm migrate [--dry-run] [-y] [-r] <dir>...` moves an existing clone into the
tree, at the path its `origin` remote implies.

| Flag | Effect |
|---|---|
| `--dry-run` | Show what would move |
| `-y` | Skip the confirmation prompt |
| `-r`, `--recursive` | Search the directories for clones instead of moving them |

### gm root

`gm root [--all]` prints the root directory; `--all` prints every root.

## 🔧 Configuration

- File: `~/.config/gm/gm.toml` (or `$XDG_CONFIG_HOME/gm/gm.toml`). Optional.
- A parse error or an unknown key is an error; the file is never half ignored.

```toml
root         = "~/ghq"         # or ["~/ghq", "~/src"], searched in order
theme        = "tokyonight"
launch_key   = "ctrl-g"        # the shell key that opens gm
worktree_key = "ctrl-w"        # the finder key that lists worktrees
branch_key   = "ctrl-l"        # the finder key that lists branches
pr_key       = "ctrl-j"        # the finder key that lists pull requests
remote_key   = "ctrl-alt-b"    # the finder key that opens the remote
```

### Root

The root is resolved in this order, so an existing ghq tree works untouched:

1. `$GM_ROOT`
2. `root` in `gm.toml`
3. `git config --get-all gm.root`
4. `$GHQ_ROOT`
5. `git config --get-all ghq.root`
6. `~/ghq`

- Each root must be absolute; `~` and `~/...` expand to your home directory.
- Relative roots are rejected instead of being resolved from the current
  working directory.
- Empty entries in `$GM_ROOT` and `$GHQ_ROOT` colon-separated lists are
  ignored.

### Themes

`tokyonight` (default), `solarized-dark`, `solarized-light`, `kanagawa-wave`,
`catppuccin-latte`, `catppuccin-frappe`, `catppuccin-macchiato`,
`catppuccin-mocha`, `rose-pine`, `dracula`. An unknown name is an error listing
the valid ones.

### Key bindings

| Setting | Where it works | Allowed chords | Rejected |
|---|---|---|---|
| `launch_key` | The shell, via `gm shell` | Plain Ctrl chord | Anything with `alt` or `shift`; `ctrl-m`, `ctrl-i`, `ctrl-j` (Enter, Tab, line feed) |
| `worktree_key`, `branch_key`, `pr_key`, `remote_key` | The finder | Ctrl, optionally with `alt` and `shift` | `ctrl-c`, `ctrl-n`, `ctrl-p`, `ctrl-g` (quit, move and back out); `ctrl-m`, `ctrl-i` with any modifier (Enter, Tab); the same chord as another of the four |

- Ctrl is written `ctrl-`, `ctrl+`, `c-` or `^`. `alt` and `shift` follow in
  any order: `ctrl-alt-b`, `c-a-b`, `ctrl-shift-b`, `ctrl-alt-shift-b`.
- An invalid chord is an error, not a binding that silently does nothing.
- After changing `launch_key`, re-run `gm shell <shell>`, or restart the shell
  if your rc file sources it.
- Avoid chords the shell or terminal already uses: `ctrl-r` (reverse history
  search), `ctrl-c`, `ctrl-d`, `ctrl-z` (terminal signals), `ctrl-m`, `ctrl-i`,
  `ctrl-j` (a terminal sends them as Enter, Tab and line feed).

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
- Frecency (how recently and how often you opened a repository) only breaks
  ties between equally good matches.
- Visits are recorded in `$XDG_STATE_HOME/gm/frecency.json` (default
  `~/.local/state/gm/frecency.json`). Delete it to start over.

## 🆚 Compared with ghq

What `gm` adds:

- **Built-in finder**: no `ghq list | fzf | cd` pipeline.
- **Own ranking**: fuzzy score first, frecency breaks ties. See [Ranking](#-ranking).
- **Details pane**: tells similarly named clones apart.
- **First-class worktrees**: worktrees, branches and pull requests in the
  finder. ghq only knows about clones.
- **`gm.toml`**: roots, theme and key bindings. `$GHQ_ROOT` and `ghq.root` are
  still honored, so an existing ghq tree needs no migration.
- **`gm create` sets up `origin`**, which ghq leaves to you.
- **`gm status`**: unfinished work across every clone and worktree.

Deliberately not implemented:

- Cloning Mercurial / Subversion / Darcs (existing clones are still listed)
- Bare clones and partial clones
- Parallel import
- `--vcs`
- Per-URL roots (`ghq.<url>.root`)

## 📄 License

MIT
