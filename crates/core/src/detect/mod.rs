//! Detection of Claude Code sessions without hooks of its own (US-GRP-007,
//! ADR-GRP-012).
//!
//! - **S1**: a process that the [`AgentMatcher`] classifies as Claude Code,
//!   whose working folder is inside an observed worktree, is a session. Its
//!   identity is `(pid, start time)`; it ends when that process disappears
//!   and is never reopened (Q41).
//! - **States** (BR-WF-001, BR-TIME-001): active while there is activity in
//!   its worktree within the inactivity threshold, inactive after it.
//! - **S3**: the ancestry of the live `git` processes, sampled as soon as
//!   the router sees a write in the repo's Git directory. A Git event points
//!   to a session only when the samples of its window show a `git` of
//!   exactly one session in its worktree and no foreign `git` in the event's
//!   [`S3Scope`] (rules 2, 4 and 6).
//! - **S4**: the claims Guardrails hooks leave ([`HookClaims`]): a hook that ran inside an
//!   agent's `git` names the branch move that `git` made, without the S3 race.
//! - **Registered sessions** (US-GRP-009): present until their registration
//!   is withdrawn (Q41), with the same states and threshold, without any
//!   process to check. A registered "other agent" that is the only present
//!   session of its worktree receives that worktree's Git events (the
//!   registration as evidence, ADR-GRP-012 rule 3).
//!
//! The detector never writes: it hands [`SessionChange`]s to the daemon
//! loop, the single writer (ADR-GRP-005). It reads processes only through
//! [`ProcLister`]. Of a foreign `git` it reads one boolean from its command
//! line and the names of its environment (whether it redirects its target);
//! no argument nor value is ever kept or logged (SEC-04).

pub mod hook;
pub mod procs;

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use gitraptor_api::messages::SessionStateView;

use crate::channel::{AgentMatcher, ExeClass};
use crate::profile::EndCause;
use crate::watch::ObserverHooks;

pub use hook::{HookClaims, RefMove};
pub use procs::{ProcEntry, ProcLister, SystemProcLister, detection_supported};

/// Longest ancestry walked.
const MAX_DEPTH: usize = 64;

/// Samples kept per repo; older ones can no longer match a batch.
const MAX_SIGHTINGS: usize = 256;

/// Most foreign `git`s whose argv and environment are read per sample; past
/// it a `git` is unreadable (`None`), so it counts as foreign in the whole
/// repo: a flood of `git`s costs no more than this, and gains nothing.
const MAX_REDIRECT_READS: usize = 32;

/// Id of the session of the Claude Code process `(pid, start)`. The same
/// text identifies the requester of a Time Machine operation
/// (`requester.rs`), so a later correction of the session reaches it
/// (ADR-TMC-005).
pub fn session_id(pid: u32, start_us: u64) -> String {
    format!("{pid}:{start_us}")
}

/// `(pid, start)` back from a [`session_id`].
pub fn parse_session_id(id: &str) -> Option<(u32, u64)> {
    let (pid, start) = id.split_once(':')?;
    Some((pid.parse().ok()?, start.parse().ok()?))
}

/// Internal parameters of the detector. The defaults are those of the
/// US-GRP-007 Dev Spec; SPIKE-GRP-001 measures them, and reading the
/// threshold from the profile and local levels belongs to US-GRP-013.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionConfig {
    /// Interval of the S1 scan (and of the inactivity check).
    pub scan_interval: Duration,
    /// Inactivity threshold (BR-TIME-001): 5 minutes by default.
    pub inactivity: Duration,
    /// How long before a batch's first mark a sample still belongs to it:
    /// a commit writes its objects, which open no window, just before its
    /// refs.
    pub s3_lead: Duration,
    /// Tolerance of "the `git` started before the write": process start
    /// times are exact on macOS, rounded to the boot second on Linux.
    pub start_tolerance: Duration,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            scan_interval: Duration::from_secs(1),
            inactivity: Duration::from_secs(5 * 60),
            s3_lead: Duration::from_millis(100),
            // Pendiente: etapa de validación multiplataforma.
            start_tolerance: if cfg!(target_os = "linux") {
                Duration::from_secs(1)
            } else {
                Duration::ZERO
            },
        }
    }
}

/// Wall clock of the sessions, in UTC milliseconds. The suspension of the
/// machine counts (a monotonic clock would stop while it sleeps).
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// What changed in the sessions, for the daemon loop to persist and publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionChange {
    Started {
        repo_id: String,
        session_id: String,
        worktree: PathBuf,
        started_ms: i64,
    },
    State {
        repo_id: String,
        session_id: String,
        state: SessionStateView,
        at_ms: i64,
    },
    Ended {
        repo_id: String,
        session_id: String,
        /// `None` when it ended while the engine was not observing.
        at_ms: Option<i64>,
        cause: EndCause,
    },
}

/// Where changes go: the daemon loop.
pub type ChangeSink = Arc<dyn Fn(Vec<SessionChange>) + Send + Sync>;

/// A session still open in the store when a repo starts being observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenSession {
    pub session_id: String,
    pub worktree: PathBuf,
    pub started_ms: i64,
    pub state: SessionStateView,
}

/// A present session, as a Git event that points to it needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentSession {
    pub session_id: String,
    pub worktree: PathBuf,
    pub started_ms: i64,
}

/// A session created or confirmed by an explicit registration, for the
/// detector to follow (US-GRP-009).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredSession {
    pub repo_id: String,
    pub session_id: String,
    pub worktree: PathBuf,
    pub started_ms: i64,
    pub state: SessionStateView,
    /// Its latest known activity: after an engine restart, the time the
    /// engine last observed, so hours without the engine do not count as
    /// activity. `None` for a new registration (now).
    pub last_activity_ms: Option<i64>,
    /// An "other agent" created by registration: while it is the only
    /// present session of its worktree, the registration is the evidence
    /// of that worktree's events (ADR-GRP-012 rule 3). Never for Claude
    /// Code, registered or confirmed.
    pub registration_evidence: bool,
}

/// Outcome of the S3 rule for one Git event (diagnostics, SPIKE-GRP-001).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum S3Outcome {
    /// The repo had no present session: nothing to attribute.
    NoSession,
    /// No sample showed a `git` of a session.
    NoSighting,
    /// Several sessions, or a `git` outside every session.
    Ambiguous,
    Attributed(PresentSession),
    /// A Guardrails hook proved the move (S4).
    Hook(PresentSession),
}

/// Where a foreign `git` counts for one Git event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum S3Scope {
    /// Only a foreign `git` whose folder is in the event's worktree, plus the
    /// daemon's, the ones placed by an ancestor, the ones in the common dir
    /// and the ones that redirect their target or cannot be read.
    Worktree,
    /// A foreign `git` whose folder is anywhere in the repo.
    Repo,
}

impl S3Scope {
    /// Stable text of the `scope` field of `s3_evidence`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Worktree => "worktree",
            Self::Repo => "repo",
        }
    }
}

/// What one S3 evaluation counted, for the dogfooding review. Integers only:
/// never a path, a name, a pid or an argv. Each counts (sample, `git`) pairs
/// of the window, so a `git` seen in two samples counts twice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct S3Counts {
    /// `git`s of a session whose folder is in the event's worktree (evidence).
    pub sessions_wt: u32,
    /// Foreign `git`s whose readable folder is in the event's worktree.
    pub foreign_wt: u32,
    /// Foreign `git`s whose readable folder is in another worktree of the
    /// repo: foreign only with `S3Scope::Repo`.
    pub foreign_other_wt: u32,
    /// The daemon's own `git`s in the repo or exiting.
    pub foreign_daemon: u32,
    /// Foreign `git`s placed by the folder of a live ancestor, in the repo.
    pub foreign_by_ancestor: u32,
    /// Foreign `git`s whose folder is in the common Git dir.
    pub foreign_gitdir: u32,
    /// Foreign `git`s in the repo that redirect their target, or whose argv
    /// or environment could not be read.
    pub foreign_redirected: u32,
    /// `git`s that started after the write: not evidence and not foreign.
    pub gits_after_notice: u32,
}

/// The S3 (or S4) outcome of one event and what it counted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct S3Evidence {
    pub outcome: S3Outcome,
    /// Zero for `NoSession` and `Hook`: the S3 loop did not run.
    pub counts: S3Counts,
}

/// Counters for the dogfooding review (SPIKE-GRP-001).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Diagnostics {
    pub samples: u64,
    /// `git`s whose working folder could not be read (exiting).
    pub cwd_unreadable: u64,
    /// Of those, placed by the folder of a live ancestor.
    pub placed_by_ancestor: u64,
    pub sessions_detected: u64,
    pub sessions_ended: u64,
}

struct Live {
    repo_id: String,
    session_id: String,
    worktree: PathBuf,
    started_ms: i64,
    state: SessionStateView,
    last_activity_ms: i64,
}

impl Live {
    fn present(&self) -> PresentSession {
        PresentSession {
            session_id: self.session_id.clone(),
            worktree: self.worktree.clone(),
            started_ms: self.started_ms,
        }
    }

    /// Idle after the threshold without activity: the change, if any.
    fn idle(&mut self, now: i64, threshold: i64) -> Option<SessionChange> {
        if self.state != SessionStateView::Active || now - self.last_activity_ms < threshold {
            return None;
        }
        self.state = SessionStateView::Inactive;
        Some(SessionChange::State {
            repo_id: self.repo_id.clone(),
            session_id: self.session_id.clone(),
            state: SessionStateView::Inactive,
            at_ms: now,
        })
    }

    /// Activity: the change back to active, if it was idle.
    fn touch(&mut self, now: i64) -> Option<SessionChange> {
        self.last_activity_ms = now;
        if self.state != SessionStateView::Inactive {
            return None;
        }
        self.state = SessionStateView::Active;
        Some(SessionChange::State {
            repo_id: self.repo_id.clone(),
            session_id: self.session_id.clone(),
            state: SessionStateView::Active,
            at_ms: now,
        })
    }
}

/// A registered session: no process, present until withdrawn.
struct Registered {
    live: Live,
    registration_evidence: bool,
}

#[derive(Clone)]
struct RepoPaths {
    common: PathBuf,
    worktrees: Vec<PathBuf>,
}

impl RepoPaths {
    fn contains(&self, path: &Path) -> bool {
        path.starts_with(&self.common) || self.worktrees.iter().any(|w| path.starts_with(w))
    }

    /// The worktree that holds `path`: the longest root (nested worktrees
    /// under `.claude/worktrees/`).
    fn worktree_of(&self, path: &Path) -> Option<&PathBuf> {
        self.worktrees
            .iter()
            .filter(|w| path.starts_with(w))
            .max_by_key(|w| w.as_os_str().len())
    }

    /// `path` is in the common Git dir, which by path alone would fall in
    /// the main worktree.
    fn in_common(&self, path: &Path) -> bool {
        path.starts_with(&self.common)
    }
}

/// The present sessions of a repo, by the `(pid, start)` of their process.
type Sessions = HashMap<(u32, u64), String>;

/// What one `git` of an S3 window means for the event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class<'a> {
    /// A `git` of this session in the event's worktree.
    Evidence(&'a str),
    /// A `git` that may have made the event and is of no session.
    Foreign,
    Ignored,
}

/// Classifies one `git` of the window of an event of `worktree`: the first
/// rule that holds decides, and names the only counter that goes up.
///
/// The common dir is checked before the worktree: `<repo>/.git/worktrees/x`
/// starts with the main worktree's root.
fn classify<'g, 'c>(
    git: &'g GitSeen,
    repo: &RepoPaths,
    worktree: &Path,
    scope: S3Scope,
    counts: &'c mut S3Counts,
) -> (Class<'g>, Option<&'c mut u32>) {
    if !git.started_before {
        return (Class::Ignored, Some(&mut counts.gits_after_notice));
    }
    match (&git.owner, &git.cwd) {
        (Owner::Session(id), Some(cwd))
            if !repo.in_common(cwd) && repo.worktree_of(cwd).is_some_and(|w| w == worktree) =>
        {
            (Class::Evidence(id), Some(&mut counts.sessions_wt))
        }
        // A `git` of the session in another worktree, in the common dir, or
        // exiting, is no evidence and not foreign.
        (Owner::Session(_), _) => (Class::Ignored, None),
        // The daemon's own `git` in this repo, or exiting: the Time
        // Machine's writer runs in the common dir, with any scope.
        (Owner::Daemon, None) => (Class::Foreign, Some(&mut counts.foreign_daemon)),
        (Owner::Daemon, Some(cwd)) if repo.contains(cwd) => {
            (Class::Foreign, Some(&mut counts.foreign_daemon))
        }
        (Owner::Daemon, Some(_)) => (Class::Ignored, None),
        // Exiting and launched from nowhere readable: like a `git` that
        // already ended, it is not seen (the S3 race accepted by
        // ADR-GRP-012; an editor such as GitKraken falls here, a declared
        // gap of SPIKE-GRP-001).
        (Owner::Other, None) => (Class::Ignored, None),
        (Owner::Other, Some(cwd)) if !repo.contains(cwd) => (Class::Ignored, None),
        (Owner::Other, Some(cwd)) if repo.in_common(cwd) => {
            (Class::Foreign, Some(&mut counts.foreign_gitdir))
        }
        // The ancestor's folder says where the shell is, not where its
        // `git` wrote.
        (Owner::Other, Some(_)) if git.placed_by_ancestor => {
            (Class::Foreign, Some(&mut counts.foreign_by_ancestor))
        }
        // Its folder is only trusted when it does not redirect its target
        // and that could be read.
        (Owner::Other, Some(_)) if git.redirect != Some(false) => {
            (Class::Foreign, Some(&mut counts.foreign_redirected))
        }
        (Owner::Other, Some(cwd)) if repo.worktree_of(cwd).is_some_and(|w| w == worktree) => {
            (Class::Foreign, Some(&mut counts.foreign_wt))
        }
        (Owner::Other, Some(_)) => {
            let class = match scope {
                S3Scope::Repo => Class::Foreign,
                S3Scope::Worktree => Class::Ignored,
            };
            (class, Some(&mut counts.foreign_other_wt))
        }
    }
}

/// One `git` process of a sample.
#[derive(Debug, Clone)]
struct GitSeen {
    owner: Owner,
    /// Its working folder. For a `git` of no session whose folder cannot be
    /// read (it is exiting), that of its nearest live ancestor that can be
    /// read: the shell or the editor that launched it.
    cwd: Option<PathBuf>,
    /// It existed before the write that triggered the sample, so it may
    /// have made it.
    started_before: bool,
    /// `cwd` is the folder of the nearest live ancestor, not its own.
    placed_by_ancestor: bool,
    /// For a foreign `git` with its own readable folder in the repo, outside
    /// the common dir: whether it redirects its target (`None`: its argv or
    /// environment could not be read). `None` for every other `git`.
    redirect: Option<bool>,
}

/// Whose a `git` is, by its ancestry.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Owner {
    /// A present session of the repo.
    Session(String),
    /// The daemon itself (the Time Machine's writer): foreign to every
    /// session, or a restore could be attributed to Claude Code.
    Daemon,
    Other,
}

#[derive(Debug, Clone)]
struct Sighting {
    /// Monotonic mark of the router event that triggered the sample.
    t_recv: u64,
    gits: Vec<GitSeen>,
}

struct Notice {
    repo_id: String,
    t_recv: u64,
    wall_us: u64,
}

#[derive(Default)]
struct State {
    repos: HashMap<String, RepoPaths>,
    live: HashMap<(u32, u64), Live>,
    /// Registered sessions, by session id (US-GRP-009).
    registered: HashMap<String, Registered>,
    /// Classification of each process seen, by identity: only new
    /// processes are classified at each scan.
    classified: HashMap<(u32, u64), bool>,
    sightings: HashMap<String, VecDeque<Sighting>>,
}

struct Inner {
    config: SessionConfig,
    matcher: AgentMatcher,
    procs: Arc<dyn ProcLister>,
    hooks: Arc<HookClaims>,
    clock: Clock,
    sink: ChangeSink,
    /// The daemon itself: its `git`s are foreign to every session.
    own_pid: u32,
    state: Mutex<State>,
    notices: Mutex<Option<Sender<Notice>>>,
    samples: AtomicU64,
    cwd_unreadable: AtomicU64,
    placed_by_ancestor: AtomicU64,
    detected: AtomicU64,
    ended: AtomicU64,
}

/// The running detector, owned by the daemon.
pub struct Detector {
    inner: Arc<Inner>,
    stop: Option<Sender<()>>,
    threads: Vec<JoinHandle<()>>,
}

impl Detector {
    /// Starts the S1 scan and the S3 sampler; `hooks` are the S4 claims, which learn from it
    /// which repos have a present session.
    pub fn start(
        config: SessionConfig,
        matcher: AgentMatcher,
        procs: Arc<dyn ProcLister>,
        hooks: Arc<HookClaims>,
        clock: Clock,
        sink: ChangeSink,
    ) -> Self {
        let (notice_tx, notice_rx) = channel();
        let inner = Arc::new(Inner {
            config,
            matcher,
            procs,
            hooks: Arc::clone(&hooks),
            clock,
            sink,
            own_pid: std::process::id(),
            state: Mutex::new(State::default()),
            notices: Mutex::new(Some(notice_tx)),
            samples: AtomicU64::new(0),
            cwd_unreadable: AtomicU64::new(0),
            placed_by_ancestor: AtomicU64::new(0),
            detected: AtomicU64::new(0),
            ended: AtomicU64::new(0),
        });
        let weak = Arc::downgrade(&inner);
        hooks.attach(Arc::new(move |repo_id| {
            weak.upgrade()
                .is_some_and(|i| i.lock().live.values().any(|l| l.repo_id == repo_id))
        }));
        let (stop_tx, stop_rx) = channel::<()>();
        let mut threads = Vec::new();
        let scanner = Arc::clone(&inner);
        if let Ok(t) = std::thread::Builder::new()
            .name("raptor-sessions".into())
            .spawn(move || scan_loop(&scanner, &stop_rx))
        {
            threads.push(t);
        }
        let sampler = Arc::clone(&inner);
        if let Ok(t) = std::thread::Builder::new()
            .name("raptor-s3".into())
            .spawn(move || sample_loop(&sampler, &notice_rx))
        {
            threads.push(t);
        }
        Self {
            inner,
            stop: Some(stop_tx),
            threads,
        }
    }

    /// The hooks the observer calls from its router.
    pub fn observer_hooks(&self) -> Arc<dyn ObserverHooks> {
        Arc::new(Hooks(Arc::downgrade(&self.inner)))
    }

    /// Starts detecting sessions in a repo, and reconciles the sessions the
    /// store still has open (ADR-GRP-012, engine restart): a session whose
    /// process `(pid, start)` is alive with its working folder in its
    /// worktree continues; the ids of the others are returned, to be closed
    /// as "ended during a gap".
    pub fn watch_repo(
        &self,
        repo_id: &str,
        common: &Path,
        worktrees: Vec<PathBuf>,
        open: Vec<OpenSession>,
    ) -> Vec<String> {
        let table = self.inner.procs.list();
        let now = (self.inner.clock)();
        let mut st = self.inner.lock();
        st.repos.insert(
            repo_id.to_owned(),
            RepoPaths {
                common: common.to_owned(),
                worktrees,
            },
        );
        let mut dead = Vec::new();
        for session in open {
            let alive = parse_session_id(&session.session_id).filter(|(pid, start)| {
                table
                    .as_ref()
                    .is_some_and(|t| t.iter().any(|e| e.pid == *pid && e.start_us == *start))
                    && self
                        .inner
                        .procs
                        .cwd(*pid)
                        .is_some_and(|cwd| cwd.starts_with(&session.worktree))
            });
            match alive {
                Some(key) => {
                    let state = match session.state {
                        SessionStateView::Inactive => SessionStateView::Inactive,
                        _ => SessionStateView::Active,
                    };
                    let threshold = ms(self.inner.config.inactivity);
                    st.live.insert(
                        key,
                        Live {
                            repo_id: repo_id.to_owned(),
                            session_id: session.session_id,
                            worktree: session.worktree,
                            started_ms: session.started_ms,
                            state,
                            last_activity_ms: if state == SessionStateView::Active {
                                now
                            } else {
                                now - threshold
                            },
                        },
                    );
                    st.classified.insert(key, true);
                }
                None => dead.push(session.session_id),
            }
        }
        dead
    }

    /// The worktrees of a repo changed (one was created or removed).
    pub fn set_worktrees(&self, repo_id: &str, worktrees: Vec<PathBuf>) {
        if let Some(repo) = self.inner.lock().repos.get_mut(repo_id) {
            repo.worktrees = worktrees;
        }
    }

    /// Follows a registered session (US-GRP-009): a new registration, or
    /// an open one of the store when the repo starts being observed. It
    /// continues in its state; an idle one stays idle until activity.
    pub fn register(&self, session: RegisteredSession) {
        let now = (self.inner.clock)();
        let threshold = ms(self.inner.config.inactivity);
        let state = match session.state {
            SessionStateView::Inactive => SessionStateView::Inactive,
            _ => SessionStateView::Active,
        };
        self.inner.lock().registered.insert(
            session.session_id.clone(),
            Registered {
                live: Live {
                    repo_id: session.repo_id,
                    session_id: session.session_id,
                    worktree: session.worktree,
                    started_ms: session.started_ms,
                    state,
                    last_activity_ms: match state {
                        SessionStateView::Active => session.last_activity_ms.unwrap_or(now),
                        _ => now - threshold,
                    },
                },
                registration_evidence: session.registration_evidence,
            },
        );
    }

    /// Stops following a registered session: its registration was
    /// withdrawn (Q41: never reopened).
    pub fn end_registered(&self, session_id: &str) {
        self.inner.lock().registered.remove(session_id);
    }

    /// Stops detecting in a repo. Its sessions are not closed: when it is
    /// added again, [`Detector::watch_repo`] reconciles them.
    pub fn forget_repo(&self, repo_id: &str) {
        let mut st = self.inner.lock();
        st.repos.remove(repo_id);
        st.live.retain(|_, l| l.repo_id != repo_id);
        st.registered.retain(|_, r| r.live.repo_id != repo_id);
        st.sightings.remove(repo_id);
    }

    /// The registration as evidence (ADR-GRP-012 rule 3, ADR-GRP-013 § 3):
    /// the registered "other agent" session of `worktree`, when it is the
    /// only present session there.
    pub fn registration_evidence(&self, repo_id: &str, worktree: &Path) -> Option<PresentSession> {
        let st = self.inner.lock();
        let here = |l: &Live| l.repo_id == repo_id && l.worktree == worktree;
        let detected = st.live.values().filter(|l| here(l)).count();
        let mut registered = st.registered.values().filter(|r| here(&r.live));
        match (detected, registered.next(), registered.next()) {
            (0, Some(only), None) if only.registration_evidence => Some(only.live.present()),
            _ => None,
        }
    }

    /// The hint for an event S3 did not see (the short-commit race,
    /// amendment of ADR-GRP-012): the only present session of `worktree`,
    /// when it is a detected one, alive and active. `None` with zero or
    /// several present sessions there, registered ones included.
    pub fn single_session(&self, repo_id: &str, worktree: &Path) -> Option<PresentSession> {
        let st = self.inner.lock();
        let here = |l: &&Live| l.repo_id == repo_id && l.worktree == worktree;
        if st.registered.values().map(|r| &r.live).any(|l| here(&l)) {
            return None;
        }
        let mut detected = st.live.values().filter(here);
        match (detected.next(), detected.next()) {
            (Some(only), None) if only.state == SessionStateView::Active => Some(only.present()),
            _ => None,
        }
    }

    /// Activity in a worktree (BR-WF-001): a Git event observed in it.
    pub fn activity(&self, repo_id: &str, worktree: &Path) {
        self.inner.activity(repo_id, worktree);
    }

    /// Runs one S1 scan now (tests; the scan thread runs it periodically).
    pub fn scan_now(&self) {
        self.inner.scan();
    }

    /// Takes one S3 sample now, as if the router had seen a Git write at
    /// `t_recv` (tests; the router triggers it in the daemon).
    pub fn sample_now(&self, repo_id: &str, t_recv: u64) {
        self.inner.sample(&[Notice {
            repo_id: repo_id.to_owned(),
            t_recv,
            wall_us: wall_us(),
        }]);
    }

    /// The S4 and S3 rules for a Git event of `worktree` observed in the
    /// window that opened at `t_recv` and flushed at `t_flush` (monotonic
    /// marks of the batch). `moved` is the branch move of the event, which
    /// only S4 reads. `scope` says where a foreign `git` counts.
    pub fn evidence(
        &self,
        repo_id: &str,
        worktree: &Path,
        scope: S3Scope,
        moved: Option<RefMove<'_>>,
        t_recv: u64,
        t_flush: u64,
    ) -> S3Evidence {
        let uncounted = |outcome| S3Evidence {
            outcome,
            counts: S3Counts::default(),
        };
        let st = self.inner.lock();
        let present = |id: &str| {
            st.live
                .values()
                .find(|l| l.repo_id == repo_id && l.session_id == id)
                .map(|l| PresentSession {
                    session_id: l.session_id.clone(),
                    worktree: l.worktree.clone(),
                    started_ms: l.started_ms,
                })
        };
        if !st.live.values().any(|l| l.repo_id == repo_id) {
            return uncounted(S3Outcome::NoSession);
        }
        let Some(repo) = st.repos.get(repo_id) else {
            return uncounted(S3Outcome::NoSession);
        };
        // S4 first: it proves which `git` made this move, even with a foreign
        // `git` in the repo at the same time (DS-US-GRP-007 § 7).
        if let Some(moved) = moved {
            let from = t_recv.saturating_sub(ns(hook::CLAIM_LEAD));
            let sessions = self.inner.hooks.take(repo_id, moved, from, t_flush, |cwd| {
                repo.worktree_of(cwd).is_some_and(|w| w == worktree)
            });
            if let [id] = sessions.as_slice()
                && let Some(p) = present(id)
            {
                return uncounted(S3Outcome::Hook(p));
            }
        }
        let from = t_recv.saturating_sub(ns(self.inner.config.s3_lead));
        let mut sessions: Vec<&str> = Vec::new();
        let mut foreign = false;
        let mut counts = S3Counts::default();
        let samples = st.sightings.get(repo_id).into_iter().flatten();
        for sighting in samples.filter(|s| s.t_recv >= from && s.t_recv <= t_flush) {
            for git in &sighting.gits {
                let (class, counter) = classify(git, repo, worktree, scope, &mut counts);
                if let Some(counter) = counter {
                    *counter = counter.saturating_add(1);
                }
                match class {
                    Class::Evidence(id) => {
                        if !sessions.contains(&id) {
                            sessions.push(id);
                        }
                    }
                    Class::Foreign => foreign = true,
                    Class::Ignored => {}
                }
            }
        }
        let outcome = match (sessions.as_slice(), foreign) {
            ([], false) => S3Outcome::NoSighting,
            ([id], false) => present(id).map_or(S3Outcome::NoSighting, S3Outcome::Attributed),
            _ => S3Outcome::Ambiguous,
        };
        S3Evidence { outcome, counts }
    }

    pub fn diagnostics(&self) -> Diagnostics {
        Diagnostics {
            samples: self.inner.samples.load(Ordering::Relaxed),
            cwd_unreadable: self.inner.cwd_unreadable.load(Ordering::Relaxed),
            placed_by_ancestor: self.inner.placed_by_ancestor.load(Ordering::Relaxed),
            sessions_detected: self.inner.detected.load(Ordering::Relaxed),
            sessions_ended: self.inner.ended.load(Ordering::Relaxed),
        }
    }
}

impl Drop for Detector {
    fn drop(&mut self) {
        self.stop.take();
        self.inner
            .notices
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn is_claude(&self, entry: &ProcEntry) -> bool {
        let exe = match &entry.exe {
            Some(exe) => Some(exe.clone()),
            None => self.procs.exe(entry),
        };
        exe.as_deref().is_some_and(|exe| {
            // Linux: a binary replaced while running (the native installer
            // updates itself) shows as `<path> (deleted)`.
            let text = exe.to_string_lossy();
            let exe = text.strip_suffix(" (deleted)").map_or(exe, Path::new);
            self.matcher.classify(exe) == ExeClass::ClaudeCode
        })
    }

    /// One S1 scan: sessions that appeared, ended or went idle.
    fn scan(&self) {
        // Without paths: `is_claude` reads the path of the processes not classified yet.
        let table = self.procs.list_bare();
        let now = (self.clock)();
        let mut st = self.lock();
        let mut changes = Vec::new();
        if let Some(table) = &table {
            self.scan_processes(&mut st, table, now, &mut changes);
        }
        // Idle: no activity for the threshold. Registered sessions too,
        // even where processes cannot be listed.
        let threshold = ms(self.config.inactivity);
        let State {
            live, registered, ..
        } = &mut *st;
        let all = live
            .values_mut()
            .chain(registered.values_mut().map(|r| &mut r.live));
        changes.extend(all.filter_map(|l| l.idle(now, threshold)));
        if !changes.is_empty() {
            // Under the lock: changes reach the loop in the order they
            // happened.
            (self.sink)(changes);
        }
    }

    /// The process part of a scan: sessions that appeared or ended.
    fn scan_processes(
        &self,
        st: &mut State,
        table: &[ProcEntry],
        now: i64,
        changes: &mut Vec<SessionChange>,
    ) {
        let by_pid: HashMap<u32, &ProcEntry> = table.iter().map(|e| (e.pid, e)).collect();
        // Classify only processes not seen before.
        st.classified
            .retain(|key, _| by_pid.get(&key.0).is_some_and(|e| e.start_us == key.1));
        let mut claude: Vec<&ProcEntry> = Vec::new();
        for entry in table {
            let key = (entry.pid, entry.start_us);
            let is = match st.classified.get(&key) {
                Some(is) => *is,
                None => {
                    let is = self.is_claude(entry);
                    st.classified.insert(key, is);
                    is
                }
            };
            if is {
                claude.push(entry);
            }
        }
        // Ended: the process `(pid, start)` is gone (Q41: never reopened).
        let gone: Vec<(u32, u64)> = st
            .live
            .keys()
            .filter(|key| by_pid.get(&key.0).is_none_or(|e| e.start_us != key.1))
            .copied()
            .collect();
        for key in gone {
            if let Some(live) = st.live.remove(&key) {
                self.ended.fetch_add(1, Ordering::Relaxed);
                changes.push(SessionChange::Ended {
                    repo_id: live.repo_id,
                    session_id: live.session_id,
                    at_ms: Some(now),
                    cause: EndCause::ProcessGone,
                });
            }
        }
        // Appeared: a Claude Code process in an observed worktree.
        let place = |st: &State, pid: u32| -> Option<(String, PathBuf)> {
            let cwd = self.procs.cwd(pid)?;
            st.repos
                .iter()
                .filter_map(|(id, repo)| repo.worktree_of(&cwd).map(|w| (id, w)))
                .max_by_key(|(_, w)| w.as_os_str().len())
                .map(|(id, w)| (id.clone(), w.clone()))
        };
        let mut placed: HashMap<u32, Option<(String, PathBuf)>> = HashMap::new();
        for entry in &claude {
            let key = (entry.pid, entry.start_us);
            if st.live.contains_key(&key) {
                continue;
            }
            let here = placed
                .entry(entry.pid)
                .or_insert_with(|| place(st, entry.pid))
                .clone();
            let Some((repo_id, worktree)) = here else {
                continue;
            };
            // A Claude Code under another one in the same worktree is part
            // of that session; in another worktree, it is a session of its
            // own (an orchestrator that launches `claude -p` elsewhere).
            let mut folded = false;
            let mut current = *entry;
            for _ in 0..MAX_DEPTH {
                let Some(parent) = by_pid.get(&current.ppid) else {
                    break;
                };
                if parent.pid == current.pid || parent.start_us > current.start_us {
                    break;
                }
                if claude.iter().any(|c| c.pid == parent.pid) {
                    let theirs = match st.live.get(&(parent.pid, parent.start_us)) {
                        Some(l) => Some((l.repo_id.clone(), l.worktree.clone())),
                        None => placed
                            .entry(parent.pid)
                            .or_insert_with(|| place(st, parent.pid))
                            .clone(),
                    };
                    if theirs.as_ref() == Some(&(repo_id.clone(), worktree.clone())) {
                        folded = true;
                        break;
                    }
                }
                current = parent;
            }
            if folded {
                continue;
            }
            let id = session_id(entry.pid, entry.start_us);
            self.detected.fetch_add(1, Ordering::Relaxed);
            changes.push(SessionChange::Started {
                repo_id: repo_id.clone(),
                session_id: id.clone(),
                worktree: worktree.clone(),
                started_ms: now,
            });
            st.live.insert(
                key,
                Live {
                    repo_id,
                    session_id: id,
                    worktree,
                    started_ms: now,
                    state: SessionStateView::Active,
                    last_activity_ms: now,
                },
            );
        }
    }

    fn activity(&self, repo_id: &str, worktree: &Path) {
        let now = (self.clock)();
        let mut st = self.lock();
        let State {
            live, registered, ..
        } = &mut *st;
        let changes: Vec<SessionChange> = live
            .values_mut()
            .chain(registered.values_mut().map(|r| &mut r.live))
            .filter(|l| l.repo_id == repo_id && l.worktree == worktree)
            .filter_map(|l| l.touch(now))
            .collect();
        if !changes.is_empty() {
            (self.sink)(changes);
        }
    }

    /// Whether the repo has a present session (S3 samples only then).
    fn has_sessions(&self, repo_id: &str) -> bool {
        self.lock().live.values().any(|l| l.repo_id == repo_id)
    }

    /// One S3 sample for each repo of `notices`: every live `git` of the
    /// user, the session it descends from and its working folder.
    fn sample(&self, notices: &[Notice]) {
        let Some(table) = self.procs.list() else {
            return;
        };
        self.samples.fetch_add(1, Ordering::Relaxed);
        let by_pid: HashMap<u32, &ProcEntry> = table.iter().map(|e| (e.pid, e)).collect();
        let gits: Vec<&ProcEntry> = table.iter().filter(|e| is_git(e)).collect();
        let cwds: HashMap<u32, Option<PathBuf>> = gits
            .iter()
            .map(|g| (g.pid, self.procs.cwd(g.pid)))
            .collect();
        let unreadable = cwds.values().filter(|c| c.is_none()).count() as u64;
        self.cwd_unreadable.fetch_add(unreadable, Ordering::Relaxed);
        let tolerance = u64::try_from(self.config.start_tolerance.as_micros()).unwrap_or(0);
        // What each notice needs from the state, copied so the reads below
        // (folders of ancestors, argv and environment) never hold the lock
        // the scan and the router wait on.
        let snapshots: Vec<(Sessions, Option<RepoPaths>)> = {
            let st = self.lock();
            notices
                .iter()
                .map(|notice| {
                    let sessions = st
                        .live
                        .iter()
                        .filter(|(_, l)| l.repo_id == notice.repo_id)
                        .map(|(k, l)| (*k, l.session_id.clone()))
                        .collect();
                    (sessions, st.repos.get(&notice.repo_id).cloned())
                })
                .collect()
        };
        // Whether each foreign `git` redirects its target: one boolean per
        // pid, never a value of its argv or environment (SEC-04), and at most
        // [`MAX_REDIRECT_READS`] reads.
        let mut redirects: HashMap<u32, Option<bool>> = HashMap::new();
        let mut redirect_of = |pid: u32| -> Option<bool> {
            if let Some(known) = redirects.get(&pid) {
                return *known;
            }
            let read = if redirects.len() < MAX_REDIRECT_READS {
                self.procs.git_redirect(pid)
            } else {
                None
            };
            redirects.insert(pid, read);
            read
        };
        let mut sightings = Vec::with_capacity(notices.len());
        for (notice, (sessions, repo)) in notices.iter().zip(&snapshots) {
            let repo = repo.as_ref();
            let seen: Vec<GitSeen> = gits
                .iter()
                .map(|g| {
                    let owner = self.owner(g, &by_pid, sessions);
                    let mut cwd = cwds.get(&g.pid).cloned().flatten();
                    let mut placed_by_ancestor = false;
                    let mut redirect = None;
                    if owner == Owner::Other {
                        match &cwd {
                            None => {
                                cwd = self.launched_from(g, &by_pid);
                                if cwd.is_some() {
                                    placed_by_ancestor = true;
                                    self.placed_by_ancestor.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                            // Only where the answer can change the outcome:
                            // its own folder, in the repo, outside the
                            // common dir. Read once per sample. A foreign
                            // `git` whose own folder is outside the repo is
                            // not read and never counts here, even if `-C`
                            // points it into the repo: a declared gap.
                            Some(own) => {
                                if repo.is_some_and(|r| r.contains(own) && !r.in_common(own)) {
                                    redirect = redirect_of(g.pid);
                                }
                            }
                        }
                    }
                    GitSeen {
                        owner,
                        cwd,
                        started_before: g.start_us <= notice.wall_us.saturating_add(tolerance),
                        placed_by_ancestor,
                        redirect,
                    }
                })
                .collect();
            sightings.push((notice, seen));
        }
        let mut st = self.lock();
        for (notice, seen) in sightings {
            let ring = st.sightings.entry(notice.repo_id.clone()).or_default();
            ring.push_back(Sighting {
                t_recv: notice.t_recv,
                gits: seen,
            });
            while ring.len() > MAX_SIGHTINGS {
                ring.pop_front();
            }
        }
    }

    /// Whose `git` is: the present session it descends from, the daemon,
    /// or nobody known. The walk follows the rules of `requester.rs`: at
    /// most [`MAX_DEPTH`] steps, a parent younger than its child is a reused
    /// pid and ends it, and it stops at the daemon.
    fn owner(
        &self,
        git: &ProcEntry,
        by_pid: &HashMap<u32, &ProcEntry>,
        sessions: &HashMap<(u32, u64), String>,
    ) -> Owner {
        let mut current = git;
        for _ in 0..MAX_DEPTH {
            if current.pid == self.own_pid {
                return Owner::Daemon;
            }
            if let Some(id) = sessions.get(&(current.pid, current.start_us)) {
                return Owner::Session(id.clone());
            }
            match parent_of(current, by_pid) {
                Some(parent) => current = parent,
                None => return Owner::Other,
            }
        }
        Owner::Other
    }

    /// The working folder of the nearest live ancestor of `git` that can be
    /// read, with the same walk rules as [`Inner::owner`].
    fn launched_from(&self, git: &ProcEntry, by_pid: &HashMap<u32, &ProcEntry>) -> Option<PathBuf> {
        let mut current = git;
        for _ in 0..MAX_DEPTH {
            let parent = parent_of(current, by_pid)?;
            if parent.pid == self.own_pid {
                return None;
            }
            if let Some(cwd) = self.procs.cwd(parent.pid) {
                return Some(cwd);
            }
            current = parent;
        }
        None
    }
}

/// The parent of `child` in the table, unless it is younger than the child
/// (a reused pid) or the root.
fn parent_of<'a>(child: &ProcEntry, by_pid: &HashMap<u32, &'a ProcEntry>) -> Option<&'a ProcEntry> {
    let parent = by_pid.get(&child.ppid)?;
    (parent.pid != child.pid && parent.start_us <= child.start_us).then_some(*parent)
}

fn is_git(entry: &ProcEntry) -> bool {
    entry
        .exe
        .as_deref()
        .and_then(Path::file_name)
        .is_some_and(|n| n.eq_ignore_ascii_case("git") || n.eq_ignore_ascii_case("git.exe"))
}

fn scan_loop(inner: &Inner, stop: &Receiver<()>) {
    // The first scan after one interval: the repos are added just after the
    // detector starts.
    while let Err(RecvTimeoutError::Timeout) = stop.recv_timeout(inner.config.scan_interval) {
        inner.scan();
    }
}

/// Samples as soon as the first notice arrives; notices that queued up
/// meanwhile share the next sample (the earliest one, the most permissive
/// "started before").
fn sample_loop(inner: &Inner, rx: &Receiver<Notice>) {
    while let Ok(first) = rx.recv() {
        inner.sample(&[first]);
        let mut pending: Vec<Notice> = Vec::new();
        while let Ok(more) = rx.try_recv() {
            if !pending.iter().any(|n| n.repo_id == more.repo_id) {
                pending.push(more);
            }
        }
        if !pending.is_empty() {
            inner.sample(&pending);
        }
    }
}

/// The observer's view of the detector. Weak: the observer may outlive it.
struct Hooks(std::sync::Weak<Inner>);

impl ObserverHooks for Hooks {
    fn worktree_touched(&self, repo_id: &str, root: &Path) {
        if let Some(inner) = self.0.upgrade() {
            inner.activity(repo_id, root);
        }
    }

    fn git_dir_touched(&self, repo_id: &str, t_recv: u64) {
        let Some(inner) = self.0.upgrade() else {
            return;
        };
        if !inner.has_sessions(repo_id) {
            return;
        }
        let wall_us = wall_us();
        if let Some(tx) = inner
            .notices
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            let _ = tx.send(Notice {
                repo_id: repo_id.to_owned(),
                t_recv,
                wall_us,
            });
        }
    }
}

fn wall_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn ms(d: Duration) -> i64 {
    i64::try_from(d.as_millis()).unwrap_or(i64::MAX)
}

fn ns(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;
