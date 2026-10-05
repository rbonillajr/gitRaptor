//! The daemon's repo layer for protected operations (US-TMC-001): the
//! observed repos with their oplog and snapshot store, and the hook where
//! the catalog of user operations plugs in.
//!
//! The catalog and its executor are TS-CKP-002 (ADR-CKP-002); they are not
//! built here. Until one is wired, the daemon answers `operation.run` with
//! "not implemented".

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use gitraptor_git::{ReaderOptions, RepoReader};

use super::scope::{McpAllowlist, NoMcpRepos, ProtectedBackend, RepoHandle, ScopeError};
use super::{
    PriorError, PriorRequest, PriorSnapshot, PriorSnapshotter, ProtectedStep, StepError,
    StoreSnapshotter,
};
use crate::profile::ProfileDirs;
use crate::timemachine::oplog::Oplog;
use crate::timemachine::store::SnapshotStore;

/// The catalog of user operations (ADR-CKP-002 § 1): turns an operation
/// name and its arguments into the step that runs it. Implemented by the
/// executor of TS-CKP-002; tests plug in their own.
pub trait OperationCatalog: Send + Sync {
    /// The step for `operation` with `args` on `repo`, validated against the
    /// catalog. Nothing runs yet.
    fn step(
        &self,
        operation: &str,
        args: &serde_json::Map<String, serde_json::Value>,
        repo: &RepoHandle,
    ) -> Result<Box<dyn ProtectedStep>, StepError>;
}

/// A layer over the production snapshotter. Tests inject faults with it
/// (no space, for example); release builds ignore it.
pub type SnapshotterLayer =
    Arc<dyn Fn(Arc<dyn PriorSnapshotter>) -> Arc<dyn PriorSnapshotter> + Send + Sync>;

/// What the daemon needs to run user operations.
#[derive(Clone)]
pub struct OperationsWiring {
    pub catalog: Arc<dyn OperationCatalog>,
    /// Deadline of the prior snapshot.
    pub prior_deadline: Duration,
    pub prior_layer: Option<SnapshotterLayer>,
}

impl std::fmt::Debug for OperationsWiring {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OperationsWiring")
            .field("prior_deadline", &self.prior_deadline)
            .field("prior_layer", &self.prior_layer.is_some())
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

    /// The repo whose canonical common directory is `common_dir`, with its
    /// oplog and its store (`None` if the store cannot be opened).
    fn by_common_dir(
        &self,
        common_dir: &Path,
    ) -> Option<FoundRepo> {
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
        let reader = RepoReader::open(folder, &ReaderOptions::default())
            .map_err(|_| ScopeError::NotObserved)?;
        let worktree = canonical(&reader.workdir().ok_or(ScopeError::NotObserved)?);
        if worktree != canonical(folder) {
            return Err(ScopeError::NotObserved);
        }
        let common_dir = canonical(reader.common_dir());
        drop(reader);
        let (repo_id, oplog, store) = self
            .repos
            .by_common_dir(&common_dir)
            .ok_or(ScopeError::NotObserved)?;
        let snapshotter: Arc<dyn PriorSnapshotter> = match store {
            Some(store) => Arc::new(StoreSnapshotter {
                store,
                oplog: Arc::clone(&oplog),
                profile: self.repos.dirs.clone(),
            }),
            None => Arc::new(UnavailableStore),
        };
        // Fault injection is honored only in debug builds (tests).
        let snapshotter = match &self.wiring.prior_layer {
            Some(layer) if cfg!(debug_assertions) => layer(snapshotter),
            _ => snapshotter,
        };
        Ok(RepoHandle {
            repo_id,
            worktree,
            oplog,
            snapshotter,
        })
    }

    fn step(
        &self,
        operation: &str,
        args: &serde_json::Map<String, serde_json::Value>,
        repo: &RepoHandle,
    ) -> Result<Box<dyn ProtectedStep>, StepError> {
        self.wiring.catalog.step(operation, args, repo)
    }

    fn allowlist(&self) -> &dyn McpAllowlist {
        &self.allowlist
    }
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}
