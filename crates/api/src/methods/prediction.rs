//! The conflict predictor's publication (DS-TS-CKP-001 § Contrato de API, D16): the capability
//! that brings `RepoView.prediction` and `prediction.changed`, and the read `prediction.list`.

use super::{Group, method};
use crate::capability::{CAPABILITIES_PROTOCOL, Capability};

/// `RepoView.prediction` and `prediction.changed`. Without it the wire does not change.
pub const CAP_PREDICTION_PAIRS: Capability = Capability::new("prediction.pairs");
/// The pairs of a repo in detail, by page. Found in `hello.methods`, with no capability.
pub const PREDICTION_LIST: &str = "prediction.list";
/// A batch of changed pairs of a repo (repo scope).
pub const PREDICTION_CHANGED: &str = "prediction.changed";

pub(super) const GROUP: Group = Group {
    // Carries worktree roots: `raptor-mcp` reads the pairs through its own projection
    // (`mcp.conflicts`), never this method directly (SEC-14).
    methods: &[method(PREDICTION_LIST, false, false).since(CAPABILITIES_PROTOCOL)],
    capabilities: &[CAP_PREDICTION_PAIRS],
    ..Group::new("prediction")
};
