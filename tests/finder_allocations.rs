//! Hard allocation budgets for the finder's synchronous per-keystroke filter.

mod allocator {
    #![allow(
        unsafe_code,
        reason = "the dedicated integration-test allocator must delegate to System"
    )]

    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    pub struct CountingAllocator;

    static ENABLED: AtomicBool = AtomicBool::new(false);
    static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

    impl CountingAllocator {
        fn record(&self) {
            if ENABLED.load(Ordering::Relaxed) {
                ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            }
        }

        pub fn measure<T>(&self, operation: impl FnOnce() -> T) -> (usize, T) {
            ALLOCATIONS.store(0, Ordering::Relaxed);
            ENABLED.store(true, Ordering::SeqCst);
            let result = operation();
            ENABLED.store(false, Ordering::SeqCst);
            (ALLOCATIONS.load(Ordering::Relaxed), result)
        }
    }

    // SAFETY: every allocation operation delegates unchanged to `System`,
    // and allocations are deallocated or reallocated by that same allocator.
    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            self.record();
            // SAFETY: the exact layout supplied by the caller is forwarded to
            // System, which implements the global allocation contract.
            unsafe { System.alloc(layout) }
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            self.record();
            // SAFETY: the exact layout supplied by the caller is forwarded to
            // System, which implements the global allocation contract.
            unsafe { System.alloc_zeroed(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            // SAFETY: ptr and layout came from the matching System allocation.
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            self.record();
            // SAFETY: ptr and layout came from System and new_size is forwarded
            // unchanged to the allocator that owns the allocation.
            unsafe { System.realloc(ptr, layout, new_size) }
        }
    }
}

#[global_allocator]
static GLOBAL: allocator::CountingAllocator = allocator::CountingAllocator;

use gm::config::parse_chord;
use gm::finder::{self, Key, Keys, Model, Msg};
use gm::repo::{History, Repo, Tree};

struct QueryCase {
    name: &'static str,
    prefix: &'static str,
    key: Key,
    // Baselines are from the red run on this toolchain; cap() adds about 10%.
    baseline: [usize; 2],
}

fn query_cases() -> [QueryCase; 3] {
    [
        QueryCase {
            name: "empty",
            prefix: "x",
            key: Key {
                name: "backspace".into(),
                text: None,
            },
            baseline: [28, 28],
        },
        QueryCase {
            name: "one-token",
            prefix: "rep",
            key: Key::char('o'),
            baseline: [3_083, 30_091],
        },
        QueryCase {
            name: "several-tokens",
            prefix: "repo-",
            key: Key::char('7'),
            baseline: [1_624, 16_966],
        },
    ]
}

fn model(rows: usize) -> Model {
    let repos: Vec<Repo> = (0..rows)
        .map(|i| Repo {
            root: "/allocation-budget".into(),
            rel: format!("github.com/user{i:05}/repo-{i:05}"),
        })
        .collect();
    let tree = Tree {
        roots: vec!["/allocation-budget".into()],
    };
    let history = History::open("");
    let theme = finder::lookup_theme("").unwrap();
    let keys = Keys {
        worktree: parse_chord("", finder::DEFAULT_WORKTREE_KEY).unwrap(),
        branch: parse_chord("", finder::DEFAULT_BRANCH_KEY).unwrap(),
        pr: parse_chord("", finder::DEFAULT_PR_KEY).unwrap(),
        issue: parse_chord("", finder::DEFAULT_ISSUE_KEY).unwrap(),
        remote: parse_chord("", finder::DEFAULT_REMOTE_KEY).unwrap(),
    };
    Model::new(&tree, &repos, &history, &theme, keys)
}

fn press(model: &mut Model, key: Key) {
    drop(model.update(Msg::Key(key)));
}

fn type_text(model: &mut Model, text: &str) {
    for c in text.chars() {
        press(model, Key::char(c));
    }
}

fn cap(baseline: usize) -> usize {
    baseline + baseline.div_ceil(10)
}

fn measure_case(model: &mut Model, case: &QueryCase) -> usize {
    // Warm the exact synchronous event first, then restore its input state.
    // Model construction, row creation, key creation and input setup stay
    // outside the measured window. update is the narrowest public path to the
    // private Model::filter; the count therefore also includes bounded key
    // dispatch and command-completion work around the filter pass.
    press(
        model,
        Key {
            name: "ctrl+u".into(),
            text: None,
        },
    );
    type_text(model, case.prefix);
    press(model, case.key.clone());

    press(
        model,
        Key {
            name: "ctrl+u".into(),
            text: None,
        },
    );
    type_text(model, case.prefix);
    let key = case.key.clone();
    let (count, commands) = GLOBAL.measure(|| model.update(Msg::Key(key)));
    drop(commands);
    count
}

#[test]
fn finder_filter_allocation_budgets() {
    let mut measurements = Vec::new();
    for (size_index, rows) in [1_000, 10_000].into_iter().enumerate() {
        let mut model = model(rows);
        for case in query_cases() {
            let count = measure_case(&mut model, &case);
            let baseline = case.baseline[size_index];
            measurements.push((rows, case.name, count, baseline, cap(baseline)));
        }
    }

    eprintln!(
        "finder allocation measurements (rows, query, count, baseline, cap): {measurements:?}"
    );
    for (rows, query, count, baseline, cap) in measurements {
        assert!(
            count <= cap,
            "{query} query with {rows} rows allocated {count} times (baseline {baseline}, cap {cap})"
        );
    }
}
