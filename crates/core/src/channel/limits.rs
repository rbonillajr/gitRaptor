//! Limits per requester (ADR-MCP-001 § 6, S-03): the connections of one agent session share a
//! cap on how many may be open and one read and one write bucket, whichever client they come
//! from. Kept in memory: a restart of the daemon closes every connection and empties the table.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use gitraptor_api::mcp_view::{
    MCP_READ_BURST, MCP_READS_PER_MINUTE, MCP_WRITE_BURST, MCP_WRITES_PER_MINUTE,
};
use gitraptor_api::messages::{ClientKind, ConnectionProfile};
use gitraptor_api::timemachine::ResolvedVia;

use super::conn::Bucket;
use super::requester::Resolution;
use crate::timemachine::oplog::Requester;

/// Most connections open at once for one requester key.
pub(crate) const MAX_CONNECTIONS_PER_REQUESTER: usize = 8;

/// Whom a connection counts for. Fixed when its `hello` ends: a requester that changes later
/// keeps the key it connected with.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum RequesterKey {
    /// An agent session, also for the CLI it launches and for the processes an operation of
    /// its plan started.
    Agent(String),
    /// Every unattributed caller that does not pass the reserved checks: one shared key.
    Unattributed,
}

/// The requester already holds [`MAX_CONNECTIONS_PER_REQUESTER`] connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Full;

struct Entry {
    slots: usize,
    reads: Bucket,
    writes: Bucket,
}

impl Entry {
    fn new() -> Self {
        Self {
            slots: 0,
            reads: Bucket::per_minute(MCP_READS_PER_MINUTE, MCP_READ_BURST),
            writes: Bucket::per_minute(MCP_WRITES_PER_MINUTE, MCP_WRITE_BURST),
        }
    }

    /// Nothing distinguishes it from a new entry: dropping it gives no requester a fresh budget.
    fn is_idle(&mut self) -> bool {
        self.slots == 0 && self.reads.is_full() && self.writes.is_full()
    }
}

/// The table of requester keys, one per daemon, shared by every connection.
#[derive(Default)]
pub(crate) struct RequesterLimits {
    entries: Mutex<HashMap<RequesterKey, Entry>>,
}

/// A claimed connection slot; frees it on drop.
pub(crate) struct Slot {
    key: RequesterKey,
    limits: Arc<RequesterLimits>,
}

impl Slot {
    pub(crate) fn key(&self) -> &RequesterKey {
        &self.key
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut entries = self.limits.lock();
        if let Some(entry) = entries.get_mut(&self.key) {
            entry.slots = entry.slots.saturating_sub(1);
            if entry.is_idle() {
                entries.remove(&self.key);
            }
        }
    }
}

impl RequesterLimits {
    fn lock(&self) -> MutexGuard<'_, HashMap<RequesterKey, Entry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A slot for one more connection of `key`, or [`Full`] at the cap.
    pub(crate) fn claim(self: &Arc<Self>, key: RequesterKey) -> Result<Slot, Full> {
        let mut entries = self.lock();
        // Keys whose buckets refilled and hold no connection are forgotten here, so the table
        // stays as small as the requesters that are active.
        entries.retain(|_, e| !e.is_idle());
        let entry = entries.entry(key.clone()).or_insert_with(Entry::new);
        if entry.slots >= MAX_CONNECTIONS_PER_REQUESTER {
            return Err(Full);
        }
        entry.slots += 1;
        drop(entries);
        Ok(Slot {
            key,
            limits: Arc::clone(self),
        })
    }

    /// Spends one read of `key`'s shared budget.
    pub(crate) fn take_read(&self, key: &RequesterKey) -> bool {
        self.lock()
            .entry(key.clone())
            .or_insert_with(Entry::new)
            .reads
            .take()
    }

    /// Spends one write of `key`'s shared budget.
    pub(crate) fn take_write(&self, key: &RequesterKey) -> bool {
        self.lock()
            .entry(key.clone())
            .or_insert_with(Entry::new)
            .writes
            .take()
    }
}

/// The hook client (`raptor hook`): a full connection of kind `other` from the installed
/// binary. A connection with the MCP profile is never one, so the exemption from the slots
/// never reaches the MCP write path.
pub(crate) fn is_hook_client(
    profile: ConnectionProfile,
    client: ClientKind,
    installed_exe: bool,
) -> bool {
    profile == ConnectionProfile::Full && client == ClientKind::Other && installed_exe
}

/// The key a connection counts for (D9), or `None` when it takes no slot: the hook client, and
/// an unattributed caller that passes the reserved checks (`developer`, asked only then). A
/// caller whose identity could not be verified shares the unattributed key (fail closed).
pub(crate) fn key_of(
    resolution: Option<&Resolution>,
    hook_client: bool,
    developer: impl FnOnce() -> bool,
) -> Option<RequesterKey> {
    if hook_client {
        return None;
    }
    let Some(r) = resolution else {
        return Some(RequesterKey::Unattributed);
    };
    match &r.who.requester {
        // A descendant of the executor resolves to the requester of the plan that started it.
        Requester::Agent { session_id, .. } => Some(RequesterKey::Agent(session_id.clone())),
        _ if r.via == ResolvedVia::Executor => Some(RequesterKey::Unattributed),
        _ if developer() => None,
        _ => Some(RequesterKey::Unattributed),
    }
}

#[cfg(test)]
#[path = "limits_tests.rs"]
mod tests;
