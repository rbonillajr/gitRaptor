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

/// Under `repo_intact::` so the single CI gate of INF-GRP-001 selects it.
mod repo_intact {
    use super::*;

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
}

/// ADR-TMC-002 § 1 (Enmienda 2026-10-04) and Validación 1: gitoxide writes only in the store
/// writer of the Time Machine write layer (`src/tm_write/store/`).
const GIX_WRITE_PATTERNS: &[&str] = &[
    "write_blob",
    "write_object",
    "write_buf",
    "write_stream",
    "edit_tree",
    "new_commit",
    "commit_as",
    "edit_reference",
    ".reference(",
    "transaction(",
    "init_bare",
    "gix::init",
    "ThreadSafeRepository::init",
    "write_data_iter_to_stream",
    "index::File::write",
];

mod repo_intact_tm {
    use super::*;

    #[test]
    fn gix_writes_only_in_store_writer() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let store = src.join("tm_write").join("store");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        let mut offenders = Vec::new();
        let mut store_files = 0;
        for file in files {
            if file.starts_with(&store) {
                store_files += 1;
                continue;
            }
            let text = std::fs::read_to_string(&file).unwrap();
            for p in GIX_WRITE_PATTERNS {
                if text.contains(p) {
                    offenders.push(format!("{}: {p}", file.display()));
                }
            }
        }
        assert!(store_files > 0, "store writer not found");
        assert!(
            offenders.is_empty(),
            "gitoxide write outside tm_write/store: {offenders:?}"
        );
    }

    #[test]
    fn reader_does_not_expose_the_repository() {
        let reader = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("src/reader.rs"),
        )
        .unwrap();
        assert!(reader.contains("pub(crate) repo: gix::Repository"));
        assert!(!reader.contains("pub repo:"));
        assert!(!reader.contains("-> &gix::Repository"));
    }
}
