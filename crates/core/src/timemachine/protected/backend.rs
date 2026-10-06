//! The daemon's repo layer for protected operations (US-TMC-001): the
//! observed repos with their oplog and snapshot store, and the hook where
//! the catalog of user operations plugs in.
//!
//! The executor of the catalog is TS-CKP-002 (`crate::executor`); each
//! operation's own part (preconditions, expected values, step) is its
//! story's and plugs in through [`OperationCatalog`]. Until a catalog is
//! wired, the daemon answers `operation.prepare` with "not implemented".

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use gitraptor_api::catalog::{Layer, OperationArgs, OperationId, RejectReason};
use gitraptor_git::{ReaderOptions, RepoReader};

use super::scope::{McpAllowlist, NoMcpRepos, ProtectedBackend, RepoHandle, ScopeError};
use super::{
    PriorError, PriorRequest, PriorSnapshot, PriorSnapshotter, ProtectedStep, StepError,
    StoreSnapshotter,
};
use crate::executor::{GuardrailsGate, OpPlan, PlanError, RepoFacts, StepPlan};
use crate::profile::ProfileDirs;
use crate::timemachine::oplog::Oplog;
use crate::timemachine::store::SnapshotStore;
use crate::timemachine::undo::{TmRepoHandle, UndoBackend};

/// The operations' own parts (ADR-CKP-002 § 1): each one's preconditions,
/// expected values, warnings and affected work, and the step that runs a
/// checked plan. The executor of TS-CKP-002 does the rest (plan, queue,
/// revalidation, Guardrails, protected operation). Each operation's story
/// implements its part; tests plug in their own.
pub trait OperationCatalog: Send + Sync {
    /// The operation's part of the plan. `PlanError::NotImplemented` until
    /// its story.
    fn plan_op(
        &self,
        operation: OperationId,
        args: &OperationArgs,
        repo: &RepoHandle,
        facts: &RepoFacts,
    ) -> Result<OpPlan, PlanError>;
    /// The step that runs a plan whose fingerprint was just checked.
    fn step(&self, plan: &StepPlan<'_>) -> Result<Box<dyn ProtectedStep>, StepError>;
}

/// A layer over the production snapshotter. Tests inject faults with it
/// (no space, for example); release builds ignore it.
pub type SnapshotterLayer =
    Arc<dyn Fn(Arc<dyn PriorSnapshotter>) -> Arc<dyn PriorSnapshotter> + Send + Sync>;

/// What the daemon needs to run user operations.
#[derive(Clone)]
pub struct OperationsWiring {
    pub catalog: Arc<dyn OperationCatalog>,
    /// Guardrails (TS-CKP-003); `NoGuardrails` until then (fail-closed).
    pub gate: Arc<dyn GuardrailsGate>,
    /// Tests only: see `ProtectedWiring::test_layer_override`. `None` in
    /// production.
    #[doc(hidden)]
    pub test_layer_override: Option<Layer>,
    /// Deadline of the prior snapshot.
    pub prior_deadline: Duration,
    pub prior_layer: Option<SnapshotterLayer>,
}

impl std::fmt::Debug for OperationsWiring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OperationsWiring")
            .field("prior_deadline", &self.prior_deadline)
            .field("prior_layer", &self.prior_layer.is_some())
            .field("test_layer_override", &self.test_layer_override)
            .finish_non_exhaustive()
    }
}

/// Repo id, oplog and store (`None` if it cannot be opened).
type FoundRepo = (String, Arc<Mutex<Oplog>>, Option<Arc<SnapshotStore>>);

struct TmRepo {
    repo_id: String,
    /// Canonical Git common directory.
    common_dir: PathBuf,
    oplog: Arc<Mutex<Oplog>>,
    /// Opened on first need and kept: the capture's incremental state lives
    /// in the instance, so there is only one per repo.
    store: Option<Arc<SnapshotStore>>,
}

/// The Time Machine of every observed repo: its recovered oplog and its
/// snapshot store. Shared by the daemon loop (which adds and retires repos)
/// and the connection threads (which run protected operations).
pub struct TmRepos {
    dirs: ProfileDirs,
    repos: Mutex<Vec<TmRepo>>,
}

impl TmRepos {
    pub fn new(dirs: ProfileDirs) -> Self {
        Self {
            dirs,
            repos: Mutex::new(Vec::new()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Vec<TmRepo>> {
        self.repos.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Adds a repo with its recovered oplog; replaces an older entry.
    pub fn insert(&self, repo_id: &str, common_dir: &Path, oplog: Oplog) {
        let mut repos = self.lock();
        repos.retain(|r| r.repo_id != repo_id);
        repos.push(TmRepo {
            repo_id: repo_id.to_owned(),
            common_dir: canonical(common_dir),
            oplog: Arc::new(Mutex::new(oplog)),
            store: None,
        });
    }

    pub fn contains(&self, repo_id: &str) -> bool {
        self.lock().iter().any(|r| r.repo_id == repo_id)
    }

    /// Stops offering a retired repo. Operations already running keep their
    /// handle until they end.
    pub fn remove(&self, repo_id: &str) {
        self.lock().retain(|r| r.repo_id != repo_id);
    }

    /// Drops every repo (orderly stop).
    pub fn clear(&self) {
        self.lock().clear();
    }

    /// The oplog of an observed repo.
    pub fn oplog(&self, repo_id: &str) -> Option<Arc<Mutex<Oplog>>> {
        self.lock()
            .iter()
            .find(|r| r.repo_id == repo_id)
            .map(|r| Arc::clone(&r.oplog))
    }

    /// Key of the repo's write lock: the path of its snapshot store, the same
    /// key the Time Machine applier uses (ADR-CKP-002 § 5). Derived from the
    /// profile, so it never depends on whether the store could be opened.
    fn lock_key(&self, repo_id: &str) -> String {
        match SnapshotStore::location(&self.dirs, repo_id) {
            Ok(path) => path.display().to_string(),
            // An id the store refuses never reaches a write: keep it apart.
            Err(_) => format!("invalid-store:{repo_id}"),
        }
    }

    /// The repo whose canonical common directory is `common_dir`, with its
    /// oplog and its store (`None` if the store cannot be opened).
    fn by_common_dir(&self, common_dir: &Path) -> Option<FoundRepo> {
        let mut repos = self.lock();
        let repo = repos.iter_mut().find(|r| r.common_dir == common_dir)?;
        if repo.store.is_none() {
            repo.store = SnapshotStore::open_or_create(&self.dirs, &repo.repo_id)
                .ok()
                .map(|(store, _)| Arc::new(store));
        }
        Some((
            repo.repo_id.clone(),
            Arc::clone(&repo.oplog),
            repo.store.clone(),
        ))
    }

    /// The observed repo whose worktree root is `folder`, with the
    /// production snapshotter wrapped by `layer` in debug builds. Nothing
    /// is searched upwards, like `repo.add`.
    fn open_worktree(
        &self,
        folder: &Path,
        layer: Option<&SnapshotterLayer>,
    ) -> Result<Opened, ScopeError> {
        let reader = RepoReader::open(folder, &ReaderOptions::default())
            .map_err(|_| ScopeError::NotObserved)?;
        let worktree = canonical(&reader.workdir().ok_or(ScopeError::NotObserved)?);
        if worktree != canonical(folder) {
            return Err(ScopeError::NotObserved);
        }
        let common_dir = canonical(reader.common_dir());
        drop(reader);
        let (repo_id, oplog, store) = self
            .by_common_dir(&common_dir)
            .ok_or(ScopeError::NotObserved)?;
        let snapshotter: Arc<dyn PriorSnapshotter> = match &store {
            Some(store) => Arc::new(StoreSnapshotter {
                store: Arc::clone(store),
                oplog: Arc::clone(&oplog),
                profile: self.dirs.clone(),
            }),
            None => Arc::new(UnavailableStore),
        };
        // Fault injection is honored only in debug builds (tests).
        let snapshotter = match layer {
            Some(layer) if cfg!(debug_assertions) => layer(snapshotter),
            _ => snapshotter,
        };
        Ok(Opened {
            handle: RepoHandle {
                repo_id,
                worktree,
                oplog,
                snapshotter,
            },
            store,
            common_dir,
        })
    }
}

/// A worktree of an observed repo, opened.
struct Opened {
    handle: RepoHandle,
    store: Option<Arc<SnapshotStore>>,
    common_dir: PathBuf,
}

/// A store that could not be opened: every prior fails as "store
/// unavailable" and the operation does not run (ADR-TMC-003 § 3).
struct UnavailableStore;

impl PriorSnapshotter for UnavailableStore {
    fn prior(&self, _req: &PriorRequest) -> Result<PriorSnapshot, PriorError> {
        Err(PriorError::StoreUnavailable)
    }
}

/// The production [`ProtectedBackend`]: observed repos, the real store and
/// the catalog that was wired. The MCP allowlist admits no repo until
/// DEP-MCP-4.
pub struct DaemonBackend {
    repos: Arc<TmRepos>,
    wiring: OperationsWiring,
    allowlist: NoMcpRepos,
}

impl DaemonBackend {
    pub fn new(repos: Arc<TmRepos>, wiring: OperationsWiring) -> Self {
        Self {
            repos,
            wiring,
            allowlist: NoMcpRepos,
        }
    }
}

impl ProtectedBackend for DaemonBackend {
    /// `folder` must be the root of a worktree of an observed repo: nothing
    /// is searched upwards, like `repo.add`.
    fn repo_of(&self, folder: &Path) -> Result<RepoHandle, ScopeError> {
        self.repos
            .open_worktree(folder, self.wiring.prior_layer.as_ref())
            .map(|o| o.handle)
    }

    fn write_lock_key(&self, repo: &RepoHandle) -> String {
        self.repos.lock_key(&repo.repo_id)
    }

    fn facts(&self, repo: &RepoHandle) -> Result<RepoFacts, RejectReason> {
        RepoFacts::read(&repo.worktree)
    }

    fn plan_op(
        &self,
        operation: OperationId,
        args: &OperationArgs,
        repo: &RepoHandle,
        facts: &RepoFacts,
    ) -> Result<OpPlan, PlanError> {
        self.wiring.catalog.plan_op(operation, args, repo, facts)
    }

    fn step(&self, plan: &StepPlan<'_>) -> Result<Box<dyn ProtectedStep>, StepError> {
        self.wiring.catalog.step(plan)
    }

    fn allowlist(&self) -> &dyn McpAllowlist {
        &self.allowlist
    }
}

/// The production [`UndoBackend`]: the observed repos with their real store,
/// wired whether or not a catalog of operations is (US-TMC-002). The MCP
/// allowlist admits no repo until DEP-MCP-4.
pub struct TimeMachineBackend {
    repos: Arc<TmRepos>,
    prior_layer: Option<SnapshotterLayer>,
    allowlist: NoMcpRepos,
}

impl TimeMachineBackend {
    /// `prior_layer` wraps the snapshotter of the Time Machine's own
    /// commands; honored only in debug builds (tests).
    pub fn new(repos: Arc<TmRepos>, prior_layer: Option<SnapshotterLayer>) -> Self {
        Self {
            repos,
            prior_layer,
            allowlist: NoMcpRepos,
        }
    }
}

impl UndoBackend for TimeMachineBackend {
    fn repo_of(&self, folder: &Path) -> Result<TmRepoHandle, ScopeError> {
        let opened = self
            .repos
            .open_worktree(folder, self.prior_layer.as_ref())?;
        // The main worktree holds refs and objects: the parent of a
        // `.git` common folder. A bare repo has none.
        let main_root = opened
            .common_dir
            .parent()
            .filter(|_| opened.common_dir.ends_with(".git"))
            .map(Path::to_path_buf);
        let tm_dir = crate::timemachine::oplog::repo_dir(&self.repos.dirs, &opened.handle.repo_id)
            .map_err(|_| ScopeError::NotObserved)?;
        Ok(TmRepoHandle {
            write_lock_key: self.repos.lock_key(&opened.handle.repo_id),
            repo: opened.handle,
            store: opened.store,
            main_root,
            tm_dir,
            profile_root: self.repos.dirs.data.clone(),
        })
    }

    fn allowlist(&self) -> &dyn McpAllowlist {
        &self.allowlist
    }
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Unix only: on Windows the store fails with `Unsupported`.
/// Pendiente: etapa de validación multiplataforma (XP-12).
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// ADR-CKP-002 § 5: the executor and the applier serialize on the same
    /// key, the path of the repo's store, whether or not it is open yet.
    #[test]
    fn the_lock_key_is_the_appliers() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
        let repo_id = "0a1b2c3d-0000-4000-8000-0000000000aa";
        let repos = TmRepos::new(dirs.clone());
        let before = repos.lock_key(repo_id);
        let (store, _) = SnapshotStore::open_or_create(&dirs, repo_id).unwrap();
        // The applier's key (`apply/mod.rs`): the opened store's path.
        assert_eq!(before, store.path().display().to_string());
        assert_eq!(repos.lock_key(repo_id), before);
    }
}
