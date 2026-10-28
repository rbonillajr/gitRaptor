//! The Git for Windows launcher (`<root>\cmd\git.exe`) resolves to the real `git.exe` it starts
//! (ADR-GRP-009 § 4, amendment 2026-10-28), so the `git` the engine starts is the process that
//! runs the hooks (ADR-GRD-007 Validation 12, US-GRD-006). On the real machine.
#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use gitraptor_git::Invoker;
use gitraptor_git::resolve::{self, Resolution, ResolveConfig};

const LAUNCHER: &str = r"C:\Program Files\Git\cmd\git.exe";

fn resolved(configured: &Path) -> PathBuf {
    let config = ResolveConfig {
        configured_path: Some(configured.into()),
        path_env: None,
        known_locations: vec![],
        shim_paths: vec![],
        toolchain_gits: vec![],
    };
    match resolve::resolve(&config, &Invoker::default()) {
        Resolution::Found { git, .. } => git.path,
        other => panic!("not found: {other:?}"),
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .unwrap()
        .to_string_lossy()
        .to_ascii_lowercase()
}

#[test]
fn the_launcher_resolves_to_the_git_that_runs_the_hooks() {
    if !Path::new(LAUNCHER).exists() {
        eprintln!("skipped: no Git in {LAUNCHER}");
        return;
    }
    let git = resolved(Path::new(LAUNCHER));
    assert_eq!(file_name(&git), "git.exe");
    let bin = git.parent().unwrap();
    assert_eq!(file_name(bin), "bin");
    assert!(
        ["mingw64", "ucrt64", "clangarm64", "mingw32"]
            .contains(&file_name(bin.parent().unwrap()).as_str()),
        "{git:?}"
    );

    // What the resolved git starts for a hook is its own child: no other git in between.
    let tmp = tempfile::tempdir().unwrap();
    let mut child = Command::new(&git)
        .arg("-C")
        .arg(tmp.path())
        .args(["-c", "alias.wait=!read line", "wait"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    let children = || {
        let out = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command"])
            .arg(format!(
                "Get-CimInstance Win32_Process -Filter 'ParentProcessId={pid}' | \
                 ForEach-Object {{ $_.Name }}"
            ))
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).to_ascii_lowercase()
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let mut seen = children();
    while !seen.contains("sh.exe") && std::time::Instant::now() < deadline {
        seen = children();
    }
    drop(child.stdin.take());
    let _ = child.wait();
    assert!(seen.contains("sh.exe"), "children of the git: {seen:?}");
    assert!(!seen.contains("git.exe"), "children of the git: {seen:?}");
}
