//! `sibling_bin` asks cargo to build the sibling even when a binary is already there (a stale one
//! is the failure it prevents), and only once per process. Cargo is a fake that logs its argv, in
//! a temporary directory: no real build runs and nothing of the checkout is touched.

use std::path::Path;

const DIR_ENV: &str = "SIBLING_PROBE_DIR";

#[test]
fn sibling_bin_builds_even_if_the_binary_exists_and_only_once() {
    let tmp = tempfile::tempdir().unwrap();
    let debug = tmp.path().join("target").join("debug");
    std::fs::create_dir_all(&debug).unwrap();
    std::fs::write(tmp.path().join("target").join("CACHEDIR.TAG"), "").unwrap();
    let suffix = std::env::consts::EXE_SUFFIX;
    std::fs::write(debug.join(format!("raptor{suffix}")), "").unwrap();
    std::fs::write(debug.join(format!("raptor-mcp{suffix}")), "stale").unwrap();

    let log = tmp.path().join("argv.log");
    let (fake, body) = if cfg!(windows) {
        (
            "cargo.cmd",
            format!("@echo %* >> \"{}\"\r\n", log.display()),
        )
    } else {
        (
            "cargo.sh",
            format!("#!/bin/sh\necho \"$@\" >> '{}'\n", log.display()),
        )
    };
    let fake = tmp.path().join(fake);
    std::fs::write(&fake, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // A fresh process, so the fake `CARGO` never reaches the other tests of this binary.
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "probe", "--ignored", "--nocapture"])
        .env(DIR_ENV, &debug)
        .env("CARGO", &fake)
        .output()
        .expect("probe");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let logged = std::fs::read_to_string(&log).expect("cargo was never invoked");
    assert_eq!(
        logged.lines().count(),
        1,
        "built once per process: {logged}"
    );
    for want in [
        "build",
        "-p gitraptor-mcp",
        "--bin raptor-mcp",
        "--profile dev",
    ] {
        assert!(logged.contains(want), "missing {want:?} in {logged}");
    }
}

/// Run by the test above in a fresh process (the helper's once-per-process guard).
#[test]
#[ignore = "run by sibling_bin_builds_even_if_the_binary_exists_and_only_once"]
fn probe() {
    let debug = std::env::var_os(DIR_ENV).expect("run by the parent test");
    let next_to = Path::new(&debug).join(format!("raptor{}", std::env::consts::EXE_SUFFIX));
    gitraptor_testkit::sibling_bin(&next_to, "gitraptor-mcp", "raptor-mcp");
    gitraptor_testkit::sibling_bin(&next_to, "gitraptor-mcp", "raptor-mcp");
}
