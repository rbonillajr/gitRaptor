//! Rules of the detector over a synthetic process table and a fake clock.

use std::sync::atomic::AtomicI64;

use super::*;

const AGENT: &str = "fake-claude";
const OLD: &str = "1111111111111111111111111111111111111111";
const NEW: &str = "2222222222222222222222222222222222222222";

#[derive(Default)]
struct Table {
    procs: Mutex<Vec<ProcEntry>>,
    cwds: Mutex<HashMap<u32, PathBuf>>,
    /// `(pid, start)` of every executable path read, in order.
    exe_reads: Mutex<Vec<(u32, u64)>>,
}

impl Table {
    fn add(&self, pid: u32, ppid: u32, start_us: u64, exe: &str, cwd: Option<&str>) {
        self.procs.lock().unwrap().push(ProcEntry {
            pid,
            ppid,
            start_us,
            exe: Some(PathBuf::from(exe)),
        });
        if let Some(cwd) = cwd {
            self.cwds.lock().unwrap().insert(pid, PathBuf::from(cwd));
        }
    }

    fn kill(&self, pid: u32) {
        self.procs.lock().unwrap().retain(|p| p.pid != pid);
        self.cwds.lock().unwrap().remove(&pid);
    }
}

impl ProcLister for Arc<Table> {
    fn list(&self) -> Option<Vec<ProcEntry>> {
        Some(self.procs.lock().unwrap().clone())
    }
    /// As the system lister: no paths, read one by one through `exe`.
    fn list_bare(&self) -> Option<Vec<ProcEntry>> {
        let mut table = self.list()?;
        for e in &mut table {
            e.exe = None;
        }
        Some(table)
    }
    fn exe(&self, entry: &ProcEntry) -> Option<PathBuf> {
        self.exe_reads
            .lock()
            .unwrap()
            .push((entry.pid, entry.start_us));
        let procs = self.procs.lock().unwrap();
        let now = procs.iter().find(|p| p.pid == entry.pid)?;
        // Gone, or the pid now names another process.
        (now.start_us == entry.start_us)
            .then(|| now.exe.clone())
            .flatten()
    }
    fn cwd(&self, pid: u32) -> Option<PathBuf> {
        self.cwds.lock().unwrap().get(&pid).cloned()
    }
}

struct Rig {
    table: Arc<Table>,
    now: Arc<AtomicI64>,
    changes: Arc<Mutex<Vec<SessionChange>>>,
    detector: Detector,
    hooks: Arc<HookClaims>,
}

impl Rig {
    /// Repo `/r` (common dir `/r/.git`) with worktrees `/r` (main),
    /// `/wt/feat-login` and the nested `/r/.claude/worktrees/x`.
    fn new() -> Self {
        let table = Arc::new(Table::default());
        // The shell the agents start from.
        table.add(10, 1, 100, "/bin/zsh", Some("/home"));
        let now = Arc::new(AtomicI64::new(1_000_000));
        let changes = Arc::new(Mutex::new(Vec::new()));
        let clock_now = Arc::clone(&now);
        let sink_changes = Arc::clone(&changes);
        let config = SessionConfig {
            // The tests drive the scan by hand.
            scan_interval: Duration::from_secs(3600),
            ..SessionConfig::default()
        };
        let hooks = Arc::new(HookClaims::default());
        let detector = Detector::start(
            config,
            AgentMatcher::only(vec![AGENT.into()]),
            Arc::new(Arc::clone(&table)),
            Arc::clone(&hooks),
            Arc::new(move || clock_now.load(Ordering::SeqCst)),
            Arc::new(move |c| sink_changes.lock().unwrap().extend(c)),
        );
        let dead = detector.watch_repo(
            "r",
            Path::new("/r/.git"),
            vec![
                PathBuf::from("/r"),
                PathBuf::from("/wt/feat-login"),
                PathBuf::from("/r/.claude/worktrees/x"),
            ],
            Vec::new(),
        );
        assert!(dead.is_empty());
        Self {
            table,
            now,
            changes,
            detector,
            hooks,
        }
    }

    fn take(&self) -> Vec<SessionChange> {
        std::mem::take(&mut *self.changes.lock().unwrap())
    }

    fn scan(&self) -> Vec<SessionChange> {
        self.detector.scan_now();
        self.take()
    }

    fn advance(&self, ms: i64) {
        self.now.fetch_add(ms, Ordering::SeqCst);
    }

    /// A Claude Code in `cwd`.
    fn claude(&self, pid: u32, start: u64, cwd: &str) {
        self.table
            .add(pid, 10, start, &format!("/opt/bin/{AGENT}"), Some(cwd));
    }

    fn evidence(&self, worktree: &str, t_recv: u64) -> S3Outcome {
        self.detector
            .evidence("r", Path::new(worktree), None, t_recv, t_recv + 75_000_000)
    }

    /// The S4 rule for the move of `feat-login` from [`OLD`] to [`NEW`].
    fn evidence_moved(&self, worktree: &str, t_recv: u64) -> S3Outcome {
        let moved = RefMove {
            branch: "feat-login",
            old: Some(OLD),
            new: NEW,
        };
        self.detector.evidence(
            "r",
            Path::new(worktree),
            Some(moved),
            t_recv,
            t_recv + 75_000_000,
        )
    }

    /// A hook claim of `session` for that move, from `cwd`, at `t`.
    fn hook_claim(&self, session: &str, cwd: &str, t: u64) {
        self.hooks.claim_for_test(
            "r",
            cwd,
            "refs/heads/feat-login",
            Some(OLD),
            NEW,
            session,
            t,
        );
    }
}

fn started(changes: &[SessionChange]) -> Vec<(String, PathBuf)> {
    changes
        .iter()
        .filter_map(|c| match c {
            SessionChange::Started {
                session_id,
                worktree,
                ..
            } => Some((session_id.clone(), worktree.clone())),
            _ => None,
        })
        .collect()
}

/// RES-01: the S1 scan reads the table every second, but the executable path only of the
/// processes it has not classified yet, once per `(pid, start)`.
#[test]
fn the_scan_reads_each_executable_path_once() {
    let rig = Rig::new();
    rig.claude(20, 2_000, "/wt/feat-login");
    assert_eq!(started(&rig.scan()).len(), 1);
    for _ in 0..5 {
        assert!(rig.scan().is_empty());
    }
    let mut reads = rig.table.exe_reads.lock().unwrap().clone();
    reads.sort_unstable();
    assert_eq!(reads, [(10, 100), (20, 2_000)], "one read per process");
    // A new process with a reused pid is a new identity: its path is read again.
    rig.table.kill(20);
    rig.claude(20, 3_000, "/wt/feat-login");
    let changes = rig.scan();
    assert_eq!(
        started(&changes),
        [("20:3000".to_owned(), PathBuf::from("/wt/feat-login"))]
    );
    assert_eq!(
        rig.table.exe_reads.lock().unwrap().last(),
        Some(&(20, 3_000))
    );
}

/// The path of a pid reused between the table and the path read belongs to the new process:
/// the old one is not classified with it.
#[test]
fn a_pid_reused_before_its_path_is_read_lends_no_path() {
    struct Reused(Arc<Table>);
    impl ProcLister for Reused {
        fn list(&self) -> Option<Vec<ProcEntry>> {
            self.0.list()
        }
        fn list_bare(&self) -> Option<Vec<ProcEntry>> {
            let table = self.0.list_bare();
            // Between the two reads, the shell 30 ends and a Claude Code takes its pid.
            self.0.kill(30);
            self.0
                .add(30, 10, 9_000, &format!("/opt/bin/{AGENT}"), Some("/r"));
            table
        }
        fn exe(&self, entry: &ProcEntry) -> Option<PathBuf> {
            self.0.exe(entry)
        }
        fn cwd(&self, pid: u32) -> Option<PathBuf> {
            self.0.cwd(pid)
        }
    }
    let table = Arc::new(Table::default());
    table.add(30, 1, 100, "/bin/zsh", Some("/r"));
    let changes = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&changes);
    let detector = Detector::start(
        SessionConfig {
            scan_interval: Duration::from_secs(3600),
            ..SessionConfig::default()
        },
        AgentMatcher::only(vec![AGENT.into()]),
        Arc::new(Reused(Arc::clone(&table))),
        Arc::new(HookClaims::default()),
        Arc::new(|| 1_000_000),
        Arc::new(move |c| sink.lock().unwrap().extend(c)),
    );
    let _ = detector.watch_repo(
        "r",
        Path::new("/r/.git"),
        vec![PathBuf::from("/r")],
        Vec::new(),
    );
    detector.scan_now();
    assert!(
        started(&changes.lock().unwrap()).is_empty(),
        "the shell `(30, 100)` is not a session"
    );
}

#[test]
fn a_session_appears_with_its_process_and_ends_with_it() {
    let rig = Rig::new();
    rig.claude(20, 2_000, "/wt/feat-login/src");
    let changes = rig.scan();
    assert_eq!(
        started(&changes),
        [("20:2000".to_owned(), PathBuf::from("/wt/feat-login"))]
    );
    assert!(rig.scan().is_empty(), "a session starts once");
    rig.table.kill(20);
    let changes = rig.scan();
    assert!(matches!(
        &changes[..],
        [SessionChange::Ended { session_id, cause: EndCause::ProcessGone, at_ms: Some(_), .. }]
            if session_id == "20:2000"
    ));
    // Launched again: a new session; the old one is not reopened (Q41).
    rig.claude(21, 3_000, "/wt/feat-login");
    assert_eq!(started(&rig.scan())[0].0, "21:3000");
}

#[test]
fn a_reused_pid_is_another_session() {
    let rig = Rig::new();
    rig.claude(20, 2_000, "/wt/feat-login");
    rig.scan();
    rig.table.kill(20);
    rig.claude(20, 9_000, "/wt/feat-login");
    let changes = rig.scan();
    assert!(
        changes.iter().any(
            |c| matches!(c, SessionChange::Ended { session_id, .. } if session_id == "20:2000")
        )
    );
    assert_eq!(started(&changes)[0].0, "20:9000");
}

#[test]
fn only_claude_code_in_an_observed_worktree_is_a_session() {
    let rig = Rig::new();
    rig.claude(20, 2_000, "/elsewhere");
    rig.table
        .add(21, 10, 2_000, "/usr/bin/vim", Some("/wt/feat-login"));
    rig.table.add(22, 10, 2_000, "/opt/bin/fake-claude", None);
    assert!(rig.scan().is_empty());
}

#[test]
fn the_longest_worktree_holds_the_session() {
    let rig = Rig::new();
    rig.claude(20, 2_000, "/r/.claude/worktrees/x/src");
    rig.claude(21, 2_000, "/r/src");
    let mut got = started(&rig.scan());
    got.sort();
    assert_eq!(
        got,
        [
            (
                "20:2000".to_owned(),
                PathBuf::from("/r/.claude/worktrees/x")
            ),
            ("21:2000".to_owned(), PathBuf::from("/r")),
        ]
    );
}

#[test]
fn a_child_claude_folds_into_its_parent_only_in_the_same_worktree() {
    let rig = Rig::new();
    rig.claude(20, 2_000, "/wt/feat-login");
    // A helper of the same session, through a shell.
    rig.table
        .add(30, 20, 2_100, "/bin/sh", Some("/wt/feat-login"));
    rig.table.add(
        31,
        30,
        2_200,
        "/opt/bin/fake-claude",
        Some("/wt/feat-login"),
    );
    // An orchestrator launching Claude Code in another worktree.
    rig.table
        .add(32, 20, 2_300, "/opt/bin/fake-claude", Some("/r"));
    let mut got = started(&rig.scan());
    got.sort();
    assert_eq!(
        got,
        [
            ("20:2000".to_owned(), PathBuf::from("/wt/feat-login")),
            ("32:2300".to_owned(), PathBuf::from("/r")),
        ]
    );
}

#[test]
fn five_minutes_without_activity_make_it_inactive_and_activity_makes_it_active() {
    let rig = Rig::new();
    rig.claude(20, 2_000, "/wt/feat-login");
    rig.scan();
    rig.advance(5 * 60_000 - 1);
    assert!(rig.scan().is_empty(), "still active at 4:59.999");
    rig.advance(1);
    assert!(matches!(
        &rig.scan()[..],
        [SessionChange::State {
            state: SessionStateView::Inactive,
            ..
        }]
    ));
    assert!(rig.scan().is_empty(), "inactive once");
    // Activity in another worktree does not count.
    rig.detector.activity("r", Path::new("/r"));
    assert!(rig.take().is_empty());
    rig.detector.activity("r", Path::new("/wt/feat-login"));
    assert!(matches!(
        &rig.take()[..],
        [SessionChange::State {
            state: SessionStateView::Active,
            ..
        }]
    ));
    // Activity keeps it active: the threshold counts from the last one.
    rig.advance(4 * 60_000);
    rig.detector.activity("r", Path::new("/wt/feat-login"));
    rig.advance(4 * 60_000);
    assert!(rig.scan().is_empty());
}

#[test]
fn the_observer_hooks_report_activity() {
    let rig = Rig::new();
    rig.claude(20, 2_000, "/wt/feat-login");
    rig.scan();
    rig.advance(5 * 60_000);
    rig.scan();
    rig.detector
        .observer_hooks()
        .worktree_touched("r", Path::new("/wt/feat-login"));
    assert!(matches!(
        &rig.take()[..],
        [SessionChange::State {
            state: SessionStateView::Active,
            ..
        }]
    ));
}

/// A `git` started now (wall clock), under `parent`, in `cwd`.
fn git(rig: &Rig, pid: u32, parent: u32, cwd: Option<&str>) {
    rig.table
        .add(pid, parent, wall_us() - 1_000, "/usr/bin/git", cwd);
}

#[test]
fn s3_attributes_a_git_event_to_the_only_session_whose_git_made_it() {
    let rig = Rig::new();
    assert_eq!(rig.evidence("/wt/feat-login", 1_000), S3Outcome::NoSession);
    rig.claude(20, 2_000, "/wt/feat-login");
    rig.scan();
    // Claude Code's shell tool runs `git commit`.
    rig.table
        .add(30, 20, 2_100, "/bin/zsh", Some("/wt/feat-login"));
    git(&rig, 31, 30, Some("/wt/feat-login"));
    // Another repo's `git` does not matter.
    git(&rig, 40, 10, Some("/other-repo"));
    rig.detector.sample_now("r", 1_000);
    match rig.evidence("/wt/feat-login", 1_000) {
        S3Outcome::Attributed(p) => {
            assert_eq!(p.session_id, "20:2000");
            assert_eq!(p.worktree, PathBuf::from("/wt/feat-login"));
        }
        other => panic!("{other:?}"),
    }
    // Outside the batch's window the sample does not count.
    assert_eq!(
        rig.evidence("/wt/feat-login", 500_000_000),
        S3Outcome::NoSighting
    );
    // Nor for an event of another worktree.
    assert_eq!(rig.evidence("/r", 1_000), S3Outcome::NoSighting);
}

#[test]
fn s3_ignores_a_git_of_the_session_that_started_after_the_write() {
    let rig = Rig::new();
    rig.claude(20, 2_000, "/wt/feat-login");
    rig.scan();
    rig.table.add(
        31,
        20,
        wall_us() + 60_000_000,
        "/usr/bin/git",
        Some("/wt/feat-login"),
    );
    rig.detector.sample_now("r", 1_000);
    assert_eq!(rig.evidence("/wt/feat-login", 1_000), S3Outcome::NoSighting);
}

/// The developer commits while Claude Code runs a `git status` in the same
/// worktree: both `git`s are alive, so the commit stays unattributed
/// (BR-EDGE-004).
#[test]
fn s3_with_a_git_outside_every_session_is_ambiguous() {
    let rig = Rig::new();
    rig.claude(20, 2_000, "/wt/feat-login");
    rig.scan();
    git(&rig, 31, 20, Some("/wt/feat-login"));
    git(&rig, 41, 10, Some("/wt/feat-login/src"));
    rig.detector.sample_now("r", 1_000);
    assert_eq!(rig.evidence("/wt/feat-login", 1_000), S3Outcome::Ambiguous);
}

#[test]
fn s3_counts_the_daemons_own_git_and_exiting_ones_of_the_repo_as_foreign() {
    let rig = Rig::new();
    rig.claude(20, 2_000, "/wt/feat-login");
    rig.scan();
    git(&rig, 31, 20, Some("/wt/feat-login"));
    let me = std::process::id();
    rig.table.add(me, 10, 1_000, "/opt/raptor", Some("/"));
    git(&rig, 50, me, Some("/r"));
    rig.detector.sample_now("r", 1_000);
    assert_eq!(rig.evidence("/wt/feat-login", 1_000), S3Outcome::Ambiguous);

    let rig = Rig::new();
    rig.claude(20, 2_000, "/wt/feat-login");
    rig.scan();
    git(&rig, 31, 20, Some("/wt/feat-login"));
    // An exiting `git` (folder unreadable) launched by the developer's
    // shell in the worktree.
    rig.table
        .add(60, 10, 1_500, "/bin/zsh", Some("/wt/feat-login"));
    git(&rig, 51, 60, None);
    rig.detector.sample_now("r", 1_000);
    assert_eq!(rig.evidence("/wt/feat-login", 1_000), S3Outcome::Ambiguous);
    let d = rig.detector.diagnostics();
    assert_eq!((d.cwd_unreadable, d.placed_by_ancestor), (1, 1));
}

/// An exiting `git` launched from outside the repo (another repo of the
/// machine, or an editor whose folder is elsewhere) does not hide the
/// session's `git`; nor does an exiting `git` of the session itself.
#[test]
fn s3_ignores_exiting_gits_launched_outside_the_repo() {
    let rig = Rig::new();
    rig.claude(20, 2_000, "/wt/feat-login");
    rig.scan();
    git(&rig, 31, 20, Some("/wt/feat-login"));
    git(&rig, 32, 20, None);
    git(&rig, 51, 10, None);
    rig.table.add(70, 1, 500, "/Applications/Kraken", None);
    git(&rig, 52, 70, None);
    rig.detector.sample_now("r", 1_000);
    assert!(matches!(
        rig.evidence("/wt/feat-login", 1_000),
        S3Outcome::Attributed(_)
    ));
}

#[test]
fn s3_with_two_sessions_in_the_worktree_is_ambiguous() {
    let rig = Rig::new();
    rig.claude(20, 2_000, "/wt/feat-login");
    rig.claude(21, 2_500, "/wt/feat-login");
    rig.scan();
    git(&rig, 31, 20, Some("/wt/feat-login"));
    git(&rig, 32, 21, Some("/wt/feat-login"));
    rig.detector.sample_now("r", 1_000);
    assert_eq!(rig.evidence("/wt/feat-login", 1_000), S3Outcome::Ambiguous);
}

#[test]
fn a_reused_pid_breaks_the_ancestry() {
    let rig = Rig::new();
    rig.claude(20, 2_000, "/wt/feat-login");
    rig.scan();
    // Its parent claims pid 20 but started after the `git`: the `git`
    // belongs to no session, so it is foreign and nothing is attributed.
    rig.table
        .add(31, 20, 1_500, "/usr/bin/git", Some("/wt/feat-login"));
    rig.detector.sample_now("r", 1_000);
    assert_eq!(rig.evidence("/wt/feat-login", 1_000), S3Outcome::Ambiguous);
}

#[test]
fn open_sessions_continue_only_with_their_process() {
    let rig = Rig::new();
    rig.claude(20, 2_000, "/wt/feat-login");
    let open = |id: &str| OpenSession {
        session_id: id.into(),
        worktree: PathBuf::from("/wt/feat-login"),
        started_ms: 5,
        state: SessionStateView::Inactive,
    };
    let dead = rig.detector.watch_repo(
        "r2",
        Path::new("/wt/.git"),
        vec![PathBuf::from("/wt/feat-login")],
        vec![open("20:2000"), open("20:1999"), open("99:1"), open("bad")],
    );
    assert_eq!(dead, ["20:1999", "99:1", "bad"]);
    // The live one is not started again, and keeps its inactive state.
    assert!(rig.scan().is_empty());
    rig.detector.activity("r2", Path::new("/wt/feat-login"));
    assert!(matches!(
        &rig.take()[..],
        [SessionChange::State { state: SessionStateView::Active, session_id, .. }] if session_id == "20:2000"
    ));
}

#[test]
fn a_forgotten_repo_detects_nothing() {
    let rig = Rig::new();
    rig.claude(20, 2_000, "/wt/feat-login");
    rig.detector.forget_repo("r");
    assert!(rig.scan().is_empty());
}

#[test]
fn session_ids_match_the_requester_format() {
    assert_eq!(session_id(82, 820), "82:820");
    assert_eq!(parse_session_id("82:820"), Some((82, 820)));
    assert_eq!(parse_session_id("cc-82"), None);
}

// ------------------------------------------------- Registered (US-GRP-009)

fn codex(rig: &Rig, id: &str, worktree: &str, state: SessionStateView, last: Option<i64>) {
    rig.detector.register(RegisteredSession {
        repo_id: "r".into(),
        session_id: id.into(),
        worktree: PathBuf::from(worktree),
        started_ms: 1_000_000,
        state,
        last_activity_ms: last,
        registration_evidence: true,
    });
}

fn states(changes: &[SessionChange]) -> Vec<(String, SessionStateView)> {
    changes
        .iter()
        .filter_map(|c| match c {
            SessionChange::State {
                session_id, state, ..
            } => Some((session_id.clone(), *state)),
            _ => None,
        })
        .collect()
}

/// BR-WF-001, Q41: a registered session has no process to end it; it goes
/// idle after the threshold and back with activity, and only its withdrawal
/// stops it.
#[test]
fn a_registered_session_goes_idle_and_back_without_any_process() {
    let rig = Rig::new();
    codex(
        &rig,
        "reg:1:0",
        "/wt/feat-login",
        SessionStateView::Active,
        None,
    );
    rig.advance(5 * 60 * 1000 - 1);
    assert!(rig.scan().is_empty(), "not idle yet, and never ended");
    rig.advance(1);
    assert_eq!(
        states(&rig.scan()),
        [("reg:1:0".to_owned(), SessionStateView::Inactive)]
    );
    rig.detector.activity("r", Path::new("/wt/feat-login"));
    assert_eq!(
        states(&rig.take()),
        [("reg:1:0".to_owned(), SessionStateView::Active)]
    );
    rig.detector.end_registered("reg:1:0");
    rig.advance(10 * 60 * 1000);
    assert!(rig.scan().is_empty(), "a withdrawn session is not followed");
}

/// ADR-GRP-013 § 5 (Architect, D9): after a restart, hours without the
/// engine are not activity.
#[test]
fn a_reloaded_registered_session_counts_from_its_last_activity() {
    let rig = Rig::new();
    let long_ago = 1_000_000 - 3 * 60 * 60 * 1000;
    codex(
        &rig,
        "reg:1:0",
        "/wt/feat-login",
        SessionStateView::Active,
        Some(long_ago),
    );
    assert_eq!(
        states(&rig.scan()),
        [("reg:1:0".to_owned(), SessionStateView::Inactive)]
    );
}

/// The idle check runs where processes cannot be listed (Windows).
#[test]
fn registered_sessions_go_idle_without_a_process_table() {
    struct NoProcs;
    impl ProcLister for NoProcs {
        fn list(&self) -> Option<Vec<ProcEntry>> {
            None
        }
        fn cwd(&self, _pid: u32) -> Option<PathBuf> {
            None
        }
    }
    let now = Arc::new(AtomicI64::new(1_000_000));
    let changes = Arc::new(Mutex::new(Vec::new()));
    let (clock_now, sink) = (Arc::clone(&now), Arc::clone(&changes));
    let detector = Detector::start(
        SessionConfig {
            scan_interval: Duration::from_secs(3600),
            ..SessionConfig::default()
        },
        AgentMatcher::only(vec![AGENT.into()]),
        Arc::new(NoProcs),
        Arc::new(HookClaims::default()),
        Arc::new(move || clock_now.load(Ordering::SeqCst)),
        Arc::new(move |c| sink.lock().unwrap().extend(c)),
    );
    detector.register(RegisteredSession {
        repo_id: "r".into(),
        session_id: "reg:1:0".into(),
        worktree: PathBuf::from("/wt/feat-login"),
        started_ms: 1_000_000,
        state: SessionStateView::Active,
        last_activity_ms: None,
        registration_evidence: true,
    });
    now.fetch_add(5 * 60 * 1000, Ordering::SeqCst);
    detector.scan_now();
    assert_eq!(
        states(&changes.lock().unwrap()),
        [("reg:1:0".to_owned(), SessionStateView::Inactive)]
    );
}

/// ADR-GRP-012 rule 3 and ADR-GRP-013 Validation 5: the registration is the
/// evidence only while that "other agent" is the only present session of
/// the worktree.
#[test]
fn the_registration_is_evidence_only_as_the_only_present_session() {
    let rig = Rig::new();
    let evidence = || {
        rig.detector
            .registration_evidence("r", Path::new("/wt/feat-login"))
            .map(|p| p.session_id)
    };
    codex(
        &rig,
        "reg:1:0",
        "/wt/feat-login",
        SessionStateView::Active,
        None,
    );
    assert_eq!(evidence().as_deref(), Some("reg:1:0"));
    assert_eq!(
        rig.detector.registration_evidence("r", Path::new("/r")),
        None,
        "another worktree"
    );
    // A second registered agent: shared, nobody.
    codex(
        &rig,
        "reg:2:0",
        "/wt/feat-login",
        SessionStateView::Active,
        None,
    );
    assert_eq!(evidence(), None);
    rig.detector.end_registered("reg:2:0");
    assert_eq!(evidence().as_deref(), Some("reg:1:0"));
    // A detected Claude Code there too: shared, nobody.
    rig.claude(20, 2_000, "/wt/feat-login");
    rig.scan();
    assert_eq!(evidence(), None);
}

/// ADR-GRP-013 Validation 6: Claude Code, registered or confirmed, never
/// gets the registration as evidence: a human edit stays unattributed.
#[test]
fn claude_code_never_gets_the_registration_as_evidence() {
    let rig = Rig::new();
    rig.detector.register(RegisteredSession {
        repo_id: "r".into(),
        session_id: "reg:1:0".into(),
        worktree: PathBuf::from("/wt/feat-login"),
        started_ms: 1_000_000,
        state: SessionStateView::Active,
        last_activity_ms: None,
        registration_evidence: false,
    });
    assert_eq!(
        rig.detector
            .registration_evidence("r", Path::new("/wt/feat-login")),
        None
    );
    // A forgotten repo drops its registered sessions too.
    codex(&rig, "reg:2:0", "/r", SessionStateView::Active, None);
    rig.detector.forget_repo("r");
    assert_eq!(
        rig.detector.registration_evidence("r", Path::new("/r")),
        None
    );
}

/// Amendment of ADR-GRP-012 (short-commit race): the hint for an event S3
/// did not see is the only present session of the worktree, and only when
/// it is a detected one, active. With none or two, no hint.
#[test]
fn the_hint_is_the_only_active_detected_session_of_the_worktree() {
    let rig = Rig::new();
    let hint = || {
        rig.detector
            .single_session("r", Path::new("/wt/feat-login"))
            .map(|p| p.session_id)
    };
    // 0 sessions.
    assert_eq!(hint(), None);
    // 1 session: S3 saw no `git` (it already ended), the hint is that session.
    rig.claude(20, 2_000, "/wt/feat-login");
    rig.scan();
    rig.detector.sample_now("r", 1_000);
    assert_eq!(rig.evidence("/wt/feat-login", 1_000), S3Outcome::NoSighting);
    assert_eq!(hint().as_deref(), Some("20:2000"));
    assert_eq!(
        rig.detector.single_session("r", Path::new("/r")),
        None,
        "another worktree"
    );
    // A session in another worktree does not count.
    rig.claude(21, 2_100, "/r");
    rig.scan();
    assert_eq!(hint().as_deref(), Some("20:2000"));
    // 2 sessions in the worktree: no hint.
    rig.claude(22, 2_200, "/wt/feat-login");
    rig.scan();
    assert_eq!(hint(), None);
    rig.table.kill(22);
    rig.scan();
    assert_eq!(hint().as_deref(), Some("20:2000"));
    // A registered session there makes it two.
    codex(
        &rig,
        "reg:1:0",
        "/wt/feat-login",
        SessionStateView::Active,
        None,
    );
    assert_eq!(hint(), None);
    rig.detector.end_registered("reg:1:0");
    assert_eq!(hint().as_deref(), Some("20:2000"));
}

#[test]
fn an_inactive_session_is_no_hint() {
    let rig = Rig::new();
    rig.claude(20, 2_000, "/wt/feat-login");
    rig.scan();
    rig.advance(5 * 60_000);
    rig.scan();
    assert_eq!(
        rig.detector
            .single_session("r", Path::new("/wt/feat-login")),
        None
    );
}

// ------------------------------------------------------------------- S4

/// A hook claim attributes the move even with no sample at all: the `git`
/// already ended (the S3 race), DS-US-GRP-007 § 7.
#[test]
fn s4_attributes_the_move_s3_did_not_see() {
    let rig = Rig::new();
    rig.claude(500, 7, "/wt/feat-login");
    rig.scan();
    rig.hook_claim("500:7", "/wt/feat-login", 1_000);
    match rig.evidence_moved("/wt/feat-login", 1_000) {
        S3Outcome::Hook(p) => assert_eq!(p.session_id, "500:7"),
        other => panic!("{other:?}"),
    }
    // Consumed: the next event of the same move has no S4.
    assert_eq!(
        rig.evidence_moved("/wt/feat-login", 1_000),
        S3Outcome::NoSighting
    );
}

/// The claim must come from the event's worktree (longest root), name a
/// present session, and be the only session claiming the move; otherwise S3
/// decides, as without hooks.
#[test]
fn s4_needs_the_events_worktree_and_one_present_session() {
    let rig = Rig::new();
    rig.claude(500, 7, "/wt/feat-login");
    rig.claude(600, 8, "/r");
    rig.scan();
    // Another worktree, and a nested worktree under the main one.
    rig.hook_claim("500:7", "/r", 1_000);
    assert_eq!(
        rig.evidence_moved("/wt/feat-login", 1_000),
        S3Outcome::NoSighting
    );
    // It belongs to the event of `/r`, where that `git` ran (as for S3).
    assert!(matches!(
        rig.evidence_moved("/r", 1_000),
        S3Outcome::Hook(p) if p.session_id == "500:7"
    ));
    rig.hook_claim("600:8", "/r/.claude/worktrees/x", 1_000);
    assert_eq!(rig.evidence_moved("/r", 1_000), S3Outcome::NoSighting);
    // A session that is not present.
    rig.hook_claim("999:1", "/wt/feat-login", 1_000);
    assert_eq!(
        rig.evidence_moved("/wt/feat-login", 1_000),
        S3Outcome::NoSighting
    );
    // Two sessions claim the same move.
    rig.hook_claim("500:7", "/wt/feat-login", 1_000);
    rig.hook_claim("600:8", "/wt/feat-login", 1_000);
    assert_eq!(
        rig.evidence_moved("/wt/feat-login", 1_000),
        S3Outcome::NoSighting
    );
    // Out of the window: after the flush, or older than the lead.
    rig.hook_claim("500:7", "/wt/feat-login", 900_000_000);
    assert_eq!(
        rig.evidence_moved("/wt/feat-login", 1_000),
        S3Outcome::NoSighting
    );
    assert_eq!(
        rig.evidence_moved("/wt/feat-login", 10_000_000_000),
        S3Outcome::NoSighting
    );
}

/// No move (a switch, a push) or no session in the repo: S4 is not read and
/// the outcome is the one without hooks.
#[test]
fn s4_is_not_read_without_a_move_or_a_session() {
    let rig = Rig::new();
    rig.hook_claim("500:7", "/wt/feat-login", 1_000);
    assert_eq!(
        rig.evidence_moved("/wt/feat-login", 1_000),
        S3Outcome::NoSession
    );
    rig.claude(500, 7, "/wt/feat-login");
    rig.scan();
    assert_eq!(rig.evidence("/wt/feat-login", 1_000), S3Outcome::NoSighting);
    // The claim is still there for its own event.
    assert!(matches!(
        rig.evidence_moved("/wt/feat-login", 1_000),
        S3Outcome::Hook(_)
    ));
}
