//! The worker process of the predictor (T006, ADR-CKP-001 § 1 and § 5, Enmienda 2026-10-20;
//! ADR-GRP-009, Enmienda 2026-10-20): the daemon's own binary with the hidden subcommand
//! `predict-worker`, the only process `predict` launches. [`WorkerMerger`] is its
//! [`DryMerger`] in the daemon: fixed argv, no shell, an environment built from scratch,
//! length-prefixed frames with a cap, a wall-clock deadline per job and the child's resident
//! memory read every [`WORKER_RSS_POLL`]; past either, the child is killed and relaunched with
//! the next job.

pub mod serve;

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use gitraptor_api::resources::WorkClass;
use gitraptor_git::dry_merge::{
    ConflictFile, ConflictKind, DryMergeError, DryMergeInput, DryMergeLimits, DryMergeOutcome,
    LineRange, MergeBaseUsed,
};
use serde::{Deserialize, Serialize};

use self::serve::{read_frame, validate_outcome, write_frame};
use super::merger::DryMerger;
use crate::work_class;

/// The hidden subcommand of the daemon's binary that runs [`serve::serve`].
pub const WORKER_SUBCOMMAND: &str = "predict-worker";
/// Wall-clock time of one job, imposed by the daemon.
pub const PAIR_DEADLINE: Duration = Duration::from_secs(2);
/// Resident memory of the worker past which it is killed (a kill threshold, not a budget).
pub const JOB_MEMORY_CAP: u64 = 512 * 1024 * 1024;
/// How often the daemon reads the worker's resident memory.
pub const WORKER_RSS_POLL: Duration = Duration::from_millis(20);
/// Idle time after which the worker is closed (⚠️ ASSUMPTION of the spec).
pub const WORKER_IDLE_EXIT: Duration = Duration::from_secs(60);
/// Largest frame either side accepts.
pub const MAX_WORKER_FRAME: usize = 4 * 1024 * 1024;
/// The version the worker announces in its first frame and the daemon expects.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// `rewrites_limit` of the second job of a pair with modify/delete or rename conflicts.
pub const REWRITES_LIMIT_ESCALATED: usize = 100_000;

/// The caps the daemon imposes on the worker. Production uses [`WorkerLimits::default`]; tests
/// tighten them to make a cut deterministic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerLimits {
    pub deadline: Duration,
    /// Deadline of the escalated second job.
    pub escalated_deadline: Duration,
    pub memory_cap: u64,
    pub rss_poll: Duration,
    pub idle_exit: Duration,
}

impl Default for WorkerLimits {
    fn default() -> Self {
        Self {
            deadline: PAIR_DEADLINE,
            escalated_deadline: PAIR_DEADLINE,
            memory_cap: JOB_MEMORY_CAP,
            rss_poll: WORKER_RSS_POLL,
            idle_exit: WORKER_IDLE_EXIT,
        }
    }
}

/// What to launch: an absolute program, resolved once (`current_exe` in the daemon).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerConfig {
    pub program: PathBuf,
    pub limits: WorkerLimits,
}

/// Longest the daemon waits for a new worker's version frame.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);
/// Longest a closed worker (EOF on its stdin) is given to exit before it is killed.
const CLOSE_GRACE: Duration = Duration::from_secs(2);

/// The version the worker announces and the daemon expects: the crate's version and, when the
/// build recorded one, its `git describe`.
fn announced_version() -> String {
    match option_env!("GIT_DESCRIBE") {
        Some(describe) => format!("{VERSION}+{describe}"),
        None => VERSION.to_owned(),
    }
}

/// The daemon's [`DryMerger`]: one long-lived worker, launched with the first job.
pub struct WorkerMerger {
    config: WorkerConfig,
    expected_version: String,
    launches: AtomicU64,
    state: Mutex<Slot>,
}

/// The live worker, if any, and when it last finished a job.
struct Slot {
    worker: Option<Worker>,
    last_used: Instant,
}

/// A launched worker: the child, its stdin, and the frames a reader thread takes from its stdout.
struct Worker {
    child: Child,
    stdin: Option<ChildStdin>,
    frames: Receiver<std::io::Result<Option<Vec<u8>>>>,
    reader: Option<JoinHandle<()>>,
}

impl Worker {
    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// SIGKILL (`TerminateProcess` on Windows) and reap: nothing of it stays alive.
    fn kill(mut self) {
        // An error means it already exited; `wait` reaps it either way.
        let _ = self.child.kill();
        self.reap();
    }

    /// EOF on its stdin; past [`CLOSE_GRACE`] without exiting, killed.
    fn close(mut self) {
        drop(self.stdin.take());
        let end = Instant::now() + CLOSE_GRACE;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.frames.recv_timeout(left) {
                // A late frame of a job already given up: discarded.
                Ok(Ok(Some(_))) => {}
                Ok(Ok(None) | Err(_)) | Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    let _ = self.child.kill();
                    break;
                }
            }
        }
        self.reap();
    }

    fn reap(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            // The child is gone, so its stdout is closed and the reader ends.
            let _ = reader.join();
        }
    }
}

/// Why a job did not produce a reply.
enum Cut {
    /// Deadline or memory cap: the worker was killed.
    Limit,
    /// The worker could not be launched, broke the protocol, or died.
    Unavailable(String),
}

impl From<Cut> for DryMergeError {
    fn from(cut: Cut) -> Self {
        match cut {
            Cut::Limit => Self::LimitExceeded,
            Cut::Unavailable(why) => Self::Unavailable(why),
        }
    }
}

impl WorkerMerger {
    pub fn new(config: WorkerConfig) -> Self {
        Self::with_expected_version(config, &announced_version())
    }

    /// A merger that expects the worker to answer `version` in its first frame instead of
    /// [`VERSION`] (tests of the mismatch: relaunched once, then `Unavailable`).
    pub fn with_expected_version(config: WorkerConfig, version: &str) -> Self {
        Self {
            config,
            expected_version: version.to_owned(),
            launches: AtomicU64::new(0),
            state: Mutex::new(Slot {
                worker: None,
                last_used: Instant::now(),
            }),
        }
    }

    fn slot(&self) -> MutexGuard<'_, Slot> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The pid of the live worker, if any.
    pub fn worker_pid(&self) -> Option<u32> {
        self.slot().worker.as_ref().map(Worker::pid)
    }

    /// Workers launched so far.
    pub fn launches(&self) -> u64 {
        self.launches.load(Ordering::Acquire)
    }

    /// Closes the worker (EOF on its stdin) if it has been idle for `idle_exit`. Returns
    /// whether it closed it.
    pub fn close_if_idle(&self, now: Instant) -> bool {
        let mut slot = self.slot();
        if slot.worker.is_none()
            || now.saturating_duration_since(slot.last_used) < self.config.limits.idle_exit
        {
            return false;
        }
        if let Some(worker) = slot.worker.take() {
            worker.close();
        }
        true
    }

    /// Closes the worker now (power saving, dormant repo, stop).
    pub fn close(&self) {
        let worker = self.slot().worker.take();
        if let Some(worker) = worker {
            worker.close();
        }
    }

    /// Launches a worker and checks its version; on a mismatch or a failed handshake, once more.
    fn launch(&self) -> Result<Worker, Cut> {
        match self.launch_once() {
            Ok(worker) => Ok(worker),
            Err(_) => self.launch_once(),
        }
    }

    fn launch_once(&self) -> Result<Worker, Cut> {
        let program = &self.config.program;
        if !program.is_absolute() {
            return Err(Cut::Unavailable(
                "the worker program is not absolute".into(),
            ));
        }
        let mut command = Command::new(program);
        command
            .arg(WORKER_SUBCOMMAND)
            .env_clear()
            .envs(worker_env())
            .current_dir(worker_cwd())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        self.launches.fetch_add(1, Ordering::AcqRel);
        let mut child = command
            .spawn()
            .map_err(|e| Cut::Unavailable(format!("launch the worker: {e}")))?;
        let (Some(stdin), Some(mut stdout)) = (child.stdin.take(), child.stdout.take()) else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Cut::Unavailable("the worker has no pipes".into()));
        };
        let (tx, frames) = mpsc::channel();
        let reader =
            work_class::spawn_unpooled(WorkClass::Utility, "raptor-predict-reader", move || {
                loop {
                    let frame = read_frame(&mut stdout, MAX_WORKER_FRAME);
                    let last = !matches!(frame, Ok(Some(_)));
                    if tx.send(frame).is_err() || last {
                        break;
                    }
                }
            });
        let mut worker = Worker {
            child,
            stdin: Some(stdin),
            frames,
            reader: None,
        };
        match reader {
            Ok(reader) => worker.reader = Some(reader),
            Err(e) => {
                worker.kill();
                return Err(Cut::Unavailable(format!("start the worker's reader: {e}")));
            }
        }
        let hello = match worker.frames.recv_timeout(HANDSHAKE_TIMEOUT) {
            Ok(Ok(Some(frame))) => serde_json::from_slice::<Hello>(&frame).ok(),
            _ => None,
        };
        match hello {
            Some(hello) if hello.version == self.expected_version => Ok(worker),
            Some(_) => {
                worker.kill();
                Err(Cut::Unavailable("the worker is of another version".into()))
            }
            None => {
                worker.kill();
                Err(Cut::Unavailable(
                    "the worker did not announce its version".into(),
                ))
            }
        }
    }

    /// Runs one job in the worker, launching it if needed, within `deadline` and the memory cap.
    fn job(
        &self,
        slot: &mut Slot,
        job: &WireJob,
        limits: &DryMergeLimits,
        deadline: Instant,
    ) -> Result<Result<DryMergeOutcome, DryMergeError>, Cut> {
        let body = serde_json::to_vec(job)
            .map_err(|e| Cut::Unavailable(format!("encode the job: {e}")))?;
        let mut worker = self.send(slot.worker.take(), &body)?;
        let reply = self.await_reply(&mut worker, deadline);
        match reply {
            Ok(reply) => {
                let result = reply.into_result();
                let checked = match &result {
                    Ok(outcome) => validate_outcome(outcome, limits),
                    Err(_) => Ok(()),
                };
                match checked {
                    Ok(()) => {
                        slot.worker = Some(worker);
                        slot.last_used = Instant::now();
                        Ok(result)
                    }
                    Err(why) => {
                        // The worker is untrusted: one bad reply and it is gone.
                        worker.kill();
                        Err(Cut::Unavailable(format!(
                            "the worker broke the protocol: {why}"
                        )))
                    }
                }
            }
            Err(cut) => {
                worker.kill();
                slot.last_used = Instant::now();
                Err(cut)
            }
        }
    }

    /// Writes `body` to `worker` (or to a new one). A worker that died while idle is replaced
    /// once.
    fn send(&self, worker: Option<Worker>, body: &[u8]) -> Result<Worker, Cut> {
        let fresh = worker.is_none();
        let mut worker = match worker {
            Some(worker) => worker,
            None => self.launch()?,
        };
        if write_job(&mut worker, body).is_ok() {
            return Ok(worker);
        }
        worker.kill();
        if fresh {
            return Err(Cut::Unavailable("the worker took no job".into()));
        }
        let mut worker = self.launch()?;
        match write_job(&mut worker, body) {
            Ok(()) => Ok(worker),
            Err(e) => {
                worker.kill();
                Err(Cut::Unavailable(format!("send the job: {e}")))
            }
        }
    }

    /// Waits for the reply, reading the child's resident memory every `rss_poll` (the first
    /// time right away); past `deadline` or the cap, [`Cut::Limit`].
    fn await_reply(&self, worker: &mut Worker, deadline: Instant) -> Result<Reply, Cut> {
        let limits = &self.config.limits;
        let mut next_poll = Instant::now();
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(Cut::Limit);
            }
            if now >= next_poll {
                if resident_bytes(worker.pid()).is_some_and(|rss| rss > limits.memory_cap) {
                    return Err(Cut::Limit);
                }
                next_poll = now + limits.rss_poll;
            }
            let wait = deadline.min(next_poll).saturating_duration_since(now);
            match worker.frames.recv_timeout(wait) {
                Err(RecvTimeoutError::Timeout) => {}
                Ok(Ok(Some(frame))) => {
                    return serde_json::from_slice::<Reply>(&frame)
                        .map_err(|e| Cut::Unavailable(format!("a malformed reply: {e}")));
                }
                Ok(Ok(None)) | Err(RecvTimeoutError::Disconnected) => {
                    return Err(Cut::Unavailable("the worker ended".into()));
                }
                Ok(Err(e)) => return Err(Cut::Unavailable(format!("read the reply: {e}"))),
            }
        }
    }
}

fn write_job(worker: &mut Worker, body: &[u8]) -> std::io::Result<()> {
    let stdin = worker
        .stdin
        .as_mut()
        .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::BrokenPipe))?;
    write_frame(stdin, body, MAX_WORKER_FRAME)?;
    stdin.flush()
}

impl DryMerger for WorkerMerger {
    /// The first job runs with `limits` until the earlier of `deadline` and the configured
    /// deadline. When its outcome has a conflict whose detection depends on the rename limit,
    /// a second job with [`REWRITES_LIMIT_ESCALATED`] runs with its own `escalated_deadline`;
    /// if a cap cuts it, the first outcome stands.
    fn merge(
        &self,
        repo: &Path,
        input: &DryMergeInput,
        limits: &DryMergeLimits,
        deadline: Instant,
    ) -> Result<DryMergeOutcome, DryMergeError> {
        let mut slot = self.slot();
        let first_deadline = deadline.min(Instant::now() + self.config.limits.deadline);
        let job = WireJob::new(repo, input, limits)?;
        let first = self.job(&mut slot, &job, limits, first_deadline)??;
        if !first.has_kind_needing_escalation() {
            return Ok(first);
        }
        let escalated = DryMergeLimits {
            rewrites_limit: REWRITES_LIMIT_ESCALATED,
            ..limits.clone()
        };
        let job = WireJob::new(repo, input, &escalated)?;
        let second_deadline = Instant::now() + self.config.limits.escalated_deadline;
        match self.job(&mut slot, &job, &escalated, second_deadline) {
            Ok(Ok(mut second)) => {
                second.rewrites_escalated = true;
                Ok(second)
            }
            // Cut or failed: the first outcome is published, as it is.
            Ok(Err(_)) | Err(_) => Ok(first),
        }
    }

    fn close(&self) {
        WorkerMerger::close(self);
    }

    fn close_if_idle(&self, now: Instant) -> bool {
        WorkerMerger::close_if_idle(self, now)
    }
}

impl Drop for WorkerMerger {
    fn drop(&mut self) {
        self.close();
    }
}

/// The worker's environment: `HOME`, the absolute entries of `PATH` and what the OS needs to
/// start a process. Nothing else of the daemon's (no `GIT_*`, `DYLD_*`, `LD_*`).
fn worker_env() -> Vec<(OsString, OsString)> {
    #[cfg(unix)]
    const KEEP: &[&str] = &["HOME"];
    #[cfg(windows)]
    const KEEP: &[&str] = &[
        "HOME",
        "USERPROFILE",
        "SystemRoot",
        "SystemDrive",
        "windir",
        "TEMP",
        "TMP",
    ];
    let mut env: Vec<(OsString, OsString)> = KEEP
        .iter()
        .filter_map(|name| std::env::var_os(name).map(|v| (OsString::from(name), v)))
        .collect();
    let absolute: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| {
            std::env::split_paths(&path)
                .filter(|entry| entry.is_absolute())
                .collect()
        })
        .unwrap_or_default();
    if let Ok(path) = std::env::join_paths(absolute) {
        env.push((OsString::from("PATH"), path));
    }
    env
}

/// The worker's working directory: the root of the filesystem that holds the temporary
/// directory (`/` on Unix, the system drive on Windows), never a repo.
fn worker_cwd() -> PathBuf {
    let temp = std::env::temp_dir();
    temp.ancestors().last().map_or_else(
        || PathBuf::from(std::path::MAIN_SEPARATOR_STR),
        Path::to_path_buf,
    )
}

/// Resident memory of the live child `pid`; `None` when it cannot be read, so it is not counted
/// as over the cap (the deadline still bounds the job).
#[cfg(target_os = "macos")]
fn resident_bytes(pid: u32) -> Option<u64> {
    use libproc::libproc::proc_pid::pidinfo;
    use libproc::libproc::task_info::TaskInfo;
    let pid = i32::try_from(pid).ok()?;
    pidinfo::<TaskInfo>(pid, 0)
        .ok()
        .map(|info| info.pti_resident_size)
}

/// Resident memory of the live child `pid`: `VmRSS` of `/proc/<pid>/status`, in KiB there.
/// The same resident pages as the second field of `statm`, without needing the page size.
#[cfg(target_os = "linux")]
fn resident_bytes(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let kib: u64 = status
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    kib.checked_mul(1024)
}

/// No memory cap where the OS gives no reading here. Pendiente: etapa de validación
/// multiplataforma (a Job Object on Windows).
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn resident_bytes(_pid: u32) -> Option<u64> {
    None
}

// The wire between the daemon and the worker: private, versioned by the handshake.

/// The worker's first frame.
#[derive(Serialize, Deserialize)]
struct Hello {
    version: String,
}

/// One job: the repo, the two commits and the caps.
#[derive(Serialize, Deserialize)]
struct WireJob {
    repo: String,
    ours: String,
    theirs: String,
    limits: WireLimits,
}

impl WireJob {
    fn new(
        repo: &Path,
        input: &DryMergeInput,
        limits: &DryMergeLimits,
    ) -> Result<Self, DryMergeError> {
        let repo = repo
            .to_str()
            .ok_or_else(|| DryMergeError::Unavailable("the repo path is not UTF-8".into()))?;
        Ok(Self {
            repo: repo.to_owned(),
            ours: input.ours.clone(),
            theirs: input.theirs.clone(),
            limits: WireLimits::from(limits),
        })
    }
}

#[derive(Serialize, Deserialize)]
struct WireLimits {
    max_blob_bytes: u64,
    max_written_bytes: u64,
    rewrites_limit: usize,
    max_files: usize,
    max_hunks_per_file: usize,
    marker_size: u8,
    hunk_join_gap: u32,
}

impl From<&DryMergeLimits> for WireLimits {
    fn from(l: &DryMergeLimits) -> Self {
        Self {
            max_blob_bytes: l.max_blob_bytes,
            max_written_bytes: l.max_written_bytes,
            rewrites_limit: l.rewrites_limit,
            max_files: l.max_files,
            max_hunks_per_file: l.max_hunks_per_file,
            marker_size: l.marker_size,
            hunk_join_gap: l.hunk_join_gap,
        }
    }
}

impl From<WireLimits> for DryMergeLimits {
    fn from(l: WireLimits) -> Self {
        Self {
            max_blob_bytes: l.max_blob_bytes,
            max_written_bytes: l.max_written_bytes,
            rewrites_limit: l.rewrites_limit,
            max_files: l.max_files,
            max_hunks_per_file: l.max_hunks_per_file,
            marker_size: l.marker_size,
            hunk_join_gap: l.hunk_join_gap,
        }
    }
}

/// The worker's answer to one job.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Reply {
    Ok(WireOutcome),
    Err(WireError),
}

impl Reply {
    fn from_result(result: Result<DryMergeOutcome, DryMergeError>) -> Self {
        match result {
            Ok(outcome) => Self::Ok(WireOutcome::from(outcome)),
            Err(e) => Self::Err(WireError::from(e)),
        }
    }

    fn into_result(self) -> Result<DryMergeOutcome, DryMergeError> {
        match self {
            Self::Ok(outcome) => Ok(outcome.into()),
            Self::Err(e) => Err(e.into()),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireError {
    MissingObject,
    Shallow,
    TooLarge,
    LimitExceeded,
    Unavailable(String),
}

impl From<DryMergeError> for WireError {
    fn from(e: DryMergeError) -> Self {
        match e {
            DryMergeError::MissingObject => Self::MissingObject,
            DryMergeError::Shallow => Self::Shallow,
            DryMergeError::TooLarge => Self::TooLarge,
            DryMergeError::LimitExceeded => Self::LimitExceeded,
            DryMergeError::Unavailable(why) => Self::Unavailable(why),
        }
    }
}

impl From<WireError> for DryMergeError {
    fn from(e: WireError) -> Self {
        match e {
            WireError::MissingObject => Self::MissingObject,
            WireError::Shallow => Self::Shallow,
            WireError::TooLarge => Self::TooLarge,
            WireError::LimitExceeded => Self::LimitExceeded,
            WireError::Unavailable(why) => Self::Unavailable(why),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct WireOutcome {
    merge_base: WireBase,
    files: Vec<WireFile>,
    truncated: bool,
    rewrites_escalated: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireBase {
    Single(String),
    Virtual(Vec<String>),
    None,
}

#[derive(Serialize, Deserialize)]
struct WireFile {
    path: String,
    kind: WireKind,
    ours: Vec<(u32, u32)>,
    theirs: Vec<(u32, u32)>,
    hunks_truncated: bool,
    has_merge_rule: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WireKind {
    Content,
    AddAdd,
    ModifyDelete,
    Rename,
    DirectoryFile,
    Binary,
    Submodule,
}

impl From<DryMergeOutcome> for WireOutcome {
    fn from(o: DryMergeOutcome) -> Self {
        Self {
            merge_base: match o.merge_base {
                MergeBaseUsed::Single(id) => WireBase::Single(id),
                MergeBaseUsed::Virtual { bases } => WireBase::Virtual(bases),
                MergeBaseUsed::None => WireBase::None,
            },
            files: o.files.into_iter().map(WireFile::from).collect(),
            truncated: o.truncated,
            rewrites_escalated: o.rewrites_escalated,
        }
    }
}

impl From<WireOutcome> for DryMergeOutcome {
    fn from(o: WireOutcome) -> Self {
        Self {
            merge_base: match o.merge_base {
                WireBase::Single(id) => MergeBaseUsed::Single(id),
                WireBase::Virtual(bases) => MergeBaseUsed::Virtual { bases },
                WireBase::None => MergeBaseUsed::None,
            },
            files: o.files.into_iter().map(ConflictFile::from).collect(),
            truncated: o.truncated,
            rewrites_escalated: o.rewrites_escalated,
        }
    }
}

fn ranges_out(ranges: Vec<LineRange>) -> Vec<(u32, u32)> {
    ranges.into_iter().map(|r| (r.start, r.len)).collect()
}

fn ranges_in(ranges: Vec<(u32, u32)>) -> Vec<LineRange> {
    ranges
        .into_iter()
        .map(|(start, len)| LineRange { start, len })
        .collect()
}

impl From<ConflictFile> for WireFile {
    fn from(f: ConflictFile) -> Self {
        Self {
            path: f.path,
            kind: match f.kind {
                ConflictKind::Content => WireKind::Content,
                ConflictKind::AddAdd => WireKind::AddAdd,
                ConflictKind::ModifyDelete => WireKind::ModifyDelete,
                ConflictKind::Rename => WireKind::Rename,
                ConflictKind::DirectoryFile => WireKind::DirectoryFile,
                ConflictKind::Binary => WireKind::Binary,
                ConflictKind::Submodule => WireKind::Submodule,
            },
            ours: ranges_out(f.ours),
            theirs: ranges_out(f.theirs),
            hunks_truncated: f.hunks_truncated,
            has_merge_rule: f.has_merge_rule,
        }
    }
}

impl From<WireFile> for ConflictFile {
    fn from(f: WireFile) -> Self {
        Self {
            path: f.path,
            kind: match f.kind {
                WireKind::Content => ConflictKind::Content,
                WireKind::AddAdd => ConflictKind::AddAdd,
                WireKind::ModifyDelete => ConflictKind::ModifyDelete,
                WireKind::Rename => ConflictKind::Rename,
                WireKind::DirectoryFile => ConflictKind::DirectoryFile,
                WireKind::Binary => ConflictKind::Binary,
                WireKind::Submodule => ConflictKind::Submodule,
            },
            ours: ranges_in(f.ours),
            theirs: ranges_in(f.theirs),
            hunks_truncated: f.hunks_truncated,
            has_merge_rule: f.has_merge_rule,
        }
    }
}

#[cfg(test)]
mod tests;
