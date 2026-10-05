//! The protected operation: the only way GitRaptor modifies a repo
//! (ADR-TMC-004 § 1, BR-TMC-CONS-001, TS-TMC-004).
//!
//! (1) intent in the oplog with the requester frozen, (2) guaranteed prior
//! snapshot of the scope within a deadline, (3) `prior-snapshot` and
//! `ready`, (4) the step, (5) `finished` or `interrupted`. Without a valid
//! prior snapshot the step never runs and the repo does not change.
//!
//! The step is a [`ProtectedStep`]: the user-operation executor of
//! F-001-02/05 and the applier of TS-TMC-003 implement it. Neither is built
//! here. A step cannot write the oplog: it only annotates its own progress
//! and the children it starts through [`StepCtx`].

pub mod backend;
pub mod challenge;
pub mod scope;

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gitraptor_api::timemachine::PriorFailure;
use gitraptor_git::{ReaderOptions, RepoReader};

use crate::channel::marks::ExecutorMarks;
use crate::channel::peer::ProcSource;
use crate::channel::requester::Who;
use crate::profile::ProfileDirs;
use crate::timemachine::oplog::{
    Channel, NewOperation, OperationKind, OperationTransition, Oplog, Scope, Target,
};
use crate::timemachine::store::{CaptureError, CaptureRequest, SnapshotStore, WorktreeScope};

pub use backend::{
    DaemonBackend, OperationCatalog, OperationsWiring, SnapshotterLayer, TimeMachineBackend,
    TmRepos,
};
pub use challenge::{Binding, ChallengeBook, ChallengeError, plan_hash};
pub use scope::{McpAllowlist, NoMcpRepos, ProtectedBackend, RepoHandle, ScopeError};

/// Default deadline of the prior snapshot.
pub const DEFAULT_PRIOR_DEADLINE: Duration = Duration::from_secs(10);

/// What the prior snapshot must cover.
#[derive(Debug, Clone)]
pub struct PriorRequest {
    pub operation_id: String,
    /// Any worktree of the repo.
    pub repo: PathBuf,
    /// Every worktree of the operation's scope.
    pub worktrees: Vec<PathBuf>,
    pub engine_mark: Option<i64>,
}

/// A completed prior snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PriorSnapshot {
    pub snapshot_id: String,
    pub fast_path: bool,
}

/// Why the prior snapshot did not complete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PriorError {
    NoSpace,
    StoreUnavailable,
    Failed(String),
}

/// Takes the guaranteed prior snapshot.
pub trait PriorSnapshotter: Send + Sync {
    fn prior(&self, req: &PriorRequest) -> Result<PriorSnapshot, PriorError>;
}

/// `ENOSPC` and `EDQUOT`.
fn is_no_space(e: &std::io::Error) -> bool {
    #[cfg(target_os = "macos")]
    const CODES: &[i32] = &[28, 69];
    #[cfg(target_os = "linux")]
    const CODES: &[i32] = &[28, 122];
    #[cfg(windows)]
    const CODES: &[i32] = &[39, 112]; // ERROR_HANDLE_DISK_FULL, ERROR_DISK_FULL
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    const CODES: &[i32] = &[28];
    e.raw_os_error().is_some_and(|c| CODES.contains(&c))
}

impl From<CaptureError> for PriorError {
    fn from(e: CaptureError) -> Self {
        use gitraptor_git::tm_write::store::StoreError;
        match e {
            CaptureError::Io(io) | CaptureError::Store(StoreError::Io(io)) if is_no_space(&io) => {
                Self::NoSpace
            }
            CaptureError::Store(StoreError::Untrusted(_) | StoreError::Corrupt(_)) => {
                Self::StoreUnavailable
            }
            other => Self::Failed(other.to_string()),
        }
    }
}

/// The production snapshotter: the repo's store, at level
/// `guaranteed-prior` (ADR-TMC-004 § 1), with the profile's credentials
/// option read on every prior (BR-TMC-CONS-002).
pub struct StoreSnapshotter {
    pub store: Arc<SnapshotStore>,
    pub oplog: Arc<Mutex<Oplog>>,
    pub profile: ProfileDirs,
}

impl PriorSnapshotter for StoreSnapshotter {
    fn prior(&self, req: &PriorRequest) -> Result<PriorSnapshot, PriorError> {
        let registered = registered_worktrees(&req.repo).unwrap_or_default();
        let worktrees = req
            .worktrees
            .iter()
            .enumerate()
            .map(|(i, path)| WorktreeScope {
                key: worktree_key(&registered, path, i),
                path: path.clone(),
                hint: None,
            })
            .collect();
        let capture = CaptureRequest {
            level: crate::timemachine::oplog::SnapshotLevel::GuaranteedPrior,
            repo: req.repo.clone(),
            worktrees,
            engine_mark: req.engine_mark,
            cause_operation: Some(req.operation_id.clone()),
            cause_event_seq: None,
            include_credentials: crate::profile::settings::include_credential_files(&self.profile),
        };
        let out = self.store.capture(&self.oplog, &capture)?;
        Ok(PriorSnapshot {
            snapshot_id: out.snapshot_id,
            fast_path: out.fast_path,
        })
    }
}

/// Every worktree of the repo of `any_worktree`: canonical root and, for a
/// linked one, its name under `.git/worktrees/`.
pub(crate) fn registered_worktrees(
    any_worktree: &Path,
) -> Result<Vec<(PathBuf, Option<String>)>, gitraptor_git::ReadError> {
    let reader = RepoReader::open(any_worktree, &ReaderOptions::default())?;
    // Canonical: from a linked worktree gix reports `.git/worktrees/<id>/../..`.
    let common = canonical(reader.common_dir());
    let mut out = Vec::new();
    if let Some(main) = common.parent().filter(|_| common.ends_with(".git")) {
        out.push((canonical(main), None));
    }
    for wt in reader.worktrees()? {
        out.push((canonical(&wt.path), Some(wt.id)));
    }
    Ok(out)
}

pub(crate) fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// A key under `wt/` that stays the same for the same worktree from one
/// snapshot to the next, so its incremental state and the fast path are kept
/// (ADR-TMC-006 § 5): `main`, or `wt-<name>` for a linked worktree. Falls
/// back to the position when the name is not a valid key.
fn worktree_key(registered: &[(PathBuf, Option<String>)], path: &Path, i: usize) -> String {
    let path = canonical(path);
    let key = match registered.iter().find(|(p, _)| *p == path) {
        Some((_, None)) => Some("main".to_owned()),
        Some((_, Some(id))) => Some(format!("wt-{id}")),
        None => None,
    };
    key.filter(|k| {
        k.len() <= 64
            && k.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    })
    .unwrap_or_else(|| format!("wt{i}"))
}

/// What a step reports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StepOutput {
    /// Refs the step moved (names from the repo: untrusted).
    pub changed_refs: Vec<String>,
    /// How the operation ended when it is not plainly done (e.g. `stopped`
    /// with an operation in progress); `None` is `done`.
    pub outcome: Option<gitraptor_api::catalog::OperationOutcome>,
    /// Git's output, sanitized by the client; only layer `cockpit` gets it.
    pub git_output: Option<String>,
}

/// Why a step failed or refused to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepError {
    pub message: String,
}

impl StepError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// What an operation touches besides the worktree it was asked for
/// (ADR-TMC-002 § 5, ADR-TMC-007 § 2): a merge with its base checked out in
/// another worktree, for example. The prior snapshot covers every worktree
/// here; the refs are recorded in the oplog (the snapshot always holds every
/// branch).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StepScope {
    /// Roots of other worktrees of the same repo.
    pub worktrees: Vec<PathBuf>,
    /// Full ref names the operation moves or deletes.
    pub refs: Vec<String>,
}

/// The execution of a protected operation: a catalog operation of the
/// user-operation executor or the Time Machine applier.
pub trait ProtectedStep: Send {
    /// Subtype recorded in the oplog (e.g. `checkout`).
    fn subtype(&self) -> &str;
    /// The scope the operation declares beyond the requested worktree. No
    /// default: every operation must say what it touches (NFR-01).
    fn scope(&self) -> StepScope;
    fn run(&mut self, ctx: &mut StepCtx<'_>) -> Result<StepOutput, StepError>;
    /// The Time Machine applier annotates its own steps and its close from
    /// `ready` (ADR-TMC-002 § 3, TS-TMC-003): the protected operation then
    /// neither annotates `applying` before it nor closes after it, unless
    /// the step left the operation open. Every other step keeps the default.
    fn self_annotated(&self) -> bool {
        false
    }
}

/// A child process the step started, marked for the channel (DEP-MCP-3).
#[derive(Debug)]
pub struct MarkedChild {
    pub child: Child,
    pub pid: u32,
}

/// What a step may do besides its own work.
pub struct StepCtx<'a> {
    operation_id: &'a str,
    prior_snapshot_id: &'a str,
    who: &'a Who,
    channel: Channel,
    stopping: &'a AtomicBool,
    oplog: &'a Mutex<Oplog>,
    marks: &'a ExecutorMarks,
    procs: &'a dyn ProcSource,
    step: u32,
}

impl StepCtx<'_> {
    pub fn operation_id(&self) -> &str {
        self.operation_id
    }

    pub fn prior_snapshot_id(&self) -> &str {
        self.prior_snapshot_id
    }

    pub fn requester(&self) -> &Who {
        self.who
    }

    pub fn channel(&self) -> Channel {
        self.channel
    }

    /// The oplog, for the Time Machine applier only (it annotates its own
    /// steps, locks and close).
    pub(crate) fn oplog(&self) -> &Mutex<Oplog> {
        self.oplog
    }

    /// The daemon is stopping: finish the current step and return.
    pub fn should_stop(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    /// Annotates the next step before running it (ADR-TMC-002 § 3).
    pub fn applying(&mut self) -> Result<u32, StepError> {
        self.step += 1;
        lock(self.oplog)
            .advance_operation(
                self.operation_id,
                OperationTransition::Applying { step: self.step },
                now_ms(),
            )
            .map_err(|e| StepError::new(format!("oplog: {e}")))?;
        Ok(self.step)
    }

    /// Starts a child in its own process group, marked as started by this
    /// operation and annotated in the journal. If its identity cannot be
    /// read it is killed: an unmarked child must not run (fail-closed).
    pub fn spawn(&mut self, cmd: &mut Command) -> std::io::Result<MarkedChild> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        self.spawn_with(|| cmd.spawn())
    }

    /// Like [`Self::spawn`] for a launch the caller builds (the opaque user
    /// operations of `crates/git`, which set their own process group). The
    /// start barrier is open while `launch` runs and until the child is
    /// marked (I-02).
    pub fn spawn_with(
        &mut self,
        launch: impl FnOnce() -> std::io::Result<Child>,
    ) -> std::io::Result<MarkedChild> {
        let pending = self.marks.begin_spawn();
        let mut child = launch()?;
        let pid = child.id();
        match self.procs.read(pid) {
            Ok(info) => self
                .marks
                .add_child(self.operation_id, pid, info.start_us, info.pgid),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(std::io::Error::other("child identity unreadable"));
            }
        }
        drop(pending);
        let _ = lock(self.oplog).record_child_started(self.operation_id, pid, now_ms());
        Ok(MarkedChild { child, pid })
    }

    /// Annotates the end of a marked child the caller waited for itself.
    pub fn child_ended(&mut self, pid: u32) {
        let _ = lock(self.oplog).record_child_ended(self.operation_id, pid, now_ms());
    }

    /// Waits for a marked child and annotates its end.
    pub fn wait(&mut self, mut marked: MarkedChild) -> std::io::Result<std::process::ExitStatus> {
        let status = marked.child.wait()?;
        let _ = lock(self.oplog).record_child_ended(self.operation_id, marked.pid, now_ms());
        Ok(status)
    }
}

fn lock(oplog: &Mutex<Oplog>) -> std::sync::MutexGuard<'_, Oplog> {
    oplog.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> i64 {
    crate::daemon::now_ms()
}

fn now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
}

/// A protected operation to run.
#[derive(Debug, Clone)]
pub struct ProtectedRequest {
    pub kind: OperationKind,
    pub scope: Scope,
    /// Root of each worktree in the scope, for the prior snapshot.
    pub worktree_paths: Vec<PathBuf>,
    pub who: Who,
    pub channel: Channel,
    pub confirmed: bool,
    pub target: Target,
    pub warnings: Vec<String>,
    pub engine_mark: i64,
}

impl ProtectedRequest {
    /// A protected operation of kind `Protected` for `step` on `repo`: the
    /// requested worktree plus the scope the step declares (ADR-TMC-001
    /// § 1). Every declared worktree must be a registered worktree of the
    /// same repo; otherwise nothing is recorded and nothing runs.
    pub fn for_step(
        repo: &RepoHandle,
        step: &dyn ProtectedStep,
        who: Who,
        channel: Channel,
        engine_mark: i64,
    ) -> Result<Self, ScopeError> {
        let declared = step.scope();
        let mut worktree_paths = vec![repo.worktree.clone()];
        if !declared.worktrees.is_empty() {
            let registered =
                registered_worktrees(&repo.worktree).map_err(|_| ScopeError::ForeignWorktree)?;
            for wt in declared.worktrees {
                let wt = canonical(&wt);
                if !registered.iter().any(|(p, _)| *p == wt) {
                    return Err(ScopeError::ForeignWorktree);
                }
                if !worktree_paths.contains(&wt) {
                    worktree_paths.push(wt);
                }
            }
        }
        Ok(Self {
            kind: OperationKind::Protected,
            scope: Scope {
                worktrees: worktree_paths
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect(),
                refs: declared.refs,
            },
            worktree_paths,
            who,
            channel,
            confirmed: false,
            target: Target::None,
            warnings: Vec::new(),
            engine_mark,
        })
    }
}

/// A finished protected operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedOutcome {
    pub operation_id: String,
    pub prior: PriorSnapshot,
    pub output: StepOutput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtectedError {
    /// The prior snapshot failed: the step did not run, the repo did not
    /// change. `operation_id` is the aborted record, when one was written.
    Prior {
        reason: PriorFailure,
        operation_id: Option<String>,
        detail: Option<String>,
    },
    /// The oplog could not record the intent: nothing ran.
    Oplog(String),
    /// The step ran and failed: the operation is `interrupted` and its
    /// prior snapshot can undo it.
    Step {
        operation_id: String,
        message: String,
    },
}

/// Runs protected operations on one repo.
pub struct ProtectedOperation<'a> {
    pub oplog: &'a Arc<Mutex<Oplog>>,
    pub snapshotter: Arc<dyn PriorSnapshotter>,
    pub marks: &'a Arc<ExecutorMarks>,
    pub procs: &'a dyn ProcSource,
    /// Set when the daemon starts stopping.
    pub stopping: &'a AtomicBool,
    pub deadline: Duration,
}

impl ProtectedOperation<'_> {
    pub fn run(
        &self,
        req: &ProtectedRequest,
        step: &mut dyn ProtectedStep,
    ) -> Result<ProtectedOutcome, ProtectedError> {
        let subtype = (req.kind == OperationKind::Protected).then(|| step.subtype().to_owned());
        let new = NewOperation {
            kind: req.kind,
            subtype,
            scope: req.scope.clone(),
            requester: req.who.requester.clone(),
            channel: req.channel,
            confirmed: req.confirmed,
            target: req.target.clone(),
            warnings: req.warnings.clone(),
            engine_mark: req.engine_mark,
        };
        // (1) Intent: every request is recorded, even one the daemon is
        // stopping for.
        let operation_id = lock(self.oplog)
            .record_operation(&new, now_ms())
            .map_err(|e| ProtectedError::Oplog(e.to_string()))?;
        let abort = |reason: PriorFailure, detail: Option<String>| {
            let text = match &detail {
                Some(d) => format!("{}: {d}", failure_text(reason)),
                None => failure_text(reason).to_owned(),
            };
            let _ = lock(self.oplog).advance_operation(
                &operation_id,
                OperationTransition::Aborted { reason: &text },
                now_ms(),
            );
            ProtectedError::Prior {
                reason,
                operation_id: Some(operation_id.clone()),
                detail,
            }
        };
        if self.stopping.load(Ordering::SeqCst) {
            return Err(abort(PriorFailure::DaemonStopping, None));
        }

        // (2) Prior snapshot within the deadline. A late capture stays as a
        // point of the timeline, never as this operation's prior.
        let prior_req = PriorRequest {
            operation_id: operation_id.clone(),
            repo: req.worktree_paths.first().cloned().unwrap_or_default(),
            worktrees: req.worktree_paths.clone(),
            engine_mark: Some(req.engine_mark),
        };
        let (tx, rx) = mpsc::sync_channel(1);
        let snapshotter = Arc::clone(&self.snapshotter);
        let spawned = std::thread::Builder::new()
            .name("raptor-prior".into())
            .spawn(move || {
                let _ = tx.send(snapshotter.prior(&prior_req));
            });
        if spawned.is_err() {
            return Err(abort(PriorFailure::CaptureFailed, Some("no thread".into())));
        }
        let prior = match rx.recv_timeout(self.deadline) {
            Ok(Ok(prior)) => prior,
            Ok(Err(PriorError::NoSpace)) => return Err(abort(PriorFailure::NoSpace, None)),
            Ok(Err(PriorError::StoreUnavailable)) => {
                return Err(abort(PriorFailure::StoreUnavailable, None));
            }
            Ok(Err(PriorError::Failed(d))) => {
                return Err(abort(PriorFailure::CaptureFailed, Some(d)));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err(abort(PriorFailure::Timeout, None));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(abort(
                    PriorFailure::CaptureFailed,
                    Some("snapshot worker died".into()),
                ));
            }
        };

        // (3) The prior snapshot is recorded; then ready, unless stopping.
        let advanced = lock(self.oplog).advance_operation(
            &operation_id,
            OperationTransition::PriorSnapshot {
                snapshot_id: &prior.snapshot_id,
            },
            now_ms(),
        );
        if let Err(e) = advanced {
            return Err(abort(PriorFailure::CaptureFailed, Some(e.to_string())));
        }
        if self.stopping.load(Ordering::SeqCst) {
            return Err(abort(PriorFailure::DaemonStopping, None));
        }
        lock(self.oplog)
            .advance_operation(&operation_id, OperationTransition::Ready, now_ms())
            .map_err(|e| abort(PriorFailure::CaptureFailed, Some(e.to_string())))?;

        // (4) The step, with its children marked until the operation closes.
        let _marks = self.marks.open(&operation_id, &req.who, now_us());
        let mut ctx = StepCtx {
            operation_id: &operation_id,
            prior_snapshot_id: &prior.snapshot_id,
            who: &req.who,
            channel: req.channel,
            stopping: self.stopping,
            oplog: self.oplog,
            marks: self.marks,
            procs: self.procs,
            step: 0,
        };
        let result = if step.self_annotated() {
            step.run(&mut ctx)
        } else {
            ctx.applying().and_then(|_| step.run(&mut ctx))
        };

        // (5) Record the end, unless the step already closed it.
        let end = if result.is_ok() {
            OperationTransition::Finished
        } else {
            OperationTransition::Interrupted
        };
        let closed = {
            let mut log = lock(self.oplog);
            let open = !step.self_annotated()
                || log
                    .operation(&operation_id)
                    .ok()
                    .flatten()
                    .is_none_or(|op| !op.state.is_terminal());
            if open {
                log.advance_operation(&operation_id, end, now_ms())
            } else {
                Ok(())
            }
        };
        match (result, closed) {
            (Ok(output), Ok(())) => Ok(ProtectedOutcome {
                operation_id,
                prior,
                output,
            }),
            (Ok(_), Err(e)) => Err(ProtectedError::Step {
                operation_id,
                message: format!("oplog: {e}"),
            }),
            (Err(e), _) => Err(ProtectedError::Step {
                operation_id,
                message: e.message,
            }),
        }
    }
}

pub fn failure_text(reason: PriorFailure) -> &'static str {
    match reason {
        PriorFailure::NoSpace => "no space for the prior snapshot",
        PriorFailure::StoreUnavailable => "snapshot store unavailable",
        PriorFailure::Timeout => "prior snapshot timed out",
        PriorFailure::DaemonStopping => "the daemon is stopping",
        PriorFailure::CaptureFailed => "prior snapshot failed",
    }
}

#[cfg(test)]
mod tests;
