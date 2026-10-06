//! The catalog of user operations (ADR-CKP-002).

use super::{Group, RepoWrite, method};

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

pub(super) const GROUP: Group = Group {
    methods: &[
        method(OPERATION_DESCRIBE, false, true),
        method(OPERATION_PREPARE, false, true),
        method(OPERATION_RUN, false, true).writes(RepoWrite::Protected),
        // Not reserved: the executor requires layer `cockpit`, which a
        // descendant of the daemon never has; not offered to `raptor-mcp`.
        method(OPERATION_CANCEL, false, false),
    ],
    ..Group::new("operation")
};
