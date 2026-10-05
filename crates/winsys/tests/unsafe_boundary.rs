//! `gitraptor-winsys` is the only crate allowed `unsafe`, and within it only
//! its private FFI modules (ADR-GRP-002, Enmienda 2026-10-05). Every other
//! package of the workspace inherits the workspace lints, which forbid it.
//! Checked on `cargo metadata` and on the sources, on every OS.

use std::path::{Path, PathBuf};

const EXCEPTION: &str = "gitraptor-winsys";

fn manifests() -> Vec<(String, PathBuf)> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = std::process::Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    meta["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["name"].as_str().unwrap().to_owned(),
                PathBuf::from(p["manifest_path"].as_str().unwrap()),
            )
        })
        .collect()
}

/// `[lints]` followed by `workspace = true`, ignoring blank lines.
fn inherits_workspace_lints(manifest: &str) -> bool {
    let mut lines = manifest.lines().map(str::trim).filter(|l| !l.is_empty());
    while let Some(line) = lines.next() {
        if line == "[lints]" {
            return lines
                .next()
                .is_some_and(|l| l.replace(' ', "") == "workspace=true");
        }
    }
    false
}

#[test]
fn the_workspace_forbids_unsafe() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let text = std::fs::read_to_string(root).unwrap();
    assert!(
        text.lines()
            .any(|l| l.replace(' ', "") == "unsafe_code=\"forbid\""),
        "the workspace must keep `unsafe_code = \"forbid\"`"
    );
}

#[test]
fn every_other_package_inherits_the_workspace_lints() {
    let packages = manifests();
    assert!(packages.iter().any(|(n, _)| n == EXCEPTION));
    let offenders: Vec<_> = packages
        .iter()
        .filter(|(name, _)| name != EXCEPTION)
        .filter(|(_, path)| !inherits_workspace_lints(&std::fs::read_to_string(path).unwrap()))
        .map(|(name, _)| name.clone())
        .collect();
    assert!(
        offenders.is_empty(),
        "without `[lints] workspace = true`: {offenders:?}"
    );
}

/// Only files named `ffi*.rs` may contain `unsafe`; the crate root denies it
/// and allows it back only on those modules.
#[test]
fn unsafe_lives_only_in_the_ffi_modules() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let lib = std::fs::read_to_string(src.join("lib.rs")).unwrap();
    assert!(lib.contains("#![deny(unsafe_code)]"));
    let lines: Vec<&str> = lib.lines().map(str::trim).collect();
    for (i, line) in lines.iter().enumerate() {
        if line.contains("allow(unsafe_code)") {
            assert_eq!(*line, "#[allow(unsafe_code)]");
            let module = lines.get(i + 1).copied().unwrap_or_default();
            assert!(module.starts_with("mod ffi"), "allow on `{module}`");
        }
    }
    for entry in std::fs::read_dir(&src).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with("ffi") || name == "lib.rs" {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("unsafe {")
                && !text.contains("unsafe fn")
                && !text.contains("unsafe_code"),
            "{name} uses `unsafe` outside an FFI module"
        );
    }
}
