//! The catalog of user operations (ADR-CKP-002).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{ERROR_BLOCK_LEN, FIRST_ERROR_BLOCK, Group, RepoWrite, method};
use crate::capability::Capability;
use crate::rpc::ErrorSpec;

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

/// `operation.prepare` and `operation.run` accept the `snapshot` operation and the shapes
/// that come with it (`write-in-progress`, the quota and time-limit errors, the
/// `SnapshotRunResult`). Without it the call stays `NOT_IMPLEMENTED`.
pub const CAP_OPERATION_SNAPSHOT: Capability = Capability::new("operation.snapshot");

/// The module's error block (ADR-GRP-016).
const BLOCK: i64 = FIRST_ERROR_BLOCK - 3 * ERROR_BLOCK_LEN;

/// A manual snapshot quota window is full; `data` is a [`SnapshotQuotaData`].
pub const OPERATION_SNAPSHOT_QUOTA: ErrorSpec = ErrorSpec::new(BLOCK, "snapshot-quota-exceeded");
/// The daemon's time budget for a manual snapshot ran out; nothing was saved.
pub const OPERATION_SNAPSHOT_TIME_LIMIT: ErrorSpec =
    ErrorSpec::new(BLOCK - 1, "snapshot-time-limit");

pub(super) const GROUP: Group = Group {
    methods: &[
        method(OPERATION_DESCRIBE, false, true),
        method(OPERATION_PREPARE, false, true),
        method(OPERATION_RUN, false, true).writes(RepoWrite::Protected),
        // Not reserved: the executor requires layer `cockpit`, which a
        // descendant of the daemon never has; not offered to `raptor-mcp`.
        method(OPERATION_CANCEL, false, false),
    ],
    capabilities: &[CAP_OPERATION_SNAPSHOT],
    error_block: Some(BLOCK),
    errors: &[OPERATION_SNAPSHOT_QUOTA, OPERATION_SNAPSHOT_TIME_LIMIT],
    ..Group::new("operation")
};

/// Which window of the manual snapshot quota refused a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum QuotaWindow {
    Minute,
    Day,
    WorktreeDay,
    RepoDay,
    Disk,
}

/// `data` of the [`OPERATION_SNAPSHOT_QUOTA`] error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SnapshotQuotaData {
    pub window: QuotaWindow,
    /// Seconds until a slot frees; absent for `disk`.
    pub retry_after_s: Option<u64>,
    /// When the oldest entry leaves the window (ms since the epoch); absent for `disk`.
    pub release_utc_ms: Option<i64>,
}
