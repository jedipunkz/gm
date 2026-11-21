use super::*;

const SPAWN_BUDGET_OP: &str = "GM_SPAWN_BUDGET_OP";
const SPAWN_BUDGET_TEST: &str = "cli::tests::spawn_budget::git_spawn_budgets_hold";

const SPAWN_BUDGETS: &[(&str, usize)] = &[
    ("status", 7),
    ("list", 0),
    ("worktrees", 6),
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
