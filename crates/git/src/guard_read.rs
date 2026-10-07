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

/// Whether a ref update `old → new` is the shape of one new commit (DS-US-GRD-018 § 5.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitShape {
    /// One new commit: its first parent is `old` (commit, merge), its parents are those of
    /// `old` (amend), or it is a root commit on a new ref. `message` is `None` when it is
    /// larger than the limit asked for.
    One { message: Option<Vec<u8>> },
    /// Anything else: no commit, several, or not a commit.
    Other,
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

    /// The shape of the update `old → new` (`old = None` for a new ref), read without
    /// replacement objects or the commit-graph, and the raw message of the new commit when it is
    /// one and at most `max_message` bytes long. A commit any other ref (outside `updated`)
    /// already reaches is not new: a fast-forward or a reset to it is [`CommitShape::Other`].
    pub fn commit_shape(
        &self,
        old: Option<&str>,
        new: &str,
        updated: &[&str],
        max_message: usize,
    ) -> Result<CommitShape, ReadError> {
        let new = object_id(new)?;
        let old = old.map(object_id).transpose()?;
        let parents = |id: gix::ObjectId| -> Result<Option<Vec<gix::ObjectId>>, ReadError> {
            match self.repo.try_find_object(id) {
                Ok(Some(object)) if object.kind == gix::object::Kind::Commit => Ok(Some(
                    object
                        .into_commit()
                        .parent_ids()
                        .map(|p| p.detach())
                        .collect(),
                )),
                Ok(_) => Ok(None),
                Err(e) => Err(ReadError::Unavailable(format!("object: {e}"))),
            }
        };
        let Some(new_parents) = parents(new)? else {
            return Ok(CommitShape::Other);
        };
        let one = match old {
            None => new_parents.is_empty(),
            Some(old) if old == new => false,
            Some(old) => {
                new_parents.first() == Some(&old) || parents(old)?.is_some_and(|p| p == new_parents)
            }
        };
        if !one {
            return Ok(CommitShape::Other);
        }
        let commit = self
            .repo
            .find_commit(new)
            .map_err(|e| ReadError::Unavailable(format!("commit: {e}")))?;
        if self.reached_by_other_refs(&commit, updated)? {
            return Ok(CommitShape::Other);
        }
        let message = commit
            .message_raw()
            .map_err(|e| ReadError::Unavailable(format!("commit: {e}")))?;
        Ok(CommitShape::One {
            message: (message.len() <= max_message).then(|| message.to_vec()),
        })
    }

    /// Whether a ref outside `updated` already reaches `commit`. The walk stops at commits older
    /// than it and after [`MAX_ANCESTRY_WALK`] commits: past either, "not reached", so the commit
    /// is evaluated (fail-closed).
    fn reached_by_other_refs(
        &self,
        commit: &gix::Commit<'_>,
        updated: &[&str],
    ) -> Result<bool, ReadError> {
        let unavailable = |e: &dyn std::fmt::Display| ReadError::Unavailable(format!("refs: {e}"));
        let platform = self.repo.references().map_err(|e| unavailable(&e))?;
        let mut tips = Vec::new();
        for reference in platform.all().map_err(|e| unavailable(&e))? {
            let mut reference = reference.map_err(|e| unavailable(&e))?;
            let name = reference.name().as_bstr().to_string();
            if updated.contains(&name.as_str()) {
                continue;
            }
            let Ok(id) = reference.peel_to_id() else {
                continue;
            };
            let id = id.detach();
            if id == commit.id {
                return Ok(true);
            }
            if self
                .repo
                .try_find_object(id)
                .ok()
                .flatten()
                .is_some_and(|o| o.kind == gix::object::Kind::Commit)
            {
                tips.push(id);
            }
        }
        if tips.is_empty() {
            return Ok(false);
        }
        let seconds = commit.time().map_err(|e| unavailable(&e))?.seconds;
        let walk = self
            .repo
            .rev_walk(tips)
            .use_commit_graph(false)
            .sorting(gix::revision::walk::Sorting::ByCommitTimeCutoff {
                order: gix::traverse::commit::simple::CommitTimeOrder::NewestFirst,
                seconds,
            })
            .all()
            .map_err(|e| unavailable(&e))?;
        for (n, info) in walk.enumerate() {
            if n >= MAX_ANCESTRY_WALK {
                return Ok(false);
            }
            if info.map_err(|e| unavailable(&e))?.id == commit.id {
                return Ok(true);
            }
        }
        Ok(false)
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

    /// `commit.cleanup` and `core.commentChar`, to read a commit message as Git will clean it
    /// (US-GRD-018, D7). `None` where unset.
    pub fn commit_message_config(&self) -> (Option<String>, Option<String>) {
        let config = self.repo.config_snapshot();
        let get = |key: &str| config.string(key).map(|v| v.to_string());
        (get("commit.cleanup"), get("core.commentChar"))
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
