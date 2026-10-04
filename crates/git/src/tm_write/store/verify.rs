//! Verification before a snapshot is handed out for restoring (SEC-TMC-09): every object under
//! the snapshot's tree is read back from the store and re-hashed. The store is untrusted input:
//! the same user, or an agent, can change it.

use std::collections::HashSet;

use gix::objs::Find as _;

use super::{Result, StoreError, StoreHandle};
use crate::Oid;

/// What a verification checked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VerifyReport {
    pub objects: u64,
    pub bytes: u64,
}

impl StoreHandle {
    /// Re-hashes the commit of a snapshot, its tree and everything below it. Parents (the user's
    /// history) are not walked: they are checked when a restore needs them.
    pub fn verify_commit(&self, commit: Oid) -> Result<VerifyReport> {
        let mut report = VerifyReport::default();
        let mut buf = Vec::new();
        let tree = {
            let data = self.read_checked(commit.0, gix::object::Kind::Commit, &mut buf)?;
            report.objects += 1;
            report.bytes += data.len() as u64;
            gix::objs::CommitRefIter::from_bytes(data, gix::hash::Kind::Sha1)
                .tree_id()
                .map_err(|e| StoreError::Corrupt(format!("commit {commit}: {e}")))?
        };
        let below = self.verify_tree(Oid(tree))?;
        report.objects += below.objects;
        report.bytes += below.bytes;
        Ok(report)
    }

    /// Re-hashes `tree` and everything below it (gitlinks excluded).
    pub fn verify_tree(&self, tree: Oid) -> Result<VerifyReport> {
        let mut report = VerifyReport::default();
        let mut seen = HashSet::new();
        let mut stack = vec![(tree.0, gix::object::Kind::Tree)];
        let mut buf = Vec::new();
        while let Some((id, kind)) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let data = self.read_checked(id, kind, &mut buf)?;
            report.objects += 1;
            report.bytes += data.len() as u64;
            if kind == gix::object::Kind::Tree {
                for entry in gix::objs::TreeRefIter::from_bytes(data, gix::hash::Kind::Sha1) {
                    let entry =
                        entry.map_err(|e| StoreError::Corrupt(format!("tree {id}: {e}")))?;
                    if entry.mode.is_commit() {
                        continue;
                    }
                    let kind = if entry.mode.is_tree() {
                        gix::object::Kind::Tree
                    } else {
                        gix::object::Kind::Blob
                    };
                    stack.push((entry.oid.to_owned(), kind));
                }
            }
        }
        Ok(report)
    }

    /// Reads an object and checks its kind and that its bytes hash to its id.
    fn read_checked<'b>(
        &self,
        id: gix::ObjectId,
        kind: gix::object::Kind,
        buf: &'b mut Vec<u8>,
    ) -> Result<&'b [u8]> {
        let data = self
            .repo
            .objects
            .try_find(&id, buf)
            .map_err(|e| StoreError::Corrupt(format!("{id}: {e}")))?
            .ok_or_else(|| StoreError::Corrupt(format!("{id} is missing")))?;
        if data.kind != kind {
            return Err(StoreError::Corrupt(format!(
                "{id} is a {}, expected a {kind}",
                data.kind
            )));
        }
        let actual = gix::objs::compute_hash(gix::hash::Kind::Sha1, kind, data.data)
            .map_err(|e| StoreError::Corrupt(format!("{id}: {e}")))?;
        if actual != id {
            return Err(StoreError::Corrupt(format!("{id} hashes to {actual}")));
        }
        Ok(data.data)
    }
}
