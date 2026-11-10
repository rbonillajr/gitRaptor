//! TD-GRP-006 (ADR-GRP-010, Enmienda 2026-11-10): every S3 read notice says what made the repo
//! task read the repo that found the write: the window of a file event (`ReadVia::Event`), a read
//! asked with `Observer::poll_repo` (`ReadVia::Wake`) or the backup poll's timer, or
//! `Observer::simulate_poll` standing for it (`ReadVia::Backup`). Observed through the
//! `git_dir_written` hook of the observer, without the daemon. Temporary repos (NFR-01); every
//! wait is for a signal, bounded by attempts.

use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gitraptor_core::observe::{self, reconcile};
use gitraptor_core::watch::{Observer, ObserverHooks, ReadVia, WatchConfig};
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;

/// The `(repo_id, via)` of every `git_dir_written` the observer calls.
struct Reads(Mutex<Sender<(String, ReadVia)>>);

impl ObserverHooks for Reads {
    fn worktree_touched(&self, _: &str, _: &Path) {}
    fn git_dir_touched(&self, _: &str, _: u64) {}
    fn git_dir_written(&self, repo_id: &str, _: u64, _: u64, via: ReadVia) {
        let _ = self.0.lock().unwrap().send((repo_id.to_owned(), via));
    }
}

/// How long one wait for a read notice lasts before it is a hang.
const READ_WAIT: Duration = Duration::from_secs(30);
/// How long the notices of one read may keep coming after the first.
const QUIET: Duration = Duration::from_millis(500);
/// A backup poll that never comes within a test.
const FAR: Duration = Duration::from_secs(3600);

struct Watched {
    f: Fixture,
    observer: Observer,
    reads: Receiver<(String, ReadVia)>,
    commits: u32,
}

fn watched(backup_poll: Duration, lose_events: bool) -> Watched {
    let f = Fixture::new(&git_from_path());
    f.write("login.txt", "user\n");
    f.git(&["add", "login.txt"]);
    f.git(&["commit", "-q", "-m", "login"]);
    let (tx, reads) = channel();
    let config = WatchConfig {
        backup_poll,
        ..WatchConfig::default()
    };
    let observer = Observer::start_counted(
        config,
        Arc::new(|_| {}),
        Some(Arc::new(Reads(Mutex::new(tx)))),
        Arc::new(AtomicU64::new(0)),
    );
    // Before the repo is watched: no file event of the commits wins the race to the read.
    observer.simulate_lost_events(lose_events);
    let common = observe::locate(&f.repo).unwrap();
    let read = reconcile(&common, &observe::base_branch(None)).unwrap();
    observer.watch_repo("r", &common, &read);
    Watched {
        f,
        observer,
        reads,
        commits: 0,
    }
}

impl Watched {
    /// A commit in the watched worktree: its `HEAD` moves.
    fn commit(&mut self) {
        self.commits += 1;
        let msg = format!("step {}", self.commits);
        self.f.git(&["commit", "-q", "--allow-empty", "-m", &msg]);
    }

    /// The next read notice, `None` after `wait`.
    fn next(&self, wait: Duration) -> Option<(String, ReadVia)> {
        match self.reads.recv_timeout(wait) {
            Ok(r) => Some(r),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => panic!("the observer hooks are gone"),
        }
    }

    /// The notices of one read: the first within `READ_WAIT`, then the rest until quiet.
    fn read_notices(&self, what: &str) -> Vec<(String, ReadVia)> {
        let first = self
            .next(READ_WAIT)
            .unwrap_or_else(|| panic!("no git_dir_written after {what} in {READ_WAIT:?}"));
        let mut out = vec![first];
        while let Some(r) = self.next(QUIET) {
            out.push(r);
        }
        out
    }
}

fn assert_all(seen: &[(String, ReadVia)], want: ReadVia, what: &str) {
    assert!(!seen.is_empty(), "{what}: no read notice");
    for (repo, via) in seen {
        assert_eq!(repo, "r", "{what}: the notice is of the watched repo");
        assert_eq!(
            *via, want,
            "{what}: the read notice must carry ReadVia::{want:?} (what made the task read); \
             seen {seen:?}"
        );
    }
}

/// GRP006-VIA-WAKE: with the file events lost and the backup poll far away, a commit followed by
/// `Observer::poll_repo` is found by the read that poll asked for: `ReadVia::Wake`, and no notice
/// with another via.
#[test]
fn grp006_via_wake_a_poll_repo_read_is_a_wake() {
    let mut w = watched(FAR, true);
    w.commit();
    w.observer.poll_repo("r");
    let seen = w.read_notices("a commit and poll_repo");
    assert_all(&seen, ReadVia::Wake, "poll_repo");
}

/// GRP006-VIA-BACKUP (the timer): with the file events lost and a backup poll of 1 s, a commit
/// without `poll_repo` nor `simulate_poll` is found by the timer: `ReadVia::Backup`, and no notice
/// with another via.
#[test]
fn grp006_via_backup_the_timer_read_is_a_backup() {
    let mut w = watched(Duration::from_secs(1), true);
    w.commit();
    let seen = w.read_notices("a commit and the backup poll's timer");
    assert_all(&seen, ReadVia::Backup, "the backup poll's timer");
}

/// GRP006-VIA-BACKUP (its test hook): in another observer, with the file events lost and the
/// backup poll far away (the timer cannot win the race), a commit followed by
/// `Observer::simulate_poll`, which stands for the timer, also gives `ReadVia::Backup`, and no
/// notice with another via.
#[test]
fn grp006_via_backup_a_simulate_poll_read_is_a_backup() {
    let mut w = watched(FAR, true);
    w.commit();
    w.observer.simulate_poll();
    let seen = w.read_notices("a commit and simulate_poll");
    assert_all(&seen, ReadVia::Backup, "simulate_poll");
}

/// Attempts of a commit whose file event the OS may lose under load (FSEvents drops them
/// silently): 4 × `READ_WAIT` is the hang guard (~120 s).
const EVENT_ATTEMPTS: u32 = 4;

/// GRP006-VIA-EVENT: with the file events delivered and the backup poll far away, and no
/// `poll_repo` nor `simulate_poll`, the read that finds a commit is the window of its file event:
/// `ReadVia::Event`. The commit is repeated, counting attempts, when the OS loses its event. Then,
/// in the same observer, a read asked with `poll_repo` while the events are lost is a
/// `ReadVia::Wake`: the via follows what made the task read, it is not a constant.
#[test]
fn grp006_via_event_a_file_event_read_is_an_event() {
    let mut w = watched(FAR, false);
    let mut seen = Vec::new();
    for _ in 0..EVENT_ATTEMPTS {
        w.commit();
        if let Some(first) = w.next(READ_WAIT) {
            seen.push(first);
            while let Some(r) = w.next(QUIET) {
                seen.push(r);
            }
            break;
        }
    }
    assert!(
        !seen.is_empty(),
        "no git_dir_written from the file events of {EVENT_ATTEMPTS} commits"
    );
    assert_all(&seen, ReadVia::Event, "the window of a file event");

    w.observer.simulate_lost_events(true);
    w.commit();
    w.observer.poll_repo("r");
    let seen = w.read_notices("a commit and poll_repo after the event reads");
    assert_all(&seen, ReadVia::Wake, "poll_repo after the event reads");
}
