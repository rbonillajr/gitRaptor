//! The protected repos as the channel sees them: published by the daemon loop after an install
//! or at startup, read by `guard.evaluate` on the connection thread. An evaluation never waits
//! for the loop or for a repo's write lock (ADR-GRD-003, Enmienda Cockpit: no deadlock with an
//! operation of the executor whose own `git` runs the hook).

use std::collections::HashMap;
use std::sync::RwLock;

/// What the evaluation needs of one protected repo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardEntry {
    pub common_dir: String,
    /// Base branches the minimum protects (the confirmed one, or the union while unconfirmed).
    pub bases: Vec<String>,
}

/// Protected repos by id.
#[derive(Debug, Default)]
pub struct GuardRegistry(RwLock<HashMap<String, GuardEntry>>);

impl GuardRegistry {
    pub fn get(&self, repo_id: &str) -> Option<GuardEntry> {
        self.0.read().ok()?.get(repo_id).cloned()
    }

    pub fn set(&self, repo_id: &str, entry: GuardEntry) {
        if let Ok(mut map) = self.0.write() {
            map.insert(repo_id.to_owned(), entry);
        }
    }

    pub fn remove(&self, repo_id: &str) {
        if let Ok(mut map) = self.0.write() {
            map.remove(repo_id);
        }
    }
}
