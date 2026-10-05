//! Refs of the applier (ADR-TMC-002 § 3, step 5).
//!
//! Branches and `refs/stash` move in **one** `update-ref --stdin -z` transaction with the old
//! value the plan expects for each: if an agent moved one since planning, the transaction fails
//! whole and nothing changes. Only `refs/heads/*` and `refs/stash` are representable
//! (SEC-TMC-14); for `refs/stash` only the top moves, its reflog (the rest of the stack) is not
//! rewritten.
//!
//! `HEAD` is written apart, with Git's lock protocol and a compare-and-swap on its content
//! ([`swap_head`]): `update-ref` cannot write a symbolic `HEAD` before Git 2.46 (`symref-update`)
//! nor a linked worktree's `HEAD` from the common folder, and while `HEAD.lock` is held it cannot
//! move the branch `HEAD` points to. So the order is: verify every `HEAD`, run the transaction,
//! then swap each `HEAD`. A swapped `HEAD` leaves no reflog entry.

use super::lock::GitLock;
use super::worktree::{HeadValue, WriteWorktree};
use super::{Result, WriteContext, WriteError};
use crate::{Oid, RefName};

/// Validates a full branch name `refs/heads/<name>`.
pub fn branch_ref(name: &str) -> Result<RefName> {
    let valid = RefName::new(name)?;
    if !name.starts_with("refs/heads/") || name.len() == "refs/heads/".len() {
        return Err(WriteError::InvalidInput(format!(
            "not a branch ref: {name:?}"
        )));
    }
    Ok(valid)
}

/// Validates a ref the applier may move: `refs/heads/*` or `refs/stash` (SEC-TMC-14).
pub fn movable_ref(name: &str) -> Result<RefName> {
    if name == "refs/stash" {
        return Ok(RefName::new(name)?);
    }
    branch_ref(name)
}

/// One ref of the transaction: `None` means absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefUpdate {
    pub name: RefName,
    pub old: Option<Oid>,
    pub new: Option<Oid>,
}

impl RefUpdate {
    /// Builds an update after checking the name with [`movable_ref`].
    pub fn new(name: &str, old: Option<Oid>, new: Option<Oid>) -> Result<Self> {
        Ok(Self {
            name: movable_ref(name)?,
            old,
            new,
        })
    }
}

/// The `update-ref --stdin -z` payload of `updates`. Updates that change nothing are verified.
fn payload(updates: &[RefUpdate]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut push = |fields: &[&str]| {
        for f in fields {
            out.extend_from_slice(f.as_bytes());
            out.push(0);
        }
    };
    push(&["start"]);
    let zero = "0".repeat(40);
    for u in updates {
        let name = u.name.as_str();
        let old = u.old.map(|o| o.to_hex()).unwrap_or_else(|| zero.clone());
        match u.new {
            _ if u.new == u.old => push(&[&format!("verify {name}"), &old]),
            Some(new) => push(&[&format!("update {name}"), &new.to_hex(), &old]),
            None => push(&[&format!("delete {name}"), &old]),
        }
    }
    push(&["prepare", "commit"]);
    out
}

/// Runs the transaction on the common folder of `repo`.
pub fn transaction(ctx: &WriteContext, repo: &WriteWorktree, updates: &[RefUpdate]) -> Result<()> {
    if updates.is_empty() {
        return Ok(());
    }
    super::cli::update_ref(ctx, repo.common_dir(), &payload(updates))
}

/// Replaces `HEAD` of `worktree` with `target` if it still holds `expected`: takes `HEAD.lock`
/// exclusively, compares under the lock, writes, syncs and renames. A foreign `HEAD.lock` is
/// [`WriteError::Busy`] and stays.
pub fn swap_head(worktree: &WriteWorktree, expected: &HeadValue, target: &HeadValue) -> Result<()> {
    let lock = GitLock::acquire(&worktree.head_path())?;
    let current = worktree.read_head()?;
    if &current != expected {
        lock.release()?;
        return Err(WriteError::RefMoved(format!(
            "HEAD of {} changed",
            worktree.root().display()
        )));
    }
    if current == *target {
        return lock.release();
    }
    lock.commit(&target.to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_branches_and_stash_are_movable() {
        for ok in ["refs/heads/main", "refs/heads/feat/x", "refs/stash"] {
            assert!(movable_ref(ok).is_ok(), "{ok}");
        }
        for bad in [
            "--upload-pack=x",
            "refs/remotes/origin/main",
            "refs/tags/v1",
            "HEAD",
            "main",
            "refs/heads/",
            "refs/heads/a\nb",
            "refs/heads/-x/../y",
            "refs/heads/a..b",
            "refs/tm/snap/x",
        ] {
            assert!(movable_ref(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn payload_is_one_transaction_with_old_values() {
        let a = Oid::from_hex(&"a".repeat(40)).unwrap();
        let b = Oid::from_hex(&"b".repeat(40)).unwrap();
        let updates = [
            RefUpdate::new("refs/heads/main", Some(a), Some(b)).unwrap(),
            RefUpdate::new("refs/heads/new", None, Some(a)).unwrap(),
            RefUpdate::new("refs/heads/old", Some(b), None).unwrap(),
            RefUpdate::new("refs/heads/same", Some(a), Some(a)).unwrap(),
        ];
        let text = String::from_utf8(payload(&updates))
            .unwrap()
            .replace('\0', "|");
        let zero = "0".repeat(40);
        let (a, b) = ("a".repeat(40), "b".repeat(40));
        assert_eq!(
            text,
            format!(
                "start|update refs/heads/main|{b}|{a}|update refs/heads/new|{a}|{zero}|\
                 delete refs/heads/old|{b}|verify refs/heads/same|{a}|prepare|commit|"
            )
        );
    }
}
