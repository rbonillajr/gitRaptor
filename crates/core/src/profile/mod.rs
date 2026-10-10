//! Engine data store in the user's profile (TS-GRP-001, ADR-GRP-006).
//!
//! The profile is the only place outside the repo the engine writes to
//! (Q17). It holds a global index of observed repos and one SQLite store per
//! repo, so a corrupt store only affects its own repo. The daemon is the
//! single writer (ADR-GRP-005); clients never open these files.

mod dirs;
mod error;
pub(crate) mod fsperm;
mod guard_log;
mod guard_store;
mod index;
mod repo_key;
mod schema;
pub mod settings;
pub(crate) mod sqlite;
mod store;
mod team_baseline;

use std::path::{Path, PathBuf};

pub use dirs::{APP_DIR, PROFILE_DIR_ENV, ProfileDirs};
pub use error::{ProfileError, Result};
pub use fsperm::{create_private_file, set_restrictive_umask};
pub use guard_store::GuardKeys;
pub use index::{
    AddOutcome, AuditRow, DaemonRun, DiscoveryCandidate, DiscoveryRoot, RepoEntry, RepoState,
    read_only_repos,
};
pub use repo_key::{NormalizedPath, normalize_common_dir, validate_input_path};
pub use store::{
    Agent, AgentKind, AttributionRecord, Author, BatchResult, EndCause, Event, Gap, GapCause,
    KnownState, NewEvent, Origin, RecordKind, RepoStore, Session, Timestamp, Worktree, WriteOp,
};

use index::Index;

/// File name of the global index inside the data folder.
pub const INDEX_FILE: &str = "index.sqlite";

/// What happened while opening the profile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpenReport {
    /// The profile (global index) was created by this open. A recreated
    /// profile has a new instance id.
    pub created: bool,
    /// The global index was corrupt and was moved here; every repo starts
    /// as a lost profile (Q26).
    pub quarantined_index: Option<PathBuf>,
}

/// Result of opening a per-repo store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreOpen {
    /// The store existed and passed the integrity check.
    Existing,
    /// No store existed yet: first observation of the repo.
    Created,
    /// The store was corrupt: it was moved to `quarantined` and a new, empty
    /// store replaces it. The repo starts as a lost profile (Q26); the
    /// caller opens the matching gap.
    Recovered { quarantined: PathBuf },
}

/// The open profile. Owned by the daemon, the single writer.
pub struct Profile {
    dirs: ProfileDirs,
    index: Index,
}

impl Profile {
    /// Creates or verifies the profile folders and opens the global index.
    ///
    /// Sets the process umask to 077 first. Fails with
    /// [`ProfileError::InsecureDir`] if a pre-existing folder is not private
    /// to the current user, and with [`ProfileError::SchemaTooNew`] if the
    /// index comes from a newer binary.
    pub fn open(dirs: ProfileDirs) -> Result<(Self, OpenReport)> {
        set_restrictive_umask();
        for dir in dirs.owned_dirs() {
            fsperm::ensure_private_dir(dir)?;
        }
        let mut report = OpenReport::default();

        let opened = sqlite::open_db(
            &dirs.data.join(INDEX_FILE),
            schema::INDEX_MIGRATIONS,
            &dirs.quarantine_dir(),
        )?;
        report.created = opened.fresh;
        report.quarantined_index = opened.quarantined;
        let index = Index::from_conn(opened.conn)?;
        Ok((Self { dirs, index }, report))
    }

    pub fn dirs(&self) -> &ProfileDirs {
        &self.dirs
    }

    /// Opaque id generated when the profile is created and presented in the
    /// channel handshake (ADR-GRP-006 § 4, Guardrails amendment).
    pub fn instance_id(&self) -> &str {
        self.index.instance_id()
    }

    /// Adds the repo whose Git common directory is `common_dir`. All its
    /// worktrees share that directory and therefore the key. Adding a
    /// retired repo recovers its key and data (Q25).
    pub fn add_repo(
        &mut self,
        common_dir: &Path,
        root_commit_hint: Option<&str>,
        now_ms: i64,
    ) -> Result<(RepoEntry, AddOutcome)> {
        let normalized = normalize_common_dir(common_dir)?;
        self.index.add(&normalized, root_commit_hint, now_ms)
    }

    /// Stops observing a repo. Its store and data are kept.
    pub fn retire_repo(&mut self, repo_id: &str, now_ms: i64) -> Result<()> {
        self.index.retire(repo_id, now_ms)
    }

    /// Puts (`enabled`) or takes the MCP mark of an observed repo
    /// (US-MCP-002). `None`: the repo is not observed; `Some(changed)`.
    pub fn set_mcp_enabled(
        &mut self,
        repo_id: &str,
        enabled: bool,
        by: &str,
        now_ms: i64,
    ) -> Result<Option<bool>> {
        self.index.set_mcp_enabled(repo_id, enabled, by, now_ms)
    }

    /// The observed repos in the MCP allowlist (US-MCP-002).
    pub fn mcp_enabled_repos(&self) -> Result<Vec<String>> {
        self.index.mcp_enabled()
    }

    /// Last recorded run of the daemon (ADR-GRP-005 § 4, SEC-13).
    pub fn daemon_run(&self) -> Result<DaemonRun> {
        self.index.daemon_run()
    }

    /// Records that a daemon is running. If it dies without
    /// [`Profile::mark_daemon_stopped`], the next start sees a crash.
    pub fn mark_daemon_running(&mut self, started_ms: i64) -> Result<()> {
        self.index.set_daemon_running(started_ms)
    }

    /// Records an orderly stop with its cause and, when a command caused
    /// it, the client that asked for it (SEC-13).
    pub fn mark_daemon_stopped(
        &mut self,
        stopped_ms: i64,
        cause: &str,
        requested_by: Option<&str>,
    ) -> Result<()> {
        self.index
            .set_daemon_stopped(stopped_ms, cause, requested_by)
    }

    /// Appends one attempt of a reserved command to the audit. The audit
    /// cannot be updated or deleted (SEC-03).
    pub fn append_audit(&mut self, row: &AuditRow) -> Result<i64> {
        self.index.append_audit(row)
    }

    /// Audit entries with an id greater than `after_id`, oldest first.
    pub fn audit(&self, after_id: i64, limit: u32) -> Result<Vec<(i64, AuditRow)>> {
        self.index.audit(after_id, limit, false)
    }

    /// [`Profile::audit`] of only the outcomes `accepted`, `rejected` and `not-implemented`,
    /// chosen before the `limit` applies: what a client without `audit.outcomes` reads.
    pub fn audit_legacy(&self, after_id: i64, limit: u32) -> Result<Vec<(i64, AuditRow)>> {
        self.index.audit(after_id, limit, true)
    }

    /// Every repo of the index, observed or retired.
    pub fn repos(&self) -> Result<Vec<RepoEntry>> {
        self.index.all()
    }

    pub fn repo(&self, repo_id: &str) -> Result<Option<RepoEntry>> {
        self.index.get(repo_id)
    }

    /// Looks a repo up by its Git common directory (any worktree's view).
    pub fn repo_by_common_dir(&self, common_dir: &Path) -> Result<Option<RepoEntry>> {
        let normalized = normalize_common_dir(common_dir)?;
        self.index.by_key(&normalized.key_path)
    }

    /// The declared discovery roots (US-GRP-020).
    pub fn discovery_roots(&self) -> Result<Vec<DiscoveryRoot>> {
        self.index.discovery_roots()
    }

    /// Declares a discovery root, canonical; `false` if it already was.
    pub fn add_discovery_root(&mut self, path: &str, broad: bool, now_ms: i64) -> Result<bool> {
        self.index.add_discovery_root(path, broad, now_ms)
    }

    /// Removes a root and its pending candidates; `None` if it was not
    /// declared (US-GRP-022).
    pub fn remove_discovery_root(&mut self, path: &str) -> Result<Option<u32>> {
        self.index.remove_discovery_root(path)
    }

    /// The discovered repos waiting for the developer's decision.
    pub fn discovery_candidates(&self) -> Result<Vec<DiscoveryCandidate>> {
        self.index.discovery_candidates()
    }

    /// Replaces the candidates of `root` with a listing's repos, `(path,
    /// key_path)`, and returns the new ones (see [`DiscoveryCandidate`]).
    pub fn sync_discovery_candidates(
        &mut self,
        root: &str,
        found: &[(String, String)],
        now_ms: i64,
    ) -> Result<Vec<DiscoveryCandidate>> {
        self.index.sync_candidates(root, found, now_ms)
    }

    /// Dismisses a candidate by its path, for good (US-GRP-022); `false` if
    /// it is not a candidate.
    pub fn dismiss_discovery_candidate(&mut self, path: &str, now_ms: i64) -> Result<bool> {
        self.index.dismiss_candidate(path, now_ms)
    }

    /// The repo with this key was added: no longer a candidate nor dismissed.
    pub fn forget_discovered_key(&mut self, key_path: &str) -> Result<()> {
        self.index.forget_discovered_key(key_path)
    }

    /// Removes the candidate at `path` (it no longer exists).
    pub fn forget_discovery_candidate(&mut self, path: &str) -> Result<bool> {
        self.index.forget_candidate(path)
    }

    /// Path of the SQLite store of a repo.
    pub fn store_path(&self, repo_id: &str) -> PathBuf {
        self.dirs.repos_dir().join([repo_id, ".sqlite"].concat())
    }

    /// Opens (or creates) the store of a repo, checking its integrity. A
    /// corrupt store is set aside without touching any other repo; a store
    /// from a newer binary is left untouched and reported as
    /// [`ProfileError::SchemaTooNew`].
    pub fn open_store(&self, repo_id: &str) -> Result<(RepoStore, StoreOpen)> {
        let entry = self
            .index
            .get(repo_id)?
            .ok_or_else(|| ProfileError::UnknownRepo(repo_id.to_owned()))?;
        let path = self.store_path(&entry.repo_id);
        let existed = path.exists();
        let opened = sqlite::open_db(&path, schema::STORE_MIGRATIONS, &self.dirs.quarantine_dir())?;
        let status = match opened.quarantined {
            Some(quarantined) => StoreOpen::Recovered { quarantined },
            None if !existed => StoreOpen::Created,
            None => StoreOpen::Existing,
        };
        let store = RepoStore::from_conn(opened.conn, &entry.repo_id, &entry.canonical_path)?;
        Ok((store, status))
    }
}

#[cfg(test)]
mod tests {
    /// SQL is always parameterized (SEC-06). Query helpers only accept
    /// `&'static str`, and this check fails if a file with SQL uses
    /// `format!`, the usual way of splicing values into a statement.
    #[test]
    fn sql_is_never_built_with_format() {
        let sql_files = [
            ("index.rs", include_str!("index.rs")),
            ("schema.rs", include_str!("schema.rs")),
            ("sqlite.rs", include_str!("sqlite.rs")),
            ("store.rs", include_str!("store.rs")),
        ];
        let banned = ["format", "!("].concat();
        for (name, source) in sql_files {
            for (n, line) in source.lines().enumerate() {
                assert!(
                    !line.contains(&banned),
                    "{name}:{}: SQL files must not use format!; bind parameters instead",
                    n + 1
                );
            }
        }
    }
}
