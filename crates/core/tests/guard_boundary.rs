//! ADR-GRD-001 § 7 and Validación 12 (US-GRD-001): only the `guardrails` module of
//! `crates/core` reaches the Guardrails write layer of `crates/git`; `raptor hook` launches no
//! process of its own, and the native dispatcher only ever starts the `raptor` of its constants
//! (the named exception, ADR-GRD-001 Enmienda 2026-10-05).

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
        .to_path_buf()
}

mod repo_intact {
    use super::*;

    #[test]
    fn only_guardrails_uses_the_guardrails_write_layer() {
        let root = workspace();
        let allowed = root.join("crates/core/src/guardrails");
        let mut files = Vec::new();
        for dir in [
            "crates/core/src",
            "apps/cli/src",
            "apps/mcp/src",
            "crates/api/src",
            "crates/policy/src",
        ] {
            rust_files(&root.join(dir), &mut files);
        }
        assert!(files.len() > 10);
        let offenders: Vec<String> = files
            .iter()
            .filter(|f| !f.starts_with(&allowed))
            .filter(|f| {
                let text = std::fs::read_to_string(f).unwrap();
                text.contains("guard_write") || text.contains("GuardWriter")
            })
            .map(|f| f.display().to_string())
            .collect();
        assert!(
            offenders.is_empty(),
            "Guardrails write layer used outside crates/core/src/guardrails: {offenders:?}"
        );
    }

    #[test]
    fn the_hook_client_launches_no_process() {
        let root = workspace();
        let mut files = vec![root.join("apps/cli/src/guard.rs")];
        rust_files(&root.join("crates/core/src/guardrails"), &mut files);
        for f in files {
            let text = std::fs::read_to_string(&f).unwrap();
            for pattern in ["Command::new", "process::Command", "CommandExt"] {
                assert!(!text.contains(pattern), "{} has {pattern}", f.display());
            }
        }
    }

    /// ADR-GRD-001 § 7, Enmiendas 2026-10-05 and 2026-10-08: the dispatcher starts the `raptor`
    /// of its constants (with a cleared environment) and the prior hook of its `prior` constant
    /// (US-GRD-002), nothing else. A prior script without a `#!` line runs through `/bin/sh`
    /// with the script as its argument, as Git does: never `-c`, never a command line.
    #[test]
    fn the_dispatcher_only_starts_the_raptor_of_its_constants_and_the_prior_hook() {
        let text =
            std::fs::read_to_string(workspace().join("apps/cli/src/bin/raptor-hook.rs")).unwrap();
        assert_eq!(text.matches("Command::new(").count(), 3);
        assert!(text.contains("Command::new(raptor)"));
        assert!(text.contains("Command::new(&program)"));
        assert!(text.contains("Command::new(\"/bin/sh\")"));
        assert_eq!(text.matches("/bin/sh").count(), 1);
        assert!(text.contains("sh.arg(&program)"));
        assert!(text.contains(".env_clear()"));
        // The prior hook is only ever `<prior constant>/<fixed hook name>`.
        assert!(text.contains("let path = dir.join(hook.name());"));
        for word in ["\"-c\"", "\"sh\"", "cmd.exe", "powershell"] {
            assert!(!text.contains(word), "{word}");
        }
    }
}
