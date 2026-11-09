//! Review fixes of the engine supervisor and its launcher: an incompatible report is not cached
//! as a runtime, a repeated incompatibility keeps its path, a gap does not subscribe again, and
//! shutdown does not wait for the thread. Temporary folders only (NFR-01).
#![cfg(unix)]

#[path = "support/fake_daemon.rs"]
mod fake_daemon;
#[path = "support/fake_raptor.rs"]
mod fake_raptor;

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use fake_daemon::FakeDaemon;
use fake_raptor::FakeRaptor;
use gitraptor_api::messages::{EngineStateView, EngineView, ResyncReason as ApiResync};
use gitraptor_desktop::engine::EngineHub;
use gitraptor_desktop::engine::launcher::BundleConnector;
use gitraptor_desktop::engine::link::{
    ConnectFailure, Connector, Incompatibility, IncompatibilityKind, Link,
};
use gitraptor_desktop::engine::supervisor::{self, Sink};
use gitraptor_desktop::engine::wire::{ConnState, EngineMessage, IncompatibleAction};

const DEADLINE: Duration = Duration::from_secs(5);

fn wait_for<T>(
    rx: &Receiver<EngineMessage>,
    what: &str,
    mut pick: impl FnMut(&EngineMessage) -> Option<T>,
) -> T {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(m) => {
                if let Some(found) = pick(&m) {
                    return found;
                }
            }
            Err(e) => panic!("waiting for {what}: {e}"),
        }
    }
}

fn is_live(m: &EngineMessage) -> Option<()> {
    matches!(
        m,
        EngineMessage::Conn {
            state: ConnState::Live,
            ..
        }
    )
    .then_some(())
}

/// Fails with each scripted failure in turn, then keeps repeating the last one.
struct Sequence(VecDeque<ConnectFailure>);

impl Connector for Sequence {
    fn connect(&mut self, _start: bool) -> Result<Box<dyn Link>, ConnectFailure> {
        let next = if self.0.len() > 1 {
            self.0.pop_front()
        } else {
            self.0.front().cloned()
        };
        Err(next.unwrap_or(ConnectFailure::Lost))
    }
}

fn older(path: Option<&str>) -> ConnectFailure {
    ConnectFailure::Incompatible(Incompatibility {
        which: IncompatibilityKind::DaemonOlder,
        daemon_version: Some("0.0.8".to_owned()),
        daemon_path: path.map(str::to_owned),
        missing: Vec::new(),
    })
}

#[test]
fn a_repeated_incompatibility_without_its_path_keeps_the_copy_command() {
    let connector = Sequence(VecDeque::from([
        older(Some("/opt/other/raptor")),
        older(None),
    ]));
    let (tx, rx) = mpsc::channel();
    let handle = supervisor::spawn(Box::new(connector), None, Sink::Collect(tx));
    let expected = ConnState::Incompatible {
        which: gitraptor_desktop::engine::wire::IncompatibleWhich::DaemonOlder,
        daemon_version: Some("0.0.8".to_owned()),
        daemon_path: Some("/opt/other/raptor".to_owned()),
        stop_command: Some("'/opt/other/raptor' 'daemon' 'stop'".to_owned()),
        action: IncompatibleAction::CopyStop,
    };
    wait_for(&rx, "the first incompatible", |m| {
        matches!(m, EngineMessage::Conn { state, .. } if *state == expected).then_some(())
    });
    // The second attempt (the one without a path) must not change what is shown: no later conn
    // may be an incompatible without the command or any other state.
    let deadline = Instant::now() + Duration::from_millis(900);
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        if let Ok(EngineMessage::Conn { state, .. }) = rx.recv_timeout(left) {
            assert_eq!(
                state, expected,
                "the pathless repeat must keep the path and command"
            );
        }
    }
    drop(handle);
}

#[test]
fn an_incompatible_report_is_not_cached_as_a_runtime() {
    let fake = FakeRaptor::printing_fixture("daemon-older.json");
    let mut connector = BundleConnector::for_tests(fake.raptor.clone(), |_| true);
    for round in 0..2 {
        match connector.connect(round == 0) {
            Err(ConnectFailure::Incompatible(inc)) => assert_eq!(
                inc.daemon_path.as_deref(),
                Some("/opt/homebrew/bin/raptor"),
                "round {round}: the path must survive"
            ),
            Err(_) => panic!("round {round}: not Incompatible"),
            Ok(_) => panic!("round {round}: connected instead of reporting incompatible"),
        }
    }
    assert_eq!(
        fake.calls().len(),
        2,
        "an incompatible report must not cache the runtime"
    );
}

fn subscribes(daemon: &FakeDaemon) -> usize {
    daemon.calls_of("scope.subscribe").len()
}

#[test]
fn a_gap_does_not_subscribe_again_but_a_scope_resync_does() {
    let daemon = FakeDaemon::start();
    let (tx, rx) = mpsc::channel();
    let handle = supervisor::spawn(Box::new(daemon.connector()), None, Sink::Collect(tx));
    wait_for(&rx, "live", is_live);
    daemon.set_global_position("run-1", 20);
    let skipped = EngineView {
        state: EngineStateView::NoRepos,
        git_version: None,
    };
    assert!(daemon.push_global_event(9, "engine.state", &skipped));
    wait_for(&rx, "syncing", |m| {
        matches!(
            m,
            EngineMessage::Conn {
                state: ConnState::Syncing,
                ..
            }
        )
        .then_some(())
    });
    wait_for(&rx, "live after the gap", is_live);
    assert_eq!(daemon.calls_of("scope.snapshot").len(), 2);
    assert_eq!(
        subscribes(&daemon),
        1,
        "a gap takes a snapshot, not a new subscription"
    );

    daemon.set_global_position("run-1", 30);
    assert!(daemon.push_scope_resync(ApiResync::ReplayUnavailable));
    wait_for(&rx, "syncing after scope.resync", |m| {
        matches!(
            m,
            EngineMessage::Conn {
                state: ConnState::Syncing,
                ..
            }
        )
        .then_some(())
    });
    wait_for(&rx, "live after the resync", is_live);
    assert_eq!(subscribes(&daemon), 2, "scope.resync subscribes again");
    drop(handle);
}

#[test]
fn shutdown_does_not_wait_for_a_connect_in_flight() {
    struct Slow;
    impl Connector for Slow {
        fn connect(&mut self, _start: bool) -> Result<Box<dyn Link>, ConnectFailure> {
            std::thread::sleep(Duration::from_secs(3));
            Err(ConnectFailure::Lost)
        }
    }
    let hub = EngineHub::new(Box::new(Slow), None);
    let (tx, _rx) = mpsc::channel();
    hub.watch(Sink::Collect(tx));
    let started = Instant::now();
    hub.shutdown();
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "shutdown waited {:?} for the supervisor thread",
        started.elapsed()
    );
}
