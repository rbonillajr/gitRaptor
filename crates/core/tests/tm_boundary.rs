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
