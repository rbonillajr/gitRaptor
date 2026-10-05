//! ADR-TMC-002 § 1 and Validación 1: only the Time Machine (`crates/core/src/timemachine/`)
//! reaches the write layer of `crates/git` (`tm_write`); the engine, the daemon and the apps
//! never do.

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

mod repo_intact {
    use super::*;

    #[test]
    fn only_the_time_machine_uses_the_write_layer() {
        let core = Path::new(env!("CARGO_MANIFEST_DIR"));
        let workspace = core.parent().unwrap().parent().unwrap();
        let allowed = core.join("src").join("timemachine");
        let mut files = Vec::new();
        rust_files(&core.join("src"), &mut files);
        for app in [
            "apps/cli/src",
            "apps/mcp/src",
            "crates/api/src",
            "crates/policy/src",
        ] {
            rust_files(&workspace.join(app), &mut files);
        }
        assert!(files.len() > 10);
        let offenders: Vec<String> = files
            .iter()
            .filter(|f| !f.starts_with(&allowed))
            .filter(|f| std::fs::read_to_string(f).unwrap().contains("tm_write"))
            .map(|f| f.display().to_string())
            .collect();
        assert!(
            offenders.is_empty(),
            "write layer used outside the Time Machine: {offenders:?}"
        );
    }
}

/// ADR-GRP-002 and ADR-TMC-002 § 1 (review of the Architect, TS-TMC-003): writes on the user's
/// repository live in `crates/git/src/tm_write/`. The only exception is the release of an
/// annotated lock by the oplog recovery (TS-TMC-002), pending a move into the write layer.
mod repo_intact_fs {
    use super::*;

    const FS_WRITE_PATTERNS: &[&str] = &[
        "renameat",
        "mkdirat",
        "symlinkat",
        "RenameFlags",
        "unlinkat",
    ];
    const KNOWN_EXCEPTIONS: &[&str] = &["timemachine/oplog/recovery.rs"];

    #[test]
    fn core_does_not_write_repos_with_raw_calls() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&src, &mut files);
        let offenders: Vec<String> = files
            .iter()
            .filter(|f| {
                let rel = f
                    .strip_prefix(&src)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                !KNOWN_EXCEPTIONS.contains(&rel.as_str())
            })
            .filter(|f| {
                let text = std::fs::read_to_string(f).unwrap();
                FS_WRITE_PATTERNS.iter().any(|p| text.contains(p))
            })
            .map(|f| f.display().to_string())
            .collect();
        assert!(
            offenders.is_empty(),
            "raw repo writes outside crates/git: {offenders:?}"
        );
    }
}
