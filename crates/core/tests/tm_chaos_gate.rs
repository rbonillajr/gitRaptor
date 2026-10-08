//! The crash points of the chaos harness (INF-TMC-001) never reach a build
//! for users (SEC-06, DS-INF-TMC-001 § 2): they live behind the `chaos`
//! feature, which is not a default and which only dev-dependencies turn on.
//! A release build, even with debug assertions, compiles an empty
//! `crash_point`. Static checks over the manifests and the module.

use std::path::{Path, PathBuf};

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_owned()
}

/// Every `Cargo.toml` of the workspace's members.
fn manifests() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for group in ["apps", "crates"] {
        for entry in std::fs::read_dir(workspace().join(group)).unwrap() {
            let manifest = entry.unwrap().path().join("Cargo.toml");
            if manifest.is_file() {
                out.push(manifest);
            }
        }
    }
    assert!(out.len() > 5, "{out:?}");
    out
}

#[test]
fn the_chaos_feature_is_never_a_default() {
    let core = std::fs::read_to_string(workspace().join("crates/core/Cargo.toml")).unwrap();
    let features = core
        .split("\n[")
        .find(|s| s.starts_with("features]"))
        .expect("crates/core has a [features] table");
    assert!(features.contains("\nchaos = []"), "{features}");
    for line in features.lines() {
        assert!(
            !(line.trim_start().starts_with("default") && line.contains("chaos")),
            "{line}"
        );
    }
}

#[test]
fn only_dev_dependencies_turn_the_chaos_feature_on() {
    for manifest in manifests() {
        let text = std::fs::read_to_string(&manifest).unwrap();
        let mut table = String::new();
        for line in text.lines() {
            if line.starts_with('[') {
                table = line.to_owned();
            }
            if line.contains("\"chaos\"") {
                assert!(
                    table.contains("dev-dependencies]"),
                    "{}: `chaos` under {table}",
                    manifest.display()
                );
            }
        }
    }
}

#[test]
fn without_the_feature_the_crash_point_is_empty() {
    let source =
        std::fs::read_to_string(workspace().join("crates/core/src/timemachine/chaos.rs")).unwrap();
    assert!(
        source.contains(
            "#[cfg(not(feature = \"chaos\"))]\n#[inline(always)]\npub fn crash_point(_name: &str) {}"
        ),
        "the crash point without the feature must be empty"
    );
    // Everything that reads the variable or kills sits behind the feature.
    for needle in ["std::env::var(", "kill_process(", "abort()"] {
        for (i, line) in source.lines().enumerate() {
            if line.contains(needle) {
                let before: Vec<&str> = source.lines().take(i).collect();
                let gate = before
                    .iter()
                    .rev()
                    .find(|l| l.starts_with("#[cfg("))
                    .copied()
                    .unwrap_or_default();
                assert!(
                    gate.contains("feature = \"chaos\"") && !gate.contains("not(feature"),
                    "line {}: `{needle}` outside the chaos feature",
                    i + 1
                );
            }
        }
    }
}
