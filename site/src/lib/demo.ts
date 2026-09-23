// Sample data for the finder demo on the landing page. Nothing here is read
// from a real machine: the repositories, commits and visit counts are made up
// to show what each part of the finder looks like.

export interface DemoCommit {
  hash: string;
  refs: { name: string; kind: "head" | "local" | "remote" | "tag" }[];
  subject: string;
}

export interface DemoWorktree {
  label: string;
  path: string;
  branch: string;
  dirty: number;
  commits: DemoCommit[];
}

export interface DemoRepo {
  rel: string;
  remote: string;
  branch: string;
  dirty: number;
  visits: number;
  last: string; // how long ago, already phrased
  frecency: number; // Visit.Score(now); decides the order of an empty query
  commits: DemoCommit[];
  worktrees?: DemoWorktree[];
}

const c = (hash: string, subject: string, refs: DemoCommit["refs"] = []): DemoCommit => ({ hash, refs, subject });

// The tail of a real tree: repositories that are cloned and rarely opened.
// They exist so the list is longer than the window, the way it is on a
// machine that has been collecting clones for a while.
const quiet = (rel: string, branch = "main"): DemoRepo => ({
  rel,
  remote: `https://${rel}`,
  branch,
  dirty: 0,
  visits: 0,
  last: "",
  frecency: 0,
  commits: [c("a1b2c3d", "chore: bump dependencies", [{ name: "HEAD", kind: "head" }, { name: branch, kind: "local" }])],
});

export const ROOT = "~/ghq";

export const repos: DemoRepo[] = [
  {
    rel: "github.com/jedipunkz/gm",
    remote: "https://github.com/jedipunkz/gm",
    branch: "main",
    dirty: 0,
    visits: 42,
    last: "8m ago",
    frecency: 168,
    commits: [
      c("8477413", "Merge pull request #43 from jedipunkz/ci/release-dispatch", [
        { name: "HEAD", kind: "head" },
        { name: "main", kind: "local" },
        { name: "tag: v0.1.0", kind: "tag" },
        { name: "origin/main", kind: "remote" },
      ]),
      c("468ff39", "ci: leave the formula to the tap, so releasing needs no extra token"),
      c("ec5c371", "ci: release from a dispatch that picks major, minor or patch"),
    ],
    worktrees: [
      {
        label: "feat/login",
        path: "~/ghq/.worktrees/github.com/jedipunkz/gm/feat/login",
        branch: "feat/login",
        dirty: 2,
        commits: [
          c("b91e0c2", "feat: remember the last query", [
            { name: "HEAD", kind: "head" },
            { name: "feat/login", kind: "local" },
          ]),
          c("8477413", "Merge pull request #43 from jedipunkz/ci/release-dispatch", [{ name: "main", kind: "local" }]),
        ],
      },
      {
        label: "fix/esc",
        path: "~/ghq/.worktrees/github.com/jedipunkz/gm/fix/esc",
        branch: "fix/esc",
        dirty: 0,
        commits: [
          c("3fd21aa", "fix: esc clears the query first", [
            { name: "HEAD", kind: "head" },
            { name: "fix/esc", kind: "local" },
          ]),
        ],
      },
    ],
  },
  {
    rel: "github.com/acme/api-server",
    remote: "git@github.com:acme/api-server.git",
    branch: "feat/rate-limit",
    dirty: 3,
    visits: 17,
    last: "2h ago",
    frecency: 34,
    commits: [
      c("e4c19d0", "wip: token bucket per client", [
        { name: "HEAD", kind: "head" },
        { name: "feat/rate-limit", kind: "local" },
      ]),
      c("a02b7f1", "refactor: split the middleware chain"),
      c("5d8e3c4", "release 2.3.0", [{ name: "tag: v2.3.0", kind: "tag" }, { name: "origin/main", kind: "remote" }]),
    ],
  },
  {
    rel: "github.com/acme/alpha",
    remote: "https://github.com/acme/alpha",
    branch: "main",
    dirty: 0,
    visits: 9,
    last: "yesterday",
    frecency: 4.5,
    commits: [
      c("19ab44e", "docs: describe the config file", [
        { name: "HEAD", kind: "head" },
        { name: "main", kind: "local" },
        { name: "origin/main", kind: "remote" },
      ]),
      c("0c7d215", "test: cover the empty case"),
    ],
  },
  {
    rel: "github.com/charmbracelet/bubbletea",
    remote: "https://github.com/charmbracelet/bubbletea",
    branch: "main",
    dirty: 0,
    visits: 6,
    last: "3d ago",
    frecency: 3,
    commits: [
      c("6c1e9b2", "chore: bump dependencies", [
        { name: "HEAD", kind: "head" },
        { name: "main", kind: "local" },
      ]),
    ],
  },
  {
    rel: "github.com/charmbracelet/lipgloss",
    remote: "https://github.com/charmbracelet/lipgloss",
    branch: "main",
    dirty: 0,
    visits: 4,
    last: "5d ago",
    frecency: 2,
    commits: [c("2a7f0e3", "feat: layer compositing", [{ name: "HEAD", kind: "head" }, { name: "main", kind: "local" }])],
  },
  {
    rel: "gitlab.com/acme/infra",
    remote: "git@gitlab.com:acme/infra.git",
    branch: "main",
    dirty: 1,
    visits: 5,
    last: "2w ago",
    frecency: 1.25,
    commits: [c("f00d1e5", "terraform: rotate the staging keys", [{ name: "HEAD", kind: "head" }, { name: "main", kind: "local" }])],
  },
  {
    rel: "github.com/x-motemen/ghq",
    remote: "https://github.com/x-motemen/ghq",
    branch: "master",
    dirty: 0,
    visits: 2,
    last: "1mo ago",
    frecency: 0.5,
    commits: [c("91c3d7a", "Merge pull request from dependabot", [{ name: "HEAD", kind: "head" }, { name: "master", kind: "local" }])],
  },
  {
    rel: "github.com/junegunn/fzf",
    remote: "https://github.com/junegunn/fzf",
    branch: "master",
    dirty: 0,
    visits: 0,
    last: "",
    frecency: 0,
    commits: [c("d2e8b0f", "Update CHANGELOG", [{ name: "HEAD", kind: "head" }, { name: "master", kind: "local" }])],
  },
  {
    rel: "github.com/jedipunkz/dotfiles",
    remote: "https://github.com/jedipunkz/dotfiles",
    branch: "main",
    dirty: 4,
    visits: 0,
    last: "",
    frecency: 0,
    commits: [c("7e21c90", "fish: bind ctrl-g to gm", [{ name: "HEAD", kind: "head" }, { name: "main", kind: "local" }])],
  },
  quiet("github.com/jedipunkz/tmux-window-frame"),
  quiet("github.com/jedipunkz/tokyonight.chrome"),
  quiet("github.com/jedipunkz/keyboard-checker"),
  quiet("github.com/jedipunkz/linux-tiny-exporter"),
  quiet("github.com/jedipunkz/hn-digest"),
  quiet("github.com/jedipunkz/miniecs"),
  quiet("github.com/jedipunkz/kanban"),
  quiet("github.com/jedipunkz/soliton"),
  quiet("github.com/acme/web"),
  quiet("github.com/acme/cli"),
  quiet("github.com/acme/docs"),
  quiet("github.com/acme/terraform-modules"),
  quiet("github.com/acme/proto-schemas"),
  quiet("github.com/acme/grafana-dashboards"),
  quiet("github.com/BurntSushi/toml"),
  quiet("github.com/sahilm/fuzzy", "master"),
  quiet("github.com/spf13/cobra"),
  quiet("gitlab.com/acme/runners"),
  quiet("git.sr.ht/~acme/scratch"),
  quiet("codeberg.org/acme/notes"),
];
