//! The engine process: one background daemon per OS user (TS-GRP-003,
//! ADR-GRP-005 § 1, § 2 and § 4).
//!
//! Lifecycle: take the instance lock, open the profile, resolve Git and
//! enter the state of BR-WF-002. While observing, persist the "observed
//! until" mark periodically. On an orderly stop, persist it again, record
//! the cause and release the lock. On the next start, a run that never
//! recorded its stop is a crash; with an active session it is classified as
//! "crash during an active session" (SEC-13).
//!
//! The client channel (TS-GRP-004, [`crate::channel`]) is bound in
//! [`Daemon::start`] and served from [`Daemon::run`]; its stop command and
//! audit reach the loop through [`ShutdownHandle`]. The gap itself is
//! recorded by US-GRP-005 from [`StartupReport::repos`].
//!
//! Before accepting Time Machine operations, the start also opens the
//! oplog of every observed repo and runs its recovery (TS-TMC-002,
//! ADR-TMC-003 § 6). It does not depend on the engine store: a repo whose
//! engine store cannot be opened still recovers its oplog (Q26).

mod env;
mod guard;
mod lock;
mod log;
mod modules;
mod repos;
mod serve;
mod sessions;
mod shutdown;
mod state;
mod tm;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gitraptor_api::Untrusted;
use gitraptor_api::event::{DAEMON_STOPPING, ENGINE_STATE};
use gitraptor_api::messages::{
    EngineStateView, EngineView, GitEventDetails, GitEventKind, GitEventView, RepoAddOutcome,
    RepoStateView, RepoView, StoppingData,
};
use gitraptor_policy::team::BaseBranch;

use gitraptor_git::SystemGit;
use gitraptor_git::resolve::{Resolution, ResolveConfig, resolve};

use crate::channel::{ChannelConfig, EngineShared, EventBus};
use crate::guardrails::GuardRegistry;
use crate::observe::{self, RepoRead};
use crate::profile::{
    DaemonRun, Event as StoredEvent, GapCause, Profile, ProfileDirs, ProfileError, RepoEntry,
    RepoState, RepoStore, StoreOpen, WriteOp, fsperm,
};
use crate::timemachine::oplog::{
    AbsentStore, ChainBreak, Oplog, OplogStatus, RecoveryOptions, RecoveryReport, SnapshotRefs,
    SystemProbe,
};
use crate::timemachine::protected::{OperationsWiring, TmRepos};
use crate::timemachine::store::SnapshotStore;
use crate::watch::Observer;

pub use env::{AGENT_EXECUTABLES_ENV, CLOCK_SKEW_FILE_ENV, DaemonEnv, TM_NO_FREE_SPACE_FLOOR_ENV};
use guard::recover_guardrails;
pub use lock::{InstanceLock, LOCK_FILE, running_pid, wait_until_released};
pub use log::{Field, LOG_FILE, Level, LogLimits, Logger};
use shutdown::Control;
pub(crate) use shutdown::{GuardReply, GuardRequest};
pub(crate) use shutdown::{RegisterRequest, RepoAddRequest, WithdrawRequest};
pub use shutdown::{
    RegistrationError, RepoCommandError, ShutdownHandle, StopCause, install_signal_handlers,
};
pub use state::{EngineState, InvalidTransition, Trigger};
pub use tm::TmCapture;

/// Exit code of a second `raptor daemon` that found another one running.
/// Service managers must not treat it as a failure to relaunch (US-GRP-004).
pub const EXIT_ALREADY_RUNNING: i32 = 3;

/// Longest the start waits, over all repos, for annotated children of an
/// interrupted operation before keeping their locks (ADR-TMC-003 § 6.4).
pub const TM_RECOVERY_WAIT: Duration = Duration::from_secs(5);

/// Serialized size past which a `worktree.state` event or a snapshot drops
/// its change lists and keeps the counts, under the 1 MiB message limit.
pub const CHANGE_LIST_BUDGET: usize = 768 * 1024;

/// Everything that can stop the daemon from starting or running.
#[derive(Debug)]
pub enum DaemonError {
    /// Another daemon holds the instance lock. Nothing was observed and the
    /// store was not touched.
    AlreadyRunning {
        pid: Option<u32>,
    },
    /// The lock file is not a private regular file of the current user.
    InsecureLock {
        path: PathBuf,
        reason: String,
    },
    Profile(ProfileError),
    Io(std::io::Error),
    Unsupported(&'static str),
}

impl std::fmt::Display for DaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRunning { pid: Some(pid) } => {
                write!(f, "the GitRaptor daemon is already running (pid {pid})")
            }
            Self::AlreadyRunning { pid: None } => {
                write!(f, "the GitRaptor daemon is already running")
            }
            Self::InsecureLock { path, reason } => {
                write!(f, "instance lock {} {reason}", path.display())
            }
            Self::Profile(err) => write!(f, "{err}"),
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Unsupported(what) => write!(f, "not supported: {what}"),
        }
    }
}

impl std::error::Error for DaemonError {}

impl From<ProfileError> for DaemonError {
    fn from(err: ProfileError) -> Self {
        Self::Profile(err)
    }
}

impl From<std::io::Error> for DaemonError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// Inputs of the daemon. Tests inject a temporary profile, their own
/// environment and Git candidates.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub dirs: ProfileDirs,
    pub env: DaemonEnv,
    pub git: ResolveConfig,
    /// How often "observed until" is persisted while observing.
    pub heartbeat: Duration,
    pub log: LogLimits,
    /// Past this, a stuck orderly stop exits the process (it is then seen
    /// as a crash). `None` in tests that run the daemon in-process.
    pub stop_deadline: Option<Duration>,
    pub channel: ChannelConfig,
    /// Protected operations with a double of the repo layer (TS-TMC-004
    /// tests). Wins over `operations`.
    pub protected: Option<crate::channel::ProtectedWiring>,
    /// The catalog of user operations over the daemon's own repo layer
    /// (US-TMC-001, TS-CKP-002). `None` with no `protected` either:
    /// `operation.prepare` answers "not implemented" until the first
    /// operation story wires its catalog (US-MCP-008).
    pub operations: Option<OperationsWiring>,
    /// Tests only: wraps the snapshotter of the Time Machine's own commands
    /// (undo) to inject faults. Honored only in debug builds.
    #[doc(hidden)]
    pub tm_prior_layer: Option<TmPriorLayer>,
    /// The Time Machine's continuous capture (US-TMC-004).
    pub tm_capture: TmCapture,
}

/// A layer over the snapshotter of the Time Machine's own commands (tests).
#[derive(Clone)]
pub struct TmPriorLayer(pub crate::timemachine::protected::SnapshotterLayer);

impl std::fmt::Debug for TmPriorLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TmPriorLayer")
    }
}

impl DaemonConfig {
    /// Configuration of the real daemon for the current user.
    pub fn for_current_user() -> Result<Self, DaemonError> {
        let env = DaemonEnv::capture();
        let mut channel = ChannelConfig::default();
        if let Some(names) = env::agent_executables_override() {
            channel.agents = crate::channel::AgentMatcher::only(names);
        }
        channel.autostart = crate::autostart::Autostart::for_current_user();
        Ok(Self {
            dirs: ProfileDirs::resolve()?,
            git: env.git_resolve_config(None),
            env,
            heartbeat: Duration::from_secs(60),
            log: LogLimits::default(),
            stop_deadline: Some(Duration::from_secs(5)),
            channel,
            protected: None,
            operations: None,
            tm_prior_layer: None,
            tm_capture: TmCapture {
                no_free_space_floor: env::tm_no_free_space_floor(),
                ..TmCapture::default()
            },
        })
    }
}

/// What the previous run means for one observed repo: the gap US-GRP-005
/// records, from the repo's "observed until" mark to this start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingGap {
    /// Last persisted "observed until", if any.
    pub from_ms: Option<i64>,
    pub cause: GapCause,
    /// Client or signal that caused the stop (diagnostic for signals).
    pub requested_by: Option<String>,
}

/// One observed repo at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoStartup {
    pub repo_id: String,
    pub store: StoreOpen,
    /// `None` when no daemon ran before on this profile.
    pub pending_gap: Option<PendingGap>,
}

/// The Time Machine oplog of one observed repo at startup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmStartup {
    pub repo_id: String,
    pub oplog: OplogStatus,
    /// Breaks of the hash chain declared by this start (SEC-TMC-09).
    pub new_breaks: Vec<ChainBreak>,
    pub recovery: RecoveryReport,
}

/// What the daemon found when it started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupReport {
    /// The previous run, as recorded before this start overwrote it.
    pub previous: DaemonRun,
    pub git: Option<SystemGit>,
    pub repos: Vec<RepoStartup>,
    /// Observed repos whose store could not be opened (e.g. schema from a
    /// newer binary); they are not observed.
    pub unavailable: Vec<String>,
    /// Oplog and recovery of every observed repo (TS-TMC-002).
    pub time_machine: Vec<TmStartup>,
    /// Observed repos whose oplog could not be opened or recovered: no Time
    /// Machine operation is accepted on them.
    pub tm_unavailable: Vec<String>,
}

/// How the daemon stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StopReport {
    pub cause: StopCause,
    pub stopped_ms: i64,
    /// Whether the stop was recorded as orderly. If persisting a mark failed
    /// it is not, and the next start sees a crash.
    pub recorded: bool,
}

/// Classifies the interval since the previous run for a repo (ADR-GRP-013
/// § 5, SEC-13). A signal never identifies who sent it, so a stop by signal
/// with an active session counts as a crash during that session; only an
/// authorized stop command is an attributed stop.
pub fn classify_previous_run(
    previous: &DaemonRun,
    active_session: bool,
) -> Option<(GapCause, Option<String>)> {
    let down = if active_session {
        GapCause::DaemonDownDuringSession
    } else {
        GapCause::DaemonDown
    };
    match previous {
        DaemonRun::Never => None,
        DaemonRun::Running { .. } => Some((down, None)),
        DaemonRun::Stopped {
            cause,
            requested_by,
            ..
        } if cause == "stop-command" || replaced_by_newer(cause) => {
            Some((GapCause::DaemonStopped, requested_by.clone()))
        }
        DaemonRun::Stopped { requested_by, .. } => Some((down, requested_by.clone())),
    }
}

/// The running daemon. Holds the instance lock for its whole life.
pub struct Daemon {
    config: DaemonConfig,
    lock: InstanceLock,
    logger: Logger,
    profile: Profile,
    state: EngineState,
    stores: Vec<(String, RepoStore)>,
    /// Oplog and snapshot store of every observed repo, shared with the
    /// channel's protected operations (US-TMC-001).
    tm: Arc<TmRepos>,
    /// Protected repos as `guard.evaluate` reads them (US-GRD-001).
    guard: Arc<GuardRegistry>,
    /// Engine marks and calm per repo, for the Time Machine (US-TMC-004).
    marks: Arc<crate::timemachine::engine::RepoMarks>,
    /// The feature modules: the Time Machine's continuous capture
    /// (US-TMC-004) and the ones after it (ADR-GRP-016 § 5).
    modules: modules::Modules,
    report: StartupReport,
    handle: ShutdownHandle,
    control_rx: Receiver<Control>,
    bus: Arc<EventBus>,
    /// The change observer (US-GRP-002); `None` outside "Observing".
    observer: Option<Observer>,
    /// The session detector (US-GRP-007), started with the observer.
    detector: Option<crate::detect::Detector>,
    /// The engine's own consumption (US-GRP-017).
    resources: Arc<crate::resources::ResourceMonitor>,
    /// Ahead/behind already counted for observed changes (US-GRP-012).
    divergence_cache: observe::DivergenceCache,
    /// Periodic reconciliations that found differences: a value above 0 in
    /// dogfooding points to an unidentified cause of loss (ADR-GRP-010 § 5).
    periodic_diffs: u64,
    #[cfg_attr(not(unix), allow(dead_code))]
    started_ms: i64,
    #[cfg(unix)]
    bound: Option<crate::channel::BoundChannel>,
    #[cfg(unix)]
    server: Option<crate::channel::Server>,
}

impl Daemon {
    /// Starts the daemon: lock, log, profile, Git, state.
    ///
    /// A second daemon fails here with [`DaemonError::AlreadyRunning`]
    /// before opening the profile or the log: it only creates or verifies
    /// the private folders that lead to the lock.
    pub fn start(config: DaemonConfig) -> Result<Self, DaemonError> {
        fsperm::set_restrictive_umask();
        for dir in config.dirs.owned_dirs() {
            if config.dirs.state.starts_with(dir) {
                fsperm::ensure_private_dir(dir)?;
            }
        }
        let lock = InstanceLock::acquire(&config.dirs.state)?;
        let logger = Logger::open(&config.dirs.state, config.log)?;

        let (mut profile, opened) = match Profile::open(config.dirs.clone()) {
            Ok(opened) => opened,
            Err(err) => {
                logger.error(
                    "profile_open_failed",
                    &[("kind", profile_error_kind(&err).into())],
                );
                logger.flush();
                return Err(err.into());
            }
        };
        if opened.created {
            logger.info(
                "profile_created",
                &[("instance", Field::id(profile.instance_id()))],
            );
        }
        if opened.quarantined_index.is_some() {
            logger.warn("index_quarantined", &[]);
        }

        // Read the previous mark before overwriting it.
        let previous = profile.daemon_run()?;
        profile.mark_daemon_running(now_ms())?;

        let git = match resolve(&config.git, &config.env.invoker()) {
            Resolution::Found { git, .. } => Some(git),
            Resolution::NotFound { .. } => None,
        };

        let mut report = StartupReport {
            previous,
            git: git.clone(),
            repos: Vec::new(),
            unavailable: Vec::new(),
            time_machine: Vec::new(),
            tm_unavailable: Vec::new(),
        };
        let mut stores = Vec::new();
        for entry in profile.repos()? {
            if entry.state != RepoState::Observed {
                continue;
            }
            match profile.open_store(&entry.repo_id) {
                Ok((store, status)) => {
                    let active = store.has_active_sessions()?;
                    let pending_gap = classify_previous_run(&report.previous, active).map(
                        |(cause, requested_by)| PendingGap {
                            from_ms: store.observed_until().ok().flatten(),
                            cause,
                            requested_by,
                        },
                    );
                    if let Some(gap) = &pending_gap {
                        logger.info(
                            "repo_pending_gap",
                            &[
                                ("repo", Field::id(&entry.repo_id)),
                                ("cause", gap.cause.as_str().into()),
                            ],
                        );
                    }
                    report.repos.push(RepoStartup {
                        repo_id: entry.repo_id.clone(),
                        store: status,
                        pending_gap,
                    });
                    stores.push((entry.repo_id, store));
                }
                Err(err) => {
                    logger.warn(
                        "repo_unavailable",
                        &[
                            ("repo", Field::id(&entry.repo_id)),
                            ("kind", profile_error_kind(&err).into()),
                        ],
                    );
                    report.unavailable.push(entry.repo_id);
                }
            }
        }

        let tm = Arc::new(TmRepos::new(config.dirs.clone()));
        for (repo_id, common_dir, oplog) in
            recover_time_machine(&config.dirs, &profile, &logger, &mut report)?
        {
            tm.insert(&repo_id, &common_dir, oplog);
        }

        // Guardrails: a confirmed install is published again; one left half-way is completed
        // or undone before anything else runs (ADR-GRD-001 § 4, Recuperación).
        let guard = Arc::new(GuardRegistry::default());
        recover_guardrails(
            &config,
            &profile,
            &logger,
            git.as_ref(),
            &mut stores,
            &guard,
        );

        let state = EngineState::initial(git.is_some(), stores.len());
        if state != EngineState::Observing {
            // Nothing is observed outside "Observing" (BR-WF-002).
            stores.clear();
        }

        // The channel is bound before the daemon creates any thread.
        #[cfg(unix)]
        let bound = match config.dirs.runtime.as_deref() {
            Some(runtime) => {
                for dir in config.dirs.owned_dirs() {
                    if runtime.starts_with(dir) {
                        fsperm::ensure_private_dir(dir)?;
                    }
                }
                match crate::channel::BoundChannel::bind(runtime) {
                    Ok(bound) => Some(bound),
                    Err(err) => {
                        logger.error(
                            "channel_bind_failed",
                            &[("kind", profile_error_kind(&err).into())],
                        );
                        logger.flush();
                        return Err(err.into());
                    }
                }
            }
            None => None,
        };
        // Windows: no channel at all rather than one without access control
        // (`channel::TRANSPORT_UNSUPPORTED`); the engine still observes.
        // Pendiente: etapa de validación multiplataforma.
        #[cfg(not(unix))]
        logger.warn("channel_unsupported", &[]);

        // Full reconciliation of every observed repo before serving
        // (ADR-GRP-010 § 6), one thread per repo; the start does not count
        // for NFR-04.
        let entries = profile.repos()?;
        let reads = if state == EngineState::Observing {
            reconcile_all(&entries, &stores)
        } else {
            Vec::new()
        };
        let mut repo_views = Vec::new();
        let mut divergence = BTreeMap::new();
        for entry in entries {
            let state = if report.unavailable.contains(&entry.repo_id) {
                RepoStateView::Unavailable
            } else if entry.state == RepoState::Observed {
                RepoStateView::Observed
            } else {
                continue;
            };
            let read = reads
                .iter()
                .find(|(id, _)| *id == entry.repo_id)
                .and_then(|(_, read)| read.as_ref());
            if let (Some(read), Some((_, store))) =
                (read, stores.iter_mut().find(|(id, _)| *id == entry.repo_id))
            {
                persist_read(store, read, &logger, &entry.repo_id);
            }
            if let Some(read) = read {
                divergence.insert(entry.repo_id.clone(), read.divergence_inputs());
            }
            let base = match read {
                Some(read) => read.base.clone(),
                None => repo_base(&stores, &entry.repo_id),
            };
            repo_views.push(RepoView {
                repo_id: entry.repo_id,
                state,
                path: Untrusted::from_os(entry.canonical_path.as_os_str()),
                base: observe::base_view(&base),
                worktrees: read.map(RepoRead::views).unwrap_or_default(),
            });
        }
        let bus = Arc::new(EventBus::new(
            run_id(),
            EngineShared {
                engine: engine_view(state, git.as_ref()),
                repos: repo_views,
                divergence,
            },
            config.channel.limits.replay,
        ));
        let mut fields = vec![
            ("state", state.as_str().into()),
            ("repos", report.repos.len().into()),
            ("previous", previous_kind(&report.previous).into()),
            ("pid", std::process::id().into()),
        ];
        match &git {
            Some(git) => fields.push(("git", git.version.into())),
            None => fields.push(("git", "not-found".into())),
        }
        let (handle, control_rx) = ShutdownHandle::new();
        let config_dirs = config.dirs.clone();
        let mut daemon = Self {
            config,
            lock,
            logger,
            profile,
            state,
            stores,
            tm,
            guard,
            marks: Arc::default(),
            modules: modules::Modules::default(),
            report,
            handle,
            control_rx,
            bus,
            observer: None,
            detector: None,
            resources: Arc::new(crate::resources::ResourceMonitor::new(
                crate::resources::ResourceConfig::from_env(),
                config_dirs,
                Arc::default(),
            )),
            divergence_cache: observe::DivergenceCache::default(),
            periodic_diffs: 0,
            started_ms: now_ms(),
            #[cfg(unix)]
            bound,
            #[cfg(unix)]
            server: None,
        };
        let repo_ids: Vec<String> = daemon.stores.iter().map(|(id, _)| id.clone()).collect();
        for repo_id in &repo_ids {
            daemon.link_marks(repo_id);
        }
        daemon.modules = modules::Modules::start(&daemon);
        // The observer starts after the reconciliation; each worktree task
        // reads once more when its watch runs, so nothing in between is
        // lost (ADR-GRP-010 § 6).
        for (repo_id, read) in &reads {
            let (Some(read), Some(entry)) = (read, daemon.profile.repo(repo_id).ok().flatten())
            else {
                continue;
            };
            daemon.observe(repo_id, &entry.canonical_path, read);
        }
        // Logged once everything runs: observing, detecting sessions
        // (US-GRP-007). Clients and tests take it as "started".
        daemon.logger.info("daemon_started", &fields);
        Ok(daemon)
    }

    pub fn state(&self) -> EngineState {
        self.state
    }

    pub fn report(&self) -> &StartupReport {
        &self.report
    }

    pub fn logger(&self) -> &Logger {
        &self.logger
    }

    pub fn profile(&self) -> &Profile {
        &self.profile
    }

    /// The recovered oplog of an observed repo; `None` if it is unavailable.
    pub fn oplog(&self, repo_id: &str) -> Option<Arc<Mutex<Oplog>>> {
        self.tm.oplog(repo_id)
    }

    /// What a Time Machine capture needs: repos, the engine, the profile.
    fn capture_deps(&self) -> crate::timemachine::continuous::CaptureDeps {
        let tm = &self.config.tm_capture;
        crate::timemachine::continuous::CaptureDeps {
            repos: Arc::clone(&self.tm),
            engine: Arc::new(tm::DaemonEngine {
                marks: Arc::clone(&self.marks),
                handle: self.handle.clone(),
            }),
            profile: self.config.dirs.clone(),
            logger: self.logger.clone(),
            layer: tm.layer.clone().filter(|_| cfg!(debug_assertions)),
            free_space_floor: (!(tm.no_free_space_floor && cfg!(debug_assertions)))
                .then(crate::timemachine::continuous::FreeSpaceFloor::default),
        }
    }

    /// Handle for signals and the channel's stop command.
    pub fn shutdown_handle(&self) -> ShutdownHandle {
        self.handle.clone()
    }

    /// The event bus: stories publish their events here (ADR-GRP-005 § 5).
    pub fn events(&self) -> Arc<EventBus> {
        Arc::clone(&self.bus)
    }

    /// Starts serving the channel, then runs until a stop request arrives
    /// and stops in order.
    pub fn run(mut self) -> StopReport {
        self.resources.start();
        self.serve_channel();
        let mut next_beat = Instant::now() + self.config.heartbeat;
        loop {
            let wait = next_beat.saturating_duration_since(Instant::now());
            match self.control_rx.recv_timeout(wait) {
                Ok(Control::Stop(cause)) => return self.stop(cause),
                Ok(Control::Audit(row, reply)) => {
                    let id = match self.profile.append_audit(&row) {
                        Ok(id) => Some(id),
                        Err(err) => {
                            self.logger.error(
                                "audit_write_failed",
                                &[("kind", profile_error_kind(&err).into())],
                            );
                            None
                        }
                    };
                    let _ = reply.send(id);
                }
                Ok(Control::AuditList {
                    after_id,
                    limit,
                    reply,
                }) => {
                    let _ = reply.send(self.profile.audit(after_id, limit).ok());
                }
                Ok(Control::RepoAdd(request, reply)) => {
                    let _ = reply.send(self.add_repo(*request));
                }
                Ok(Control::RepoRetire { repo_id, reply }) => {
                    let _ = reply.send(self.retire_repo(&repo_id));
                }
                Ok(Control::Observed(batch)) => self.observed(*batch),
                Ok(Control::Sessions(changes)) => self.sessions_changed(changes),
                Ok(Control::SessionsList { params, reply }) => {
                    let _ = reply.send(self.sessions_list(&params));
                }
                Ok(Control::Register { request, reply }) => {
                    let _ = reply.send(self.register(request));
                }
                Ok(Control::Withdraw { request, reply }) => {
                    let _ = reply.send(self.withdraw(request));
                }
                Ok(Control::EventHistory { params, reply }) => {
                    let _ = reply.send(self.event_history(&params));
                }
                Ok(Control::Guard {
                    common_dir,
                    request,
                    deadline,
                    reply,
                }) => {
                    // A late install never runs after its caller was told it failed.
                    let answer = if Instant::now() >= deadline {
                        GuardReply::Failed
                    } else {
                        self.guard(&common_dir, request)
                    };
                    let _ = reply.send(answer);
                }
                Ok(Control::RawEvents {
                    repo_id,
                    worktree,
                    reply,
                }) => {
                    let _ = reply.send(self.raw_events(&repo_id, &worktree));
                }
                Err(RecvTimeoutError::Timeout) => {
                    self.persist_observed_until(now_ms());
                    self.check_channel();
                    next_beat = Instant::now() + self.config.heartbeat;
                }
                // The daemon holds a sender, so this cannot happen; stop
                // rather than spin.
                Err(RecvTimeoutError::Disconnected) => {
                    return self.stop(StopCause::Signal("INTERNAL"));
                }
            }
        }
    }

    /// Orderly stop: persist "observed until", record the stop with its
    /// cause, flush the log and release the lock, in that order.
    pub fn stop(mut self, cause: StopCause) -> StopReport {
        if let Some(deadline) = self.config.stop_deadline {
            let logger = self.logger.clone();
            let _ = std::thread::Builder::new()
                .name("raptor-stop-deadline".into())
                .spawn(move || {
                    std::thread::sleep(deadline);
                    logger.error("stop_deadline_exceeded", &[]);
                    logger.flush();
                    std::process::exit(1);
                });
        }
        self.bus.publish(
            DAEMON_STOPPING,
            StoppingData {
                cause: cause.code(),
            },
            None,
            |_| {},
        );
        // Answers already queued (the stop command's included) are written,
        // then every connection closes.
        #[cfg(unix)]
        if let Some(mut server) = self.server.take() {
            server.shutdown();
        }
        self.resources.stop();
        let stopped_ms = now_ms();
        let marks_ok = self.persist_observed_until(stopped_ms);
        let recorded = marks_ok
            && match self.profile.mark_daemon_stopped(
                stopped_ms,
                &cause.stored(),
                Some(&cause.requested_by()),
            ) {
                Ok(()) => true,
                Err(err) => {
                    self.logger.error(
                        "stop_mark_failed",
                        &[("kind", profile_error_kind(&err).into())],
                    );
                    false
                }
            };
        let mut fields = vec![
            ("cause", cause.as_str().into()),
            ("recorded", recorded.into()),
        ];
        if let StopCause::Signal(name) = &cause {
            fields.push(("signal", (*name).into()));
        }
        self.logger.info("daemon_stopped", &fields);
        self.logger.flush();
        let Self {
            lock,
            stores,
            tm,
            profile,
            observer,
            detector,
            modules,
            ..
        } = self;
        drop(observer);
        drop(detector);
        // A module's work in progress ends before the stores close.
        modules.stop();
        // An operation still running keeps its own handle until it ends.
        tm.clear();
        drop(tm);
        drop(stores);
        drop(profile);
        if lock.release().is_err() {
            // Closing the file below releases it anyway.
        }
        StopReport {
            cause,
            stopped_ms,
            recorded,
        }
    }

    /// The installed `raptor`: the binary the daemon was launched from, as launched (absolute,
    /// links not resolved: a stable link survives an upgrade, ADR-GRD-001 § 8).
    fn raptor_path(&self) -> Option<PathBuf> {
        raptor_path(&self.config)
    }

    /// Applies a BR-WF-002 transition and publishes the new engine view.
    fn transition(&mut self, trigger: Trigger) {
        let Ok(next) = self.state.on(trigger) else {
            return;
        };
        self.state = next;
        let view = engine_view(next, self.report.git.as_ref());
        self.logger
            .info("engine_state", &[("state", next.as_str().into())]);
        let shared = view.clone();
        self.bus
            .publish(ENGINE_STATE, view, None, move |s| s.engine = shared);
    }

    fn repo_command_failed(&self, event: &'static str, err: &ProfileError) -> RepoCommandError {
        self.logger
            .error(event, &[("kind", profile_error_kind(err).into())]);
        RepoCommandError::Internal
    }

    /// Writes "observed until" to every observed repo. Returns whether all
    /// writes succeeded.
    fn persist_observed_until(&mut self, ms: i64) -> bool {
        let mut ok = true;
        for (repo_id, store) in &mut self.stores {
            if let Err(err) = store.write_batch(&[WriteOp::SetObservedUntil { ms }]) {
                ok = false;
                self.logger.error(
                    "observed_until_failed",
                    &[
                        ("repo", Field::id(repo_id)),
                        ("kind", profile_error_kind(&err).into()),
                    ],
                );
            }
        }
        ok
    }
}

/// Opens the oplog of every observed repo and recovers it before any Time
/// Machine operation is accepted. A repo whose oplog fails is reported and
/// left without Time Machine; the daemon still starts. The snapshot store
/// arrives with TS-TMC-001: until then nothing about refs is decided.
fn recover_time_machine(
    dirs: &ProfileDirs,
    profile: &Profile,
    logger: &Logger,
    report: &mut StartupReport,
) -> Result<Vec<(String, PathBuf, Oplog)>, DaemonError> {
    let deadline = Instant::now() + TM_RECOVERY_WAIT;
    let mut oplogs = Vec::new();
    for entry in profile.repos()? {
        if entry.state != RepoState::Observed {
            continue;
        }
        match recover_repo(dirs, &entry, deadline) {
            Ok((oplog, startup)) => {
                if !startup.new_breaks.is_empty() {
                    logger.warn(
                        "tm_chain_break",
                        &[
                            ("repo", Field::id(&entry.repo_id)),
                            ("breaks", startup.new_breaks.len().into()),
                        ],
                    );
                }
                let r = &startup.recovery;
                if !r.is_clean() {
                    logger.info(
                        "tm_recovered",
                        &[
                            ("repo", Field::id(&entry.repo_id)),
                            ("discarded", r.discarded_snapshots.len().into()),
                            ("aborted", r.aborted_operations.len().into()),
                            ("interrupted", r.interrupted_operations.len().into()),
                            ("locks_released", r.released_locks.len().into()),
                            ("locks_kept", r.kept_locks.len().into()),
                        ],
                    );
                }
                report.time_machine.push(startup);
                oplogs.push((entry.repo_id, entry.canonical_path, oplog));
            }
            Err(err) => {
                logger.warn(
                    "tm_unavailable",
                    &[
                        ("repo", Field::id(&entry.repo_id)),
                        ("kind", profile_error_kind(&err).into()),
                    ],
                );
                report.tm_unavailable.push(entry.repo_id);
            }
        }
    }
    Ok(oplogs)
}

fn recover_repo(
    dirs: &ProfileDirs,
    entry: &RepoEntry,
    deadline: Instant,
) -> Result<(Oplog, TmStartup), ProfileError> {
    let (mut oplog, opened) = Oplog::open(dirs, &entry.repo_id, now_ms())?;
    // The real store when there is one it can trust; otherwise nothing about refs is decided.
    let mut store = SnapshotStore::open_existing(dirs, &entry.repo_id)
        .ok()
        .flatten();
    let mut absent = AbsentStore;
    let refs: &mut dyn SnapshotRefs = match store.as_mut() {
        Some(store) => store,
        None => &mut absent,
    };
    let recovery = oplog.recover(
        refs,
        &RecoveryOptions {
            git_dir: &entry.canonical_path,
            deadline,
            poll: Duration::from_millis(50),
            probe: &SystemProbe,
        },
        now_ms(),
    )?;
    let startup = TmStartup {
        repo_id: entry.repo_id.clone(),
        oplog: opened.status,
        new_breaks: opened.new_breaks,
        recovery,
    };
    Ok((oplog, startup))
}

/// A stored event as the contract shows it; `None` for kinds of other
/// stories. Without a session the actor is "unattributed" (BR-CONS-003);
/// with one, its initial attribution until US-GRP-010 resolves the records.
fn git_event_view(repo_id: &str, store: &RepoStore, e: StoredEvent) -> Option<GitEventView> {
    let kind = GitEventKind::parse(&e.kind)?;
    let actor = e
        .session_id
        .as_deref()
        .and_then(|id| store.session(id).ok().flatten())
        .map_or(gitraptor_api::Actor::Unattributed, |s| {
            sessions::session_actor(store, &s)
        });
    Some(GitEventView {
        repo_id: repo_id.to_owned(),
        seq: e.seq,
        worktree: Untrusted::from_os(e.worktree.as_os_str()),
        kind,
        actor,
        observed_utc_ms: e.observed.utc_ms,
        utc_offset_s: e.observed.offset_s,
        details: serde_json::from_str::<GitEventDetails>(&e.metadata).unwrap_or_default(),
        gap_id: e.gap_id,
    })
}

/// Reconciles every observed repo that has a store, one thread per repo.
/// A repo that cannot be read gets `None` and keeps an empty worktree list.
fn reconcile_all(
    entries: &[RepoEntry],
    stores: &[(String, RepoStore)],
) -> Vec<(String, Option<RepoRead>)> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = entries
            .iter()
            .filter(|e| {
                e.state == RepoState::Observed && stores.iter().any(|(id, _)| *id == e.repo_id)
            })
            .map(|e| {
                let path = e.canonical_path.clone();
                let base = repo_base(stores, &e.repo_id);
                (
                    e.repo_id.clone(),
                    scope.spawn(move || observe::reconcile(&path, &base).ok()),
                )
            })
            .collect();
        handles
            .into_iter()
            .map(|(id, handle)| (id, handle.join().ok().flatten()))
            .collect()
    })
}

/// The base branch of a repo (US-GRP-012): the one its store keeps as
/// confirmed; without a store or a readable confirmation, the unconfirmed
/// default (reading it never confirms anything).
/// The installed `raptor` for the dispatchers' constants.
fn raptor_path(config: &DaemonConfig) -> Option<PathBuf> {
    let path = config
        .channel
        .launch_exe
        .clone()
        .or_else(|| std::env::current_exe().ok())?;
    std::path::absolute(path).ok()
}

fn repo_base(stores: &[(String, RepoStore)], repo_id: &str) -> BaseBranch {
    let confirmed = stores
        .iter()
        .find(|(id, _)| id == repo_id)
        .and_then(|(_, store)| store.confirmed_team_baseline().ok().flatten());
    observe::base_branch(confirmed.as_ref())
}

/// Persists a reconciliation in the repo's store. A failure is logged: the
/// view is still published, and the next reconciliation writes it again.
fn persist_read(store: &mut RepoStore, read: &RepoRead, logger: &Logger, repo_id: &str) {
    let known: Vec<PathBuf> = store
        .worktrees()
        .map(|all| {
            all.into_iter()
                .filter(|w| w.gone_ms.is_none())
                .map(|w| w.path)
                .collect()
        })
        .unwrap_or_default();
    if let Err(err) = store.write_batch(&read.store_ops(&known, now_ms())) {
        logger.error(
            "worktree_state_persist_failed",
            &[
                ("repo", Field::id(repo_id)),
                ("kind", profile_error_kind(&err).into()),
            ],
        );
    }
}

fn outcome_field(outcome: RepoAddOutcome) -> &'static str {
    match outcome {
        RepoAddOutcome::New => "new",
        RepoAddOutcome::AlreadyObserved => "already-observed",
        RepoAddOutcome::Reactivated => "reactivated",
    }
}

/// Entry point of `raptor daemon`: starts the daemon, installs the
/// process-wide hooks (redacting panic hook, termination signals), moves the
/// working folder into the profile and runs until stopped.
pub fn run_process(config: DaemonConfig) -> Result<StopReport, DaemonError> {
    // Leave the launcher's session and process group: a client (possibly an
    // MCP server under an agent) that started it on demand must not take it
    // down when it exits. Fails harmlessly when run as a group leader in a
    // terminal.
    #[cfg(unix)]
    let _ = rustix::process::setsid();
    let state_dir = config.dirs.state.clone();
    let daemon = Daemon::start(config)?;
    daemon.logger().install_panic_hook();
    // A fixed working folder: never inside an observed repo, and children
    // never inherit the launcher's folder.
    std::env::set_current_dir(&state_dir)?;
    install_signal_handlers(daemon.shutdown_handle())?;
    Ok(daemon.run())
}

/// A `replace:<protocol>` stop counts as attributed only if this binary
/// speaks a newer protocol than the one that stepped down. Otherwise the
/// "upgrade" was not one (a client that lied about its version, or a binary
/// swapped by another process) and the interval is a plain crash (SEC-13).
fn replaced_by_newer(cause: &str) -> bool {
    cause
        .strip_prefix("replace:")
        .and_then(|p| p.parse::<u32>().ok())
        .is_some_and(|old| old < gitraptor_api::PROTOCOL_VERSION)
}

/// Engine view of the contract for a state and the Git found.
fn engine_view(state: EngineState, git: Option<&SystemGit>) -> EngineView {
    EngineView {
        state: match state {
            EngineState::WaitingForGit => EngineStateView::WaitingForGit,
            EngineState::NoRepos => EngineStateView::NoRepos,
            EngineState::Observing => EngineStateView::Observing,
        },
        git_version: git.map(|g| g.version.to_string()),
    }
}

/// Random id of this daemon run: sequences start again at every run.
fn run_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u128(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    hasher.write_u32(std::process::id());
    format!("{:016x}", hasher.finish())
}

/// Milliseconds since the Unix epoch, UTC.
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

fn previous_kind(previous: &DaemonRun) -> &'static str {
    match previous {
        DaemonRun::Never => "never",
        DaemonRun::Running { .. } => "crashed",
        DaemonRun::Stopped { .. } => "stopped",
    }
}

/// Error kind for the log: never the message, which may carry a path.
fn profile_error_kind(err: &ProfileError) -> &'static str {
    match err {
        ProfileError::NoHomeDir => "no-home-dir",
        ProfileError::InsecureDir { .. } => "insecure-dir",
        ProfileError::InvalidPath { .. } => "invalid-path",
        ProfileError::SchemaTooNew { .. } => "schema-too-new",
        ProfileError::UnknownRepo(_) => "unknown-repo",
        ProfileError::InvalidWrite(_) => "invalid-write",
        ProfileError::Io(_) => "io",
        ProfileError::Sqlite(_) => "sqlite",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crash_during_active_session_is_marked() {
        let crashed = DaemonRun::Running { started_ms: 1 };
        assert_eq!(
            classify_previous_run(&crashed, true),
            Some((GapCause::DaemonDownDuringSession, None))
        );
        assert_eq!(
            classify_previous_run(&crashed, false),
            Some((GapCause::DaemonDown, None))
        );
        assert_eq!(classify_previous_run(&DaemonRun::Never, true), None);
    }

    #[test]
    fn a_signal_never_counts_as_an_attributed_stop() {
        let by_signal = DaemonRun::Stopped {
            stopped_ms: 5,
            cause: "signal".into(),
            requested_by: Some("signal:TERM".into()),
        };
        assert_eq!(
            classify_previous_run(&by_signal, true),
            Some((
                GapCause::DaemonDownDuringSession,
                Some("signal:TERM".into())
            ))
        );
        let by_command = DaemonRun::Stopped {
            stopped_ms: 5,
            cause: "stop-command".into(),
            requested_by: Some("client-7".into()),
        };
        assert_eq!(
            classify_previous_run(&by_command, true),
            Some((GapCause::DaemonStopped, Some("client-7".into())))
        );
    }

    #[test]
    fn a_replacement_only_counts_if_the_new_binary_is_newer() {
        let replaced = |protocol: u32| DaemonRun::Stopped {
            stopped_ms: 5,
            cause: format!("replace:{protocol}"),
            requested_by: Some("client-9".into()),
        };
        let older = gitraptor_api::PROTOCOL_VERSION - 1;
        assert_eq!(
            classify_previous_run(&replaced(older), true),
            Some((GapCause::DaemonStopped, Some("client-9".into())))
        );
        assert_eq!(
            classify_previous_run(&replaced(gitraptor_api::PROTOCOL_VERSION), true),
            Some((GapCause::DaemonDownDuringSession, Some("client-9".into())))
        );
    }
}
