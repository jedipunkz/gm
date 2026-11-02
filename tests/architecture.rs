//! The direction the module dependencies point, held by reading the source
//! rather than by review: docs/design.md places the cli above the finder,
//! the finder above the repository and config, and nothing below reaching
//! back up ("The finder confirms; repo carries out", "repo::delete asks
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

// The module a file is: src/repo/git/mod.rs is repo::git and src/config.rs
// is config. A super:: chain is counted up from here.
fn module_of(file: &Path, manifest: &Path) -> Vec<String> {
    let rel = file.strip_prefix(manifest.join("src")).unwrap_or(file);
    let mut module: Vec<String> = rel
        .iter()
        .map(|part| part.to_string_lossy().trim_end_matches(".rs").to_string())
        .collect();
    if module.last().is_some_and(|last| last == "mod") {
        module.pop();
    }
    module
}

// The identifier at the start of s, empty when s does not start with one.
fn ident(s: &str) -> &str {
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(s.len());
    &s[..end]
}

// reaches is the first line of text that names the target module, and the
// spelling it used. Four spellings reach a module: the path itself
// ("crate::cli"), a group that starts with it ("crate::{Result, cli::App}",
// across lines too), a super:: chain that climbs to the crate root and
// comes down into it ("super::super::cli" from repo::git), and an alias of
// the crate, which hides every path after it and so is refused outright.
// An inline module's super:: is counted from its file, which is one level
// short; the code under these rules has no inline module that climbs.
fn reaches(text: &str, module: &[String], target: &str) -> Option<(usize, String)> {
    let direct = format!("crate::{target}");
    // depth is how deep the scan is inside a crate::{ group; a name right
    // after the group's "{" or one of its commas is a path's first segment.
    let (mut depth, mut first) = (0usize, false);
    for (number, line) in text.lines().enumerate() {
        let code = code_part(line);
        // "crate::client" is not crate::cli: the name has to end there.
        if code
            .match_indices(&direct)
            .any(|(i, _)| ident(&code[i + "crate::".len()..]) == target)
        {
            return Some((number, direct));
        }
        if code.contains("crate as ") || code.contains("extern crate self") {
            return Some((number, "an alias of the crate".into()));
        }
        let mut at = 0;
        while let Some(found) = code[at..].find("super::") {
            let start = at + found;
            let starts_chain = start == 0 || {
                let before = code[..start].chars().next_back().unwrap_or(' ');
                !(before.is_ascii_alphanumeric() || before == '_' || before == ':')
            };
            let mut rest = &code[start..];
            let mut climbs = 0;
            while let Some(after) = rest.strip_prefix("super::") {
                climbs += 1;
                rest = after;
            }
            at = code.len() - rest.len();
            // Only a chain that climbs to the crate root can come down into
            // a top-level module; one that climbs past it does not compile.
            if starts_chain && climbs == module.len() && ident(rest) == target {
                return Some((number, code[start..at].to_string() + target));
            }
        }
        let mut chars = code.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if depth == 0 {
                if code[i..].starts_with("crate::{") {
                    depth = 1;
                    first = true;
                    for _ in 0.."crate::".len() {
                        chars.next();
                    }
                }
                continue;
            }
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                ',' => first = depth == 1,
                c if c.is_whitespace() => {}
                _ => {
                    if first && depth == 1 && ident(&code[i..]) == target {
                        return Some((number, format!("crate::{{{target}}}")));
                    }
                    first = false;
                }
            }
        }
    }
    None
}

// One rule run across every file it names. The first line in violation
// fails the test, and the next run names the next line, if one follows it.
fn check(files: &[&str], target: &str, prose: &str) {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    for named in files {
        for file in entries(&manifest.join(named), manifest) {
            let text = fs::read_to_string(&file)
                .unwrap_or_else(|why| panic!("{} cannot be read: {why}", shown(&file, manifest)));
            if let Some((number, spelled)) = reaches(&text, &module_of(&file, manifest), target) {
                panic!(
                    "{}:{} reaches for crate::{target} ({spelled}).\ndocs/design.md: {}",
                    shown(&file, manifest),
                    number + 1,
                    prose,
                );
            }
        }
    }
}

// The checker itself: every spelling that reaches a module is caught, and
// the near misses are not.
#[test]
fn reaches_sees_every_spelling() {
    let repo_git = ["repo".to_string(), "git".to_string()];
    let repo = ["repo".to_string()];
    let caught = [
        ("use crate::cli::App;", &repo[..]),
        ("use crate::{Result, cli::App};", &repo[..]),
        ("use crate::{\n    Result,\n    cli::App,\n};", &repo[..]),
        ("use crate::{cli, err};", &repo[..]),
        ("use super::super::cli::App;", &repo_git[..]),
        ("use super::cli;", &repo[..]),
        ("use crate as gm;", &repo[..]),
        ("extern crate self as gm;", &repo[..]),
    ];
    for (text, module) in caught {
        assert!(reaches(text, module, "cli").is_some(), "missed: {text}");
    }
    let clear = [
        ("use crate::{Result, err, paths};", &repo[..]),
        ("use crate::{repo::{cli_like, x}, paths};", &repo[..]),
        ("use crate::client::X;", &repo[..]),
        ("use super::cli;", &repo_git[..]),
        ("use super::*;", &repo_git[..]),
        ("// use crate::cli::App;", &repo[..]),
        ("let u = \"https://x\"; // crate::cli", &repo[..]),
    ];
    for (text, module) in clear {
        assert_eq!(reaches(text, module, "cli"), None, "flagged: {text}");
    }
}

#[test]
fn repo_never_reaches_for_the_finder() {
    check(
        &["src/repo"],
        "finder",
        "repo knows nothing of the finder or the cli; anything that filters, ranks or shows a repository belongs in the finder.",
    );
}

#[test]
fn repo_never_reaches_for_the_cli() {
    check(
        &["src/repo"],
        "cli",
        "repo asks nothing — the caller has already confirmed; anything that prompts or prints belongs in the cli.",
    );
}

#[test]
fn the_finder_and_config_never_reach_for_the_cli() {
    check(
        &["src/finder", "src/config.rs"],
        "cli",
        "the finder confirms and repo carries out; the cli owns the terminal — anything that prompts, prints or parses arguments belongs in the cli.",
    );
}
