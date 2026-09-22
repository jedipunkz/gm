# gm

A [ghq](https://github.com/x-motemen/ghq)-style repository manager with a
built-in fuzzy finder. Clones land in one predictable `host/user/repo` tree,
and `Ctrl-G` jumps to any of them.

```
                                    │ zellij-org/zellij
                                    │
                                    │ path
                                    │ ~/ghq/github.com/zellij-org/zellij
                                    │ remote
                                    │ https://github.com/zellij-org/zellij
                                    │ branch
                                    │ main
                                    │ commit
  github.com/tmux/tmux              │ 8f3a91c  2 days ago  fix: resize
  github.com/vadimdemedes/ink       │ status
▸ github.com/zellij-org/zellij      │ clean
                                    │ visits
                                    │ 34, last 2h ago
╭─────────────────────────────────────────────────────────────────────────╮
│ ❯ zellij                                                                │
╰─────────────────────────────────────────────────────────────────────────╯
```

The best match sits at the bottom, next to the prompt, so the repository you
most likely want costs zero keystrokes.

## Install

```sh
go install github.com/jedipunkz/gm@latest
```

Requires Go 1.25 or newer and `git` on `$PATH`.

## Shell integration

`gm` with no arguments opens the finder and prints the chosen path on stdout;
the TUI draws on stderr, so it composes with `$(...)`. One binding turns that
into a `cd`:

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

Type to filter, `↑`/`↓` (or `Ctrl-P`/`Ctrl-N`) to move, `Enter` to jump, `Esc`
to cancel.

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
that cannot be parsed stops `gm` rather than letting it clone somewhere
unexpected.

```toml
root  = "~/ghq"          # or ["~/ghq", "~/src"], searched in order
theme = "tokyonight"
```

Themes: `tokyonight` (default), `solarized-dark`, `solarized-light`,
`kanagawa-wave`, `catppuccin-latte`, `catppuccin-frappe`,
`catppuccin-macchiato`, `catppuccin-mocha`, `rose-pine`, `dracula`.

The root is resolved in this order, so an existing ghq tree works untouched:

1. `$GM_ROOT`
2. `root` in `gm.toml`
3. `git config --get-all gm.root`
4. `$GHQ_ROOT`
5. `git config --get-all ghq.root`
6. `~/ghq`

## Ranking

Typing filters by fuzzy match: word boundaries and consecutive characters earn
points, gaps cost them, the repository name outweighs the user name, and the
shared `host` segment counts against a match. A literal substring always beats
a subsequence pieced together from elsewhere.

Visits are recorded in `$XDG_STATE_HOME/gm/frecency.json` (default
`~/.local/state/gm/frecency.json`) and weighted by recency the way `z` and
`zoxide` do. Frecency only breaks ties between equally good matches. Entries
for deleted repositories are pruned on write.

## Differences from ghq

Not implemented: Mercurial/Subversion/Darcs cloning (they are still *listed*),
bare clones, partial clones, parallel import, `--vcs`, and `ghq.<url>.root`
per-URL roots. `gm create` sets up the `origin` remote, which ghq leaves to
you.

## License

MIT
