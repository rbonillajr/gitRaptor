//! TS-GRP-004: `raptor-mcp` starts the engine on demand and completes the
//! handshake, over a temporary profile (NFR-01). macOS only (`script`);
//! Linux: Pendiente: etapa de validación multiplataforma.
#![cfg(target_os = "macos")]

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use gitraptor_core::daemon::running_pid;
use gitraptor_core::profile::ProfileDirs;

const MCP: &str = env!("CARGO_BIN_EXE_raptor-mcp");

/// The `raptor` that `raptor-mcp` launches lives next to it; build it if
/// this package was tested alone.
fn raptor() -> PathBuf {
    let raptor = PathBuf::from(MCP).with_file_name("raptor");
    if !raptor.exists() {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let status = Command::new(cargo)
            .args(["build", "-q", "-p", "gitraptor-cli", "--bin", "raptor"])
            .status()
            .unwrap();
        assert!(status.success());
    }
    raptor
}

#[test]
fn raptor_mcp_starts_the_engine_on_demand() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("profile");
    let raptor = raptor();
    let out = Command::new(MCP)
        .env_clear()
        .env("GITRAPTOR_PROFILE_DIR", &root)
        .env("GITRAPTOR_AGENT_EXECUTABLES", "raptor-fake-agent")
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("connected to the engine"), "{stderr}");
    let state = ProfileDirs::under_root(&root).state;
    assert!(running_pid(&state).unwrap().is_some());

    // The developer stops it (pty, not under an agent).
    let stop = Command::new("/usr/bin/script")
        .args(["-q", "/dev/null"])
        .arg(&raptor)
        .args(["daemon", "stop", "--yes"])
        .env_clear()
        .env("GITRAPTOR_PROFILE_DIR", &root)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(
        stop.status.success(),
        "{}",
        String::from_utf8_lossy(&stop.stdout)
    );
    let start = Instant::now();
    while running_pid(&state).unwrap().is_some() {
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(20));
    }
}
