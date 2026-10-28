//! TS-GRP-005 T006 (Guard 6.2): the periodic reconciliation of the observer runs in `Utility`
//! and the thread is back in `Default` afterwards (D5). Observed through the batch sink, which
//! the worktree task calls on its own thread: inside `Task::periodic` for the batch that closes
//! a periodic gap, and outside it for any other batch. Temporary repo (NFR-01).
#![cfg(target_os = "macos")]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gitraptor_core::observe::{self, reconcile};
use gitraptor_core::profile::GapCause;
use gitraptor_core::watch::{Observer, WatchBackend, WatchConfig};
use gitraptor_core::work_class::{self, WorkClass};
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;

/// (is a periodic-reconciliation batch, the class the sink's thread reads from the OS).
type Seen = Arc<Mutex<Vec<(bool, Option<WorkClass>)>>>;

fn wait_for(what: &str, seen: &Seen, done: impl Fn(&[(bool, Option<WorkClass>)]) -> bool) {
    let start = Instant::now();
    while !done(&seen.lock().unwrap()) {
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "timed out waiting for {what}; seen {:?}",
            seen.lock().unwrap()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_periodic_reconciliation_runs_in_utility_and_returns_to_default() {
    let f = Fixture::new(&git_from_path());
    f.write("login.txt", "user\n");
    f.git(&["add", "login.txt"]);
    f.git(&["commit", "-q", "-m", "login"]);
    f.git(&["branch", "feat-login"]);
    let wt = f
        .add_worktree("feat-login", "feat-login")
        .canonicalize()
        .unwrap();

    let seen: Seen = Arc::default();
    let sink_seen = seen.clone();
    let config = WatchConfig {
        backend: WatchBackend::Fsevents,
        periodic: Duration::from_secs(1),
        ..WatchConfig::default()
    };
    let observer = Observer::start(
        config,
        Arc::new(move |b| {
            let periodic = b
                .gap
                .is_some_and(|g| g.cause == GapCause::PeriodicReconciliation);
            let class = work_class::current();
            sink_seen.lock().unwrap().push((periodic, class));
        }),
    );
    let common = observe::locate(&f.repo).unwrap();
    let read = reconcile(&common, &observe::base_branch(None)).unwrap();
    observer.watch_repo("r", &common, &read);

    // A change whose event is lost: only the periodic reconciliation finds it, in a gap.
    observer.simulate_lost_events(true);
    std::fs::write(wt.join("login.txt"), "lost\n").unwrap();
    std::thread::sleep(Duration::from_millis(200));
    observer.simulate_lost_events(false);
    wait_for("the periodic reconciliation batch", &seen, |s| {
        s.iter().any(|(p, _)| *p)
    });
    {
        let s = seen.lock().unwrap();
        let (_, class) = s.iter().find(|(p, _)| *p).unwrap();
        assert_eq!(
            *class,
            Some(WorkClass::Utility),
            "Task::periodic must run in Utility"
        );
    }

    // A normal window after it, on the same thread: back in Default.
    let before = seen.lock().unwrap().len();
    std::fs::write(wt.join("login.txt"), "after\n").unwrap();
    wait_for("a batch outside the periodic reconciliation", &seen, |s| {
        s[before..].iter().any(|(p, _)| !*p)
    });
    let s = seen.lock().unwrap();
    let (_, class) = s[before..].iter().find(|(p, _)| !*p).unwrap();
    assert_ne!(
        *class,
        Some(WorkClass::Utility),
        "after the periodic reconciliation the thread must be back in its own class"
    );
    assert_ne!(*class, Some(WorkClass::Background));
}
