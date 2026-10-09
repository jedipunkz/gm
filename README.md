<p align="center">
  <img width="96" src="site/public/favicon.svg" alt="gm">
</p>
<h1 align="center">gm</h1>
<p align="center">
  <a href="https://github.com/jedipunkz/gm/actions/workflows/pr.yaml"><img src="https://github.com/jedipunkz/gm/actions/workflows/pr.yaml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue" alt="MIT"></a>
</p>
<p align="center">
  <a href="https://jedipunkz.rocks/gm/">Website</a> ·
  <a href="#-installation">Installation</a> ·
  <a href="#-configuration">Configuration</a>
</p>

<p align="center">
  <strong>A <a href="https://github.com/x-motemen/ghq">ghq</a>-style repository manager with a built-in fuzzy finder.</strong>
</p>

- **One tree:** clones land in `host/user/repo`. An existing ghq tree works as is.
- **One key:** `Ctrl-G` jumps to any clone or worktree.
- **Worktree-first:** branches and pull requests open as worktrees.

## 🚀 Installation

### Prerequisites

- macOS or Linux (not Windows: the finder and git plumbing are Unix only)
- `git` on `$PATH`
- [`gh`](https://cli.github.com/), logged in, for the pull request list
- A true-color terminal, for themes

### Step 1. Install the binary

```sh
brew install jedipunkz/gm/gm
```

From source: `git clone https://github.com/jedipunkz/gm && cd gm && make install`

### Step 2. Set up your shell

<details>
<summary>Fish</summary>

Add to `~/.config/fish/config.fish`:

```sh
gm shell fish | source
```

</details>

<details>
<summary>Zsh</summary>

Add to `~/.zshrc`:

```sh
eval "$(gm shell zsh)"
```

</details>

<details>
<summary>Bash</summary>

Add to `~/.bashrc`:

```sh
eval "$(gm shell bash)"
```

</details>

### Step 3. Use it

`gm get jedipunkz/gm`, press `Ctrl-G`, type, `Enter`: the shell `cd`s there.
Without the binding, `gm` prints the chosen path, so `$(gm)` works.

## 🔎 Finder

| List | Open | Rows | `Enter` |
|---|---|---|---|
| Repositories | `gm` / `Ctrl-G` | Clones under the roots | Go there |
| Worktrees | `Ctrl-W` | Worktrees of the selected repository | Go there |
| Branches | `Ctrl-L` | Branches of the selected repository | Go to its worktree, created if missing |
| Pull requests | `Ctrl-J` | Open pull requests of the selected repository | Go to its worktree, created if missing |

- The best match is the bottom row, next to the prompt.
- A details pane shows path, remote, branch, status, visit count and the last three commits.
- The hint line shows the current keys and a spinner while git or GitHub works. The finder stays usable meanwhile.

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

Other keys edit text (`Ctrl-A`, `Ctrl-E`, `Ctrl-U`, …). `Ctrl-W` does not delete a word. The four list keys are [configurable](#key-bindings).

### Slash commands

A `/` at the start of the input types a command; `Tab` completes. To act on a search result, type `query;/command`.
`/create` and `/remove` confirm `y` / `n` and warn about uncommitted changes that would be lost.

| Command | Lists | Action |
|---|---|---|
| `/help` | All | Command list |
| `/get [flags] <repo>` | All | Run [`gm get`](#-sub-commands) in the terminal, go to the clone |
| `/browse` | All | = `Ctrl-Alt-B` |
| `/worktrees` `/branches` `/prs` | All | = `Ctrl-W` `Ctrl-L` `Ctrl-J` |
| `/dirty` / `/unpushed` | Repositories | Toggle: only repositories with uncommitted changes / unpushed commits. Both on: match both |
| `/create <repo>` | Repositories | Create a repository with `origin` set |
| `/create <branch>` | Worktrees, branches, PRs | Check the branch out as a worktree; a new branch starts from `HEAD` |
| `/remove` | Repositories | Remove the selected repository and its worktrees |
| `/remove` | Worktrees | Remove the selected worktree, then its branch if `git branch -d` allows. Not the main worktree |
| `/expire <days>d` | Worktrees | Remove every worktree whose HEAD has not moved for that long (by reflog), then its branch if `git branch -d` allows. Uncommitted changes keep a worktree |

## 🌳 Worktrees

```
~/ghq/github.com/jedipunkz/gm/                                repository
~/ghq/.worktrees/github.com/jedipunkz/gm/feat/login           branch
~/ghq/.worktrees/github.com/jedipunkz/gm/.forks/bob/main      fork's pull request
```

- You give the branch name; `gm` picks the path. Dotted directories keep worktrees out of the repository list and fork branches from colliding with yours.
- Branch list: local branches plus remote-only ones (`origin/feat/login`), newest at the bottom. Branches the last fetch missed appear once `git ls-remote` answers. Uses git's credentials, never prompts.
- Pull request list: open pull requests, newest at the bottom, drafts marked `[draft]`. Type `#42` to find one by number. `gh pr checkout` creates the worktree, named after the head branch.

## 🧰 Sub Commands

| Sub Command | Action | Flags |
|---|---|---|
| `gm` | Open the finder, print the chosen path | |
| `gm get <repo>...` | Clone into the tree; an existing clone is skipped | `-u` update existing clone ¹<br>`--ssh` SSH<br>`--shallow` depth 1<br>`--no-recursive` no submodules<br>`-b <branch>` single branch<br>`-s` quiet<br>`-l` open a shell there |
| `gm list [<query>]` | List repositories | `-p` full paths<br>`-e` exact match<br>`--unique` shortest unambiguous name |
| `gm status` | List unfinished work in repositories and worktrees ² | `--dirty` uncommitted only<br>`--unpushed` unpushed only<br>`-a` include clean ones<br>`-p` full paths |
| `gm create <repo>` | `git init` with `origin` set, print the path | `--ssh` SSH `origin` |
| `gm remove <repo>...` | Remove a repository and its worktrees, prune empty parents. Alias `gm rm` | `--dry-run`<br>`-y` no prompt |
| `gm wt create <repo> <branch>` | Add a worktree, print its path. Alias `gm wt new` | |
| `gm wt remove <repo> <branch>` | Remove a worktree, then its branch if `git branch -d` allows. Alias `gm wt rm` | `--dry-run`<br>`-y` no prompt |
| `gm wt expire <repo> <days>d` | Same as `/expire` | `--dry-run`<br>`-y` no prompt |
| `gm migrate <dir>...` | Move an existing clone into the tree by its `origin` | `--dry-run`<br>`-y` no prompt<br>`-r` search the directories for clones |
| `gm root` | Print the root | `--all` every root |
| `gm shell <fish\|zsh\|bash>` | Print the `Ctrl-G` binding | |
| `gm version` / `gm help` | Print the version / usage | |

1. Fetches, then fast-forwards the checked-out branch if the working tree is clean, then updates submodules.
2. Uncommitted changes, or commits on any local branch that no remote has. Never fetches.

`<repo>` is one of: URL, `git@host:user/repo.git`, `host/user/repo`, `user/repo`, `repo` (user from `git config github.user`).

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

First match wins: `$GM_ROOT`, `root` in `gm.toml`, `git config --get-all gm.root`, `$GHQ_ROOT`, `git config --get-all ghq.root`, `~/ghq`.
Roots must be absolute (`~` expands).

### Themes

`tokyonight` (default), `solarized-dark`, `solarized-light`, `kanagawa-wave`,
`catppuccin-latte`, `catppuccin-frappe`, `catppuccin-macchiato`,
`catppuccin-mocha`, `rose-pine`, `dracula`.

### Key bindings

| Setting | Works in | Allowed | Rejected |
|---|---|---|---|
| `launch_key` | Shell | Plain Ctrl chord | `alt`, `shift`; `ctrl-m` `ctrl-i` `ctrl-j` (Enter, Tab, line feed) |
| `worktree_key` `branch_key` `pr_key` `remote_key` | Finder | Ctrl, optionally `alt`, `shift` | `ctrl-c` `ctrl-n` `ctrl-p` `ctrl-g`; `ctrl-m` `ctrl-i` with any modifier; a chord another of the four uses |

- Syntax: `ctrl-`, `ctrl+`, `c-` or `^`, then `alt` / `shift` in any order: `ctrl-alt-b`, `c-a-b`.
- `ctrl-shift-<letter>` needs the Kitty keyboard protocol (Ghostty, kitty, WezTerm, foot, recent Alacritty); elsewhere it arrives as `ctrl-<letter>`.
- After changing `launch_key`, re-run `gm shell <shell>` or restart the shell.

## 🎯 Ranking

Fuzzy score first (a substring beats a subsequence; the repository name outweighs the user name), then frecency to break ties.
Visits live in `$XDG_STATE_HOME/gm/frecency.json` (default `~/.local/state/gm/frecency.json`); delete it to reset.

## 💭 Inspired By

- [ghq](https://github.com/x-motemen/ghq)

## 📝 License

[MIT](LICENSE)
