//! Guardrails (US-GRD-001, ADR-GRD-007 § 1), protocol 7.

use super::{ERROR_BLOCK_LEN, FIRST_ERROR_BLOCK, Group, RepoWrite, method};
use crate::capability::{CAPABILITIES_PROTOCOL, Capability};
use crate::rpc::ErrorSpec;

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
/// What Guardrails blocked in a repo and the KPI (US-GRD-005, ADR-GRD-006 § 6). Read-only, not
/// reserved and not offered to `raptor-mcp`.
pub const GUARD_LOG: &str = "guard.log";

/// Removes the hook layer and leaves the repo as it was (US-GRD-003; reserved, and it relaxes:
/// announced, with a cancellable window enforced by the daemon, ADR-GRD-007 § 1, D5). The
/// first call opens the window; the requester applies it once it closed.
pub const GUARD_UNINSTALL: &str = "guard.uninstall";
/// Cancels the reserved action waiting in a repo (not reserved: it only keeps the protection).
/// Never for `raptor-mcp`.
pub const GUARD_CANCEL: &str = "guard.cancel";

/// `GuardPlan.prior` and the `chain-impossible` blocker (US-GRD-002). Without it the daemon
/// leaves the field out and says `prior-hooks` instead.
pub const CAP_GUARD_PRIOR_HOOKS: Capability = Capability::new("guard.prior-hooks");
/// The protection health of US-GRD-004: `GuardStatus.hooks`, `.diagnostics` and `.minimum_set`,
/// the `protection-state` kind of `guard.log` and the `guard.protection-lost` event. A
/// connection without it never receives any of them.
pub const CAP_GUARD_PROTECTION: Capability = Capability::new("guard.protection");
/// `GuardStatus.pending` (US-GRD-003): the reserved action waiting for its window.
pub const CAP_GUARD_PENDING_ACTION: Capability = Capability::new("guard.pending-action");

// Two blocks after `FIRST_ERROR_BLOCK` (ADR-GRP-016 § 3; discovery has the first).
const BLOCK: i64 = FIRST_ERROR_BLOCK - 2 * ERROR_BLOCK_LEN;

/// `guard.uninstall` or `guard.cancel` did nothing: `data` is
/// [`crate::guard::GuardUninstallRefusedData`].
pub const GUARD_UNINSTALL_REFUSED: ErrorSpec = ErrorSpec::new(BLOCK, "guard-uninstall-refused");

/// Commit authorship (US-GRD-018, D11): `EvaluateParams.authorship`, the `commit` operation,
/// the `pre-commit` and `commit-msg` hooks and `Decision.notices`. The hook client sends them
/// only when the daemon granted it; a daemon without it evaluates as before.
pub const CAP_GUARD_AUTHORSHIP: Capability = Capability::new("guard.authorship");
/// The second line against `--no-verify` (DS-US-GRD-018 D6, § 5.3): the `second-line` stage of
/// the `commit` operation, sent from `reference-transaction`. A daemon with `guard.authorship`
/// alone would reject the stage, so the hook client sends it only when this is granted too.
pub const CAP_GUARD_AUTHORSHIP_SECOND_LINE: Capability =
    Capability::new("guard.authorship.second-line");

/// Protected branches and forbidden paths (US-GRD-008, D9): the daemon resolves the actor of
/// `ref-transaction` and `push` operations and applies `policies.protectedBranches` and
/// `policies.forbiddenPaths` only to a connection that asked for it; without it a daemon, or a
/// hook, evaluates as before.
pub const CAP_GUARD_POLICIES: Capability = Capability::new("guard.policies");

/// The configuration protection: `policy.config-protected` in decisions and
/// `config.relax-ignored` notices in `guard.log`.
pub const CAP_GUARD_CONFIG_PROTECTION: Capability = Capability::new("guard.config-protection");

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
        method(GUARD_LOG, false, false).since(CAPABILITIES_PROTOCOL),
        method(GUARD_UNINSTALL, true, false)
            .writes(RepoWrite::Guardrails)
            .since(CAPABILITIES_PROTOCOL),
        method(GUARD_CANCEL, false, false).since(CAPABILITIES_PROTOCOL),
    ],
    capabilities: &[
        CAP_GUARD_AUTHORSHIP,
        CAP_GUARD_AUTHORSHIP_SECOND_LINE,
        CAP_GUARD_POLICIES,
        CAP_GUARD_CONFIG_PROTECTION,
        CAP_GUARD_PRIOR_HOOKS,
        CAP_GUARD_PENDING_ACTION,
        CAP_GUARD_PROTECTION,
    ],
    error_block: Some(BLOCK),
    errors: &[GUARD_UNINSTALL_REFUSED],
    ..Group::new("guard")
};
