//! One Time Machine writer per repo in the daemon (ADR-TMC-002 § 3, step 3): an application
//! takes the repo before its Git locks and keeps it until it ends. Taking it never waits: a
//! second application on the same repo is told the repo is busy.
//!
//! The key is the path of the repo's snapshot store, unique per repo and per profile.
//!
//! The registry lives in `timemachine` so the capture, the purge and the store maintenance can
//! share it (pending: they still use their own writer lock, US-TMC-016 and TS-TMC-004).

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

fn held() -> &'static Mutex<HashSet<String>> {
    static HELD: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    HELD.get_or_init(Default::default)
}

/// The repo, taken. Dropping it (also on panic) frees the repo.
#[derive(Debug)]
pub struct RepoGuard {
    repo_id: String,
}

/// Takes the repo keyed `repo_id` if no one holds it.
pub fn try_lock(repo_id: &str) -> Option<RepoGuard> {
    let mut held = held().lock().unwrap_or_else(|e| e.into_inner());
    held.insert(repo_id.to_owned()).then(|| RepoGuard {
        repo_id: repo_id.to_owned(),
    })
}

impl Drop for RepoGuard {
    fn drop(&mut self) {
        let mut held = held().lock().unwrap_or_else(|e| e.into_inner());
        held.remove(&self.repo_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_holder_per_repo() {
        let a = try_lock("repo-lock-test-a").unwrap();
        assert!(try_lock("repo-lock-test-a").is_none());
        let b = try_lock("repo-lock-test-b").unwrap();
        drop(a);
        assert!(try_lock("repo-lock-test-a").is_some());
        drop(b);
    }
}
