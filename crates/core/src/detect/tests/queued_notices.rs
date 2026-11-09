//! Contract tests of TD-GRP-005: every notice queued while an S3 sample runs
//! is evidence of its own commit, stamped with its own `t_recv` and its own
//! "started before the notice".
//!
//! The process table blocks the S3 read (`list`) until the test lets it go,
//! so notices pile up in the queue exactly as under load. Every wait is a
//! signal with a timeout: no fixed sleep.

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, Sender, channel};

use super::*;

const WT: &str = "/wt/feat-login";
/// How long any signal of the detector may take before the test fails.
const WAIT: Duration = Duration::from_secs(10);
/// A notice after every other one: its sample entering `list` proves the
/// previous sample is stored.
const SENTINEL: u64 = 10_000_000_000;

/// A [`Table`] whose S3 read (`list`) says it entered and waits for a permit
/// once armed. The S1 scan reads `list_bare`, which never waits.
struct GatedProcs {
    table: Arc<Table>,
    armed: AtomicBool,
    entered: Mutex<Sender<()>>,
    permits: Mutex<Receiver<()>>,
}

impl ProcLister for GatedProcs {
    fn list(&self) -> Option<Vec<ProcEntry>> {
        if self.armed.load(Ordering::SeqCst) {
            let _ = self.entered.lock().unwrap().send(());
            // Err once the test dropped its permit sender: let it go.
            let _ = self.permits.lock().unwrap().recv();
        }
        self.table.list()
    }
    fn list_bare(&self) -> Option<Vec<ProcEntry>> {
        self.table.list_bare()
    }
    fn exe(&self, entry: &ProcEntry) -> Option<PathBuf> {
        self.table.exe(entry)
    }
    fn cwd(&self, pid: u32) -> Option<PathBuf> {
        self.table.cwd(pid)
    }
    fn cwd_gone(&self, pid: u32) -> bool {
        self.table.cwd_gone(pid)
    }
    fn git_redirect(&self, pid: u32) -> Option<bool> {
        self.table.git_redirect(pid)
    }
    fn git_call(&self, pid: u32) -> Option<GitCall> {
        self.table.git_call(pid)
    }
}

/// A detector over a [`GatedProcs`] with the session `20:2000` in [`WT`].
///
/// Fields drop in order: the permit sender goes first, so a sample still
/// waiting in `list` ends and `Drop for Detector` can join its thread.
struct Queued {
    permit: Sender<()>,
    entered: Receiver<()>,
    table: Arc<Table>,
    procs: Arc<GatedProcs>,
    hooks: Arc<dyn ObserverHooks>,
    detector: Detector,
}

impl Queued {
    fn new() -> Self {
        let table = Arc::new(Table::default());
        // The shell the agents start from, and Claude Code in the worktree.
        table.add(10, 1, 100, "/bin/zsh", Some("/home"));
        table.add(20, 10, 2_000, &format!("/opt/bin/{AGENT}"), Some(WT));
        let (entered_tx, entered) = channel();
        let (permit, permits) = channel();
        let procs = Arc::new(GatedProcs {
            table: Arc::clone(&table),
            armed: AtomicBool::new(false),
            entered: Mutex::new(entered_tx),
            permits: Mutex::new(permits),
        });
        let config = SessionConfig {
            // The test drives the scan by hand.
            scan_interval: Duration::from_secs(3600),
            start_tolerance: Duration::ZERO,
            ..SessionConfig::default()
        };
        let detector = Detector::start(
            config,
            AgentMatcher::only(vec![AGENT.into()]),
            Arc::clone(&procs) as Arc<dyn ProcLister>,
            Arc::new(HookClaims::default()),
            Arc::new(|| 1_000_000),
            Arc::new(|_| {}),
        );
        let dead = detector.watch_repo(
            "r",
            Path::new("/r/.git"),
            vec![
                PathBuf::from("/r"),
                PathBuf::from(WT),
                PathBuf::from("/r/.claude/worktrees/x"),
            ],
            Vec::new(),
        );
        assert!(dead.is_empty());
        detector.scan_now();
        let hooks = detector.observer_hooks();
        procs.armed.store(true, Ordering::SeqCst);
        Self {
            permit,
            entered,
            table,
            procs,
            hooks,
            detector,
        }
    }

    /// A `git` notice of the repo, as the observer's router sends it.
    fn notice(&self, t_recv: u64) {
        self.hooks.git_dir_touched("r", t_recv);
    }

    /// Waits until the S3 sample number `n` enters `list`.
    fn wait_list(&self, n: u32) {
        self.entered
            .recv_timeout(WAIT)
            .unwrap_or_else(|e| panic!("S3 read #{n} never started: {e}"));
    }

    /// Lets the waiting S3 read go.
    fn grant(&self) {
        self.permit.send(()).expect("the S3 sampler thread is gone");
    }

    /// Lets sample #2 go and proves it is stored: the sentinel's sample #3
    /// enters `list` only after #2 pushed its sightings.
    fn finish_second_sample(&self) {
        self.grant();
        self.notice(SENTINEL);
        self.wait_list(3);
    }

    fn evidence(&self, t_recv: u64) -> S3Evidence {
        self.detector.evidence(
            "r",
            Path::new(WT),
            S3Scope::Repo,
            None,
            t_recv,
            t_recv + 75_000_000,
        )
    }

    /// Opens the gate for good: every waiting or later read goes on.
    fn release(&self) {
        self.procs.armed.store(false, Ordering::SeqCst);
        let _ = self.permit.send(());
    }
}

/// Busy-waits until the wall clock is past `w` microseconds.
fn spin_past(w: u64) {
    while wall_us() <= w {
        std::hint::spin_loop();
    }
}

/// The batch of TD-GRP-005: notice A (1_000) is being sampled while B1
/// (2_000) and B2 (500_000_000) wait in the queue; the session's writing
/// `git` (pid 31) starts between B1 and B2. With `foreign`, a writing `git`
/// outside every session (pid 41) runs in the same worktree since before A.
/// Returns once the sample of B1 and B2 is stored.
fn queued_batch(foreign: bool) -> Queued {
    let q = Queued::new();
    if foreign {
        q.table
            .add(41, 10, wall_us() - 1_000, "/usr/bin/git", Some(WT));
    }
    q.notice(1_000);
    q.wait_list(1);
    q.notice(2_000);
    let w = wall_us();
    spin_past(w);
    q.table.add(31, 20, w + 1, "/usr/bin/git", Some(WT));
    spin_past(w + 1);
    q.notice(500_000_000);
    q.grant();
    q.wait_list(2);
    q.finish_second_sample();
    q
}

/// TD005-QUEUED-EVIDENCE: the commit's notice B2 waited in the queue behind
/// B1; its own sighting sees the session's `git` that started before it.
#[test]
fn a_later_queued_notice_is_evidence_of_its_commit() {
    let q = queued_batch(false);
    let e = q.evidence(500_000_000);
    q.release();
    match &e.outcome {
        S3Outcome::Attributed(p) => assert_eq!(
            p.session_id, "20:2000",
            "B2 must be attributed to the session whose git made it"
        ),
        other => panic!(
            "the queued notice B2 has no sighting of its own (TD-GRP-005): expected \
             Attributed(20:2000), got {other:?} {e:?}"
        ),
    }
}

/// TD005-NO-LEAK: B1 was queued before the session's `git` started, so its
/// sighting never takes that `git`.
#[test]
fn a_queued_notice_never_takes_a_git_started_after_it() {
    let q = queued_batch(false);
    let e = q.evidence(2_000);
    q.release();
    assert_eq!(
        e.outcome,
        S3Outcome::NoSighting,
        "B1 took a git that started after it: {e:?}"
    );
    assert!(
        e.counts.gits_after_notice >= 1,
        "B1's sighting must count the git started after it: {e:?}"
    );
}

/// TD005-NO-LEAK: a foreign writing `git` alive in the worktree when B2
/// arrived keeps B2's event unattributed.
#[test]
fn a_queued_notice_with_a_foreign_git_still_makes_it_ambiguous() {
    let q = queued_batch(true);
    let e = q.evidence(500_000_000);
    q.release();
    assert_eq!(
        e.outcome,
        S3Outcome::Ambiguous,
        "B2's own sighting must see the foreign git next to the session's: {e:?}"
    );
}

/// TD005-BOUNDED: A is sampled with a foreign `git` alive; 300 notices queue
/// up behind it, outside A's window, with the foreign and a session's `git`
/// still alive. The burst must not push A's sighting out of the ring.
#[test]
fn a_burst_of_queued_notices_keeps_the_foreign_sighting() {
    let q = Queued::new();
    q.table
        .add(41, 10, wall_us() - 1_000, "/usr/bin/git", Some(WT));
    q.notice(1_000);
    q.wait_list(1);
    q.table
        .add(31, 20, wall_us() - 1_000, "/usr/bin/git", Some(WT));
    // Outside the window [0, 75_001_000] of A's event: only A's sighting
    // can show the foreign `git` there.
    for k in 0..300u64 {
        q.notice(1_000_000_000 + k);
    }
    q.grant();
    q.wait_list(2);
    q.finish_second_sample();
    let e = q.evidence(1_000);
    q.release();
    assert_eq!(
        e.outcome,
        S3Outcome::Ambiguous,
        "the burst pushed A's sighting of the foreign git out of the ring: {e:?}"
    );
}
