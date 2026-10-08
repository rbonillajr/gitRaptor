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
    /// What the developer confirmed of the team level (ADR-GRD-004 § 3): only a confirmed floor
    /// relaxes `policies.commitAuthorship` (US-GRD-018, D2).
    pub confirmed: Option<gitraptor_policy::team::Confirmed>,
}

/// Protected repos by id, and the profile the daemon serves.
#[derive(Debug, Default)]
pub struct GuardRegistry {
    repos: RwLock<HashMap<String, GuardEntry>>,
    /// The profile's folders: its `settings.json` sets `policies.commitAuthorship` too
    /// (US-GRD-018). Read on every commit evaluation, never cached.
    profile: RwLock<Option<crate::profile::ProfileDirs>>,
    /// The decision log entries in flight to the loop (US-GRD-005, D4).
    log: super::log::LogSink,
}

impl GuardRegistry {
    /// A registry for the daemon of `dirs`.
    pub fn for_profile(dirs: crate::profile::ProfileDirs) -> Self {
        Self {
            repos: RwLock::default(),
            profile: RwLock::new(Some(dirs)),
            log: super::log::LogSink::default(),
        }
    }

    pub fn get(&self, repo_id: &str) -> Option<GuardEntry> {
        self.repos.read().ok()?.get(repo_id).cloned()
    }

    pub fn set(&self, repo_id: &str, entry: GuardEntry) {
        if let Ok(mut map) = self.repos.write() {
            map.insert(repo_id.to_owned(), entry);
        }
    }

    pub fn remove(&self, repo_id: &str) {
        if let Ok(mut map) = self.repos.write() {
            map.remove(repo_id);
        }
    }

    /// The decision log's sink, shared by the connections and the loop.
    pub fn log(&self) -> &super::log::LogSink {
        &self.log
    }

    /// The profile's folders, when the daemon set them.
    pub fn profile(&self) -> Option<crate::profile::ProfileDirs> {
        self.profile.read().ok()?.clone()
    }
}
