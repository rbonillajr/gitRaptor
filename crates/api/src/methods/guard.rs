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
/// Confirms the team baseline of the main branch (US-GRD-014; reserved). Plan first, then the
/// fingerprint the developer accepted and, when it relaxes, the window. Writes only the profile.
pub const GUARD_CONFIRM: &str = "guard.confirm";

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

/// `guard.uninstall`, `guard.cancel` or the window of `guard.confirm` did nothing: `data` is
/// [`crate::guard::GuardUninstallRefusedData`].
pub const GUARD_UNINSTALL_REFUSED: ErrorSpec = ErrorSpec::new(BLOCK, "guard-uninstall-refused");

/// `guard.install` or `guard.uninstall` stopped at a step: `data` is
/// [`crate::guard::GuardFailedData`]. Only with `guard.recovery`; without it, `INTERNAL`.
pub const GUARD_FAILED: ErrorSpec = ErrorSpec::new(BLOCK - 1, "guard-failed");

/// Getting out of an install that failed or an uninstall left half way: the `guard-failed`
/// error, the `leftover-folder` diagnostic, `GuardUninstallResult.restores_hooks_path`, and
/// `guard.uninstall` over a folder this profile does not know or an unfinished uninstall.
pub const CAP_GUARD_RECOVERY: Capability = Capability::new("guard.recovery");

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

/// Diff size and commit format (US-GRD-009, D10): the daemon applies `policies.diffSize` and
/// `policies.commitFormat` and sends `policy.diff-size`, `policy.commit-format`, their causes and
/// parameters, and `NotPreventable::CommitPolicyReach`, only to a connection that asked for it;
/// without it a daemon evaluates as before. A client that finds the daemon without it applies the
/// floor's rules for everyone itself.
pub const CAP_GUARD_COMMIT_POLICIES: Capability = Capability::new("guard.commit-policies");

/// The configuration protection: `policy.config-protected` in decisions and
/// `config.relax-ignored` notices in `guard.log`.
pub const CAP_GUARD_CONFIG_PROTECTION: Capability = Capability::new("guard.config-protection");

/// `Decision.priorSnapshot`: the `hook-prior` snapshot the daemon takes inside `guard.evaluate`
/// before a destructive operation. A connection without it never receives the field and the
/// daemon takes no snapshot for it.
pub const CAP_GUARD_PRIOR_SNAPSHOT: Capability = Capability::new("guard.prior-snapshot");

/// `system.snapshot-failed` and its causes in decisions and in `guard.log` (US-GRD-017). A
/// connection without it receives the same denial as `system.internal-error`, without cause and
/// without `priorSnapshot`.
pub const CAP_GUARD_SNAPSHOT_REQUIRED: Capability = Capability::new("guard.snapshot-required");

/// `GuardLogEntry.layer` with its real value (`hooks`, `mcp`, `cockpit`). Without it the list
/// leaves out the entries that are not `hooks`; the summary counts them all.
pub const CAP_GUARD_LOG_LAYERS: Capability = Capability::new("guard.log-layers");

/// Permissions per operation (US-GRD-007): the `permission.*` rules, the `ask-unavailable` cause,
/// `NotPreventable::PermissionReach`, the `commit` operation at `pre-commit`, and a daemon that
/// applies the permissions and the safe minimum the team sets. Without it a daemon evaluates as
/// before and `guard.log` leaves out the entries with `permission.*` reasons (the summary still
/// counts them).
pub const CAP_GUARD_PERMISSIONS: Capability = Capability::new("guard.permissions");

/// The confirmation of the team baseline (US-GRD-014): the `base-change-pending`,
/// `base-change-pending-invalid` and `floor-relax-pending` diagnostics, `GuardStatus.confirm`
/// and `PendingKind::ConfirmTeam` in `GuardStatus.pending`. A connection without it gets the
/// view of before. `raptor-mcp` may take it for the diagnostics, never `confirm`.
pub const CAP_GUARD_TEAM_CONFIRMATION: Capability = Capability::new("guard.team-confirmation");

/// The conscious exception (US-GRD-006; reserved, and it relaxes: announced, with a cancellable
/// window enforced by the daemon). The first call normalizes the argv and announces; the
/// requester confirms once the window closed and receives a one-use token.
pub const GUARD_EXCEPTION: &str = "guard.exception";
/// Binds the git the requester started to its exception token (reserved).
pub const GUARD_EXCEPTION_CHILD: &str = "guard.exception-child";
/// Ends an exception and says whether it was used (not reserved: only the requester).
pub const GUARD_EXCEPTION_END: &str = "guard.exception-end";
/// The conscious exception: its methods, `EvaluateParams.exception_token`,
/// `PendingAction.exception` and the exception counters of `guard.log`.
pub const CAP_GUARD_EXCEPTION: Capability = Capability::new("guard.exception");
/// `guard.exception`, `guard.exception-child` or `guard.exception-end` did nothing: `data` is
/// [`crate::guard::GuardExceptionRefusedData`]. Joins `GROUP.errors` with its messages.
pub const GUARD_EXCEPTION_REFUSED: ErrorSpec = ErrorSpec::new(BLOCK - 2, "guard-exception-refused");

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
        method(GUARD_CONFIRM, true, false).since(CAPABILITIES_PROTOCOL),
        method(GUARD_EXCEPTION, true, false).since(CAPABILITIES_PROTOCOL),
        method(GUARD_EXCEPTION_CHILD, true, false).since(CAPABILITIES_PROTOCOL),
        method(GUARD_EXCEPTION_END, false, false).since(CAPABILITIES_PROTOCOL),
    ],
    capabilities: &[
        CAP_GUARD_AUTHORSHIP,
        CAP_GUARD_AUTHORSHIP_SECOND_LINE,
        CAP_GUARD_POLICIES,
        CAP_GUARD_COMMIT_POLICIES,
        CAP_GUARD_CONFIG_PROTECTION,
        CAP_GUARD_PRIOR_HOOKS,
        CAP_GUARD_PENDING_ACTION,
        CAP_GUARD_PROTECTION,
        CAP_GUARD_PRIOR_SNAPSHOT,
        CAP_GUARD_SNAPSHOT_REQUIRED,
        CAP_GUARD_LOG_LAYERS,
        CAP_GUARD_RECOVERY,
        CAP_GUARD_PERMISSIONS,
        CAP_GUARD_TEAM_CONFIRMATION,
        CAP_GUARD_EXCEPTION,
    ],
    error_block: Some(BLOCK),
    errors: &[
        GUARD_UNINSTALL_REFUSED,
        GUARD_FAILED,
        GUARD_EXCEPTION_REFUSED,
    ],
    ..Group::new("guard")
};
