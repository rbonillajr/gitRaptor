//! GRP007-WRITERS (TD-GRP-007): a static fitness check of who may launch `git` and who may write
//! refs, so the daemon's write windows (`detect::daemon_writes`) cannot be bypassed by a new
//! writer. It reads the sources (this crate and `../git`, with `apps/*/src` for the ref writes) and
//! pins what exists today: it passes now and breaks when a new site appears, so a new process
//! launcher or a new caller of the ref writers needs a person to extend the allowlists below.
//!
//! Test code is skipped: `tests/` folders, `tests.rs`, `*_tests.rs` and `#[cfg(test)]` items.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
            if name != "tests" && name != "target" && name != "node_modules" {
                rust_files(&path, out);
            }
        } else if name.ends_with(".rs") && name != "tests.rs" && !name.ends_with("_tests.rs") {
            out.push(path);
        }
    }
}

/// The source of `path` without its `#[cfg(test)]` / `#[cfg(all(test, ...))]` items.
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
        if skipping {
            if trimmed.starts_with("#[") && depth == 0 && !opened {
                continue;
            }
            let code = trimmed.split("//").next().unwrap_or("");
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
        out.push_str(line);
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

/// `(relative path, production source)` of every non-test file under the given roots.
fn sources(roots: &[&str]) -> Vec<(String, String)> {
    let mut files = Vec::new();
    for root in roots {
        rust_files(&workspace().join(root), &mut files);
    }
    files.sort();
    files
        .iter()
        .map(|f| (relative(f), production_source(f)))
        .collect()
}

/// How many times `needle` appears in `text` not preceded by an identifier character, so
/// `Command::new(` does not match `UserGitCommand::new(`.
fn word_matches(text: &str, needle: &str) -> usize {
    text.match_indices(needle)
        .filter(|(at, _)| {
            !text[..*at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
        .count()
}

/// Every `Command::new(` (std or tokio, whatever its program) of the product's code, per file,
/// with why that file may launch a process. A new site, in a new file or an allowed one, breaks
/// the test. The only launchers of the resolved `git` are `crates/git/src/invoke.rs` and
/// `crates/git/src/user_ops.rs` (`UserGitCommand::spawn`).
const COMMAND_SITES: &[(&str, usize, &str)] = &[
    (
        "crates/git/src/invoke.rs",
        4,
        "the read layer and the Time Machine write layer: the resolved git, no shell (SEC-10)",
    ),
    (
        "crates/git/src/user_ops.rs",
        1,
        "UserGitCommand::spawn: the executor's and the protected steps' git",
    ),
    (
        "crates/core/src/client.rs",
        1,
        "starts the daemon executable for the client (autostart), not git",
    ),
    (
        "crates/core/src/autostart.rs",
        1,
        "runs the login-item tool (`self.tool`, launchctl and the like), not git",
    ),
    (
        "crates/core/src/predict/worker/mod.rs",
        1,
        "the conflict predictor's worker program, not git",
    ),
];

#[test]
fn only_the_git_layers_launch_git_and_every_other_launcher_is_listed() {
    let mut found: BTreeMap<String, usize> = BTreeMap::new();
    for (path, text) in sources(&["crates/core/src", "crates/git/src"]) {
        let n = word_matches(&text, "Command::new(");
        if n > 0 {
            found.insert(path, n);
        }
    }
    let expected: BTreeMap<String, usize> = COMMAND_SITES
        .iter()
        .map(|(p, n, _)| ((*p).to_owned(), *n))
        .collect();
    assert_eq!(
        found, expected,
        "GRP007-WRITERS: a process launcher appeared, moved or disappeared: a new one must be \
         reviewed against the daemon's write windows and added to COMMAND_SITES with its reason"
    );
}

#[test]
fn no_launcher_outside_the_git_layers_runs_git_or_a_shell() {
    let git_layers = ["crates/git/src/invoke.rs", "crates/git/src/user_ops.rs"];
    let forbidden = [
        "\"git\"",
        "\"git.exe\"",
        "\"sh\"",
        "\"bash\"",
        "\"zsh\"",
        "\"/bin/sh\"",
        "\"/bin/bash\"",
        "\"/bin/zsh\"",
        "\"cmd\"",
        "\"cmd.exe\"",
        "\"/c\"",
        "\"powershell\"",
        "\"powershell.exe\"",
        "\"pwsh\"",
        "\"-Command\"",
        ".arg(\"-c\")",
    ];
    for (path, text) in sources(&["crates/core/src", "crates/git/src"]) {
        if git_layers.contains(&path.as_str()) || word_matches(&text, "Command::new(") == 0 {
            continue;
        }
        for token in forbidden {
            assert!(
                !text.contains(token),
                "GRP007-WRITERS: {path} launches a process and mentions {token}: git and shells \
                 belong to the git layers only"
            );
        }
    }
}

#[test]
fn a_user_git_command_is_spawned_only_by_the_protected_steps_and_the_executor() {
    let allowed = [
        "crates/git/src/user_ops.rs",
        "crates/core/src/timemachine/protected/mod.rs",
        "crates/core/src/executor/git.rs",
    ];
    let all = sources(&["crates/core/src", "crates/git/src"]);
    for (path, text) in &all {
        if text.contains("UserGitCommand") && text.contains(".spawn(") {
            assert!(
                allowed.contains(&path.as_str()),
                "GRP007-WRITERS: {path} spawns a UserGitCommand outside StepCtx and run_git_with: \
                 it would escape the daemon's write window"
            );
        }
    }
    for path in allowed {
        let text = all
            .iter()
            .find(|(p, _)| p == path)
            .map(|(_, t)| t)
            .unwrap_or_else(|| panic!("{path} exists"));
        assert!(
            text.contains(".spawn("),
            "{path} no longer spawns: update the allowlist"
        );
    }
}

#[test]
fn the_ref_writers_are_called_only_from_the_applier() {
    let applier = "crates/core/src/timemachine/apply/";
    let layer = "crates/git/src/tm_write/";
    let mut callers = 0;
    for (path, text) in sources(&["crates", "apps"]) {
        // Only the product's sources: `crates/*/src` and `apps/*/src`.
        if !path.contains("/src/") {
            continue;
        }
        for name in ["apply_updates(", "swap_head("] {
            if word_matches(&text, name) == 0 {
                continue;
            }
            assert!(
                path.starts_with(applier) || path.starts_with(layer),
                "GRP007-WRITERS: {path} mentions {name}: ref writes belong to the applier \
                 ({applier}), which opens the daemon's write window, and to their definition ({layer})"
            );
            if path.starts_with(applier) {
                callers += 1;
            }
        }
    }
    assert!(
        callers >= 2,
        "the applier calls both writers today ({callers})"
    );
}
