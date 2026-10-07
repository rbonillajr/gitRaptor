//! Guardrails (US-GRD-001, ADR-GRD-007 § 1), protocol 7.

use super::{Group, RepoWrite, method};
use crate::capability::Capability;

/// What installing the hook layer in a repo means, before the developer
/// grants the permission (ADR-GRD-007 § 1). Read-only.
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

/// Commit authorship (US-GRD-018, D11): `EvaluateParams.authorship`, the `commit` operation,
/// the `pre-commit` and `commit-msg` hooks and `Decision.notices`. The hook client sends them
/// only when the daemon granted it; a daemon without it evaluates as before.
pub const CAP_GUARD_AUTHORSHIP: Capability = Capability::new("guard.authorship");

pub(super) const GROUP: Group = Group {
    // None is offered to `raptor-mcp` (BR-AUTH-004): it neither installs nor
    // evaluates. A connection of protocol 5 or 6 sees none.
    methods: &[
        method(GUARD_PLAN, false, false).since(7),
        method(GUARD_INSTALL, true, false)
            .writes(RepoWrite::Guardrails)
            .since(7),
        method(GUARD_DECLINE, true, false).since(7),
        method(GUARD_STATUS, false, false).since(7),
        method(GUARD_EVALUATE, false, false).since(7),
    ],
    capabilities: &[CAP_GUARD_AUTHORSHIP],
    ..Group::new("guard")
};
