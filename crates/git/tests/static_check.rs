//! ADR-GRP-009, Validación 5: process launching appears only in the invocation module of
//! `crates/git` (`src/invoke.rs`).

use std::path::{Path, PathBuf};

const AUTHORIZED: &[&str] = &["invoke.rs"];
const PATTERNS: &[&str] = &[
    "Command::new",
    "process::Command",
    "std::process",
    "CommandExt",
    "libc::exec",
    "posix_spawn",
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn process_spawn_only_in_invoke_module() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() > 1);
    let mut offenders = Vec::new();
    for file in files {
        let name = file
            .strip_prefix(&src)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if AUTHORIZED.contains(&name.as_str()) {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        for p in PATTERNS {
            if text.contains(p) {
                offenders.push(format!("{name}: {p}"));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "process launch outside invoke.rs: {offenders:?}"
    );
}
