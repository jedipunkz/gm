//! divan benchmarks for gm's hot paths: the work every run or every
//! keystroke does. Run with `make bench`, or cargo bench --bench perf.
//! The bench profile is optimized, so the timings are release-grade.

fn main() {
    divan::main();
}

use std::sync::OnceLock;

use gm::config;
use gm::finder::score;
use gm::paths;
use gm::repo::{self, Repo};

// The benchmark data mirrors a real ghq tree: a handful of hosts, dozens of
// users, tens of repositories each, names the way GitHub spells them.
const NAMES: [&str; 20] = [
    "ghq",
    "miniecs",
    "ecs-lawsuits",
    "spacex-ipo-checker",
    "notes",
    "dotfiles",
    "opencode",
    "aws-sdk-rust",
    "ratatui",
    "tokio",
    "hyper",
    "rust-analyzer",
    "cargo",
    "wasmtime",
    "zlib-rs",
    "linux",
    "neovim",
    "lazygit",
    "fzf",
    "ripgrep",
];

fn labels() -> &'static Vec<String> {
    static L: OnceLock<Vec<String>> = OnceLock::new();
    L.get_or_init(|| {
        let mut v = Vec::new();
        for h in 0..3 {
            for u in 0..14 {
                for (i, base) in NAMES.iter().enumerate() {
                    let suffix = if i > 3 {
                        String::new()
                    } else {
                        format!("-{i}")
                    };
                    v.push(format!("git{h}.example.com/user{u}/{base}-{i}{suffix}"));
                }
                v.push(format!("git{h}.example.com/user{u}/dotfiles{u}"));
            }
        }
        v
    })
}

fn strs() -> &'static Vec<&'static str> {
    static S: OnceLock<Vec<&'static str>> = OnceLock::new();
    S.get_or_init(|| labels().iter().map(String::as_str).collect())
}

/// repos builds the Repository slice shortest_unique works on.
fn repos() -> &'static Vec<Repo> {
    static R: OnceLock<Vec<Repo>> = OnceLock::new();
    R.get_or_init(|| {
        labels()
            .iter()
            .map(|rel| Repo {
                root: "/home/me/ghq".into(),
                rel: rel.clone(),
            })
            .collect()
    })
}

#[divan::bench]
fn finder_find_typical_query() {
    score::find("svc7", strs());
}

#[divan::bench]
fn finder_find_wide_query() {
    // One character: nearly everything matches, ranking does the work.
    score::find("u", strs());
}

#[divan::bench]
fn finder_find_no_match() {
    score::find("zzzzz", strs());
}

#[divan::bench]
fn finder_substring_match() {
    score::substring_match("git1.example.com/user7/service-7", "service-7");
}

#[divan::bench]
fn finder_match_score() {
    score::match_score(
        "git1.example.com/user7/service-7",
        &[22, 23, 24, 25, 26, 27, 28, 29, 30, 31],
    );
}

#[divan::bench]
fn repo_shortest_unique() {
    repo::shortest_unique(repos());
}

#[divan::bench]
fn paths_clean() {
    paths::clean("/home/me/ghq/github.com/x-motemen/../x-motemen/ghq/");
}

#[divan::bench]
fn paths_join() {
    paths::join("/home/me/ghq/github.com/user7", ".git");
}

#[divan::bench]
fn config_parse_toml() {
    config::parse(
        "root = [\"~/ghq\", \"~/src\"]\n\
         theme = \"tokyonight\"\n\
         launch_key = \"ctrl-g\"\n\
         worktree_key = \"ctrl-w\"\n\
         branch_key = \"ctrl-l\"\n\
         pr_key = \"ctrl-j\"\n\
         remote_key = \"ctrl-alt-b\"\n",
    )
    .unwrap();
}

// The filesystem walk fixture: a tree of hosts/users/repositories with .git
// directories and stray files, built once and left in the OS temp dir.
fn tree() -> &'static str {
    static T: OnceLock<String> = OnceLock::new();
    T.get_or_init(|| {
        let dir = std::env::temp_dir().join("gm-bench-tree");
        std::fs::create_dir_all(&dir).unwrap();
        for h in 0..2 {
            for u in 0..12 {
                let user = dir
                    .join(format!("git{h}.example.com"))
                    .join(format!("user{u}"));
                std::fs::create_dir_all(&user).unwrap();
                std::fs::create_dir_all(user.join(".cache")).unwrap();
                std::fs::write(user.join(".cache/t"), "x").unwrap();
                for (i, base) in NAMES.iter().enumerate() {
                    let r = user.join(format!("{base}-{i}"));
                    std::fs::create_dir_all(r.join(".git")).unwrap();
                    std::fs::write(r.join("README.md"), "x").unwrap();
                    std::fs::create_dir_all(r.join("src")).unwrap();
                    std::fs::write(r.join("src/main.rs"), "x").unwrap();
                }
            }
        }
        String::from_utf8_lossy(dir.as_os_str().as_encoded_bytes()).into_owned()
    })
}

#[divan::bench]
fn repo_find_repos() {
    repo::find_repos(tree());
}
