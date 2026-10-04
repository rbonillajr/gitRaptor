//! Anchoring (ADR-TMC-001 § 3): copies into the store the objects it lacks, read from the user's
//! repository through the read-only [`RepoReader`] (replacements ignored). A snapshot never
//! depends on an object that only lives in the user's repository.
//!
//! Every object is re-hashed when written: if the id differs from the one asked for, the copy
//! fails instead of anchoring something else.

use std::collections::HashSet;

use gix::objs::{Find as _, Write as _};

use super::{Result, StoreError, StoreHandle};
use crate::{Oid, RepoReader};

/// What a copy did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CopyReport {
    pub objects: u64,
    pub bytes: u64,
}

impl StoreHandle {
    /// Copies `tips` and everything they reach that the store lacks. The walk stops at objects
    /// the store already has: what the store holds is always complete below it, because it only
    /// gets objects through this walk or from whole seeded packs.
    pub fn copy_closure(&self, user: &RepoReader, tips: &[Oid]) -> Result<CopyReport> {
        let mut report = CopyReport::default();
        let mut seen: HashSet<gix::ObjectId> = HashSet::new();
        let mut stack: Vec<gix::ObjectId> = tips.iter().map(|t| t.0).collect();
        let mut buf = Vec::new();
        while let Some(id) = stack.pop() {
            if !seen.insert(id) || self.repo.has_object(id) {
                continue;
            }
            let data = user
                .repo
                .objects
                .try_find(&id, &mut buf)
                .map_err(|e| StoreError::Git(format!("read {id}: {e}")))?
                .ok_or_else(|| StoreError::Corrupt(format!("{id} is not in the repository")))?;
            let kind = data.kind;
            match kind {
                gix::object::Kind::Commit => {
                    let mut commit = gix::objs::CommitRefIter::from_bytes(data.data, gix::hash::Kind::Sha1);
                    let tree = commit
                        .tree_id()
                        .map_err(|e| StoreError::Corrupt(format!("commit {id}: {e}")))?;
                    stack.push(tree);
                    stack.extend(commit.parent_ids());
                }
                gix::object::Kind::Tree => {
                    for entry in gix::objs::TreeRefIter::from_bytes(data.data, gix::hash::Kind::Sha1) {
                        let entry =
                            entry.map_err(|e| StoreError::Corrupt(format!("tree {id}: {e}")))?;
                        // A gitlink names a commit of another repository.
                        if !entry.mode.is_commit() {
                            stack.push(entry.oid.to_owned());
                        }
                    }
                }
                gix::object::Kind::Tag => {
                    let target = gix::objs::TagRefIter::from_bytes(data.data, gix::hash::Kind::Sha1)
                        .target_id()
                        .map_err(|e| StoreError::Corrupt(format!("tag {id}: {e}")))?;
                    stack.push(target);
                }
                gix::object::Kind::Blob => {}
            }
            let len = data.data.len() as u64;
            let written = self
                .repo
                .objects
                .write_buf(kind, data.data)
                .map_err(|e| StoreError::Git(format!("write {id}: {e}")))?;
            if written != id {
                return Err(StoreError::Corrupt(format!(
                    "object {id} hashes to {written}"
                )));
            }
            self.sync_object(Oid(id))?;
            report.objects += 1;
            report.bytes += len;
        }
        Ok(report)
    }
}
