//! The barrier of `Detector::evidence` (TD-GRP-005): an event is classified
//! only after the samples of the notices already sent are stored, or after
//! `sample_barrier` at most. The process table blocks the S3 read until the
//! test lets it go; every wait is a signal with a timeout, never a fixed sleep.

use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Instant;

use super::*;

const WT: &str = "/wt/feat-login";
const WAIT: Duration = Duration::from_secs(10);

/// A [`Table`] whose S3 read (`list`) says it entered and waits for a permit
/// once armed; `list_bare` (the S1 scan) never waits.
struct Gated {
    table: Arc<Table>,
    armed: AtomicBool,
    entered: Mutex<Sender<()>>,
    permits: Mutex<Receiver<()>>,
}

impl ProcLister for Gated {
    fn list(&self) -> Option<Vec<ProcEntry>> {
        if self.armed.load(Ordering::SeqCst) {
            let _ = self.entered.lock().unwrap().send(());
            let _ = self.permits.lock().unwrap().recv_timeout(WAIT);
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

/// Fields drop in order: the permit sender first, so a read still waiting
/// ends and `Drop for Detector` can join the sampler.
struct Barrier {
    permit: Sender<()>,
    entered: Receiver<()>,
    procs: Arc<Gated>,
    hooks: Arc<dyn ObserverHooks>,
    detector: Arc<Detector>,
}

impl Barrier {
    /// Claude Code (20) in the worktree, with its writing `git` (31) alive
    /// since before any notice, and the S3 read armed.
    fn new(sample_barrier: Duration) -> Self {
        let table = Arc::new(Table::default());
        table.add(10, 1, 100, "/bin/zsh", Some("/home"));
        table.add(20, 10, 2_000, &format!("/opt/bin/{AGENT}"), Some(WT));
        table.add(31, 20, wall_us() - 1_000, "/usr/bin/git", Some(WT));
        let (entered_tx, entered) = channel();
        let (permit, permits) = channel();
        let procs = Arc::new(Gated {
            table,
            armed: AtomicBool::new(false),
            entered: Mutex::new(entered_tx),
            permits: Mutex::new(permits),
        });
        let detector = Detector::start(
            SessionConfig {
                scan_interval: Duration::from_secs(3600),
                sample_barrier,
                ..SessionConfig::default()
            },
            AgentMatcher::only(vec![AGENT.into()]),
            Arc::clone(&procs) as Arc<dyn ProcLister>,
            Arc::new(HookClaims::default()),
            Arc::new(|| 1_000_000),
            Arc::new(|_| {}),
        );
        let dead = detector.watch_repo(
            "r",
            Path::new("/r/.git"),
            vec![PathBuf::from("/r"), PathBuf::from(WT)],
            Vec::new(),
        );
        assert!(dead.is_empty());
        detector.scan_now();
        let hooks = detector.observer_hooks();
        procs.armed.store(true, Ordering::SeqCst);
        Self {
            permit,
            entered,
            procs,
            hooks,
            detector: Arc::new(detector),
        }
    }

    /// Sends the notice `t_recv` and waits until its sample is in `list`.
    fn notice_in_progress(&self, t_recv: u64) {
        self.hooks.git_dir_touched("r", t_recv);
        self.entered
            .recv_timeout(WAIT)
            .expect("the S3 read never started");
    }

    fn evidence(&self, t_recv: u64) -> S3Outcome {
        self.detector
            .evidence(
                "r",
                Path::new(WT),
                S3Scope::Worktree,
                None,
                t_recv,
                t_recv + 75_000_000,
            )
            .outcome
    }

    fn release(&self) {
        self.procs.armed.store(false, Ordering::SeqCst);
        let _ = self.permit.send(());
    }
}

/// The event is classified while the sample of its notice is still being
/// taken: `evidence` waits for it and sees the session's `git`.
#[test]
fn evidence_waits_for_the_sample_of_a_notice_already_sent() {
    let b = Barrier::new(WAIT);
    b.notice_in_progress(1_000);
    let detector = Arc::clone(&b.detector);
    let (done_tx, done) = channel();
    let classify = std::thread::spawn(move || {
        let outcome = detector
            .evidence(
                "r",
                Path::new(WT),
                S3Scope::Worktree,
                None,
                1_000,
                75_001_000,
            )
            .outcome;
        let _ = done_tx.send(outcome);
    });
    // The sample is still in `list`: the classification cannot have its
    // sighting yet, and it waits instead of answering without it.
    b.permit.send(()).unwrap();
    let outcome = done.recv_timeout(WAIT).expect("evidence never answered");
    classify.join().unwrap();
    match outcome {
        S3Outcome::Attributed(p) => assert_eq!(p.session_id, "20:2000"),
        other => panic!("evidence did not wait for the sample: {other:?}"),
    }
    b.release();
}

/// A sample that never ends does not hold the daemon loop: after the barrier
/// the event is classified with what is stored (here nothing: no-sighting).
#[test]
fn evidence_stops_waiting_after_the_barrier() {
    let barrier = Duration::from_millis(50);
    let b = Barrier::new(barrier);
    b.notice_in_progress(1_000);
    let start = Instant::now();
    assert_eq!(b.evidence(1_000), S3Outcome::NoSighting);
    assert!(start.elapsed() >= barrier, "{:?}", start.elapsed());
    b.release();
}

/// With no notice pending there is nothing to wait for.
#[test]
fn evidence_does_not_wait_without_pending_notices() {
    let b = Barrier::new(WAIT);
    b.release();
    b.detector.sample_now("r", 1_000);
    let start = Instant::now();
    assert!(matches!(b.evidence(1_000), S3Outcome::Attributed(_)));
    assert!(start.elapsed() < WAIT / 2, "{:?}", start.elapsed());
}

/// The backup poll's notice measures "started before" against the write it
/// found (the `HEAD` reflog's mtime), not against when it ran: a session's
/// `git` that started after that write is no evidence, however late the poll.
#[test]
fn a_poll_notice_cuts_started_before_at_the_write() {
    let b = Barrier::new(WAIT);
    b.release();
    // The session's `git` (31) started 1 ms ago; this write is a minute old.
    b.hooks.git_dir_written("r", 1_000, wall_us() - 60_000_000);
    assert_eq!(b.evidence(1_000), S3Outcome::NoSighting);
    // A write after the `git` started: it may have made it.
    b.hooks.git_dir_written("r", 900_000_000, wall_us());
    match b.evidence(900_000_000) {
        S3Outcome::Attributed(p) => assert_eq!(p.session_id, "20:2000"),
        other => panic!("{other:?}"),
    }
}
