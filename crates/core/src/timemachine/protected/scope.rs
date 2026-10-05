//! Scope of a request (SEC-TMC-07, SEC-TMC-15, NFR-02).
//!
//! Over MCP the repo and the worktree come from the caller's working
//! folder, never from a parameter, and an undo or a restore needs the repo
//! in the MCP allowlist. An id that belongs to another repo answers exactly
//! like an unknown one: each repo has its own oplog, and lookups only ever
//! go through the caller's.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gitraptor_api::timemachine::RequestChannel;

use super::{PriorSnapshotter, ProtectedStep, StepError};
use crate::timemachine::oplog::{OperationView, Oplog, SnapshotView};

/// Why a scope was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeError {
    /// The caller's working folder cannot be read (MCP).
    NoWorkingFolder,
    /// The folder is in no observed repo.
    NotObserved,
    /// The repo is not in the MCP allowlist.
    NotAllowlisted,
    /// "Unattributed" over MCP may not undo or restore (TQ-7 → a).
    UnattributedOverMcp,
    /// The operation declared a worktree that is not one of its repo's.
    ForeignWorktree,
}

impl ScopeError {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoWorkingFolder => "the caller's working folder cannot be read",
            Self::NotObserved => "not in an observed repo",
            Self::NotAllowlisted => "the repo is not in the MCP allowlist",
            Self::UnattributedOverMcp => "an unattributed requester cannot do this over MCP",
            Self::ForeignWorktree => "the operation's scope is outside its repo",
        }
    }
}

/// The repos `raptor-mcp` may act on (DEP-MCP-4). Until its store exists,
/// the production list admits none.
pub trait McpAllowlist: Send + Sync {
    fn allows(&self, repo_id: &str) -> bool;
}

/// The production allowlist until DEP-MCP-4: empty (fail-closed).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoMcpRepos;

impl McpAllowlist for NoMcpRepos {
    fn allows(&self, _repo_id: &str) -> bool {
        false
    }
}

/// One repo, ready for protected operations.
#[derive(Clone)]
pub struct RepoHandle {
    pub repo_id: String,
    /// The worktree the request resolved to.
    pub worktree: PathBuf,
    pub oplog: Arc<Mutex<Oplog>>,
    pub snapshotter: Arc<dyn PriorSnapshotter>,
}

impl std::fmt::Debug for RepoHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepoHandle")
            .field("repo_id", &self.repo_id)
            .field("worktree", &self.worktree)
            .finish_non_exhaustive()
    }
}

/// What the daemon needs from the rest of the engine to run protected
/// operations: the repo of a worktree, the executor of catalog operations
/// (F-001-02/05, ADR-CKP-002) and the MCP allowlist. Production wiring
/// arrives with the executor; tests inject doubles.
pub trait ProtectedBackend: Send + Sync {
    /// The observed repo that contains `folder`.
    fn repo_of(&self, folder: &Path) -> Result<RepoHandle, ScopeError>;
    /// The step that runs catalog operation `operation` with `args` on
    /// `repo`. Validates both against the catalog; nothing runs yet.
    fn step(
        &self,
        operation: &str,
        args: &serde_json::Map<String, serde_json::Value>,
        repo: &RepoHandle,
    ) -> Result<Box<dyn ProtectedStep>, StepError>;
    fn allowlist(&self) -> &dyn McpAllowlist;
}

/// The repo for a request: over MCP from the caller's working folder and
/// only if allowlisted; otherwise from the folder the client named.
pub fn scope_for(
    backend: &dyn ProtectedBackend,
    channel: RequestChannel,
    named: Option<&Path>,
    caller_cwd: Option<&Path>,
) -> Result<RepoHandle, ScopeError> {
    if channel == RequestChannel::Mcp {
        let cwd = caller_cwd.ok_or(ScopeError::NoWorkingFolder)?;
        let repo = backend.repo_of(cwd)?;
        if !backend.allowlist().allows(&repo.repo_id) {
            return Err(ScopeError::NotAllowlisted);
        }
        return Ok(repo);
    }
    backend.repo_of(named.ok_or(ScopeError::NotObserved)?)
}

/// Over MCP an unattributed requester may not undo or restore, checked
/// before anything is computed (ADR-TMC-005 § 1, TQ-7 → a).
pub fn require_attributed(channel: RequestChannel, is_agent: bool) -> Result<(), ScopeError> {
    if channel == RequestChannel::Mcp && !is_agent {
        return Err(ScopeError::UnattributedOverMcp);
    }
    Ok(())
}

/// A snapshot of the caller's repo, or `None`: an id of another repo is
/// "not found", like an unknown one (SEC-TMC-07).
pub fn snapshot_in(repo: &RepoHandle, snapshot_id: &str) -> Option<SnapshotView> {
    let oplog = repo.oplog.lock().unwrap_or_else(|e| e.into_inner());
    oplog.snapshot(snapshot_id).ok().flatten()
}

/// An operation of the caller's repo, or `None` (SEC-TMC-07).
pub fn operation_in(repo: &RepoHandle, operation_id: &str) -> Option<OperationView> {
    let oplog = repo.oplog.lock().unwrap_or_else(|e| e.into_inner());
    oplog.operation(operation_id).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::ProfileDirs;
    use crate::timemachine::oplog::{CompleteInfo, NewSnapshot, SnapshotLevel};
    use crate::timemachine::protected::{PriorError, PriorRequest, PriorSnapshot};

    struct NoSnap;
    impl PriorSnapshotter for NoSnap {
        fn prior(&self, _req: &PriorRequest) -> Result<PriorSnapshot, PriorError> {
            Err(PriorError::StoreUnavailable)
        }
    }

    struct Allow(&'static str);
    impl McpAllowlist for Allow {
        fn allows(&self, repo_id: &str) -> bool {
            repo_id == self.0
        }
    }

    struct Backend {
        repos: Vec<RepoHandle>,
        allow: Allow,
    }

    impl ProtectedBackend for Backend {
        fn repo_of(&self, folder: &Path) -> Result<RepoHandle, ScopeError> {
            self.repos
                .iter()
                .find(|r| folder.starts_with(&r.worktree))
                .cloned()
                .ok_or(ScopeError::NotObserved)
        }
        fn step(
            &self,
            _operation: &str,
            _args: &serde_json::Map<String, serde_json::Value>,
            _repo: &RepoHandle,
        ) -> Result<Box<dyn ProtectedStep>, StepError> {
            Err(StepError::new("unused"))
        }
        fn allowlist(&self) -> &dyn McpAllowlist {
            &self.allow
        }
    }

    const A: &str = "0a1b2c3d-0000-4000-8000-0000000000aa";
    const B: &str = "0a1b2c3d-0000-4000-8000-0000000000bb";

    fn backend(dirs: &ProfileDirs) -> Backend {
        let handle = |id: &str, wt: &str| RepoHandle {
            repo_id: id.into(),
            worktree: PathBuf::from(wt),
            oplog: Arc::new(Mutex::new(Oplog::open(dirs, id, 1_000).unwrap().0)),
            snapshotter: Arc::new(NoSnap),
        };
        Backend {
            repos: vec![handle(A, "/repos/a"), handle(B, "/repos/b")],
            allow: Allow(A),
        }
    }

    #[test]
    fn mcp_needs_the_callers_folder_and_the_allowlist() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(&ProfileDirs::under_root(tmp.path().join("p")));
        let mcp = RequestChannel::Mcp;
        let named = Some(Path::new("/repos/a"));
        // A named folder means nothing over MCP: only the caller's cwd.
        assert_eq!(
            scope_for(&b, mcp, named, None).unwrap_err(),
            ScopeError::NoWorkingFolder
        );
        assert_eq!(
            scope_for(&b, mcp, None, Some(Path::new("/repos/b/src"))).unwrap_err(),
            ScopeError::NotAllowlisted
        );
        assert_eq!(
            scope_for(&b, mcp, None, Some(Path::new("/elsewhere"))).unwrap_err(),
            ScopeError::NotObserved
        );
        let a = scope_for(&b, mcp, None, Some(Path::new("/repos/a/src"))).unwrap();
        assert_eq!(a.repo_id, A);
        // The CLI names its worktree; no allowlist.
        let cli = scope_for(&b, RequestChannel::Cli, Some(Path::new("/repos/b")), None).unwrap();
        assert_eq!(cli.repo_id, B);
        assert!(!NoMcpRepos.allows(A));
        assert_eq!(
            require_attributed(mcp, false),
            Err(ScopeError::UnattributedOverMcp)
        );
        assert!(require_attributed(mcp, true).is_ok());
        assert!(require_attributed(RequestChannel::Cli, false).is_ok());
    }

    #[test]
    fn an_id_of_another_repo_is_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let b = backend(&ProfileDirs::under_root(tmp.path().join("p")));
        let (a, other) = (&b.repos[0], &b.repos[1]);
        let id = {
            let mut log = other.oplog.lock().unwrap();
            let new = NewSnapshot {
                level: SnapshotLevel::Observation,
                worktrees: vec!["/repos/b".into()],
                engine_mark: None,
                cause_operation: None,
                cause_event_seq: None,
            };
            let id = log.begin_snapshot(&new, 1).unwrap();
            log.complete_snapshot(&id, &CompleteInfo::default(), 1)
                .unwrap();
            id
        };
        assert!(snapshot_in(other, &id).is_some());
        assert!(snapshot_in(a, &id).is_none());
        assert!(operation_in(a, &id).is_none());
    }
}
