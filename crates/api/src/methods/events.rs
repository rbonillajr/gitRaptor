//! The event stream and the history of Git events.

use super::{Group, method};
use crate::capability::Capability;

/// Opens a subscription to the event stream.
pub const EVENTS_SUBSCRIBE: &str = "events.subscribe";
pub const EVENTS_UNSUBSCRIBE: &str = "events.unsubscribe";
/// Reads the history of Git events of one repo (US-GRP-002, ADR-GRP-013 § 6).
pub const EVENTS_HISTORY: &str = "events.history";

/// Notification that carries one stream event.
pub const NOTIFY_EVENT: &str = "events.event";
/// Notification sent before a slow subscriber is disconnected: the client
/// must take a new snapshot and subscribe again (SEC-08).
pub const NOTIFY_RESYNC: &str = "events.resync";

/// Git events of kind `reset` (protocol 8, US-TMC-004): a connection
/// without it never receives them.
pub const CAP_GIT_RESET: Capability = Capability::legacy("events.git-reset", 8);

/// Declared authorship of commit events and the trailer check of the
/// `inferred` hint (US-GRD-019, DS-US-GRD-018 D10). `raptor-mcp` does not
/// ask for it: its view never carries names or emails.
pub const CAP_EVENTS_AUTHORSHIP: Capability = Capability::new("events.authorship");

pub(super) const GROUP: Group = Group {
    methods: &[
        method(EVENTS_SUBSCRIBE, false, true),
        method(EVENTS_UNSUBSCRIBE, false, true),
        // Carries paths: not offered to `raptor-mcp` until F-001-05 defines its
        // projection (SEC-12).
        method(EVENTS_HISTORY, false, false),
    ],
    notifications: &[NOTIFY_EVENT, NOTIFY_RESYNC],
    capabilities: &[CAP_GIT_RESET, CAP_EVENTS_AUTHORSHIP],
    ..Group::new("events")
};
