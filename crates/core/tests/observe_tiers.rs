//! TS-GRP-006: the observation tiers of the observer, without the daemon
//! (ADR-GRP-010, Enmienda 2026-10-07): a dormant repo keeps its watches as a
//! sentinel, wakes once on its first change, loses nothing it missed while
//! asleep, and its metadata sweep finds what the sentinel did not signal.
//! Temporary repos of the "intact repo" harness (NFR-01). The transitions
//! are driven by the test (`sleep_repo`, `sweep_now`, `wake_repo`), so they
//! are deterministic: every wait is for a signal, never a fixed sleep.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use gitraptor_api::messages::{GitEventKind, WorktreeStatus};
use gitraptor_core::observe::{self, reconcile};
use gitraptor_core::profile::GapCause;
use gitraptor_core::watch::{
    ObservedBatch, Observer, ObserverHooks, RawEvent, SleepRefused, Tier, WakeCause, WatchConfig,
};
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Exceptions, Fixture, check};

/// The wakes the observer asks for.
struct Wakes(Mutex<Sender<(String, WakeCause)>>);

impl ObserverHooks for Wakes {
    fn worktree_touched(&self, _: &str, _: &Path) {}
    fn git_dir_touched(&self, _: &str, _: u64) {}
    fn repo_wake(&self, repo_id: &str, cause: WakeCause) {
        let _ = self.0.lock().unwrap().send((repo_id.to_owned(), cause));
    }
}

struct Watched {
    observer: Observer,
    rx: Receiver<ObservedBatch>,
    wakes: Receiver<(String, WakeCause)>,
    roots: Arc<AtomicU64>,
    common: PathBuf,
}

const WAIT: Duration = Duration::from_secs(10);

fn watch(f: &Fixture) -> Watched {
    watch_with(f, WatchConfig::default())
}

fn watch_with(f: &Fixture, config: WatchConfig) -> Watched {
    let (tx, rx) = channel();
    let tx = Arc::new(Mutex::new(tx));
    let (wake_tx, wakes) = channel();
    let roots = Arc::new(AtomicU64::new(0));
    let observer = Observer::start_counted(
        config,
        Arc::new(move |b| {
            let _ = tx.lock().unwrap().send(b);
        }),
        Some(Arc::new(Wakes(Mutex::new(wake_tx)))),
        Arc::clone(&roots),
    );
    let common = observe::locate(&f.repo).unwrap();
    let read = reconcile(&common, &observe::base_branch(None)).unwrap();
    observer.watch_repo("r", &common, &read);
    let w = Watched {
        observer,
        rx,
        wakes,
        roots,
        common,
    };
    // Let the streams start and the first re-read settle.
    w.drain(Duration::from_millis(400));
    w
}

impl Watched {
    fn drain(&self, quiet: Duration) -> Vec<ObservedBatch> {
        let mut out = Vec::new();
        while let Ok(b) = self.rx.recv_timeout(quiet) {
            out.push(b);
        }
        out
    }

    /// Batches until `done` holds on everything seen, or 10 s.
    fn until(&self, done: impl Fn(&[ObservedBatch]) -> bool) -> Vec<ObservedBatch> {
        let start = Instant::now();
        let mut seen = Vec::new();
        while start.elapsed() < WAIT {
            if let Ok(b) = self.rx.recv_timeout(Duration::from_millis(100)) {
                seen.push(b);
                if done(&seen) {
                    seen.extend(self.drain(Duration::from_millis(300)));
                    return seen;
                }
            }
        }
        panic!("timed out: {seen:#?}");
    }

    fn sleep(&self) {
        self.observer.sleep_repo("r").unwrap();
        assert_eq!(self.observer.tier("r"), Some(Tier::Dormant));
        // Whatever the tasks flushed on their way out.
        self.drain(Duration::from_millis(200));
    }

    fn next_wake(&self) -> WakeCause {
        let (repo, cause) = self.wakes.recv_timeout(WAIT).expect("no wake");
        assert_eq!(repo, "r");
        cause
    }

    /// The daemon's part of a wake: reconcile, then hand the read over.
    fn wake(&self, cause: WakeCause) {
        let read = reconcile(&self.common, &observe::base_branch(None)).unwrap();
        self.observer.wake_repo("r", &read, cause);
        assert_eq!(self.observer.tier("r"), Some(Tier::Active));
    }
}

fn events(batches: &[ObservedBatch]) -> Vec<RawEvent> {
    batches.iter().flat_map(|b| b.events.clone()).collect()
}

fn unstaged(b: &ObservedBatch, root: &Path) -> Option<u32> {
    b.worktrees
        .iter()
        .find(|w| Path::new(w.view.path.raw()) == root)
        .and_then(|w| match &w.view.status {
            WorktreeStatus::Ready { counts, .. } => Some(counts.unstaged),
            _ => None,
        })
}

fn demo() -> Fixture {
    let f = Fixture::new(&git_from_path());
    f.write("login.txt", "user\n");
    f.write(".gitignore", "target/\n");
    f.git(&["add", "login.txt", ".gitignore"]);
    f.git(&["commit", "-q", "-m", "login"]);
    f
}

/// N1: a dormant repo keeps every watch and has no task left.
#[test]
fn dormant_keeps_watches_and_stops_tasks() {
    let f = demo();
    let w = watch(&f);
    let watched = w.roots.load(Ordering::Relaxed);
    assert!(watched > 0);
    w.sleep();
    assert!(w.observer.worktrees("r").is_empty());
    assert_eq!(w.roots.load(Ordering::Relaxed), watched);
    assert_eq!(w.observer.sleep_repo("r"), Err(SleepRefused::NotActive));
    assert_eq!(w.observer.sleep_repo("x"), Err(SleepRefused::Unknown));
    // Retiring it closes the watches it kept.
    w.observer.forget_repo("r");
    assert_eq!(w.roots.load(Ordering::Relaxed), 0);
}

/// N2, N4, N5: an edit wakes the dormant repo through its sentinel, and the
/// wake publishes it without a gap.
#[test]
fn an_edit_wakes_a_dormant_repo_without_a_gap() {
    let f = demo();
    let w = watch(&f);
    w.sleep();
    let root = observe::canonical(&f.repo);
    std::fs::write(root.join("login.txt"), "user\npassword\n").unwrap();
    assert_eq!(w.next_wake(), WakeCause::Sentinel);
    assert_eq!(w.observer.tier("r"), Some(Tier::Waking));
    w.wake(WakeCause::Sentinel);
    let batches = w.until(|bs| bs.iter().any(|b| unstaged(b, &root) == Some(1)));
    assert!(batches.iter().all(|b| b.gap.is_none()), "{batches:#?}");
    // Active again: the next edit reaches its task as before.
    std::fs::write(root.join("login.txt"), "x\n").unwrap();
    std::fs::write(root.join("new.txt"), "n\n").unwrap();
    w.until(|bs| {
        bs.iter().any(|b| {
            b.worktrees.iter().any(|r| match &r.view.status {
                WorktreeStatus::Ready { counts, .. } => counts.untracked == 1,
                _ => false,
            })
        })
    });
}

/// A write in an ignored folder of a dormant repo does not wake it.
#[test]
fn an_ignored_write_does_not_wake() {
    let f = demo();
    let root = observe::canonical(&f.repo);
    // The build folder exists before the repo sleeps, as it would.
    std::fs::create_dir_all(root.join("target/debug")).unwrap();
    let w = watch(&f);
    w.sleep();
    let seen = w.observer.sentinel_seen();
    for i in 0..20 {
        std::fs::write(root.join(format!("target/debug/o{i}")), "o").unwrap();
    }
    let start = Instant::now();
    // Wait for the sentinel to have looked at them, then check it slept on.
    while w.observer.sentinel_seen() == seen && start.elapsed() < WAIT {
        std::thread::yield_now();
    }
    assert!(
        w.observer.sentinel_seen() > seen,
        "the sentinel saw nothing"
    );
    assert_eq!(w.observer.tier("r"), Some(Tier::Dormant));
    assert!(w.wakes.try_recv().is_err());
}

/// Condition of the coordinator (2026-10-07) and TS-GRP-006 "Reflog": a
/// burst while dormant wakes the repo once, and after the wake every change
/// is classified, in order: three commits are three events.
#[test]
fn a_burst_while_dormant_wakes_once_and_loses_nothing() {
    let f = demo();
    let w = watch(&f);
    w.sleep();
    let root = observe::canonical(&f.repo);
    let mut commits = Vec::new();
    for i in 0..3 {
        for j in 0..10 {
            std::fs::write(root.join(format!("f{i}-{j}.txt")), format!("{i}{j}")).unwrap();
        }
        f.git(&["add", "."]);
        f.git(&["commit", "-q", "-m", &format!("c{i}")]);
        commits.push(f.git(&["rev-parse", "HEAD"]).trim().to_owned());
    }
    std::fs::write(root.join("login.txt"), "dirty\n").unwrap();
    assert_eq!(w.next_wake(), WakeCause::Sentinel);
    // Nothing else asks for a wake while the repo is waking.
    assert!(w.wakes.recv_timeout(Duration::from_millis(500)).is_err());
    w.wake(WakeCause::Sentinel);
    let batches = w.until(|bs| {
        events(bs)
            .iter()
            .filter(|e| e.kind == GitEventKind::Commit)
            .count()
            >= 3
            && bs.iter().any(|b| unstaged(b, &root) == Some(1))
    });
    let seen: Vec<String> = events(&batches)
        .into_iter()
        .filter(|e| e.kind == GitEventKind::Commit)
        .filter_map(|e| e.details.new_commit)
        .collect();
    assert_eq!(seen, commits, "{batches:#?}");
    assert!(batches.iter().all(|b| b.gap.is_none()));
    assert!(w.wakes.try_recv().is_err());
}

/// N3, N5: with the sentinel blind, the metadata sweep finds a commit and
/// the wake publishes it in a `dormant` gap, unattributed.
#[test]
fn the_sweep_finds_a_commit_the_sentinel_missed() {
    let f = demo();
    let w = watch(&f);
    w.sleep();
    // Nothing changed: a sweep wakes nothing.
    w.observer.sweep_now();
    assert_eq!(w.observer.tier("r"), Some(Tier::Dormant));
    w.observer.simulate_lost_events(true);
    f.write("b.txt", "b\n");
    f.git(&["add", "b.txt"]);
    f.git(&["commit", "-q", "-m", "b"]);
    let commit = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
    w.observer.sweep_now();
    let cause = w.next_wake();
    assert!(matches!(cause, WakeCause::SafetyNet { .. }), "{cause:?}");
    w.observer.simulate_lost_events(false);
    w.wake(cause);
    let batches = w.until(|bs| {
        events(bs).iter().any(|e| {
            e.kind == GitEventKind::Commit && e.details.new_commit.as_deref() == Some(&commit)
        })
    });
    let gap = batches.iter().find_map(|b| b.gap).expect("no gap");
    assert_eq!(gap.cause, GapCause::Dormant);
    assert!(gap.started_ms <= gap.ended_ms);
    assert!(
        events(&batches)
            .iter()
            .any(|e| e.kind == GitEventKind::Reconciled)
    );
}

/// NFR-01: sleeping, waking and sweeping write nothing in the repo.
#[test]
fn repo_intact_tiers_write_nothing() {
    let f = demo();
    let report = check("tiers", &f, &Exceptions::none(), || {
        let w = watch(&f);
        w.sleep();
        w.observer.sweep_now();
        w.wake(WakeCause::Sentinel);
        w.drain(Duration::from_millis(300));
    });
    report.assert_intact();
}

/// N3: the slow reconciliation reads a dormant repo in full. Unchanged, it
/// wakes nothing; an edit without `git add` the sentinel missed (the
/// metadata print cannot see it) wakes the repo in a `dormant` gap.
#[test]
fn the_slow_reconcile_finds_an_edit_the_sentinel_missed() {
    let f = demo();
    let w = watch_with(
        &f,
        WatchConfig {
            dormant_reconcile: Duration::ZERO,
            // The budget would stretch the interval to 1 000 times a read.
            reconcile_budget_ppm: u32::MAX,
            ..WatchConfig::default()
        },
    );
    w.sleep();
    let reads = w.observer.recomputes();
    w.observer.sweep_now();
    assert!(
        w.observer.recomputes() > reads,
        "no slow reconciliation ran"
    );
    assert_eq!(w.observer.tier("r"), Some(Tier::Dormant));
    assert!(w.wakes.try_recv().is_err());

    w.observer.simulate_lost_events(true);
    let root = observe::canonical(&f.repo);
    std::fs::write(root.join("login.txt"), "user\npassword\n").unwrap();
    w.observer.sweep_now();
    let cause = w.next_wake();
    assert!(matches!(cause, WakeCause::SafetyNet { .. }), "{cause:?}");
    w.observer.simulate_lost_events(false);
    w.wake(cause);
    let batches = w.until(|bs| bs.iter().any(|b| unstaged(b, &root) == Some(1)));
    let gap = batches.iter().find_map(|b| b.gap).expect("no gap");
    assert_eq!(gap.cause, GapCause::Dormant);
    // At 0.1 % of a core, the same read would set the interval.
    assert!(w.observer.reconcile_interval() < Duration::from_secs(1));
}

/// N1: a worktree in degraded mode has no sentinel, so its repo never
/// sleeps.
#[test]
fn a_degraded_worktree_keeps_the_repo_active() {
    let f = demo();
    let (tx, _rx) = channel();
    let tx = Arc::new(Mutex::new(tx));
    let observer = Observer::start(
        WatchConfig::default(),
        Arc::new(move |b| {
            let _ = tx.lock().unwrap().send(b);
        }),
    );
    observer.simulate_watch_failure(true);
    let common = observe::locate(&f.repo).unwrap();
    let read = reconcile(&common, &observe::base_branch(None)).unwrap();
    observer.watch_repo("r", &common, &read);
    assert_eq!(observer.sleep_repo("r"), Err(SleepRefused::Degraded));
    assert_eq!(observer.tier("r"), Some(Tier::Active));
}

/// N3: the metadata sweep never launches `git` (ADR-GRP-009). The test runs
/// itself again with a `git` shim first in the `PATH` that logs every call
/// (`unsafe` is forbidden, so `set_var` is out), as INF-GRP-001 does: 100
/// sweep cycles add no line to its log.
#[cfg(unix)]
#[test]
fn the_sweep_spawns_no_git() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let real = git_from_path();
    let log = dir.path().join("git.log");
    let shim = dir.path().join("git");
    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\nexec '{}' \"$@\"\n",
            log.display(),
            real.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "sweep_cycles_under_a_git_shim",
            "--ignored",
            "--test-threads=1",
        ])
        .env("PATH", path)
        .env("RAPTOR_TEST_GIT_LOG", &log)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success() && stdout.contains("1 passed"),
        "{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The child of [`the_sweep_spawns_no_git`]: the fixture's own `git` calls
/// are logged before the count starts.
#[test]
#[ignore = "run by the_sweep_spawns_no_git under a git shim"]
fn sweep_cycles_under_a_git_shim() {
    let log = PathBuf::from(std::env::var_os("RAPTOR_TEST_GIT_LOG").expect("run by the parent"));
    let f = demo();
    let w = watch(&f);
    w.sleep();
    let lines = || {
        std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .count()
    };
    let before = lines();
    assert!(before > 0, "the shim is not the git in the PATH");
    for _ in 0..100 {
        w.observer.sweep_now();
    }
    assert_eq!(w.observer.tier("r"), Some(Tier::Dormant));
    assert_eq!(lines(), before, "the sweep launched git");
}
