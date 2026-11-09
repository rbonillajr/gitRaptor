//! V7 (ADR-GRP-017, TS-DSK-001 T003): the backend does not read the environment nor the arguments at
//! run time and does not depend on logging initialisers that read variables. `env!` and `option_env!`
//! (compile time) are allowed.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

const FORBIDDEN_CALLS: &[&str] = &[
    "std::env::var",
    "env::var(",
    "env::vars",
    "var_os",
    "env::args",
    "args_os",
];
const FORBIDDEN_DEPS: &[&str] = &[
    "env_logger",
    "tracing-subscriber",
    "log4rs",
    "simplelog",
    "pretty_env_logger",
    "tauri-plugin-log",
];

fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("V7: cannot read {}: {e}", dir.display()))
    {
        let p = entry.unwrap().path();
        if p.is_dir() {
            out.extend(rust_files(&p));
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
    out
}

#[test]
fn v7_sources_do_not_read_the_environment_or_arguments() {
    let files = rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
    assert!(
        files.len() >= 4,
        "V7: the scan found too few files: {files:?}"
    );
    let mut offenders = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        for (n, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for call in FORBIDDEN_CALLS {
                if line.contains(call) {
                    offenders.push(format!("{}:{} uses {call}", file.display(), n + 1));
                }
            }
        }
    }
    assert!(offenders.is_empty(), "V7: {offenders:#?}");
}

#[test]
fn v7_normal_dependencies_have_no_env_reading_logger() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let out = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--manifest-path",
        ])
        .arg(&manifest)
        .output()
        .expect("V7: cargo metadata must run");
    assert!(
        out.status.success(),
        "V7: cargo metadata failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: Value = serde_json::from_slice(&out.stdout).unwrap();
    let pkg = meta["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "gitraptor-desktop")
        .expect("V7: gitraptor-desktop is not in the workspace");
    let normal: Vec<&str> = pkg["dependencies"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["kind"].is_null())
        .map(|d| d["name"].as_str().unwrap())
        .collect();
    assert!(
        normal.contains(&"tauri"),
        "V7: expected the normal dependencies of the crate, got {normal:?}"
    );
    for dep in normal {
        assert!(
            !FORBIDDEN_DEPS.contains(&dep),
            "V7: normal dependency {dep} reads the environment"
        );
    }
}
