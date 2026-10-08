//! L-06 on the client side of the channel (SEC-01, INF-CKP-001 Entrega 2b): before sending a
//! byte, the client refuses a socket folder that is not private and a server that runs as
//! another user. Temporary folders only (NFR-01).
//!
//! A server of another uid needs a second account, which an unprivileged test cannot create.
//! The check is the same code with the uid it expects as a parameter: here the client expects
//! a uid other than the one the fake server runs as.
#![cfg(unix)]

use std::io::{ErrorKind, Read};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use gitraptor_api::client::transport;

/// A fake server in `runtime`: reports how many bytes the first client sent before closing.
fn fake_server(runtime: &Path) -> mpsc::Receiver<usize> {
    let listener = UnixListener::bind(transport::socket_path(runtime)).unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        // The refused client may be gone already (macOS then refuses the timeout).
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = Vec::new();
        let _ = stream.read_to_end(&mut buf);
        let _ = tx.send(buf.len());
    });
    rx
}

fn private_runtime() -> (tempfile::TempDir, std::path::PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let runtime = tmp.path().join("run");
    std::fs::create_dir(&runtime).unwrap();
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
    (tmp, runtime)
}

fn euid() -> u32 {
    rustix::process::geteuid().as_raw()
}

#[test]
fn a_server_of_another_uid_is_refused_before_anything_is_sent() {
    let (_tmp, runtime) = private_runtime();
    let sent = fake_server(&runtime);
    let refused = transport::connect_expecting(&runtime, euid().wrapping_add(1));
    let err = refused.expect_err("a server of another uid was accepted");
    assert_eq!(err.kind(), ErrorKind::PermissionDenied, "{err}");
    // The refused stream was dropped: the server saw the connection close with nothing in it.
    assert_eq!(sent.recv_timeout(Duration::from_secs(10)).unwrap(), 0);
}

#[test]
fn a_server_of_this_uid_is_accepted() {
    let (_tmp, runtime) = private_runtime();
    let _sent = fake_server(&runtime);
    transport::connect(&runtime).expect("the server runs as this user");
}

#[test]
fn an_open_or_foreign_socket_folder_is_refused_without_connecting() {
    let (_tmp, runtime) = private_runtime();
    let _sent = fake_server(&runtime);
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o755)).unwrap();
    let err = transport::connect(&runtime).expect_err("0755 accepted");
    assert_eq!(err.kind(), ErrorKind::PermissionDenied, "{err}");
    if euid() != 0 {
        // `/` belongs to root.
        let err = transport::connect(Path::new("/")).expect_err("/ accepted");
        assert_eq!(err.kind(), ErrorKind::PermissionDenied, "{err}");
    }
}
