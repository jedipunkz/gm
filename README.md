<div align="center">
  
# gm

[![CI](https://github.com/jedipunkz/gm/actions/workflows/pr.yaml/badge.svg)](https://github.com/jedipunkz/gm/actions/workflows/pr.yaml)
  <a href="https://github.com/jedipunkz/gm/releases/latest"><img src="https://img.shields.io/github/v/release/jedipunkz/gm?style=flat-square" alt="Latest release"></a>
  
A [ghq](https://github.com/x-motemen/ghq)-style repository manager with a
built-in fuzzy finder.
</div>

## ✨ Features

- Clones land in one `host/user/repo` tree.
- `Ctrl-G` jumps to any clone or worktree.
- Branches and pull requests open as worktrees.



## 🚀 Quick start

```sh
brew install jedipunkz/gm/gm
```

Add to your rc file:

```sh
gm shell fish | source    # ~/.config/fish/config.fish
eval "$(gm shell zsh)"    # ~/.zshrc
eval "$(gm shell bash)"   # ~/.bashrc
```

Then `gm get jedipunkz/gm`, press `Ctrl-G`, type, `Enter`: the shell `cd`s there.

- An existing ghq tree works as is (`$GHQ_ROOT`, `ghq.root`).
- Without the binding, `gm` prints the chosen path, so `$(gm)` works.

| Requirement | When |
|---|---|
| macOS / Linux (not Windows: the finder and git plumbing are Unix only) | Always |
| `git` on `$PATH` | Always |
| [`gh`](https://cli.github.com/), logged in, with `gh pr checkout --worktree` | Pull request list |
| True-color terminal | Themes |

## 🔎 Finder

| List | Open | Rows | `Enter` |
|---|---|---|---|
| Repositories | `gm` / `Ctrl-G` | Clones under the roots | Go there |
| Worktrees | `Ctrl-W` | Worktrees of the selected repository | Go there |
| Branches | `Ctrl-L` | Branches of the selected repository | Go to its worktree, created if missing |
| Pull requests | `Ctrl-J` | Open pull requests of the selected repository | Go to its worktree, created if missing |

- "Go there" prints the path and exits; the shell binding `cd`s.
- The best match is the bottom row, next to the prompt. In the worktree list it is the main worktree.
- Details pane: path, remote, branch, working-tree status, visit count, last three commits (with branch and tag decorations).
- Hint line: the current list's keys (your chords), what `Esc` does next, and a spinner with elapsed seconds while git or GitHub works. The finder stays usable meanwhile.
- The repository list keeps its query, cursor and highlights when you return. The other lists are rebuilt each time.

### Keys

| Key | Repository list | Other lists |
|---|---|---|
| Any character | Filter | Filter |
| `↑` `Ctrl-P` / `↓` `Ctrl-N` | Move | Move |
| `PgUp` / `PgDn` | Page | Page |
| `Ctrl-W` / `Ctrl-L` / `Ctrl-J` | Open that list | Open that list; the current list's own key goes back |
| `Ctrl-Alt-B` | Open the remote in a browser | Same |
| `Ctrl-G` | Clear query, then filter | Back to repositories |
| `Esc` | Clear query, then filter, then quit | Back to repositories |
| `Ctrl-C` | Quit, print nothing | Same |

- Other keys edit text (`Ctrl-A`, `Ctrl-E`, `Ctrl-U`, …). `Ctrl-W` does not delete a word.
- `Ctrl-W`, `Ctrl-L`, `Ctrl-J`, `Ctrl-Alt-B` are configurable: see [Key bindings](#key-bindings).

### Slash commands

- A `/` at the start of the input types a command. `Tab` accepts the completion. `acme/alpha` still filters.
- To act on a search result: `gm;/remove` (query, `;`, command). Or `Esc` empties the box, keeping the selection.
- `/create` and `/remove` ask `y` / `n` in a panel, warn about uncommitted changes that would be lost, and run without leaving the finder.

#### Every list

| Command | Action |
|---|---|
| `/help` | Command list (`q` / `Esc` closes) |
| `/get [flags] <repo>` | Close the finder, run [`gm get`](#-commands) in the terminal (progress, passphrase), go to the clone |
| `/browse` | = `Ctrl-Alt-B` |
| `/worktrees` `/branches` `/prs` | = `Ctrl-W` `Ctrl-L` `Ctrl-J`. In that list already: nothing |

#### Repository list

| Command | Action |
|---|---|
| `/dirty` | Toggle: only repositories with uncommitted changes |
| `/unpushed` | Toggle: only repositories with unpushed commits |
| `/create <repo>` | Create a repository with `origin` set; it is added and selected |
| `/remove` | Remove the selected repository and its worktrees (the panel says how many) |

- `/dirty` and `/unpushed` together keep repositories matching both. One background status scan serves both. `Esc` clears the query, then the filters.

#### Worktree list

| Command | Action |
|---|---|
| `/create <branch>` | Check the branch out as a worktree; a new branch starts from `HEAD` |
| `/remove` | Remove the selected worktree, then its branch if `git branch -d` allows; the main worktree is refused |
| `/expire <days>d` | Remove every worktree whose HEAD has not moved for that long (its reflog says when), then its branch if `git branch -d` allows. The whole list, not the selected row: the panel names each one. Uncommitted changes keep a worktree |
| `/dirty` `/unpushed` | Refused |

#### Branch list, pull request list

| Command | Action |
|---|---|
| `/create <branch>` | Same as the worktree list |
| `/remove` `/expire` `/dirty` `/unpushed` | Refused |

## 🌳 Worktrees

```
~/ghq/github.com/jedipunkz/gm/                                repository
~/ghq/.worktrees/github.com/jedipunkz/gm/feat/login           branch
~/ghq/.worktrees/github.com/jedipunkz/gm/.forks/bob/main      fork's pull request
```

- You give the branch name; `gm` picks the path.
- `.worktrees` is dotted because `gm` skips dotted directories: worktrees (which have a `.git` file) are not listed as repositories and do not block `gm migrate`.
- `.forks/<owner>/` is dotted because no branch name starts with a dot: a fork's `main` never collides with `main` or `bob/main`.

### Branch list

- Local branches, plus remote branches without a local one (`origin/feat/login`). Newest at the bottom.
- Branches the last `git fetch` missed appear at the top once `git ls-remote` answers (background). Fetched only when picked, that branch only.
- Uses git's own credentials (ssh agent, credential helper); no `gh`. Never prompts: a remote needing a password is skipped and named on the hint line.
- `Enter`: checked out already → go to its worktree. Otherwise create the worktree without asking; a remote branch becomes a local tracking branch.

### Pull request list

- Open pull requests, newest at the bottom, drafts marked `[draft]`. Type `#42` to find one by number.
- `Enter`: go to its worktree. If none, `gh pr checkout` creates it; `gh` names the branch and, for a fork, sets where it pushes.
- The worktree is named after the head branch.

## 🧰 Sub Commands

| Sub Command | Action | Flags |
|---|---|---|
| `gm` | Open the finder, print the chosen path | |
| `gm get <repo>...` | Clone into the tree; an existing clone is skipped | `-u` update existing clone ¹<br>`--ssh` SSH<br>`--shallow` depth 1<br>`--no-recursive` no submodules<br>`-b <branch>` single branch<br>`-s` quiet<br>`-l` open a shell there |
| `gm list [<query>]` | List repositories | `-p` full paths<br>`-e` exact match<br>`--unique` shortest unambiguous name |
| `gm status` | List unfinished work in repositories and worktrees ² | `--dirty` uncommitted only<br>`--unpushed` unpushed only<br>`-a` include clean ones<br>`-p` full paths |
| `gm create <repo>` | `git init` with `origin` set, print the path | `--ssh` SSH `origin` |
| `gm remove <repo>...` | Remove a repository and its worktrees, prune empty parents. Alias `gm rm` | `--dry-run`<br>`-y` no prompt |
| `gm wt create <repo> <branch>` | Add a worktree, print its path: `cd (gm wt create gm feat/login)`. Alias `gm wt new` | |
| `gm wt remove <repo> <branch>` | Remove a worktree, then its branch if `git branch -d` allows; warns about uncommitted work. Alias `gm wt rm` | `--dry-run`<br>`-y` no prompt |
| `gm wt expire <repo> <days>d` | Same as `/expire`: `gm wt expire gm 30d` | `--dry-run`<br>`-y` no prompt |
| `gm migrate <dir>...` | Move an existing clone into the tree by its `origin` | `--dry-run`<br>`-y` no prompt<br>`-r` search the directories for clones |
| `gm root` | Print the root | `--all` every root |
| `gm shell <fish\|zsh\|bash>` | Print the `Ctrl-G` binding | |
| `gm version` / `gm help` | Print the version / usage | |

1. Fetches, then fast-forwards the checked-out branch if the working tree is clean (dirty or diverged: reported, left as is), then updates submodules.
2. Uncommitted changes, or commits on any local branch that no remote has. Never fetches: fast, works offline.

`<repo>` is one of: URL, `git@host:user/repo.git`, `host/user/repo`, `user/repo`, `repo` (user from `git config github.user`).

`gm wt` is for scripts (e.g. a dotfiles bootstrap); by hand, the finder is easier.

## 🔧 Configuration

`~/.config/gm/gm.toml` (or `$XDG_CONFIG_HOME/gm/gm.toml`), optional. A parse error or an unknown key is an error.

```toml
root         = "~/ghq"         # or ["~/ghq", "~/src"], searched in order
theme        = "tokyonight"
launch_key   = "ctrl-g"        # shell: open gm
worktree_key = "ctrl-w"        # finder: worktree list
branch_key   = "ctrl-l"        # finder: branch list
pr_key       = "ctrl-j"        # finder: pull request list
remote_key   = "ctrl-alt-b"    # finder: open the remote
```

### Root

First match wins:

1. `$GM_ROOT`
2. `root` in `gm.toml`
3. `git config --get-all gm.root`
4. `$GHQ_ROOT`
5. `git config --get-all ghq.root`
6. `~/ghq`

- Roots must be absolute; `~` and `~/...` expand. Relative roots are rejected.
- Empty entries in colon-separated `$GM_ROOT` / `$GHQ_ROOT` are ignored.

### Themes

`tokyonight` (default), `solarized-dark`, `solarized-light`, `kanagawa-wave`,
`catppuccin-latte`, `catppuccin-frappe`, `catppuccin-macchiato`,
`catppuccin-mocha`, `rose-pine`, `dracula`. An unknown name is an error listing
the valid ones.

### Key bindings

| Setting | Works in | Allowed | Rejected |
|---|---|---|---|
| `launch_key` | Shell | Plain Ctrl chord | `alt`, `shift`; `ctrl-m` `ctrl-i` `ctrl-j` (Enter, Tab, line feed) |
| `worktree_key` `branch_key` `pr_key` `remote_key` | Finder | Ctrl, optionally `alt`, `shift` | `ctrl-c` `ctrl-n` `ctrl-p` `ctrl-g`; `ctrl-m` `ctrl-i` with any modifier; a chord another of the four uses |

| Chord | Reaches `gm` |
|---|---|
| `ctrl-<letter>` | Everywhere |
| `ctrl-alt-<letter>` | Nearly everywhere (Alt is sent as an ESC prefix) |
| `ctrl-shift-<letter>` | Kitty keyboard protocol only: Ghostty, kitty, WezTerm, foot, recent Alacritty. Elsewhere it arrives as `ctrl-<letter>` |

- Syntax: `ctrl-`, `ctrl+`, `c-` or `^`, then `alt` / `shift` in any order: `ctrl-alt-b`, `c-a-b`, `ctrl-alt-shift-b`.
- An invalid chord is an error, never a silent no-op.
- After changing `launch_key`, re-run `gm shell <shell>` or restart the shell.
- Avoid chords the shell or terminal uses: `ctrl-r` (history search), `ctrl-c` `ctrl-d` `ctrl-z` (signals), `ctrl-m` `ctrl-i` `ctrl-j` (Enter, Tab, line feed).

## 🎯 Ranking

1. Fuzzy score: a literal substring beats a scattered subsequence; the repository name outweighs the user name.
2. Frecency (how recently and often you opened it) breaks ties only.

Visits are stored in `$XDG_STATE_HOME/gm/frecency.json` (default `~/.local/state/gm/frecency.json`). Delete it to reset.

## 📄 License

MIT
