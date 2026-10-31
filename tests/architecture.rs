//! The direction the module dependencies point, held by reading the source
//! rather than by review: docs/design.md places the cli above the finder,
//! the finder above the repository and config, and nothing below reaching
//! back up ("The finder shows state; the CLI mutates it", "repo::delete asks
//! nothing"). The compiler cannot see the direction — every module compiles
//! into one crate — so the tree is walked, and a line in violation fails the
//! test with the file, the line, the import, and the layer the code belongs
//! in.

use std::fs;
use std::path::{Path, PathBuf};

// The code part of a line: the line cut where its comment starts. A comment
// may quote the rule it states — this test's own comments name the imports —
// and quoting is not using, so only the code part is matched. The cut is the
// first "//" that a colon does not introduce, because "https://" is a scheme
// inside code, not a comment start.
fn code_part(line: &str) -> &str {
    let mut cut = line.len();
    let mut start = 0;
    while let Some(at) = line[start..].find("//") {
        let at = start + at;
        let opens_scheme = at > 0 && line.as_bytes()[at - 1] == b':';
        if !opens_scheme {
            cut = at;
            break;
        }
        // The "//" belongs to a scheme: the comment, if there is one,
        // starts further along the line.
        start = at + 2;
    }
    &line[..cut]
}

// The name a failure shows: the path relative to the manifest directory, so
// the message reads like the files in the design document do, not like a
// build's absolute path.
fn shown(file: &Path, manifest: &Path) -> String {
    file.strip_prefix(manifest)
        .unwrap_or(file)
        .display()
        .to_string()
}

// Every file one of a rule's paths names. A directory is followed to the
// last nested file — src/repo/mod.rs sits inside src/repo, so it is inside
// the rule with its siblings — and a file stands for itself. The order is
// sorted, so two runs see the same first violation whatever read_dir
// returned.
fn entries(root: &Path, manifest: &Path) -> Vec<PathBuf> {
    // A file stands for itself; only a directory is walked. Both shapes
    // name a rule: "src/finder" is a directory, "src/config.rs" a file.
    if root.is_file() {
        return vec![root.to_path_buf()];
    }
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let listed = fs::read_dir(&dir)
            .unwrap_or_else(|why| panic!("{} cannot be listed: {why}", shown(&dir, manifest)));
        for entry in listed {
            let path = entry
                .unwrap_or_else(|why| panic!("{} cannot be walked: {why}", shown(&dir, manifest)))
                .path();
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

// One rule run across every file it names. The first line in violation
// fails the test, and the next run names the next line, if one follows it.
fn check(files: &[&str], forbidden: &str, prose: &str) {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    for named in files {
        for file in entries(&manifest.join(named), manifest) {
            let text = fs::read_to_string(&file)
                .unwrap_or_else(|why| panic!("{} cannot be read: {why}", shown(&file, manifest)));
            for (number, line) in text.lines().enumerate() {
                if code_part(line).contains(forbidden) {
                    panic!(
                        "{}:{} reaches for {}.\ndocs/design.md: {}",
                        shown(&file, manifest),
                        number + 1,
                        forbidden,
                        prose,
                    );
                }
            }
        }
    }
}

#[test]
fn repo_never_reaches_for_the_finder() {
    check(
        &["src/repo"],
        "crate::finder",
        "repo knows nothing of the finder or the cli; anything that filters, ranks or shows a repository belongs in the finder.",
    );
}

#[test]
fn repo_never_reaches_for_the_cli() {
    check(
        &["src/repo"],
        "crate::cli",
        "repo asks nothing — the caller has already confirmed; anything that prompts or prints belongs in the cli.",
    );
}

#[test]
fn the_finder_and_config_never_reach_for_the_cli() {
    check(
        &["src/finder", "src/config.rs"],
        "crate::cli",
        "the finder shows state; the cli mutates it — anything that changes, prompts or draws on the terminal belongs in the cli.",
    );
}
