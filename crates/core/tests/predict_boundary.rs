//! TS-CKP-001 T010 (CKPB-BOUNDARY, CKPB-CI-MIN): the frontiers of the predictor, checked on the
//! sources. `predict` imports no write layer and no profile, the daemon's binary is the only
//! process it launches (and only from the worker's module, by the path `current_exe` gave), the
//! worker runs with an environment built from scratch as `utility`, and the CI gate counts the
//! tests of the suite (ADR-GRP-009 Enmienda 2026-10-20, ADR-CKP-001 § 5).

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

fn core_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

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

/// The sources of the predictor that ship: everything under `src/predict/` but its test modules,
/// plus the daemon's module.
fn predict_sources() -> Vec<PathBuf> {
    let mut files = Vec::new();
    rust_files(&core_src().join("predict"), &mut files);
    files.retain(|f| f.file_name().is_none_or(|n| n != "tests.rs"));
    files.push(core_src().join("daemon/modules/predict.rs"));
    assert!(files.len() > 5, "the predictor's sources moved: {files:?}");
    files
}

/// The source without comments (`//`, `///`, `//!`), line by line.
fn code(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The identifiers of every `use` statement and every `crate::`/`super::` path of `code`.
fn imported_names(code: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = code;
    while let Some(at) = rest.find("use ") {
        let before_ok = at == 0 || !rest.as_bytes()[at - 1].is_ascii_alphanumeric();
        let tail = &rest[at..];
        let end = tail.find(';').unwrap_or(tail.len());
        if before_ok {
            names.extend(identifiers(&tail[..end]));
        }
        rest = &tail[end.min(tail.len())..];
        if rest.is_empty() {
            break;
        }
    }
    for line in code.lines() {
        if line.contains("crate::") || line.contains("super::") || line.contains("gitraptor_git::")
        {
            names.extend(identifiers(line));
        }
    }
    names
}

fn identifiers(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

#[test]
fn predict_imports_no_write_layer_no_invoke_and_no_profile() {
    const FORBIDDEN: &[&str] = &["tm_write", "guard_write", "user_ops", "invoke", "profile"];
    let mut offenders = Vec::new();
    for file in predict_sources() {
        let names = imported_names(&code(&file));
        for f in FORBIDDEN {
            if names.iter().any(|n| n == f) {
                offenders.push(format!("{}: {f}", file.display()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "predict reaches a layer it must not (ADR-GRP-009, Validación 5, punto 3): {offenders:?}"
    );
}

#[test]
fn the_only_command_new_under_predict_is_the_workers_and_runs_current_exe() {
    let src = core_src().join("predict");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    files.retain(|f| f.file_name().is_none_or(|n| n != "tests.rs"));
    let worker = src.join("worker/mod.rs");
    let mut offenders = Vec::new();
    for file in &files {
        if *file != worker && code(file).contains("Command::new") {
            offenders.push(file.display().to_string());
        }
    }
    assert!(
        offenders.is_empty(),
        "a process launched outside predict/worker/mod.rs: {offenders:?}"
    );
    let worker_code = code(&worker);
    assert!(
        worker_code.contains("Command::new("),
        "predict/worker/mod.rs does not launch the worker yet (T006): no Command::new"
    );
    assert!(
        !worker_code.contains("Command::new(\""),
        "the worker's program is a literal; it must be the path `current_exe` resolved once"
    );
    let resolved = code(&src.join("mod.rs"));
    assert!(
        resolved.contains("current_exe()"),
        "PredictConfig::for_daemon does not resolve the worker's program with current_exe"
    );
}

#[test]
fn the_cli_worker_command_exists_and_launches_nothing() {
    let file = root().join("apps/cli/src/commands/predict_worker.rs");
    assert!(
        file.is_file(),
        "{} does not exist: the hidden `predict-worker` subcommand is T006's",
        file.display()
    );
    let text = code(&file);
    for p in ["Command::new", "process::Command", "std::process::Command"] {
        assert!(
            !text.contains(p),
            "the worker's command launches a process: {p}"
        );
    }
}

#[test]
fn the_worker_child_gets_an_environment_built_from_scratch_and_a_fixed_cwd() {
    let text = code(&core_src().join("predict/worker/mod.rs"));
    assert!(
        text.contains(".env_clear()"),
        "the worker is launched without env_clear: GIT_*, DYLD_* and LD_* would reach it"
    );
    assert!(
        text.contains(".current_dir("),
        "the worker is launched without a fixed cwd outside any repo"
    );
}

#[test]
fn the_worker_sets_its_class_utility_before_its_first_frame() {
    let text = code(&core_src().join("predict/worker/serve.rs"));
    let serve = text
        .find("fn serve")
        .expect("serve.rs has no `fn serve` (T006)");
    let body = &text[serve..];
    let class = ["set_current(", "scoped("]
        .iter()
        .filter_map(|p| body.find(p))
        .min()
        .expect("serve never sets its work class (set_current or scoped)");
    assert!(
        body[class..].contains("WorkClass::Utility"),
        "serve sets a class other than WorkClass::Utility"
    );
    let first_read = body
        .find("read_frame(")
        .expect("serve never reads a frame (T006)");
    assert!(
        class < first_read,
        "serve reads its first frame before it sets its class utility"
    );
}

/// The body of `fn <name>` of `text`, braces balanced.
fn fn_body<'a>(text: &'a str, name: &str) -> &'a str {
    let at = text
        .find(&format!("fn {name}"))
        .unwrap_or_else(|| panic!("no `fn {name}`"));
    let tail = &text[at..];
    let open = tail.find('{').expect("a body");
    let mut depth = 0usize;
    for (i, c) in tail[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return &tail[open..open + i + 1];
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced body of fn {name}");
}

#[test]
fn the_daemon_entry_points_start_the_predictor_with_for_daemon() {
    let text = code(&core_src().join("daemon/mod.rs"));
    for name in ["for_current_user", "run_process"] {
        assert!(
            fn_body(&text, name).contains("PredictConfig::for_daemon"),
            "{name} does not configure the predictor with PredictConfig::for_daemon: the real \
             daemon would run with the predictor off"
        );
    }
}

/// CKPB-CI-MIN (PC-7): the gate fails when it runs fewer `repo_intact` tests than expected, so
/// every test the suite adds raises the minimum of the OS that runs it (Linux and macOS: the
/// suite is unix only).
#[test]
fn ci_min_counts_the_tests_of_the_predictor_suite() {
    let suite =
        std::fs::read_to_string(core_src().join("../tests/predict_repo_intact.rs")).unwrap();
    let added = suite.matches("#[test]").count();
    assert!(added > 0, "predict_repo_intact.rs has no test");
    let yml = std::fs::read_to_string(root().join(".github/workflows/repo-intact.yml")).unwrap();
    let mins: Vec<usize> = yml
        .lines()
        .filter_map(|l| l.trim().strip_prefix("repo_intact_min:"))
        .map(|v| v.trim().parse().expect("a number"))
        .collect();
    assert_eq!(mins.len(), 3, "the matrix changed shape: {mins:?}");
    // Before TS-CKP-001's PR-B: Linux 97, macOS 104, Windows 75 (no unix suite there).
    assert!(
        mins[0] >= 97 + added,
        "repo_intact_min of Linux is {}: it must be at least 97 + {added} tests of predict_repo_intact.rs",
        mins[0]
    );
    assert!(
        mins[1] >= 104 + added,
        "repo_intact_min of macOS is {}: it must be at least 104 + {added} tests of predict_repo_intact.rs",
        mins[1]
    );
}
