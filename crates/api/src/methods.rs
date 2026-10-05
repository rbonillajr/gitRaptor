//! The method registry of the contract.
//!
//! Each method says whether it is a reserved command (ADR-GRP-005 § 6:
//! authorized only by the daemon, never by a client claim), whether
//! `raptor-mcp` may use it (SEC-14, NFR-02) and, for methods declared ahead
//! of their story, which story implements them.

/// Handshake; must be the first message of every connection.
pub const HELLO: &str = "hello";
/// Liveness check.
pub const PING: &str = "ping";
/// Coherent snapshot with the sequence it reflects (DEP-CKP-6).
pub const ENGINE_SNAPSHOT: &str = "engine.snapshot";
/// What the engine consumes on the machine (US-GRP-017, ADR-GRP-015 § 4).
/// Read-only.
pub const ENGINE_RESOURCES: &str = "engine.resources";
/// Opens a subscription to the event stream.
pub const EVENTS_SUBSCRIBE: &str = "events.subscribe";
pub const EVENTS_UNSUBSCRIBE: &str = "events.unsubscribe";
/// Reads the history of Git events of one repo (US-GRP-002, ADR-GRP-013 § 6).
pub const EVENTS_HISTORY: &str = "events.history";
/// Lists the agent sessions of the observed repos (US-GRP-007, ADR-GRP-013
/// § 6).
pub const SESSIONS_LIST: &str = "sessions.list";
/// Reads the append-only audit of reserved commands (ADR-GRP-013 § 1).
pub const AUDIT_LIST: &str = "audit.list";
/// Orderly stop of the daemon (reserved, SEC-13).
pub const DAEMON_STOP: &str = "daemon.stop";
/// Stop requested by a newer installed binary to replace an older daemon.
pub const DAEMON_REPLACE: &str = "daemon.replace";
pub const REPO_ADD: &str = "repo.add";
pub const REPO_RETIRE: &str = "repo.retire";
pub const ATTRIBUTION_CORRECT: &str = "attribution.correct";
pub const ATTRIBUTION_WITHDRAW: &str = "attribution.withdraw-correction";
pub const REGISTRATION_WITHDRAW: &str = "registration.withdraw";
/// The catalog of user operations, filtered for the connection
/// (ADR-CKP-002 § 1). Read-only.
pub const OPERATION_DESCRIBE: &str = "operation.describe";
/// First phase: checks and returns a plan with its `plan_id` and
/// fingerprint. Touches nothing and writes no oplog entry (ADR-CKP-002 § 2).
pub const OPERATION_PREPARE: &str = "operation.prepare";
/// Second phase: runs a plan prepared by the same connection, under the
/// repo's write lock, as a protected operation: intent, prior snapshot,
/// execution, record (ADR-TMC-004 § 1). The only route that writes a
/// catalog operation (protocol 3: it takes `{plan_id, accepted_warnings,
/// confirmation?}`; TS-CKP-002 unified it with the two-phase flow).
pub const OPERATION_RUN: &str = "operation.run";
/// Asks a running operation to stop, like a Ctrl-C (layer `cockpit` only,
/// BR-CKP-WF-008).
pub const OPERATION_CANCEL: &str = "operation.cancel";
/// How the daemon sees the caller: "agent X" or "unattributed" (ADR-TMC-005
/// § 1). Read-only.
pub const REQUESTER_RESOLVE: &str = "requester.resolve";
pub const TM_SNAPSHOT: &str = "timemachine.snapshot";
pub const TM_UNDO: &str = "timemachine.undo";
pub const TM_REDO: &str = "timemachine.redo";
pub const TM_RESTORE: &str = "timemachine.restore";
pub const TM_TIMELINE: &str = "timemachine.timeline";
/// Snapshot of one scope with its own sequence (protocol 6, N1 and N3).
pub const SCOPE_SNAPSHOT: &str = "scope.snapshot";
/// Subscription to one scope, from a scope sequence (protocol 6, N1, N2).
pub const SCOPE_SUBSCRIBE: &str = "scope.subscribe";
/// The observed repo and worktree that contain a path (protocol 6, N4).
pub const REPO_LOCATE: &str = "repo.locate";

/// Guardrails (US-GRD-001): what installing the hook layer in a repo means, before the
/// developer grants the permission (ADR-GRD-007 § 1). Read-only.
pub const GUARD_PLAN: &str = "guard.plan";
/// Installs the hook layer with the developer's permission (reserved; writes the repo's
/// `core.hooksPath` and `<common>/gitraptor/`, recovered by its own journal).
pub const GUARD_INSTALL: &str = "guard.install";
/// Records the developer's denial of the permission (reserved): never offered again.
pub const GUARD_DECLINE: &str = "guard.decline";
/// The protection status of a repo (ADR-GRD-005 § 3, the part of US-GRD-001). Read-only.
pub const GUARD_STATUS: &str = "guard.status";
/// The decision on a governed operation, asked by `raptor hook` (ADR-GRD-003 § 3 and § 4).
pub const GUARD_EVALUATE: &str = "guard.evaluate";

/// Notification that carries one stream event.
pub const NOTIFY_EVENT: &str = "events.event";
/// Notification sent before a slow subscriber is disconnected: the client
/// must take a new snapshot and subscribe again (SEC-08).
pub const NOTIFY_RESYNC: &str = "events.resync";
/// Notification that carries one event of a scoped subscription.
pub const NOTIFY_SCOPE_EVENT: &str = "scope.event";
/// Notification that one scope cannot continue: take a new snapshot of it.
pub const NOTIFY_SCOPE_RESYNC: &str = "scope.resync";

/// Whether a method may modify a repository (ADR-TMC-004 § 1). Only the
/// protected operation and the Time Machine's own protected operations do:
/// every write takes a prior snapshot first (BR-TMC-CONS-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoWrite {
    None,
    /// A catalog operation run through the protected operation.
    Protected,
    /// Undo, redo or restore: protected operations of the Time Machine.
    TimeMachine,
    /// The install of the Guardrails hook layer: only `core.hooksPath` and
    /// `<common>/gitraptor/`, never the user's content; recovered by its own
    /// journal (ADR-GRD-001 § 4), not by a prior snapshot.
    Guardrails,
}

/// Static description of one method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MethodSpec {
    pub name: &'static str,
    /// Reserved to the developer (ADR-GRP-005 § 6).
    pub reserved: bool,
    /// Offered to `raptor-mcp` connections.
    pub mcp: bool,
    /// Story that implements it, for methods declared ahead of it.
    pub implemented_by: Option<&'static str>,
    pub writes: RepoWrite,
    /// First protocol version that has it: a connection that negotiated an
    /// older one does not see it (DS-TS-GRP-004 E-D1).
    pub since: u32,
}

impl MethodSpec {
    /// Whether a connection of `protocol` has this method.
    pub const fn exists_in(&self, protocol: u32) -> bool {
        protocol >= self.since
    }
}

/// Methods of protocol 5 and before: every client in the window has them.
const BASE: u32 = crate::MIN_COMPATIBLE_PROTOCOL;

const fn method(name: &'static str, reserved: bool, mcp: bool) -> MethodSpec {
    MethodSpec {
        name,
        reserved,
        mcp,
        implemented_by: None,
        writes: RepoWrite::None,
        since: BASE,
    }
}

/// A method of protocol 6 (Cockpit, ADR-CKP-003 § 4). Not reserved and not
/// offered to `raptor-mcp`: they carry paths (SEC-12).
const fn v5(name: &'static str) -> MethodSpec {
    MethodSpec {
        since: 6,
        ..method(name, false, false)
    }
}

/// A method of protocol 7 (US-GRD-001).
const fn v7(spec: MethodSpec) -> MethodSpec {
    MethodSpec { since: 7, ..spec }
}

const fn pending(name: &'static str, story: &'static str) -> MethodSpec {
    MethodSpec {
        name,
        reserved: true,
        mcp: false,
        implemented_by: Some(story),
        writes: RepoWrite::None,
        since: BASE,
    }
}

/// A Time Machine command: not reserved (an agent may undo its own work,
/// ADR-TMC-005 § 2), declared with its parameters and validated, and
/// implemented by its story.
const fn time_machine(
    name: &'static str,
    mcp: bool,
    writes: RepoWrite,
    story: &'static str,
) -> MethodSpec {
    MethodSpec {
        name,
        reserved: false,
        mcp,
        implemented_by: Some(story),
        writes,
        since: BASE,
    }
}

/// Every method of the contract.
pub const METHODS: &[MethodSpec] = &[
    method(HELLO, false, true),
    method(PING, false, true),
    method(ENGINE_SNAPSHOT, false, true),
    // Read-only, like the snapshot; not offered to `raptor-mcp`
    // (SEC-MCP-01): an agent does not see the engine's consumption.
    method(ENGINE_RESOURCES, false, false),
    method(EVENTS_SUBSCRIBE, false, true),
    method(EVENTS_UNSUBSCRIBE, false, true),
    // Carries paths: not offered to `raptor-mcp` until F-001-05 defines its
    // projection (SEC-12).
    method(EVENTS_HISTORY, false, false),
    // Carries worktree paths: not offered to `raptor-mcp` (SEC-12).
    method(SESSIONS_LIST, false, false),
    method(AUDIT_LIST, false, false),
    method(DAEMON_STOP, true, false),
    // Not reserved when the caller is the installed binary (`raptor` or
    // `raptor-mcp`); from anything else the daemon treats it as
    // `daemon.stop` (SEC-13). Its shape is frozen across protocol versions,
    // like `hello`'s, because it is how versions meet.
    method(DAEMON_REPLACE, false, true),
    method(REPO_ADD, true, false),
    // US-GRP-001 stops the observation; US-GRP-006 adds the history kept
    // and recovered.
    method(REPO_RETIRE, true, false),
    pending(ATTRIBUTION_CORRECT, "US-GRP-010"),
    pending(ATTRIBUTION_WITHDRAW, "US-GRP-010"),
    pending(REGISTRATION_WITHDRAW, "US-GRP-009"),
    method(OPERATION_DESCRIBE, false, true),
    method(OPERATION_PREPARE, false, true),
    MethodSpec {
        name: OPERATION_RUN,
        reserved: false,
        mcp: true,
        implemented_by: None,
        writes: RepoWrite::Protected,
        since: BASE,
    },
    // Not reserved: the executor requires layer `cockpit`, which a
    // descendant of the daemon never has; not offered to `raptor-mcp`.
    method(OPERATION_CANCEL, false, false),
    method(REQUESTER_RESOLVE, false, true),
    // Redo, restore and the full timeline are not offered over MCP
    // (Q-MCP-11); the hook snapshot is a CLI command (US-TMC-005).
    time_machine(TM_SNAPSHOT, false, RepoWrite::None, "US-TMC-005"),
    time_machine(TM_UNDO, true, RepoWrite::TimeMachine, "US-TMC-002"),
    time_machine(TM_REDO, false, RepoWrite::TimeMachine, "US-TMC-003"),
    time_machine(TM_RESTORE, false, RepoWrite::TimeMachine, "US-TMC-009"),
    time_machine(TM_TIMELINE, false, RepoWrite::None, "US-TMC-006"),
    v5(SCOPE_SNAPSHOT),
    v5(SCOPE_SUBSCRIBE),
    v5(REPO_LOCATE),
    // Guardrails (US-GRD-001, ADR-GRD-007 § 1), protocol 7. None is offered
    // to `raptor-mcp` (BR-AUTH-004): it neither installs nor evaluates.
    v7(method(GUARD_PLAN, false, false)),
    v7(MethodSpec {
        name: GUARD_INSTALL,
        reserved: true,
        mcp: false,
        implemented_by: None,
        writes: RepoWrite::Guardrails,
        since: BASE,
    }),
    v7(method(GUARD_DECLINE, true, false)),
    v7(method(GUARD_STATUS, false, false)),
    v7(method(GUARD_EVALUATE, false, false)),
];

pub fn spec(name: &str) -> Option<&'static MethodSpec> {
    METHODS.iter().find(|m| m.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SEC-14: no reserved command is offered to `raptor-mcp`.
    #[test]
    fn mcp_never_gets_a_reserved_command() {
        for m in METHODS {
            assert!(
                !(m.reserved && m.mcp),
                "{} is reserved and offered to MCP",
                m.name
            );
        }
        assert!(!spec(AUDIT_LIST).unwrap().mcp);
        let resources = spec(ENGINE_RESOURCES).unwrap();
        assert!(!resources.mcp && !resources.reserved);
        assert_eq!(resources.writes, RepoWrite::None);
    }

    #[test]
    fn pending_methods_name_their_story() {
        for m in METHODS.iter().filter(|m| m.implemented_by.is_some()) {
            let story = m.implemented_by.unwrap();
            if m.name.starts_with("timemachine.") {
                assert!(!m.reserved, "{}", m.name);
                assert!(story.starts_with("US-TMC-"), "{}", m.name);
            } else {
                assert!(m.reserved);
                assert!(story.starts_with("US-GRP-"));
            }
        }
    }

    /// ADR-TMC-004 § 1 and TS-TMC-004: only the protected operation and
    /// the Time Machine's undo, redo and restore may modify a repo.
    #[test]
    fn only_protected_paths_write() {
        let mut writers: Vec<_> = METHODS
            .iter()
            .filter(|m| m.writes != RepoWrite::None)
            .map(|m| (m.name, m.writes))
            .collect();
        writers.sort_by_key(|(name, _)| *name);
        assert_eq!(
            writers,
            [
                (GUARD_INSTALL, RepoWrite::Guardrails),
                (OPERATION_RUN, RepoWrite::Protected),
                (TM_REDO, RepoWrite::TimeMachine),
                (TM_RESTORE, RepoWrite::TimeMachine),
                (TM_UNDO, RepoWrite::TimeMachine),
            ]
        );
        // A writer is never a reserved command: reserved commands do not go
        // through the prior snapshot. The one exception is the Guardrails
        // install, recovered by its own journal (ADR-GRD-001 § 4); only it
        // may declare that kind of write.
        assert!(METHODS.iter().all(|m| !(m.reserved
            && m.writes != RepoWrite::None
            && m.writes != RepoWrite::Guardrails)));
        assert!(
            METHODS
                .iter()
                .filter(|m| m.writes == RepoWrite::Guardrails)
                .all(|m| m.name == GUARD_INSTALL && m.reserved)
        );
    }

    /// E-D1: protocol 5 connections keep exactly the protocol 5 methods;
    /// the Cockpit's are of protocol 6 and stay out of MCP.
    #[test]
    fn protocol_5_methods_are_new_and_not_for_mcp() {
        for name in [SCOPE_SNAPSHOT, SCOPE_SUBSCRIBE, REPO_LOCATE] {
            let m = spec(name).unwrap();
            assert!(!m.exists_in(5) && m.exists_in(6), "{name}");
            assert!(!m.mcp && !m.reserved, "{name}");
        }
        assert!(
            spec(HELLO)
                .unwrap()
                .exists_in(crate::MIN_COMPATIBLE_PROTOCOL)
        );
        assert!(METHODS.iter().all(|m| m.since <= crate::PROTOCOL_VERSION));
    }

    #[test]
    fn method_names_are_unique() {
        let mut names: Vec<_> = METHODS.iter().map(|m| m.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), METHODS.len());
    }
}
