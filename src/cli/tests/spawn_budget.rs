//! Budgets for the git processes gm starts.

use super::*;

// Budgets for the git processes gm's work costs. The count is taken in a
// child process, one per measured operation: tests run in parallel in one
// process, so a counter shared by threads would sum other tests' spawns into
// the measurement, and a fresh child counts only its own. The budgets are
// measured numbers with a little headroom, not aspirations; raising one to
// fit a slower implementation is a regression, not a fix.
const SPAWN_BUDGET_OP: &str = "GM_SPAWN_BUDGET_OP";
const SPAWN_BUDGET_TEST: &str = "cli::tests::spawn_budget::git_spawn_budgets_hold";

/// SPAWN_BUDGETS is the least a run costs today, per measured operation.
/// Each budget is the measured count with one process of headroom, and list
/// costs none at all. A repository row costs two git processes in status
/// (status, remote -v) and one more when it has remotes to count unpushed
/// work against.
const SPAWN_BUDGETS: &[(&str, usize)] = &[
    // gm status over one repository with two worktrees: three rows to read.
    ("status", 7),
    // gm list over five repositories: the tree walk is the filesystem, so
    // no row costs a process.
    ("list", 0),
    // worktrees of one repository, then the changed files of one worktree.
    ("worktrees", 6),
    // gm wt create of a new branch in a fresh clone.
    ("wt-create", 5),
];

#[test]
fn git_spawn_budgets_hold() {
    let Ok(op) = std::env::var(SPAWN_BUDGET_OP) else {
        let exe = std::env::current_exe().unwrap();
        for (op, _) in SPAWN_BUDGETS {
            let out = std::process::Command::new(&exe)
                .args(["--exact", SPAWN_BUDGET_TEST, "--nocapture"])
                .env(SPAWN_BUDGET_OP, op)
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&out.stdout);
            assert!(
                out.status.success(),
                "git spawn budget for {op}:\n{stdout}\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
            // A filter that matches nothing passes too: the child must have
            // run this test, or a renamed module would measure nothing.
            assert!(
                stdout.contains("test result: ok. 1 passed;"),
                "git spawn budget for {op} ran no test named {SPAWN_BUDGET_TEST}:\n{stdout}"
            );
        }
        return;
    };

    let measured = match op.as_str() {
        "status" => measure_status_spawns(),
        "list" => measure_list_spawns(),
        "worktrees" => measure_worktrees_spawns(),
        "wt-create" => measure_wt_create_spawns(),
        _ => panic!("no such measured operation: {op}"),
    };
    let budget = SPAWN_BUDGETS.iter().find(|(o, _)| o == &op).unwrap().1;
    println!("git spawns {measured} (budget {budget})");
    assert!(
        measured <= budget,
        "{op} spawned {measured} git processes, budget {budget}"
    );
}

// measure_status_spawns sets one repository up with two worktrees, runs
// gm status over the tree and returns how many git processes that cost.
fn measure_status_spawns() -> usize {
    let tmp = TempDir::new();
    let root = tmp.join("root");
    let t = tree(&root);
    let r = Repo {
        root: root.clone(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    for b in ["feat/login", "feat/other"] {
        repo::add_worktree(&root, &r.path(), &t.worktree_dir(&r, b), b).unwrap();
    }
    repo::git_spawns_reset();
    let run = run_in(&t, "", Config::default(), |app| app.status(&args(&[])));
    run.res.unwrap();
    repo::git_spawns_reset()
}

// measure_list_spawns runs gm list over five repositories and returns how
// many git processes that cost.
fn measure_list_spawns() -> usize {
    let tmp = TempDir::new();
    let root = tmp.join("root");
    let t = tree(&root);
    for name in ["alpha", "bravo", "charlie", "delta", "echo"] {
        git_repo(&paths::join(&paths::join(&root, "github.com/acme"), name));
    }
    repo::git_spawns_reset();
    let run = run_in(&t, "", Config::default(), |app| app.list(&args(&[])));
    run.res.unwrap();
    repo::git_spawns_reset()
}

// measure_worktrees_spawns reads the worktrees of one repository and the
// changed files of one of them, and returns how many git processes that cost.
fn measure_worktrees_spawns() -> usize {
    let tmp = TempDir::new();
    let root = tmp.join("root");
    let t = tree(&root);
    let r = Repo {
        root: root.clone(),
        rel: "github.com/acme/alpha".into(),
    };
    git_repo(&r.path());
    let login = t.worktree_dir(&r, "feat/login");
    repo::add_worktree(&root, &r.path(), &login, "feat/login").unwrap();
    repo::git_spawns_reset();
    let wts = repo::worktrees(&r.path()).unwrap();
    assert_eq!(wts.len(), 2, "{wts:?}");
    let _ = repo::changed_files(&wts[1].path);
    repo::git_spawns_reset()
}

// measure_wt_create_spawns checks a new branch out of a fresh clone with
// gm wt create and returns how many git processes that cost.
fn measure_wt_create_spawns() -> usize {
    let tmp = TempDir::new();
    let origin = tmp.join("origin.git");
    git(
        &tmp.path(),
        &["init", "-q", "--bare", "-b", "main", &origin],
    );
    let root = tmp.join("root");
    let t = tree(&root);
    let r = Repo {
        root: root.clone(),
        rel: "github.com/acme/alpha".into(),
    };
    mkdir(&paths::dir(&r.path()));
    git(&tmp.path(), &["clone", "-q", &origin, &r.path()]);
    repo::git_spawns_reset();
    let run = run_in(&t, "", Config::default(), |app| {
        app.wt_create(&args(&["github.com/acme/alpha", "feat/login"]))
    });
    run.res.unwrap();
    repo::git_spawns_reset()
}
