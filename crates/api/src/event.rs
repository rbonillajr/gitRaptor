//! The event stream (ADR-GRP-005 § 5, ADR-GRP-011 § 3, ADR-GRP-013 § 6).
//!
//! Every event has a daemon-wide sequence number, a versioned kind and an
//! opaque `data` payload whose shape belongs to the kind's version. Change
//! events (those a story emits when something in a repo changes) always
//! carry the stage timings of ADR-GRP-011 § 3. This TS defines the envelope
//! and the engine's own kinds; the payload of each story's kinds is defined
//! by that story.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Stage timings of a change event, in nanoseconds of
/// [`crate::clock::monotonic_ns`]. A client compares them with its own
/// reading of the same clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Timings {
    /// Groups the events of one debounce window.
    pub batch_id: u64,
    pub t_recv: u64,
    pub t_flush: u64,
    pub t_computed: u64,
    pub t_persisted: u64,
    pub t_published: u64,
}

/// One event of the stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Event {
    /// Daemon-wide, strictly increasing. Subscribers receive events in this
    /// order.
    pub seq: u64,
    pub kind: String,
    /// Version of `data` for this kind.
    pub version: u32,
    /// Wall-clock time for people (UTC milliseconds).
    pub wall_ms: i64,
    /// Present on every change event, absent otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timings: Option<Timings>,
    pub data: Value,
}

impl Event {
    /// Whether the envelope agrees with the kind registry: known kind and
    /// version, timings exactly on change events.
    pub fn is_well_formed(&self) -> bool {
        match kind(&self.kind) {
            Some(spec) => spec.version == self.version && spec.change == self.timings.is_some(),
            None => false,
        }
    }
}

/// Static description of an event kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventKind {
    pub kind: &'static str,
    pub version: u32,
    /// A change event carries [`Timings`].
    pub change: bool,
    /// Story that defines `data`, for kinds declared ahead of it.
    pub defined_by: Option<&'static str>,
    /// The scope its events belong to (protocol 6, N1).
    pub scope: EventScope,
}

/// Which scope the events of a kind belong to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventScope {
    Global,
    /// The repo named by the `repo_id` of its `data`.
    Repo,
}

impl Event {
    /// The scope of this event, from its kind and, for repo kinds, the
    /// `repo_id` of its data. `None` for an unknown kind or a repo kind
    /// without a `repo_id`.
    pub fn scope(&self) -> Option<crate::scope::Scope> {
        scope_of(&self.kind, &self.data)
    }
}

/// The scope of an event of `kind` with `data`.
pub fn scope_of(kind_name: &str, data: &Value) -> Option<crate::scope::Scope> {
    match kind(kind_name)?.scope {
        EventScope::Global => Some(crate::scope::Scope::Global),
        EventScope::Repo => {
            data.get("repo_id")
                .and_then(Value::as_str)
                .map(|id| crate::scope::Scope::Repo {
                    repo_id: id.to_owned(),
                })
        }
    }
}

/// The engine's availability state changed (BR-WF-002). Data:
/// [`crate::messages::EngineView`].
pub const ENGINE_STATE: &str = "engine.state";
/// The daemon is about to stop. Data: [`crate::messages::StoppingData`].
pub const DAEMON_STOPPING: &str = "daemon.stopping";
/// A reserved command was attempted. Data: [`crate::messages::AuditEntry`].
pub const RESERVED_AUDIT: &str = "reserved.audit";
/// A repo started or stopped being observed (US-GRP-001). Data:
/// [`crate::messages::RepoObservationData`].
pub const REPO_OBSERVATION: &str = "repo.observation";
/// The reconciled worktrees of one repo (US-GRP-001). Data:
/// [`crate::messages::WorktreeStateData`].
pub const WORKTREE_STATE: &str = "worktree.state";
/// One Git event recorded in a repo's history (US-GRP-002). Data:
/// [`crate::messages::GitEventView`].
pub const GIT_EVENT: &str = "git.event";
/// An agent session appeared, changed state or ended (US-GRP-007). Data:
/// [`crate::messages::SessionView`].
pub const SESSION_STATE: &str = "session.state";

/// An operation of the catalog waits for the repo's write lock. Data:
/// [`crate::catalog::OperationEventData`] (Q-CKP-19).
pub const OPERATION_QUEUED: &str = "operation.queued";
/// An operation of the catalog took the lock and starts.
pub const OPERATION_STARTED: &str = "operation.started";
/// An operation of the catalog ended, with its outcome.
pub const OPERATION_FINISHED: &str = "operation.finished";
/// The attention summary of one repo changed (global scope, N3). Data:
/// [`crate::scope::RepoAttentionData`]. Declared ahead of its publishers
/// (the predictor, Guardrails, US-GRP-005).
pub const REPO_ATTENTION: &str = "repo.attention";
/// An observed repo changed tier (TS-GRP-006), only for a connection with
/// `observation.tiers`. Data: [`crate::messages::RepoTierData`].
pub const REPO_TIER: &str = "repo.tier";
/// Repos discovered in a code root (US-GRP-020), only for a connection with
/// `discovery.events`. Data: [`crate::discovery::RepoDiscoveredData`].
pub const REPO_DISCOVERED: &str = "repo.discovered";

/// The hook layer of a repo stopped being active and Guardrails did not do it (US-GRD-004,
/// ADR-GRD-005 § 5), only for a connection with `guard.protection`. Data:
/// [`crate::guard::ProtectionLostData`].
pub const GUARD_PROTECTION_LOST: &str = "guard.protection-lost";

/// The hook layer of a repo is active again (US-GRD-004): the developer installed it again
/// or fixed it by hand. Same capability and data as [`GUARD_PROTECTION_LOST`].
pub const GUARD_PROTECTION_RESTORED: &str = "guard.protection-restored";

const fn engine(kind: &'static str) -> EventKind {
    EventKind {
        kind,
        version: 1,
        change: false,
        defined_by: None,
        scope: EventScope::Global,
    }
}

/// An engine kind about one repo (its `data` carries `repo_id`).
const fn engine_repo(kind: &'static str) -> EventKind {
    EventKind {
        scope: EventScope::Repo,
        ..engine(kind)
    }
}

const fn change(kind: &'static str, story: &'static str) -> EventKind {
    EventKind {
        kind,
        version: 1,
        change: true,
        defined_by: Some(story),
        scope: EventScope::Repo,
    }
}

/// Every event kind of the contract.
pub const KINDS: &[EventKind] = &[
    engine(ENGINE_STATE),
    engine(DAEMON_STOPPING),
    engine(RESERVED_AUDIT),
    // Global: it changes the list of observed repos.
    engine(REPO_OBSERVATION),
    engine(REPO_ATTENTION),
    // Global, like the list it qualifies: the fleet shows it.
    engine(REPO_TIER),
    // Global: a candidate belongs to no observed repo.
    engine(REPO_DISCOVERED),
    engine_repo(GUARD_PROTECTION_LOST),
    engine_repo(GUARD_PROTECTION_RESTORED),
    engine_repo(OPERATION_QUEUED),
    engine_repo(OPERATION_STARTED),
    engine_repo(OPERATION_FINISHED),
    change(WORKTREE_STATE, "US-GRP-001"),
    change(GIT_EVENT, "US-GRP-002"),
    change("gap.recorded", "US-GRP-005"),
    change(SESSION_STATE, "US-GRP-007"),
    change("attribution.changed", "US-GRP-010"),
];

pub fn kind(name: &str) -> Option<&'static EventKind> {
    KINDS.iter().find(|k| k.kind == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn change_events_must_carry_timings() {
        let timings = Timings {
            batch_id: 1,
            t_recv: 1,
            t_flush: 2,
            t_computed: 3,
            t_persisted: 4,
            t_published: 5,
        };
        let mut event = Event {
            seq: 1,
            kind: "git.event".into(),
            version: 1,
            wall_ms: 0,
            timings: None,
            data: Value::Null,
        };
        assert!(!event.is_well_formed());
        event.timings = Some(timings);
        assert!(event.is_well_formed());
        event.kind = ENGINE_STATE.into();
        assert!(!event.is_well_formed());
        event.timings = None;
        assert!(event.is_well_formed());
        event.version = 2;
        assert!(!event.is_well_formed());
    }

    /// N1: the engine's own kinds are global; each repo kind is in the
    /// scope of the repo its data names.
    #[test]
    fn every_kind_has_a_scope() {
        use crate::scope::Scope;
        let data = serde_json::json!({"repo_id": "r1"});
        for k in KINDS {
            let scope = scope_of(k.kind, &data).unwrap();
            match k.scope {
                EventScope::Global => assert_eq!(scope, Scope::Global, "{}", k.kind),
                EventScope::Repo => assert_eq!(
                    scope,
                    Scope::Repo {
                        repo_id: "r1".into()
                    },
                    "{}",
                    k.kind
                ),
            }
        }
        for global in [
            ENGINE_STATE,
            DAEMON_STOPPING,
            RESERVED_AUDIT,
            REPO_OBSERVATION,
        ] {
            assert_eq!(kind(global).unwrap().scope, EventScope::Global);
        }
        for repo in [
            WORKTREE_STATE,
            GIT_EVENT,
            OPERATION_QUEUED,
            OPERATION_FINISHED,
        ] {
            assert_eq!(kind(repo).unwrap().scope, EventScope::Repo);
            assert_eq!(scope_of(repo, &Value::Null), None);
        }
        assert_eq!(scope_of("nope", &data), None);
    }

    #[test]
    fn declared_kinds_name_their_story() {
        for k in KINDS {
            assert_eq!(k.change, k.defined_by.is_some(), "{}", k.kind);
        }
    }
}
