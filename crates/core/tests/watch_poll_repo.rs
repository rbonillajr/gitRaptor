//! The repo task reads a repo when asked, without a file event (fix/tm-restore-calm-mark): the
//! Time Machine's calm waits for the engine to persist the refs on disk, so when the file events
//! of a change come late (a busy `fseventsd`), it asks the task to read now instead of waiting
//! for the 30 s backup poll.
//!
//! A testkit fixture; never this repo or the real profile (NFR-01). Every file event is dropped,
//! so only the request can bring the change; the wait is for that batch, with a deadline far below
//! the backup poll.

use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gitraptor_core::observe::{self, reconcile};
use gitraptor_core::watch::{ObservedBatch, Observer, WatchConfig};
use gitraptor_testkit::Fixture;

#[test]
fn a_repo_asked_to_poll_reads_a_change_whose_events_were_lost() {
    let f = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let (tx, rx) = channel::<ObservedBatch>();
    let tx = Arc::new(Mutex::new(tx));
    let observer = Observer::start(
        WatchConfig::default(),
        Arc::new(move |b| {
            let _ = tx.lock().unwrap().send(b);
        }),
    );
    let common = observe::locate(&f.repo).unwrap();
    let read = reconcile(&common, &observe::base_branch(None)).unwrap();
    observer.watch_repo("r", &common, &read);
    // The first re-read of the streams.
    while rx.recv_timeout(Duration::from_millis(400)).is_ok() {}

    observer.simulate_lost_events(true);
    f.git(&["branch", "feat-z"]);
    observer.poll_repo("r");

    let start = Instant::now();
    loop {
        let left = Duration::from_secs(10).saturating_sub(start.elapsed());
        let batch = rx
            .recv_timeout(left)
            .expect("the repo was read when asked, not at the backup poll");
        if batch
            .branches
            .as_ref()
            .is_some_and(|b| b.contains_key("feat-z"))
        {
            break;
        }
    }
}
