# gm

Keep every repository you clone in one predictable tree, and jump to any of
them with `Ctrl-G`.

`gm` is a [ghq](https://github.com/x-motemen/ghq)-style repository manager with
a built-in fuzzy finder: the same `host/user/repo` layout, plus a
[Bubble Tea](https://github.com/charmbracelet/bubbletea) TUI that ranks
repositories by how often and how recently you visit them.

```
                                   │ zellij-org/zellij
                                   │
                                   │ path   ~/ghq/github.com/zellij-org/zellij
                                   │ remote https://github.com/zellij-org/zellij
  github.com/tmux/tmux             │ branch main
  github.com/vadimdemedes/ink      │ commit 8f3a91c  2 days ago  fix: …
▸ github.com/zellij-org/zellij     │ status clean
                                   │ visits 34, last 2h ago
╭─────────────────────────────────────────────────────────────────────────╮
│ ❯ zellij                                                                │
╰─────────────────────────────────────────────────────────────────────────╯
```

Coloured with [Tokyo Night](https://github.com/folke/tokyonight.nvim): the
selected row is highlighted, the characters your query matched are picked out
inside it, and each field on the right gets its own colour.

The best match sits at the **bottom**, right above the prompt where the cursor
already is, so the repository you most likely want costs zero keystrokes.

## Install

```sh
go install github.com/jedipunkz/gm@latest
```

## Ctrl-G

`gm` with no arguments opens the finder and prints the chosen path on stdout —
the TUI itself draws on stderr, so it composes with `$(...)`. Your shell needs
one binding to turn that into a `cd`:

```fish
# ~/.config/fish/config.fish
gm shell fish | source
```

```sh
# ~/.zshrc
eval "$(gm shell zsh)"

# ~/.bashrc
eval "$(gm shell bash)"
```

Keys: type to filter, `↑`/`↓` (or `Ctrl-P`/`Ctrl-N`) to move, `Enter` to jump,
`Esc` to cancel.

## Commands

| Command | What it does |
|---|---|
| `gm` | Open the fuzzy finder; print the selected path |
| `gm get [-u] [-p] [--shallow] [-b <branch>] [-s] [-l] <repo>...` | Clone into the tree; `-u` updates an existing clone |
| `gm list [-p] [-e] [--unique] [<query>]` | List repositories (`-p` full paths, `-e` exact match, `--unique` shortest unambiguous name) |
| `gm rm [--dry-run] [-y] <repo>...` | Remove a repository after confirming, pruning empty parents |
| `gm create [-p] <repo>` | Create and `git init` a new repository with `origin` already set |
| `gm migrate [--dry-run] [-y] <dir>...` | Move an existing clone into the tree, using its `origin` remote |
| `gm root [--all]` | Print the root directory |
| `gm shell <fish\|zsh\|bash>` | Print the `Ctrl-G` binding |

`<repo>` accepts a full URL, `git@host:user/repo.git`, `host/user/repo`,
`user/repo`, or a bare `repo` (resolved against `git config github.user`).

## Configuration

`~/.config/gm/gm.toml` (or `$XDG_CONFIG_HOME/gm/gm.toml`) says where your
repositories live:

```toml
root = "~/ghq"
```

```toml
# or several, searched in order
root = ["~/ghq", "~/src"]
```

The file is optional, but one that cannot be parsed stops `gm` rather than
letting it clone somewhere unexpected.

### Theme

`theme` picks the finder's colors (default `tokyonight`):

```toml
theme = "kanagawa-wave"
```

`tokyonight`, `solarized-dark`, `solarized-light`, `kanagawa-wave`,
`catppuccin-latte`, `catppuccin-frappe`, `catppuccin-macchiato`,
`catppuccin-mocha`, `rose-pine`, `dracula`. An unknown name is an error
listing the valid ones.

Every accent is desaturated toward its own brightness before it is drawn, so
the six fields of the info pane read as one muted palette rather than a
rainbow. The hue and the contrast against the background stay where the theme
put them; `calm` in `theme.go` is the single knob.

### Root directory

Resolved in this order, so an existing ghq tree works untouched:

1. `$GM_ROOT`
2. `root` in `~/.config/gm/gm.toml`
3. `git config --get-all gm.root`
4. `$GHQ_ROOT`
5. `git config --get-all ghq.root`
6. `~/ghq`

## Ranking

Typing filters by fuzzy match, scored fzy-style: characters that land on a word
boundary or continue the previous match earn points, gaps cost them, matches
inside the repository name count for more than the user name, and the `host`
segment — identical across nearly every repository — counts against a match. A
literal substring always beats a subsequence pieced together from elsewhere, so
`miniec` finds `jedipunkz/miniecs` rather than spelling itself out of
`github.com/jedipunkz/spacex-ipo-checker`. Frecency only breaks ties.

Visits are recorded in `$XDG_STATE_HOME/gm/frecency.json` (default
`~/.local/state/gm/frecency.json`) and scored the way `z` and `zoxide` do —
frequency weighted by recency, so two visits this hour outrank ten from last
month. Entries for deleted repositories are pruned on write.

## Differences from ghq

Deliberately not implemented: Mercurial/Subversion/Darcs cloning (they are
still *listed*), bare clones, partial clones, parallel import, `--vcs`, and
`ghq.<url>.root` per-URL roots. `gm create` sets up the `origin` remote, which
ghq leaves to you.

## License

MIT
