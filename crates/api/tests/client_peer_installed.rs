//! DSK002-B3 (TS-DSK-002 T003, ADR-GRP-017 § 1): before the handshake, the client can tell
//! whether the process serving the socket is an installed `raptor` of this user
//! (`peer::installed_raptor_peer`), from the executable of its pid (`peer::executable_of`).
//!
//! Temporary folders only (NFR-01). The `raptor` copy lives under `CARGO_TARGET_TMPDIR`,
//! outside the fixed temporary folders the rule refuses. macOS and Linux (`/proc`).
#![cfg(any(target_os = "macos", target_os = "linux"))]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use gitraptor_api::client::peer::{executable_of, installed_raptor_peer};

/// Kills the child when the test ends, whatever its outcome.
struct Running(Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn sleep_binary() -> PathBuf {
    ["/bin/sleep", "/usr/bin/sleep"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .expect("a `sleep` binary in /bin or /usr/bin")
}

#[test]
fn dsk002_b3_executable_of_this_process_is_its_executable() {
    let expected = std::env::current_exe().unwrap().canonicalize().unwrap();
    let got = executable_of(std::process::id())
        .expect("DSK002-B3: executable_of gave no path for this process");
    assert_eq!(
        got.canonicalize().unwrap_or_else(|_| got.clone()),
        expected,
        "DSK002-B3: executable_of(own pid) is not this process's executable"
    );
}

#[test]
fn dsk002_b3_a_peer_whose_executable_is_not_named_raptor_is_refused() {
    // This test binary is not named `raptor`.
    assert!(
        !installed_raptor_peer(std::process::id()),
        "DSK002-B3: a process whose executable is not `raptor` was taken as an installed raptor"
    );
}

#[test]
fn dsk002_b3_a_peer_run_from_an_own_0755_raptor_copy_is_accepted() {
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let raptor = bin.join("raptor");
    std::fs::copy(sleep_binary(), &raptor).unwrap();
    std::fs::set_permissions(&raptor, std::fs::Permissions::from_mode(0o755)).unwrap();
    let child = Running(
        Command::new(&raptor)
            .arg("30")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    assert!(
        installed_raptor_peer(child.0.id()),
        "DSK002-B3: a process run from an own 0755 `raptor` outside the temporary folders \
         was refused: {}",
        raptor.display()
    );
}
