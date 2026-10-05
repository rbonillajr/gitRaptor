//! Reads of the Guardrails hook layer (ADR-GRD-002 § 1 and § 4, H-05): whether a push is a
//! fast-forward, read without replacement objects, grafts or the commit-graph, and the repo
//! facts the decision depends on (refs backend, case folding).

use crate::{ReadError, RepoReader};

/// Commits walked before giving up on an ancestry check: past it the push is treated as
/// forced (fail-closed).
pub const MAX_ANCESTRY_WALK: usize = 2_000_000;

/// Whether the remote tip of a push is an ancestor of the pushed commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ancestry {
    FastForward,
    NotAncestor,
    /// The remote object is not in the local object database (H-05: treated as forced).
    RemoteMissing,
    /// The repo is a shallow clone: a fast-forward cannot be proven (`historia-superficial`).
    Shallow,
}

/// The refs backend of the repo (`extensions.refStorage`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefStorage {
    Files,
    Reftable,
}

fn object_id(hex: &str) -> Result<gix::ObjectId, ReadError> {
    gix::ObjectId::from_hex(hex.as_bytes())
        .map_err(|_| ReadError::InvalidInput("invalid object id".into()))
}

impl RepoReader {
    /// Whether `remote` (the tip the remote has) is an ancestor of `local` (the pushed commit).
    /// Replacement objects are already ignored by [`RepoReader::open`]; gitoxide does not read
    /// `info/grafts`; the commit-graph is not used, so a forged one cannot fake the parents.
    pub fn push_ancestry(&self, remote: &str, local: &str) -> Result<Ancestry, ReadError> {
        let (remote, local) = (object_id(remote)?, object_id(local)?);
        if self.repo.is_shallow() {
            return Ok(Ancestry::Shallow);
        }
        match self.repo.try_find_object(remote) {
            Ok(Some(object)) if object.kind == gix::object::Kind::Commit => {}
            Ok(_) => return Ok(Ancestry::RemoteMissing),
            Err(e) => return Err(ReadError::Unavailable(format!("object: {e}"))),
        }
        if remote == local {
            return Ok(Ancestry::FastForward);
        }
        let walk = self
            .repo
            .rev_walk([local])
            .use_commit_graph(false)
            .all()
            .map_err(|e| ReadError::Unavailable(format!("rev-walk: {e}")))?;
        for (n, info) in walk.enumerate() {
            if n >= MAX_ANCESTRY_WALK {
                return Ok(Ancestry::NotAncestor);
            }
            let info = info.map_err(|e| ReadError::Unavailable(format!("rev-walk: {e}")))?;
            if info.id == remote {
                return Ok(Ancestry::FastForward);
            }
        }
        Ok(Ancestry::NotAncestor)
    }

    /// The refs backend from the repo's own configuration.
    pub fn ref_storage(&self) -> RefStorage {
        let config = self.repo.config_snapshot();
        match config.string("extensions.refStorage") {
            Some(v) if v.eq_ignore_ascii_case(b"reftable") => RefStorage::Reftable,
            _ => RefStorage::Files,
        }
    }

    /// `core.ignoreCase`: the file system of the repo does not distinguish case, so branch
    /// names are folded before comparing them (SEC-GRD-18).
    pub fn ignores_case(&self) -> bool {
        self.repo
            .config_snapshot()
            .boolean("core.ignoreCase")
            .unwrap_or(false)
    }

    /// `core.hooksPath` as the repo's own configuration says (no global or system level when
    /// opened isolated): whether the activation key is still the one the journal recorded.
    pub fn hooks_path(&self) -> Option<String> {
        self.repo
            .config_snapshot()
            .string("core.hooksPath")
            .map(|v| v.to_string())
    }
}
