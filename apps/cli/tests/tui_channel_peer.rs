//! L-06 from the TUI (INF-CKP-001 Entrega 2b, SEC-01): with the socket's folder open (0755)
//! or owned by another user, the headless `App` shows "Channel rejected" and the server
//! never receives a byte (no handshake). Temporary folders only (NFR-01).
//!
//! A server that runs as another user needs a second account: that branch of the same check
//! is covered in `crates/api/tests/client_peer.rs`, with the uid it expects as a parameter.
#![cfg(unix)]

use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use gitraptor_api::client::{Connect, NeverLaunch, transport};
use gitraptor_api::messages::ClientKind;
use gitraptor_cli::client;
use gitraptor_cli::client::engine::EngineConnector;
use gitraptor_cli::model::{ConnState, Model, Size};
use gitraptor_cli::present::i18n::Lang;
use gitraptor_cli::queue;
use gitraptor_cli::tui::app::App;
use ratatui::Terminal;
use ratatui::backend::TestBackend;

/// A fake server in `runtime` that counts every byte any client sends it.
fn fake_server(runtime: &Path) -> Arc<AtomicUsize> {
    let listener = UnixListener::bind(transport::socket_path(runtime)).unwrap();
    let received = Arc::new(AtomicUsize::new(0));
    let count = received.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let count = count.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 256];
                while let Ok(n) = stream.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    count.fetch_add(n, Ordering::SeqCst);
                }
            });
        }
    });
    received
}

/// Steps a headless TUI on `runtime` until it shows `want`, or panics after 10 s.
fn tui_reaches(runtime: PathBuf, want: ConnState) {
    let (inbox, _input, engine) = queue::inbox();
    let mut app = App::new(
        Terminal::new(TestBackend::new(80, 24)).unwrap(),
        Model::new(
            Lang::En,
            Size {
                width: 80,
                height: 24,
            },
        ),
        inbox,
    );
    let connector = EngineConnector::new(
        Connect::new(runtime, ClientKind::Cli),
        Box::new(NeverLaunch),
    );
    let channel = client::spawn(connector, None, engine);
    app.attach(channel.cmds.clone());
    let start = Instant::now();
    while app.model.conn != want {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the TUI stayed in {:?}",
            app.model.conn
        );
        app.step(Duration::from_millis(10)).unwrap();
    }
    channel.shutdown();
}

#[test]
fn an_open_socket_folder_is_rejected_without_a_handshake() {
    let tmp = tempfile::tempdir().unwrap();
    let runtime = tmp.path().join("run");
    std::fs::create_dir(&runtime).unwrap();
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
    let received = fake_server(&runtime);
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o755)).unwrap();

    tui_reaches(runtime, ConnState::Rejected);
    assert_eq!(received.load(Ordering::SeqCst), 0, "the server got bytes");
}

#[test]
fn a_socket_folder_of_another_user_is_rejected() {
    if rustix::process::geteuid().is_root() {
        return; // `/` would be this user's.
    }
    // `/` belongs to root: the check refuses it before connecting to anything.
    tui_reaches(PathBuf::from("/"), ConnState::Rejected);
}

#[test]
fn a_private_folder_without_a_server_is_not_rejected() {
    // Control: the same TUI on a private, empty folder says the engine is unavailable
    // (nothing to start: `NeverLaunch`), not that the channel was rejected.
    let tmp = tempfile::tempdir().unwrap();
    let runtime = tmp.path().join("run");
    std::fs::create_dir(&runtime).unwrap();
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
    tui_reaches(runtime, ConnState::EngineUnavailable);
}
