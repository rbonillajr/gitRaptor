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
/// Opens a subscription to the event stream.
pub const EVENTS_SUBSCRIBE: &str = "events.subscribe";
pub const EVENTS_UNSUBSCRIBE: &str = "events.unsubscribe";
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

/// Notification that carries one stream event.
pub const NOTIFY_EVENT: &str = "events.event";
/// Notification sent before a slow subscriber is disconnected: the client
/// must take a new snapshot and subscribe again (SEC-08).
pub const NOTIFY_RESYNC: &str = "events.resync";

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
}

const fn method(name: &'static str, reserved: bool, mcp: bool) -> MethodSpec {
    MethodSpec {
        name,
        reserved,
        mcp,
        implemented_by: None,
    }
}

const fn pending(name: &'static str, story: &'static str) -> MethodSpec {
    MethodSpec {
        name,
        reserved: true,
        mcp: false,
        implemented_by: Some(story),
    }
}

/// Every method of the contract.
pub const METHODS: &[MethodSpec] = &[
    method(HELLO, false, true),
    method(PING, false, true),
    method(ENGINE_SNAPSHOT, false, true),
    method(EVENTS_SUBSCRIBE, false, true),
    method(EVENTS_UNSUBSCRIBE, false, true),
    method(AUDIT_LIST, false, false),
    method(DAEMON_STOP, true, false),
    // Not reserved when the caller is the installed binary (`raptor` or
    // `raptor-mcp`); from anything else the daemon treats it as
    // `daemon.stop` (SEC-13). Its shape is frozen across protocol versions,
    // like `hello`'s, because it is how versions meet.
    method(DAEMON_REPLACE, false, true),
    pending(REPO_ADD, "US-GRP-001"),
    pending(REPO_RETIRE, "US-GRP-006"),
    pending(ATTRIBUTION_CORRECT, "US-GRP-010"),
    pending(ATTRIBUTION_WITHDRAW, "US-GRP-010"),
    pending(REGISTRATION_WITHDRAW, "US-GRP-009"),
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
    }

    #[test]
    fn pending_methods_name_their_story() {
        for m in METHODS.iter().filter(|m| m.implemented_by.is_some()) {
            assert!(m.reserved);
            assert!(m.implemented_by.unwrap().starts_with("US-GRP-"));
        }
    }
}
