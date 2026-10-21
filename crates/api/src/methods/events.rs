//! The event stream and the history of Git events.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Group, method};
use crate::capability::{CAPABILITIES_PROTOCOL, Capability};

/// Opens a subscription to the event stream.
pub const EVENTS_SUBSCRIBE: &str = "events.subscribe";
pub const EVENTS_UNSUBSCRIBE: &str = "events.unsubscribe";
/// Reads the history of Git events of one repo (US-GRP-002, ADR-GRP-013 § 6).
pub const EVENTS_HISTORY: &str = "events.history";
/// Reads the observation gaps of one repo (ADR-GRP-013 § 6).
pub const EVENTS_GAPS: &str = "events.gaps";
/// Most gaps one `events.gaps` page returns.
pub const MAX_GAPS_PAGE: u32 = 200;

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
        // Same exposure as the history: the gaps of a repo are not an MCP view.
        method(EVENTS_GAPS, false, false).since(CAPABILITIES_PROTOCOL),
    ],
    notifications: &[NOTIFY_EVENT, NOTIFY_RESYNC],
    capabilities: &[CAP_GIT_RESET, CAP_EVENTS_AUTHORSHIP],
    ..Group::new("events")
};

/// Parameters of `events.gaps`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventsGapsParams {
    pub repo_id: String,
    /// Only gaps still open or ended at or after this time. Without it, the latest `limit`.
    #[serde(default)]
    pub since_utc_ms: Option<i64>,
    #[serde(default)]
    pub limit: Option<u32>,
}

/// Result of `events.gaps`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EventsGapsResult {
    /// Oldest first, by start.
    pub gaps: Vec<GapView>,
}

/// A period the engine did not observe. What it found there is unattributed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GapView {
    pub gap_id: String,
    pub started_utc_ms: i64,
    /// Absent: still open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_utc_ms: Option<i64>,
    pub cause: GapCauseView,
    /// The engine stopped while an agent session was present (SEC-13).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub during_session: bool,
}

/// Why a period was not observed; the text is the store's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum GapCauseView {
    MachineOff,
    DaemonDown,
    DaemonDownDuringSession,
    DaemonStopped,
    RepoRetired,
    GitUnavailable,
    ProfileLost,
    StoreCorrupt,
    WatcherOverflow,
    StreamRecreated,
    PeriodicReconciliation,
    Dormant,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_gap_view_round_trips_and_omits_what_is_absent() {
        let open = GapView {
            gap_id: "daemon-down-1".into(),
            started_utc_ms: 10,
            ended_utc_ms: None,
            cause: GapCauseView::DaemonDown,
            during_session: false,
        };
        let json = serde_json::to_value(&open).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"gap_id":"daemon-down-1","started_utc_ms":10,"cause":"daemon-down"})
        );
        assert_eq!(serde_json::from_value::<GapView>(json).unwrap(), open);

        let closed = GapView {
            ended_utc_ms: Some(20),
            during_session: true,
            cause: GapCauseView::DaemonDownDuringSession,
            ..open
        };
        let json = serde_json::to_value(&closed).unwrap();
        assert_eq!(json["ended_utc_ms"], 20);
        assert_eq!(json["during_session"], true);
        assert_eq!(serde_json::from_value::<GapView>(json).unwrap(), closed);
    }

    #[test]
    fn gap_causes_read_as_the_store_text() {
        let all = [
            (GapCauseView::MachineOff, "machine-off"),
            (GapCauseView::DaemonDown, "daemon-down"),
            (
                GapCauseView::DaemonDownDuringSession,
                "daemon-down-during-session",
            ),
            (GapCauseView::DaemonStopped, "daemon-stopped"),
            (GapCauseView::RepoRetired, "repo-retired"),
            (GapCauseView::GitUnavailable, "git-unavailable"),
            (GapCauseView::ProfileLost, "profile-lost"),
            (GapCauseView::StoreCorrupt, "store-corrupt"),
            (GapCauseView::WatcherOverflow, "watcher-overflow"),
            (GapCauseView::StreamRecreated, "stream-recreated"),
            (
                GapCauseView::PeriodicReconciliation,
                "periodic-reconciliation",
            ),
            (GapCauseView::Dormant, "dormant"),
        ];
        for (cause, text) in all {
            let json = serde_json::to_value(cause).unwrap();
            assert_eq!(json, text);
            assert_eq!(serde_json::from_value::<GapCauseView>(json).unwrap(), cause);
        }
    }

    #[test]
    fn events_gaps_is_neither_reserved_nor_mcp() {
        let spec = crate::methods::spec(EVENTS_GAPS).expect("events.gaps is declared");
        assert!(!spec.reserved);
        assert!(!spec.mcp);
    }
}
