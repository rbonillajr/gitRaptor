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
//! The client channel (TS-GRP-004) plugs into [`ShutdownHandle`] and
//! [`Daemon::state`]; the gap itself is recorded by US-GRP-005 from
//! [`StartupReport::repos`].
//!
//! Before accepting Time Machine operations, the start also opens the
//! oplog of every observed repo and runs its recovery (TS-TMC-002,
//! ADR-TMC-003 § 6). It does not depend on the engine store: a repo whose
//! engine store cannot be opened still recovers its oplog (Q26).

mod env;
mod lock;
mod log;
mod shutdown;
mod state;

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gitraptor_git::SystemGit;
use gitraptor_git::resolve::{Resolution, ResolveConfig, resolve};

use crate::profile::{
    DaemonRun, GapCause, Profile, ProfileDirs, ProfileError, RepoEntry, RepoState, RepoStore,
    StoreOpen, WriteOp, fsperm,
};
use crate::timemachine::oplog::{
    AbsentStore, ChainBreak, Oplog, OplogStatus, RecoveryOptions, RecoveryReport, SnapshotRefs,
    SystemProbe,
};
use crate::timemachine::store::SnapshotStore;

pub use env::DaemonEnv;
pub use lock::{InstanceLock, LOCK_FILE, running_pid, wait_until_released};
pub use log::{Field, LOG_FILE, Level, LogLimits, Logger};
pub use shutdown::{ShutdownHandle, StopCause, install_signal_handlers, signal_running_daemon};
pub use state::{EngineState, InvalidTransition, Trigger};

/// Exit code of a second `raptor daemon` that found another one running.
/// Service managers must not treat it as a failure to relaunch (US-GRP-004).
pub const EXIT_ALREADY_RUNNING: i32 = 3;

/// Longest the start waits, over all repos, for annotated children of an
/// interrupted operation before keeping their locks (ADR-TMC-003 § 6.4).
pub const TM_RECOVERY_WAIT: Duration = Duration::from_secs(5);

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
}

impl DaemonConfig {
    /// Configuration of the real daemon for the current user.
    pub fn for_current_user() -> Result<Self, DaemonError> {
        let env = DaemonEnv::capture();
        Ok(Self {
            dirs: ProfileDirs::resolve()?,
            git: env.git_resolve_config(None),
            env,
            heartbeat: Duration::from_secs(60),
            log: LogLimits::default(),
            stop_deadline: Some(Duration::from_secs(5)),
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
        } if cause == "stop-command" => Some((GapCause::DaemonStopped, requested_by.clone())),
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
    oplogs: Vec<(String, Oplog)>,
    report: StartupReport,
    handle: ShutdownHandle,
    stop_rx: Receiver<StopCause>,
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

        let oplogs = recover_time_machine(&config.dirs, &profile, &logger, &mut report)?;

        let state = EngineState::initial(git.is_some(), stores.len());
        if state != EngineState::Observing {
            // Nothing is observed outside "Observing" (BR-WF-002).
            stores.clear();
        }
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
        logger.info("daemon_started", &fields);

        let (handle, stop_rx) = ShutdownHandle::new();
        Ok(Self {
            config,
            lock,
            logger,
            profile,
            state,
            stores,
            oplogs,
            report,
            handle,
            stop_rx,
        })
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
    pub fn oplog(&self, repo_id: &str) -> Option<&Oplog> {
        self.oplogs
            .iter()
            .find(|(id, _)| id == repo_id)
            .map(|(_, oplog)| oplog)
    }

    /// Handle for signals and, later, the channel's stop command.
    pub fn shutdown_handle(&self) -> ShutdownHandle {
        self.handle.clone()
    }

    /// Runs until a stop request arrives, then stops in order.
    pub fn run(mut self) -> StopReport {
        loop {
            match self.stop_rx.recv_timeout(self.config.heartbeat) {
                Ok(cause) => return self.stop(cause),
                Err(RecvTimeoutError::Timeout) => {
                    self.persist_observed_until(now_ms());
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
        let stopped_ms = now_ms();
        let marks_ok = self.persist_observed_until(stopped_ms);
        let recorded = marks_ok
            && match self.profile.mark_daemon_stopped(
                stopped_ms,
                cause.as_str(),
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
            oplogs,
            profile,
            ..
        } = self;
        drop(oplogs);
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
) -> Result<Vec<(String, Oplog)>, DaemonError> {
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
                oplogs.push((entry.repo_id, oplog));
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

/// Entry point of `raptor daemon`: starts the daemon, installs the
/// process-wide hooks (redacting panic hook, termination signals), moves the
/// working folder into the profile and runs until stopped.
pub fn run_process(config: DaemonConfig) -> Result<StopReport, DaemonError> {
    let state_dir = config.dirs.state.clone();
    let daemon = Daemon::start(config)?;
    daemon.logger().install_panic_hook();
    // A fixed working folder: never inside an observed repo, and children
    // never inherit the launcher's folder.
    std::env::set_current_dir(&state_dir)?;
    install_signal_handlers(daemon.shutdown_handle())?;
    Ok(daemon.run())
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
}
