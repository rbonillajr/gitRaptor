//! ADR-CKP-002 § 11 and Validación 2 (static part): the executor does not import the write
//! layer of the Time Machine nor any Guardrails write layer or invocation module, and nobody but
//! the executor uses the invocation of user operations of `crates/git`.
//!
//! The dynamic part (every `git` of the executor has the daemon as direct parent and carries the
//! explicit repo) is covered by `crates/git/tests/user_ops_preflight.rs` and by the real-Git
//! tests of `channel_protected.rs`; the INF-GRP-001 `exec` audit suite for the executor is
//! pending until the first operation story launches `git` in production.

use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned()
}

/// Under `repo_intact::` so the single CI gate of INF-GRP-001 selects it.
mod repo_intact {
    use super::*;

    #[test]
    fn the_executor_imports_no_write_layer() {
        let executor = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("executor");
        let mut files = Vec::new();
        rust_files(&executor, &mut files);
        assert!(files.len() >= 3, "executor module not found");
        for f in files {
            let text = std::fs::read_to_string(&f).unwrap();
            for forbidden in [
                "tm_write",
                "timemachine::apply",
                "timemachine::store",
                "guardrails::write",
                "hooks::install",
                "Invoker",
            ] {
                assert!(
                    !text.contains(forbidden),
                    "{} imports {forbidden}",
                    f.display()
                );
            }
        }
    }

    #[test]
    fn only_the_executor_uses_the_user_operations() {
        let ws = workspace();
        let allowed = ws.join("crates/core/src/executor");
        let mut files = Vec::new();
        for dir in [
            "crates/core/src",
            "crates/api/src",
            "crates/policy/src",
            "apps/cli/src",
            "apps/mcp/src",
        ] {
            rust_files(&ws.join(dir), &mut files);
        }
        let offenders: Vec<String> = files
            .iter()
            .filter(|f| !f.starts_with(&allowed))
            .filter(|f| {
                let text = std::fs::read_to_string(f).unwrap();
                text.contains("user_ops") || text.contains("UserGitCommand")
            })
            .map(|f| f.display().to_string())
            .collect();
        assert!(
            offenders.is_empty(),
            "user operations used outside the executor: {offenders:?}"
        );
        // Inside crates/git, only its own module and the crate root name it.
        let mut git = Vec::new();
        rust_files(&ws.join("crates/git/src"), &mut git);
        let inside: Vec<String> = git
            .iter()
            .filter(|f| {
                !f.ends_with("user_ops.rs")
                    && !f.ends_with("lib.rs")
                    && std::fs::read_to_string(f)
                        .unwrap()
                        .contains("UserGitCommand")
            })
            .map(|f| f.display().to_string())
            .collect();
        assert!(inside.is_empty(), "{inside:?}");
    }
}
