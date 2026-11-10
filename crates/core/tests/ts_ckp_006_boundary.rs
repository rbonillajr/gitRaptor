//! CKP006-FIT (DS-TS-CKP-006 D14): a static fitness check of the modules of `diff.commit`,
//! `diff.between` and `diff.index`. Their production code must never launch a process, parse a
//! revspec, compute a merge-base or reach gix's diff platform, resource cache, filter pipeline or
//! attributes: everything is read on raw bytes (ADR-GRP-009 § 1, D5, D6, D10).
//!
//! Test code is skipped: `tests/` folders, `tests.rs`, `*_tests.rs` and `#[cfg(test)]` items, as
//! in GRP007-WRITERS. Comments are skipped too: a doc that names a forbidden token is not code.

use std::path::{Path, PathBuf};

/// The directories of the new modules, relative to the workspace root.
const ROOTS: &[&str] = &["crates/git/src/diff", "crates/core/src/channel/diff"];

/// Tokens the production code of those modules must not contain (D14).
const FORBIDDEN: &[&str] = &[
    "Command",
    "std::process",
    "rev_parse",
    "revision::Spec",
    "merge_bases",
    "Platform",
    "resource_cache",
    "filter::Pipeline",
    "attributes(",
];

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/core lives two levels below the workspace root")
        .to_path_buf()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "tests" && name != "target" {
                rust_files(&path, out);
            }
        } else if name.ends_with(".rs") && name != "tests.rs" && !name.ends_with("_tests.rs") {
            out.push(path);
        }
    }
}

/// The code of `path` without its `#[cfg(test)]` / `#[cfg(all(test, ...))]` items and without
/// comments.
fn production_source(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap();
    let mut out = String::new();
    let mut skipping = false;
    let (mut depth, mut opened) = (0i32, false);
    for line in text.lines() {
        let trimmed = line.trim_start();
        if !skipping
            && (trimmed.starts_with("#[cfg(test)]") || trimmed.starts_with("#[cfg(all(test"))
        {
            skipping = true;
            (depth, opened) = (0, false);
            continue;
        }
        let code = trimmed.split("//").next().unwrap_or("");
        if skipping {
            if trimmed.starts_with("#[") && depth == 0 && !opened {
                continue;
            }
            for c in code.chars() {
                match c {
                    '{' => {
                        depth += 1;
                        opened = true;
                    }
                    '}' => depth -= 1,
                    _ => {}
                }
            }
            if (opened && depth <= 0) || (!opened && code.trim_end().ends_with(';')) {
                skipping = false;
            }
            continue;
        }
        out.push_str(code);
        out.push('\n');
    }
    out
}

fn relative(path: &Path) -> String {
    path.strip_prefix(workspace())
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/")
}

fn files_of(root: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    rust_files(&workspace().join(root), &mut files);
    files.sort();
    files
}

#[test]
fn ckp006_fit_new_modules_never_parse_revspecs_nor_spawn() {
    let mut found = Vec::new();
    for root in ROOTS {
        for file in files_of(root) {
            let code = production_source(&file);
            for (n, line) in code.lines().enumerate() {
                for token in FORBIDDEN {
                    if line.contains(token) {
                        found.push(format!("{}:{} {token}", relative(&file), n + 1));
                    }
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "the diff modules of TS-CKP-006 use a forbidden token (D14): {found:#?}"
    );
}

#[test]
fn ckp006_fit_the_checked_directories_exist_and_hold_code() {
    for root in ROOTS {
        let files = files_of(root);
        assert!(
            !files.is_empty(),
            "{root} is missing or empty: CKP006-FIT would pass on nothing"
        );
        assert!(
            files
                .iter()
                .any(|f| !production_source(f).trim().is_empty()),
            "{root} has no production code"
        );
    }
}
