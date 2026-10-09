//! The applier of undo, redo and restore (TS-TMC-003, ADR-TMC-002 § 3): brings worktrees, index
//! and refs to the state of a snapshot of the store.
//!
//! It starts where the protected operation (TS-TMC-004, ADR-TMC-004) leaves off: the operation is
//! recorded and `ready`, with its guaranteed prior snapshot (step 2) complete. That prior is what
//! the repo is expected to hold now; every write compares against it. The steps, each annotated in
//! the journal before it is acted on:
//!
//! 1. Preconditions ([`Applier::check_preconditions`], also exposed so the protected operation can
//!    run them before its snapshot): no Git operation in progress, no Git lock, repo trusted,
//!    both snapshots re-verified and the target tree free of hostile paths. Failing them leaves
//!    everything as it was and the operation `rejected`.
//! 3. Locks: the repo in the daemon ([`super::repo_lock`]) and the `index.lock` of every worktree,
//!    each annotated with its identity. Preconditions run again under them.
//! 4. Objects the repo lacks, from the store as a pack.
//! 5. Refs: one transaction with expected old values; then each `HEAD` by compare-and-swap.
//! 6. Files: removals deepest first, then writes, by atomic exchange relative to the root.
//! 7. Index, through the own `index.lock`.
//! 8. Close: the repo is freed and the operation is `finished`, with what it reports.
//!
//! A failure after step 3 starts is not rolled back (ADR-TMC-002 § 3, TQ-10 → a): the operation
//! is `interrupted` and `raptor undo` returns to the prior snapshot. There is a single recovery
//! path, the one the chaos harness tests (INF-TMC-001).
//!
//! Choosing the target (US-TMC-002, 003, 009–011), permissions and overlap policy (US-TMC-012,
//! 013) and the "already pushed" notice (US-TMC-014) are outside this module.

mod plan;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use gitraptor_git::tm_write::WriteContext;
use gitraptor_git::tm_write::files::{Content, Expected, Kind, Outcome, RootDir};
use gitraptor_git::tm_write::lock::GitLock;
use gitraptor_git::tm_write::worktree::{Precondition, WriteWorktree};
use gitraptor_git::tm_write::{WriteError, index, objects, recreate, refs, tree_path};
use gitraptor_git::{ReadError, ReaderOptions, RepoReader};

use super::chaos;
use super::oplog::{OperationTransition, Oplog, file_identity};
use super::repo_lock;
use super::store::SnapshotStore;
use crate::profile::ProfileError;
use plan::{Loaded, LoadedWorktree};

/// What to apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyPlan {
    /// The snapshot whose state the repo is brought to.
    pub target_snapshot: String,
    /// The guaranteed prior snapshot of this operation (step 2): what the repo holds now.
    pub prior_snapshot: String,
    pub worktrees: Vec<PlanWorktree>,
    /// Which branches and whether `refs/stash` move to the target's values.
    pub refs: RefScope,
}

/// The refs an application moves to the target's values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefScope {
    /// No ref moves.
    None,
    /// Every branch and `refs/stash` (a restore of the whole repo).
    All,
    /// Only these full names (`refs/heads/<name>`, `refs/stash`): an undo moves the refs of its
    /// scope and never another worktree's branch (US-TMC-002).
    Only(BTreeSet<String>),
}

impl RefScope {
    pub fn includes(&self, full_name: &str) -> bool {
        match self {
            Self::None => false,
            Self::All => true,
            Self::Only(names) => names.contains(full_name),
        }
    }
}

/// One worktree of the plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanWorktree {
    /// Its key in both snapshots (`wt/<key>/`).
    pub key: String,
    /// Its root, from the validated state of the daemon (never from the request).
    pub root: PathBuf,
    /// Id of a linked worktree under `worktrees/`, to recreate it if its root is gone.
    pub recreate_id: Option<String>,
}

/// Why the applier did not start. Nothing was changed. Typed codes, rendered by the client
/// (NFR-TMC-14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    InProgress {
        worktree: PathBuf,
        marker: &'static str,
    },
    /// A Git lock is present ("Git busy"); it stays.
    GitBusy {
        lock: PathBuf,
    },
    /// Another application holds the repo.
    RepoBusy,
    /// The repo is not trusted (`safe.directory`), or a worktree's `.git` is not the repo's own
    /// (#223 I-03): nothing is read behind it and nothing is written.
    Untrusted {
        worktree: PathBuf,
    },
    WorktreeUnavailable {
        worktree: PathBuf,
        reason: String,
    },
    /// A snapshot failed its re-verification or its `meta` is invalid (SEC-TMC-09).
    InvalidSnapshot {
        snapshot: String,
        reason: String,
    },
    /// The target tree has a hostile path or a collision on this file system (SEC-TMC-04).
    HostileTree {
        worktree: String,
        reason: String,
    },
    /// A ref is not where the prior snapshot says.
    RefMoved {
        name: String,
    },
    Unsupported(&'static str),
}

impl Refusal {
    /// Stable code, for the journal and the client.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InProgress { .. } => "git-operation-in-progress",
            Self::GitBusy { .. } => "git-busy",
            Self::RepoBusy => "repo-busy",
            Self::Untrusted { .. } => "repo-untrusted",
            Self::WorktreeUnavailable { .. } => "worktree-unavailable",
            Self::InvalidSnapshot { .. } => "invalid-snapshot",
            Self::HostileTree { .. } => "hostile-tree",
            Self::RefMoved { .. } => "ref-moved",
            Self::Unsupported(_) => "unsupported",
        }
    }
}

/// Why an application failed.
#[derive(Debug)]
pub enum ApplyError {
    /// Nothing was changed; the operation is `rejected` (when it was given one).
    Rejected(Vec<Refusal>),
    /// A failure after the first change; the operation is `interrupted` and `raptor undo` returns
    /// to the prior snapshot. `changed` is false when the failing step had not changed anything
    /// visible yet (a ref transaction that failed whole, say).
    Interrupted {
        step: u32,
        reason: String,
        changed: bool,
    },
    /// The journal could not be written: nothing past the last annotated step was done.
    Oplog(ProfileError),
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(r) => {
                let codes: Vec<_> = r.iter().map(Refusal::code).collect();
                write!(f, "rejected: {}", codes.join(", "))
            }
            Self::Interrupted { step, reason, .. } => {
                write!(f, "interrupted at step {step}: {reason}")
            }
            Self::Oplog(e) => write!(f, "oplog: {e}"),
        }
    }
}

impl std::error::Error for ApplyError {}

/// A path that does not hold the target after applying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathReport {
    pub worktree: String,
    pub path: String,
    pub issue: PathIssue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathIssue {
    /// Someone else wrote there since the prior snapshot: their content was kept, in place or at
    /// `kept_at` (SEC-TMC-11).
    Overlap { kept_at: Option<PathBuf> },
    /// The file system has no atomic exchange: "not restorable with guarantee".
    NotGuaranteed,
    /// A folder on the way is a link, a file or on another device.
    Blocked(&'static str),
}

/// What the result carries besides the paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyWarning {
    /// `intent-to-add` has no plumbing in Git 2.38: the file is in the working tree, not marked.
    IntentToAddNotRestored { worktree: String, path: String },
    /// `HEAD` was written by compare-and-swap and left no reflog entry.
    HeadWithoutReflog { worktree: String },
    /// Only the top of `refs/stash` moved; the earlier entries stay in its reflog.
    StashTopOnly,
    /// The target has no stash: `refs/stash` was kept, since deleting it would drop its reflog
    /// (the whole stack, NFR-01).
    StashKept,
    /// A path left out of the snapshots: never written or removed.
    Excluded { worktree: String, path: String },
}

/// What an application did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApplyReport {
    pub written: usize,
    pub removed: usize,
    pub paths: Vec<PathReport>,
    pub warnings: Vec<ApplyWarning>,
}

/// A hook run right before a file exchange, with the worktree root and the path.
pub type ExchangeHook = Box<dyn Fn(&Path, &[u8]) + Send + Sync>;

/// Fault-injection points for the tests and the chaos harness (INF-TMC-001).
#[derive(Default)]
pub struct ApplyHooks {
    /// Runs at the start of each step from 3 to 7, after its journal entry.
    pub at_step: Option<Box<dyn Fn(u32) + Send + Sync>>,
    /// Runs right before each file exchange, with the worktree root and the path.
    pub before_exchange: Option<ExchangeHook>,
    /// Behave as a file system without atomic exchange (tests of "not restorable with
    /// guarantee").
    pub simulate_no_exchange: bool,
    /// Stop each file write right after the current entry went aside, as if the process died
    /// there (tests of the sweep after a crash, DS-TS-TMC-003 Enmienda T).
    pub simulate_crash_between_moves: bool,
}

impl std::fmt::Debug for ApplyHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApplyHooks").finish_non_exhaustive()
    }
}

/// The applier of one repo.
pub struct Applier<'a> {
    store: &'a SnapshotStore,
    write: &'a WriteContext,
    oplog: &'a Mutex<Oplog>,
    main_root: PathBuf,
    /// The repo's common Git directory, from the registry: refs are read from it and a worktree
    /// is opened only if the repo registers it and owns its `.git`.
    common_dir: PathBuf,
    profile_root: PathBuf,
    now_ms: Box<dyn Fn() -> i64 + 'a>,
    hooks: ApplyHooks,
}

impl std::fmt::Debug for Applier<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Applier")
            .field("main_root", &self.main_root)
            .finish_non_exhaustive()
    }
}

fn wall_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// A step that failed: whether it changed anything visible.
struct StepError {
    reason: String,
    changed: bool,
}

impl From<WriteError> for StepError {
    fn from(e: WriteError) -> Self {
        Self {
            reason: e.to_string(),
            changed: true,
        }
    }
}

impl<'a> Applier<'a> {
    /// `main_root` is the main worktree of the repo (its Git folder holds refs and objects) and
    /// `common_dir` that Git folder, as the registry has it (#223 I-03); `profile_root` is the
    /// profile, where no worktree may be recreated.
    pub fn new(
        store: &'a SnapshotStore,
        write: &'a WriteContext,
        oplog: &'a Mutex<Oplog>,
        main_root: PathBuf,
        common_dir: PathBuf,
        profile_root: PathBuf,
    ) -> Self {
        Self {
            store,
            write,
            oplog,
            main_root,
            common_dir,
            profile_root,
            now_ms: Box::new(wall_ms),
            hooks: ApplyHooks::default(),
        }
    }

    pub fn with_clock(mut self, now_ms: impl Fn() -> i64 + 'a) -> Self {
        self.now_ms = Box::new(now_ms);
        self
    }

    pub fn with_hooks(mut self, hooks: ApplyHooks) -> Self {
        self.hooks = hooks;
        self
    }

    fn oplog<T>(
        &self,
        f: impl FnOnce(&mut Oplog, i64) -> crate::profile::Result<T>,
    ) -> Result<T, ApplyError> {
        let mut log = self.oplog.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut log, (self.now_ms)()).map_err(ApplyError::Oplog)
    }

    fn main(&self) -> Result<WriteWorktree, Refusal> {
        WriteWorktree::open(&self.main_root).map_err(|e| Refusal::WorktreeUnavailable {
            worktree: self.main_root.clone(),
            reason: e.to_string(),
        })
    }

    /// Step 1, without changing anything: every refusal found now. Empty means the applier may
    /// start. The protected operation runs it before its prior snapshot; the applier runs it
    /// again under its locks.
    pub fn check_preconditions(&self, plan: &ApplyPlan) -> Vec<Refusal> {
        if cfg!(not(any(unix, windows))) {
            return vec![Refusal::Unsupported("applier")];
        }
        // Before anything behind a `.git` is opened: every worktree of the plan is one the repo
        // registers and owns (#223 I-03, NFR-01).
        let roots = std::iter::once(&self.main_root).chain(
            plan.worktrees
                .iter()
                .filter(|w| w.recreate_id.is_none() || w.root.exists())
                .map(|w| &w.root),
        );
        if let Some(r) = self.untrusted_among(roots) {
            return vec![r];
        }
        let mut refusals = Vec::new();
        let main = match self.main() {
            Ok(m) => m,
            Err(r) => return vec![r],
        };
        let mut worktrees = vec![main];
        for w in &plan.worktrees {
            match WriteWorktree::open(&w.root) {
                Ok(wt) => worktrees.push(wt),
                Err(_) if w.recreate_id.is_some() && !w.root.exists() => {}
                Err(e) => refusals.push(Refusal::WorktreeUnavailable {
                    worktree: w.root.clone(),
                    reason: e.to_string(),
                }),
            }
        }
        refusals.extend(preconditions_of(&worktrees, &[]));
        refusals
    }

    /// Applies `plan` for `operation_id`, which must be `ready`. See the module docs.
    pub fn apply(&self, operation_id: &str, plan: &ApplyPlan) -> Result<ApplyReport, ApplyError> {
        self.apply_with(operation_id, plan, None)
    }

    /// Like [`Self::apply`] for a caller that already holds the repo (an undo takes it before
    /// choosing its target, so the last operation cannot change under it). A guard of another
    /// repo is refused as "repo busy".
    pub fn apply_holding(
        &self,
        operation_id: &str,
        plan: &ApplyPlan,
        held: &repo_lock::RepoGuard,
    ) -> Result<ApplyReport, ApplyError> {
        self.apply_with(operation_id, plan, Some(held))
    }

    fn apply_with(
        &self,
        operation_id: &str,
        plan: &ApplyPlan,
        held: Option<&repo_lock::RepoGuard>,
    ) -> Result<ApplyReport, ApplyError> {
        let reject = |refusals: Vec<Refusal>| -> Result<ApplyReport, ApplyError> {
            let reason = refusals.first().map(Refusal::code).unwrap_or("rejected");
            self.oplog(|log, now| {
                log.advance_operation(operation_id, OperationTransition::Rejected { reason }, now)
            })?;
            Err(ApplyError::Rejected(refusals))
        };

        // ---- 1. preconditions and loading (read-only) ----------------------------------
        let refusals = self.check_preconditions(plan);
        if !refusals.is_empty() {
            return reject(refusals);
        }
        let mut loaded = match plan::load(
            self.store,
            &plan.target_snapshot,
            &plan.prior_snapshot,
            &plan.worktrees,
            &plan.refs,
        ) {
            Ok(l) => l,
            Err(r) => return reject(vec![r]),
        };
        let main = match self.main() {
            Ok(m) => m,
            Err(r) => return reject(vec![r]),
        };
        if let Some(r) = self.untrusted(&loaded) {
            return reject(vec![r]);
        }
        if let Err(r) = self.refs_as_expected(&main, &loaded) {
            return reject(vec![r]);
        }

        // ---- 3. locks ----------------------------------------------------------------
        let key = self.store.path().display().to_string();
        let repo_guard = match held {
            Some(guard) if guard.key() == key => None,
            Some(_) => return reject(vec![Refusal::RepoBusy]),
            None => match repo_lock::try_lock(&key) {
                Some(guard) => Some(guard),
                None => return reject(vec![Refusal::RepoBusy]),
            },
        };
        let mut locks: Vec<(usize, GitLock)> = Vec::new();
        for (i, w) in loaded.worktrees.iter().enumerate() {
            let Some(wt) = &w.worktree else { continue };
            match self.take_lock(operation_id, wt) {
                Ok(lock) => locks.push((i, lock)),
                Err(Ok(r)) => {
                    self.release_all(operation_id, locks)?;
                    return reject(vec![r]);
                }
                Err(Err(e)) => {
                    self.release_all(operation_id, locks)?;
                    return Err(e);
                }
            }
        }
        let ours: Vec<PathBuf> = locks.iter().map(|(_, l)| l.path().to_owned()).collect();
        let mut present = vec![main.clone()];
        present.extend(loaded.worktrees.iter().filter_map(|w| w.worktree.clone()));
        let mut refusals = preconditions_of(&present, &ours);
        // Once more under the locks, right before the first write: a `.git` swapped since the
        // checks above is refused here and nothing is written (NFR-01).
        if let Some(r) = self.untrusted(&loaded) {
            refusals.insert(0, r);
        }
        for w in &loaded.worktrees {
            // A worktree to recreate is checked against the file system of the main root now,
            // and against its own once it exists.
            let root = if w.worktree.is_some() {
                &w.root
            } else {
                &self.main_root
            };
            if let Err(r) = check_tree(w, root) {
                refusals.push(r);
            }
        }
        if !refusals.is_empty() {
            self.release_all(operation_id, locks)?;
            return reject(refusals);
        }

        let mut report = ApplyReport::default();
        let result = self.run_steps(operation_id, &main, &mut loaded, &mut locks, &mut report);
        match result {
            Ok(()) => {
                chaos::crash_point(chaos::OPERATION_APPLIED);
                // ---- 8. close ------------------------------------------------------------
                self.release_all(operation_id, locks)?;
                drop(repo_guard);
                self.oplog(|log, now| {
                    log.advance_operation(operation_id, OperationTransition::Finished, now)
                })?;
                Ok(report)
            }
            Err((step, e)) => {
                self.release_all(operation_id, locks)?;
                drop(repo_guard);
                self.oplog(|log, now| {
                    log.advance_operation(operation_id, OperationTransition::Interrupted, now)
                })?;
                Err(ApplyError::Interrupted {
                    step,
                    reason: e.reason,
                    changed: e.changed,
                })
            }
        }
    }

    fn untrusted(&self, loaded: &Loaded) -> Option<Refusal> {
        self.untrusted_among(
            std::iter::once(&self.main_root).chain(
                loaded
                    .worktrees
                    .iter()
                    .filter(|w| w.worktree.is_some())
                    .map(|w| &w.root),
            ),
        )
    }

    /// The first of `roots` that Git does not trust, or whose `.git` the repo does not own
    /// ([`crate::observe::open_registered_worktree`]).
    fn untrusted_among<'r>(&self, roots: impl Iterator<Item = &'r PathBuf>) -> Option<Refusal> {
        let registered = match crate::observe::registered_worktrees(&self.common_dir) {
            Ok(list) => list,
            Err(ReadError::Untrusted(_)) => {
                return Some(Refusal::Untrusted {
                    worktree: self.main_root.clone(),
                });
            }
            Err(e) => {
                return Some(Refusal::WorktreeUnavailable {
                    worktree: self.main_root.clone(),
                    reason: e.to_string(),
                });
            }
        };
        for root in roots {
            if let Err(ReadError::Untrusted(_)) =
                crate::observe::open_registered_in(&self.common_dir, &registered, root)
            {
                return Some(Refusal::Untrusted {
                    worktree: root.clone(),
                });
            }
        }
        None
    }

    /// Branches and every `HEAD` hold what the prior snapshot says, before starting.
    fn refs_as_expected(&self, main: &WriteWorktree, loaded: &Loaded) -> Result<(), Refusal> {
        let reader =
            RepoReader::open(&self.common_dir, &ReaderOptions::default()).map_err(|e| {
                Refusal::WorktreeUnavailable {
                    worktree: main.root().to_owned(),
                    reason: e.to_string(),
                }
            })?;
        let tips = reader
            .branch_tips()
            .map_err(|e| Refusal::WorktreeUnavailable {
                worktree: main.root().to_owned(),
                reason: e.to_string(),
            })?;
        let stash = reader.stash().ok().flatten();
        for u in &loaded.ref_updates {
            let name = u.name.as_str();
            let current = match name.strip_prefix("refs/heads/") {
                Some(short) => tips.iter().find(|(n, _)| n == short).map(|(_, id)| *id),
                None => stash,
            };
            if current != u.old {
                return Err(Refusal::RefMoved {
                    name: name.to_owned(),
                });
            }
        }
        for w in &loaded.worktrees {
            if let (Some(wt), Some(expected)) = (&w.worktree, &w.prior_head)
                && wt.read_head().ok().as_ref() != Some(expected)
            {
                return Err(Refusal::RefMoved {
                    name: format!("HEAD ({})", w.key),
                });
            }
        }
        Ok(())
    }

    /// Takes the `index.lock` of `wt` and annotates it. `Err(Ok(_))` is a refusal.
    fn take_lock(
        &self,
        operation_id: &str,
        wt: &WriteWorktree,
    ) -> Result<GitLock, Result<Refusal, ApplyError>> {
        let lock = match GitLock::acquire(&wt.index_path()) {
            Ok(l) => l,
            Err(WriteError::Busy(lock)) => return Err(Ok(Refusal::GitBusy { lock })),
            Err(e) => {
                return Err(Ok(Refusal::WorktreeUnavailable {
                    worktree: wt.root().to_owned(),
                    reason: e.to_string(),
                }));
            }
        };
        if let Ok(Some(identity)) = file_identity(lock.path()) {
            let path = lock.path().to_owned();
            if let Err(e) =
                self.oplog(|log, now| log.record_lock_taken(operation_id, &path, identity, now))
            {
                drop(lock);
                return Err(Err(e));
            }
        }
        Ok(lock)
    }

    /// Releases the locks still held and annotates each.
    fn release_all(
        &self,
        operation_id: &str,
        locks: Vec<(usize, GitLock)>,
    ) -> Result<(), ApplyError> {
        for (_, lock) in locks {
            let path = lock.path().to_owned();
            let _ = lock.release();
            self.oplog(|log, now| log.record_lock_released(operation_id, &path, now))?;
        }
        Ok(())
    }

    fn step(&self, operation_id: &str, step: u32) -> Result<(), (u32, StepError)> {
        self.oplog(|log, now| {
            log.advance_operation(operation_id, OperationTransition::Applying { step }, now)
        })
        .map_err(|e| {
            (
                step,
                StepError {
                    reason: e.to_string(),
                    changed: false,
                },
            )
        })?;
        if let Some(hook) = &self.hooks.at_step {
            hook(step);
        }
        chaos::crash_point(chaos::apply_step(step));
        Ok(())
    }

    fn run_steps(
        &self,
        operation_id: &str,
        main: &WriteWorktree,
        loaded: &mut Loaded,
        locks: &mut Vec<(usize, GitLock)>,
        report: &mut ApplyReport,
    ) -> Result<(), (u32, StepError)> {
        let at = |step: u32| move |e: StepError| (step, e);

        // ---- 3. locks taken; the step is annotated once they are ------------------------
        self.step(operation_id, 3)?;

        // ---- 4. objects --------------------------------------------------------------
        self.step(operation_id, 4)?;
        let kept = objects::copy_into_repo(
            self.write,
            self.store.path(),
            main,
            &loaded.wants,
            &loaded.haves,
        )
        .map_err(|e| StepError {
            reason: e.to_string(),
            changed: false,
        })
        .map_err(at(4))?;

        // ---- 5. refs -------------------------------------------------------------------
        self.step(operation_id, 5)?;
        refs::apply_updates(self.write, main, &loaded.ref_updates)
            .map_err(|e| StepError {
                reason: e.to_string(),
                changed: false,
            })
            .map_err(at(5))?;
        if let Some(kept) = kept {
            kept.release().map_err(StepError::from).map_err(at(5))?;
        }
        if loaded.stash_moves {
            report.warnings.push(ApplyWarning::StashTopOnly);
        }
        if loaded.stash_kept {
            report.warnings.push(ApplyWarning::StashKept);
        }
        for w in &loaded.worktrees {
            let (Some(wt), Some(expected), Some(target)) =
                (&w.worktree, &w.prior_head, &w.target_head)
            else {
                continue;
            };
            if expected == target {
                continue;
            }
            refs::swap_head(wt, expected, target)
                .map_err(StepError::from)
                .map_err(at(5))?;
            report.warnings.push(ApplyWarning::HeadWithoutReflog {
                worktree: w.key.clone(),
            });
        }

        // ---- 6. files ------------------------------------------------------------------
        self.step(operation_id, 6)?;
        for i in 0..loaded.worktrees.len() {
            if loaded.worktrees[i].worktree.is_none() {
                let wt = self.recreate(&loaded.worktrees[i], main).map_err(at(6))?;
                let lock = self
                    .take_lock(operation_id, &wt)
                    .map_err(|e| StepError {
                        reason: match e {
                            Ok(r) => r.code().to_owned(),
                            Err(e) => e.to_string(),
                        },
                        changed: true,
                    })
                    .map_err(at(6))?;
                check_tree(&loaded.worktrees[i], &loaded.worktrees[i].root)
                    .map_err(|r| StepError {
                        reason: r.code().to_owned(),
                        changed: true,
                    })
                    .map_err(at(6))?;
                locks.push((i, lock));
                loaded.worktrees[i].worktree = Some(wt);
            }
            self.files(&loaded.worktrees[i], report).map_err(at(6))?;
        }

        // ---- 7. index ------------------------------------------------------------------
        self.step(operation_id, 7)?;
        for (i, lock) in std::mem::take(locks) {
            let w = &loaded.worktrees[i];
            let wt = w.worktree.as_ref().expect("locked worktrees are present");
            for path in &w.intent_to_add {
                report.warnings.push(ApplyWarning::IntentToAddNotRestored {
                    worktree: w.key.clone(),
                    path: path.clone(),
                });
            }
            let path = lock.path().to_owned();
            let built = index::build(self.write, wt, &w.index, &w.skip_worktree)
                .map_err(StepError::from)
                .map_err(at(7))?;
            index::install(lock, wt, &built)
                .map_err(StepError::from)
                .map_err(at(7))?;
            self.oplog(|log, now| log.record_lock_released(operation_id, &path, now))
                .map_err(|e| StepError {
                    reason: e.to_string(),
                    changed: true,
                })
                .map_err(at(7))?;
        }
        Ok(())
    }

    fn recreate(
        &self,
        w: &LoadedWorktree,
        main: &WriteWorktree,
    ) -> Result<WriteWorktree, StepError> {
        let id = w
            .recreate_id
            .as_deref()
            .expect("only recreatable worktrees are absent");
        let head = w.target_head.as_ref().ok_or_else(|| StepError {
            reason: "recreated worktree without HEAD".into(),
            changed: false,
        })?;
        Ok(recreate::recreate(
            main,
            id,
            &w.root,
            head,
            &self.profile_root,
        )?)
    }

    /// Step 6 for one worktree: removals deepest first, empty folders, then writes.
    fn files(&self, w: &LoadedWorktree, report: &mut ApplyReport) -> Result<(), StepError> {
        let mut root = RootDir::open(&w.root)?;
        if self.hooks.simulate_no_exchange {
            root = root.simulating_no_exchange();
        }
        if self.hooks.simulate_crash_between_moves {
            root = root.simulating_crash_between_moves();
        }
        let issue = |report: &mut ApplyReport, path: &str, outcome: Outcome| match outcome {
            Outcome::Written => report.written += 1,
            Outcome::Removed => report.removed += 1,
            Outcome::Unchanged => {}
            Outcome::Overlap { kept_at } => report.paths.push(PathReport {
                worktree: w.key.clone(),
                path: path.to_owned(),
                issue: PathIssue::Overlap { kept_at },
            }),
            Outcome::NotGuaranteed => report.paths.push(PathReport {
                worktree: w.key.clone(),
                path: path.to_owned(),
                issue: PathIssue::NotGuaranteed,
            }),
            Outcome::Blocked(why) => report.paths.push(PathReport {
                worktree: w.key.clone(),
                path: path.to_owned(),
                issue: PathIssue::Blocked(why),
            }),
        };
        for path in &w.excluded {
            report.warnings.push(ApplyWarning::Excluded {
                worktree: w.key.clone(),
                path: path.clone(),
            });
        }

        let mut removals: Vec<(&String, &(Kind, gitraptor_git::Oid))> = w
            .prior_files
            .iter()
            .filter(|(p, _)| !w.target_files.contains_key(*p) && !w.is_excluded(p))
            .collect();
        removals.sort_by(|a, b| depth(b.0).cmp(&depth(a.0)).then(a.0.cmp(b.0)));
        let mut emptied = BTreeSet::new();
        for (path, (kind, id)) in removals {
            let outcome = root.remove(path.as_bytes(), *kind, *id)?;
            if outcome == Outcome::Removed {
                let mut p = path.as_str();
                while let Some((parent, _)) = p.rsplit_once('/') {
                    emptied.insert(parent.to_owned());
                    p = parent;
                }
            }
            issue(report, path, outcome);
        }
        let mut emptied: Vec<String> = emptied
            .into_iter()
            .filter(|d| {
                !w.target_files.keys().any(|p| {
                    p.strip_prefix(d.as_str())
                        .is_some_and(|r| r.starts_with('/'))
                })
            })
            .collect();
        emptied.sort_by_key(|d| std::cmp::Reverse(depth(d)));
        for dir in emptied {
            root.remove_dir_if_empty(dir.as_bytes())?;
        }

        let mut writes: Vec<(&String, &(Kind, gitraptor_git::Oid))> = w
            .target_files
            .iter()
            .filter(|(p, t)| w.prior_files.get(*p) != Some(*t) && !w.is_excluded(p))
            .collect();
        writes.sort_by(|a, b| depth(a.0).cmp(&depth(b.0)).then(a.0.cmp(b.0)));
        let before = |rel: &[u8]| {
            if let Some(hook) = &self.hooks.before_exchange {
                hook(&w.root, rel);
            }
        };
        for (n, (path, (kind, id))) in writes.into_iter().enumerate() {
            if n == 1 {
                chaos::crash_point(chaos::APPLY_MID_FILES);
            }
            let bytes = self.store.read_blob(*id).map_err(|e| StepError {
                reason: e.to_string(),
                changed: true,
            })?;
            let content = match kind {
                Kind::Symlink => Content::Symlink(&bytes),
                Kind::File => Content::File {
                    bytes: &bytes,
                    executable: false,
                },
                Kind::Executable => Content::File {
                    bytes: &bytes,
                    executable: true,
                },
            };
            let expected = match w.prior_files.get(path) {
                Some((kind, id)) => Expected::Present {
                    kind: *kind,
                    id: *id,
                },
                None => Expected::Absent,
            };
            let outcome = root.replace(path.as_bytes(), &content, &expected, &before)?;
            issue(report, path, outcome);
        }
        Ok(())
    }
}

fn depth(path: &str) -> usize {
    path.bytes().filter(|b| *b == b'/').count()
}

/// Preconditions of `worktrees`, ignoring the locks this application holds.
fn preconditions_of(worktrees: &[WriteWorktree], ours: &[PathBuf]) -> Vec<Refusal> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for wt in worktrees {
        for p in wt.preconditions() {
            let refusal = match p {
                Precondition::InProgress { worktree, marker } => {
                    Refusal::InProgress { worktree, marker }
                }
                Precondition::GitBusy { lock } if ours.contains(&lock) => continue,
                Precondition::GitBusy { lock } => Refusal::GitBusy { lock },
            };
            if seen.insert(format!("{refusal:?}")) {
                out.push(refusal);
            }
        }
    }
    out
}

/// The target tree of a worktree, against the file system its root is on (SEC-TMC-04).
fn check_tree(w: &LoadedWorktree, probe_root: &Path) -> Result<(), Refusal> {
    let hostile = |reason: String| Refusal::HostileTree {
        worktree: w.key.clone(),
        reason,
    };
    let root = RootDir::open(probe_root).map_err(|e| hostile(e.to_string()))?;
    let folding = root.probe_folding().map_err(|e| hostile(e.to_string()))?;
    let paths = w
        .target_files
        .keys()
        .map(String::as_bytes)
        .chain(w.index.iter().map(|e| e.path.as_slice()));
    tree_path::check_tree(paths, folding).map_err(|r| hostile(r.to_string()))
}
