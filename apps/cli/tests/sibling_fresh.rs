//! `raptor-mcp`, the sibling binary the `mcp_*` tests launch, is rebuilt when its sources change
//! (no manual `cargo build -p gitraptor-mcp` after a rebase). Its own test file: the one test
//! touches a source of `apps/mcp`, so no other test of this binary may run beside it.

use std::fs::{File, OpenOptions};
use std::path::Path;
use std::time::SystemTime;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");

fn mtime(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .expect("mtime")
}

#[test]
fn sibling_bin_rebuilds_a_stale_raptor_mcp() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../mcp/src/main.rs");
    let src = src.canonicalize().expect("apps/mcp/src/main.rs");
    let exe = gitraptor_testkit::sibling_bin(Path::new(RAPTOR), "gitraptor-mcp", "raptor-mcp");
    let built = mtime(&exe);

    // Same content, newer mtime: all cargo needs to consider the binary stale. Waiting for the
    // clock is not needed: the new mtime is set explicitly past the binary's.
    let newer = built + std::time::Duration::from_secs(2);
    OpenOptions::new()
        .write(true)
        .open(&src)
        .and_then(|f: File| f.set_modified(newer))
        .expect("touch main.rs");

    // The helper builds once per process, so ask cargo through the same code path in a child:
    // this test binary itself, run for the probe below.
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "probe", "--ignored", "--nocapture"])
        .output()
        .expect("probe");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        mtime(&exe) > built,
        "raptor-mcp was not rebuilt after its source changed"
    );
}

/// Run by the test above in a fresh process (the helper's once-per-process guard).
#[test]
#[ignore = "run by sibling_bin_rebuilds_a_stale_raptor_mcp"]
fn probe() {
    gitraptor_testkit::sibling_bin(Path::new(RAPTOR), "gitraptor-mcp", "raptor-mcp");
}
