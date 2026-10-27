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

use super::scope::{McpAllowlist, McpRepos, ProtectedBackend, RepoHandle, ScopeError};
use super::{
    PriorError, PriorRequest, PriorSnapshot, PriorSnapshotter, ProtectedStep, StepError,
    StoreSnapshotter,
};
use crate::executor::{GuardrailsGate, OpPlan, PlanError, RepoFacts, StepPlan};
use crate::observe;
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
    /// The operations this catalog serves, when it says (the production one does).
    fn armed(&self) -> &[OperationId] {
        &[]
    }
    /// This catalog with the system Git of `commit`; `None` when it has no `commit` arm.
    fn with_commit_env(
        &self,
        env: crate::executor::ops::CommitEnv,
    ) -> Option<Arc<dyn OperationCatalog>> {
        let _ = env;
        None
    }
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

/// Oplog and store (`None` if it cannot be opened) of a repo.
pub(crate) type OpenedStore = (Arc<Mutex<Oplog>>, Option<Arc<SnapshotStore>>);

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
    /// What the sweep after a crash kept, for `raptor status` (DS-TS-TMC-003, Enmienda T2).
    pub kept: crate::timemachine::kept::KeptTemps,
}

impl TmRepos {
    pub fn new(dirs: ProfileDirs) -> Self {
        Self {
            dirs,
            repos: Mutex::new(Vec::new()),
            kept: Default::default(),
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
        self.kept.forget(repo_id);
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

    /// An observed repo by id, with its oplog and its store (`None` if the
    /// store cannot be opened).
    pub(crate) fn repo(&self, repo_id: &str) -> Option<OpenedStore> {
        let mut repos = self.lock();
        let repo = repos.iter_mut().find(|r| r.repo_id == repo_id)?;
        if repo.store.is_none() {
            repo.store = SnapshotStore::open_or_create(&self.dirs, &repo.repo_id)
                .ok()
                .map(|(store, _)| Arc::new(store));
        }
        Some((Arc::clone(&repo.oplog), repo.store.clone()))
    }

    /// The canonical common Git directory of an observed repo, as the registry has it: where its
    /// refs and objects are read from, never through a worktree's `.git` (#223 I-03).
    pub(crate) fn common_dir(&self, repo_id: &str) -> Option<PathBuf> {
        self.lock()
            .iter()
            .find(|r| r.repo_id == repo_id)
            .map(|r| r.common_dir.clone())
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
    ///
    /// Which repo that is comes from the repos' own word, never from the folder's `.git` (#223
    /// I-03, NFR-02): the observed repo that registers `folder` as one of its worktrees. Its
    /// `.git` must then be that repo's own ([`observe::open_registered_worktree`]); otherwise
    /// nothing is read and nothing runs.
    fn open_worktree(
        &self,
        folder: &Path,
        layer: Option<&SnapshotterLayer>,
    ) -> Result<Opened, ScopeError> {
        let worktree = canonical(folder);
        let commons: Vec<PathBuf> = self.lock().iter().map(|r| r.common_dir.clone()).collect();
        // Every observed repo that claims the folder, and of those the ones that own its `.git`:
        // another repo's claim (a `gitdir` or `core.worktree` an agent wrote there) can neither
        // take the folder nor lock its real repo out (M-02 of the review).
        let claiming: Vec<PathBuf> = commons
            .into_iter()
            .filter(|common| {
                observe::registered_worktrees(common)
                    .is_ok_and(|list| list.iter().any(|(root, _)| *root == worktree))
            })
            .collect();
        if claiming.is_empty() {
            return Err(ScopeError::NotObserved);
        }
        let owning: Vec<&PathBuf> = claiming
            .iter()
            .filter(|common| observe::open_registered_worktree(common, &worktree).is_ok())
            .collect();
        let common_dir = match owning.as_slice() {
            [one] => (*one).clone(),
            [] => return Err(ScopeError::Untrusted),
            _ => return Err(ScopeError::NotObserved),
        };
        let (repo_id, oplog, store) = self
            .by_common_dir(&common_dir)
            .ok_or(ScopeError::NotObserved)?;
        let snapshotter: Arc<dyn PriorSnapshotter> = match &store {
            Some(store) => Arc::new(StoreSnapshotter {
                store: Arc::clone(store),
                oplog: Arc::clone(&oplog),
                profile: self.dirs.clone(),
                common_dir: common_dir.clone(),
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
                common_dir: common_dir.clone(),
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
/// the catalog that was wired, and the MCP allowlist of the profile
/// (US-MCP-002).
pub struct DaemonBackend {
    repos: Arc<TmRepos>,
    wiring: OperationsWiring,
    allowlist: Arc<McpRepos>,
    /// Who else is in a worktree; `None`: unknown, so every worktree counts as shared.
    presence: Option<Presence>,
}

/// Live session ids of other agents in the worktree at a canonical root; `None` when unreadable.
pub type Presence = Arc<dyn Fn(&Path) -> Option<Vec<String>> + Send + Sync>;

/// How long a session without a process identity counts as alive after the detector last saw
/// it (its start or its latest `session-*` event).
pub const PRESENCE_TTL: Duration = Duration::from_secs(600);

impl DaemonBackend {
    pub fn new(repos: Arc<TmRepos>, wiring: OperationsWiring, allowlist: Arc<McpRepos>) -> Self {
        Self {
            repos,
            wiring,
            allowlist,
            presence: None,
        }
    }

    /// The sessions of other agents in a worktree, for the shared-worktree rule of `commit`.
    #[must_use]
    pub fn with_presence(mut self, presence: Presence) -> Self {
        self.presence = Some(presence);
        self
    }
}

/// The presence the daemon reads from its profile: the open agent sessions of the worktree in
/// the store of the observed repo that registers it, alive by [`PRESENCE_TTL`]. The detector
/// keeps no process identity for its sessions, so their life is when it last saw them.
pub fn profile_presence(dirs: ProfileDirs, repos: Arc<TmRepos>) -> Presence {
    Arc::new(move |worktree: &Path| {
        let commons: Vec<(String, PathBuf)> = repos
            .lock()
            .iter()
            .map(|r| (r.repo_id.clone(), r.common_dir.clone()))
            .collect();
        let (repo_id, _) = commons.into_iter().find(|(_, common)| {
            observe::registered_worktrees(common)
                .is_ok_and(|list| list.iter().any(|(root, _)| root == worktree))
        })?;
        let db = dirs.repos_dir().join(format!("{repo_id}.sqlite"));
        live_in_store(&db, worktree, crate::daemon::now_ms())
    })
}

/// Open sessions of `worktree` in the repo database at `db` seen within [`PRESENCE_TTL`] of
/// `now_ms`. Read only; `None` when it cannot be read.
fn live_in_store(db: &Path, worktree: &Path, now_ms: i64) -> Option<Vec<String>> {
    use rusqlite::{Connection, OpenFlags};
    if !db.is_file() {
        return None;
    }
    let conn = Connection::open_with_flags(
        db,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    let mut stmt = conn
        .prepare(
            "SELECT s.session_id, s.started_ms,
                 (SELECT MAX(e.observed_utc_ms) FROM events e WHERE e.session_id = s.session_id)
             FROM sessions s JOIN worktrees w ON w.id = s.worktree_id
             WHERE w.canonical_path = ?1 AND s.end_cause IS NULL",
        )
        .ok()?;
    let ttl = i64::try_from(PRESENCE_TTL.as_millis()).unwrap_or(i64::MAX);
    let rows = stmt
        .query_map([worktree.to_str()?], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<i64>>(2)?,
            ))
        })
        .ok()?;
    let mut live = Vec::new();
    for row in rows {
        let (id, started, seen) = row.ok()?;
        let last = seen.map_or(started, |s| s.max(started));
        if now_ms.saturating_sub(last) <= ttl {
            live.push(id);
        }
    }
    Some(live)
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

    /// Read only while the repo registers the worktree and owns its `.git` (#223 I-03): the
    /// executor reads the facts again under the write lock before it runs, so a `.git` swapped
    /// after the plan is refused there too, before anything is written (NFR-01).
    fn facts(&self, repo: &RepoHandle) -> Result<RepoFacts, RejectReason> {
        let opened = observe::open_registered_worktree(&repo.common_dir, &repo.worktree)
            .map_err(|_| RejectReason::RepoIdentityChanged)?;
        let git_dir = canonical(opened.git_dir());
        drop(opened);
        let facts = RepoFacts::read(&repo.worktree)?;
        // Both Git folders the facts were read from must be the ones the gate opened (L-01 of the
        // review). Git itself resolves the `.git` again when it runs: that last window is the
        // write layer's, which checks the identity under its own locks.
        if facts.common_dir != repo.common_dir || canonical(&facts.git_dir) != git_dir {
            return Err(RejectReason::RepoIdentityChanged);
        }
        Ok(facts)
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
        self.allowlist.as_ref()
    }

    /// Fails closed: an unknown or unreadable presence is a shared worktree.
    fn live_sessions(&self, repo: &RepoHandle) -> Result<Vec<String>, RejectReason> {
        self.presence
            .as_ref()
            .and_then(|presence| presence(&repo.worktree))
            .ok_or(RejectReason::SharedWorktreeNeedsPaths)
    }
}

/// The production [`UndoBackend`]: the observed repos with their real store,
/// wired whether or not a catalog of operations is (US-TMC-002), and the MCP
/// allowlist of the profile (US-MCP-002).
pub struct TimeMachineBackend {
    repos: Arc<TmRepos>,
    prior_layer: Option<SnapshotterLayer>,
    allowlist: Arc<McpRepos>,
}

impl TimeMachineBackend {
    /// `prior_layer` wraps the snapshotter of the Time Machine's own
    /// commands; honored only in debug builds (tests).
    pub fn new(
        repos: Arc<TmRepos>,
        prior_layer: Option<SnapshotterLayer>,
        allowlist: Arc<McpRepos>,
    ) -> Self {
        Self {
            repos,
            prior_layer,
            allowlist,
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
        self.allowlist.as_ref()
    }
}

fn canonical(path: &Path) -> PathBuf {
    // Drive form on Windows, as the observed roots are kept and the applier requires.
    gitraptor_git::paths::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
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

    /// A repo database with the tables the presence reads: an open session seen a minute ago,
    /// an ended one, an open one not seen for longer than the TTL (its agent died), and one of
    /// another worktree.
    fn sessions_db(dir: &Path, now: i64) -> PathBuf {
        let db = dir.join("r.sqlite");
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE worktrees (id INTEGER PRIMARY KEY, canonical_path TEXT);
             CREATE TABLE sessions (session_id TEXT, worktree_id INTEGER, started_ms INTEGER,
                 end_cause TEXT);
             CREATE TABLE events (session_id TEXT, observed_utc_ms INTEGER);
             INSERT INTO worktrees VALUES (1, '/w/a'), (2, '/w/b');",
        )
        .unwrap();
        let old = now - 2 * i64::try_from(PRESENCE_TTL.as_millis()).unwrap();
        for (id, wt, started, end) in [
            ("live", 1, old, None),
            ("ended", 1, now, Some("process-gone")),
            ("stale", 1, old, None),
            ("elsewhere", 2, now, None),
        ] {
            conn.execute(
                "INSERT INTO sessions VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, wt, started, end],
            )
            .unwrap();
        }
        conn.execute(
            "INSERT INTO events VALUES ('live', ?1), ('stale', ?2)",
            rusqlite::params![now - 60_000, old],
        )
        .unwrap();
        db
    }

    #[test]
    fn the_profile_presence_counts_only_open_sessions_seen_within_the_ttl() {
        let tmp = tempfile::tempdir().unwrap();
        let now = 10_000_000_000;
        let db = sessions_db(tmp.path(), now);
        assert_eq!(
            live_in_store(&db, Path::new("/w/a"), now),
            Some(vec!["live".to_owned()]),
            "an ended session and one not seen within the TTL do not count"
        );
        assert_eq!(live_in_store(&db, Path::new("/w/c"), now), Some(Vec::new()));
    }

    #[test]
    fn an_unreadable_presence_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("missing.sqlite");
        assert_eq!(live_in_store(&missing, Path::new("/w/a"), 0), None);
        let bad = tmp.path().join("bad.sqlite");
        std::fs::write(&bad, b"not a database").unwrap();
        assert_eq!(live_in_store(&bad, Path::new("/w/a"), 0), None);
    }
}
