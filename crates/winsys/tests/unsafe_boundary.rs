//! `gitraptor-winsys` and `gitraptor-macsys` are the only crates allowed
//! `unsafe`, and within them only their private FFI modules (ADR-GRP-002,
//! Enmiendas 2026-10-05 and 2026-10-07). Every other package of the workspace
//! inherits the workspace lints, which forbid it. Checked on `cargo metadata`
//! and on the sources, on every OS. This is the one list of exceptions.

use std::path::{Path, PathBuf};

/// Package name and crate folder (from the workspace root) of each exception.
const EXCEPTIONS: [(&str, &str); 2] = [
    ("gitraptor-winsys", "crates/winsys"),
    ("gitraptor-macsys", "crates/macsys"),
];

fn is_exception(name: &str) -> bool {
    EXCEPTIONS.iter().any(|(n, _)| *n == name)
}

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
    for (exception, _) in EXCEPTIONS {
        assert!(packages.iter().any(|(n, _)| n == exception), "{exception}");
    }
    let offenders: Vec<_> = packages
        .iter()
        .filter(|(name, _)| !is_exception(name))
        .filter(|(_, path)| !inherits_workspace_lints(&std::fs::read_to_string(path).unwrap()))
        .map(|(name, _)| name.clone())
        .collect();
    assert!(
        offenders.is_empty(),
        "without `[lints] workspace = true`: {offenders:?}"
    );
}

/// Every `.rs` file under `dir`, recursively.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(rust_files(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out
}

/// Only top-level files named `ffi*.rs` may contain `unsafe`; each exception's crate root
/// denies it and allows it back only on those modules, and keeps the stricter lints.
#[test]
fn unsafe_lives_only_in_the_ffi_modules() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (name, folder) in EXCEPTIONS {
        let manifest = std::fs::read_to_string(root.join(folder).join("Cargo.toml")).unwrap();
        for lint in [
            "unsafe_code = \"deny\"",
            "unsafe_op_in_unsafe_fn = \"deny\"",
            "undocumented_unsafe_blocks = \"deny\"",
            "multiple_unsafe_ops_per_block = \"deny\"",
        ] {
            assert!(manifest.contains(lint), "{name} without `{lint}`");
        }
        check_sources(&root.join(folder).join("src"));
    }
}

fn check_sources(src: &Path) {
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
    for path in rust_files(src) {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        // Only top-level FFI modules: `allow` is checked on `lib.rs` above.
        if path.parent() == Some(src) && (name.starts_with("ffi") || name == "lib.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("unsafe {")
                && !text.contains("unsafe fn")
                && !text.contains("unsafe_code"),
            "{} uses `unsafe` outside an FFI module",
            path.display()
        );
    }
}
