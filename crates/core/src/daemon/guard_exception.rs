//! The loop's side of the conscious exception (US-GRD-006, D6 calls 1 and 2, D8): announce the
//! plan the channel normalized, issue its token once the window closed, and log a cancellation.
//! The repo is read on the connection thread, never here; the loop only holds the one pending
//! action per repo and the store.

use std::path::Path;

use gitraptor_api::AgentKind;
use gitraptor_api::guard::{
    Effect, ExceptionRefusal, GuardExceptionRefusedData, GuardExceptionResult, IssuedException,
    LogKind, LogLayer, LoggedReason, PendingKind, ProtectionState, UninstallRefusal,
};
use gitraptor_git::SystemGit;

use super::GuardReply;
use crate::guardrails::exception::{Bound, ExceptionPlan, monotonic_ms};
use crate::guardrails::exception_facts::issued_config;
use crate::guardrails::journal::Journal;
use crate::guardrails::log::{ExceptionLog, LogContext, LogEntry, exception_entry};
use crate::guardrails::pending::{self, Entry, Refused, Requester};
use crate::guardrails::{GuardRegistry, install as guard_install};
use crate::profile::RepoStore;

/// What the channel hands the loop to announce: the plan and the reasons it skips.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExceptionAsk {
    pub requester: Requester,
    pub plan: ExceptionPlan,
    pub reasons: Vec<LoggedReason>,
    /// The textconv drivers of the configuration its git sees (D10).
    pub drivers: Vec<String>,
}

/// The refusal of a pending action, as an exception names it.
pub(crate) fn refused(refused: Refused) -> GuardExceptionRefusedData {
    let reason = match refused.reason {
        UninstallRefusal::NotInstalled => ExceptionRefusal::NotInstalled,
        UninstallRefusal::AlreadyPending => ExceptionRefusal::AlreadyPending,
        UninstallRefusal::WindowOpen => ExceptionRefusal::WindowOpen,
        UninstallRefusal::NotTheRequester => ExceptionRefusal::NotTheRequester,
        UninstallRefusal::NoPending => ExceptionRefusal::NoPending,
    };
    GuardExceptionRefusedData {
        reason,
        remaining_ms: refused.remaining_ms,
    }
}

fn refusal(reason: ExceptionRefusal) -> GuardReply {
    GuardReply::ExceptionRefused(GuardExceptionRefusedData {
        reason,
        remaining_ms: None,
    })
}

/// Call 1: protected and on the current template (D12, checked after authorizing), then one
/// pending action per repo.
pub(super) fn announce(
    registry: &GuardRegistry,
    store: &RepoStore,
    repo_id: &str,
    common: &Path,
    now: i64,
    ask: ExceptionAsk,
) -> GuardReply {
    let journal = store
        .guard_keys()
        .ok()
        .and_then(|k| k.journal)
        .and_then(|j| Journal::from_json(&j));
    // An install of an older template is not fully active either: say how to update it
    // rather than that nothing is installed.
    if journal.is_some() && guard_install::outdated(common, journal.as_ref()) {
        return refusal(ExceptionRefusal::OutdatedInstall);
    }
    let status = guard_install::status(repo_id, common, store);
    if status.state != ProtectionState::HooksOnly {
        return refusal(ExceptionRefusal::NotInstalled);
    }
    let mut pending = registry.pending();
    let announced = pending.announce(
        repo_id,
        PendingKind::Exception,
        ask.requester,
        now,
        pending::window(),
        pending::new_action_id(),
    );
    match announced {
        Ok(mut action) => {
            let plan = ask.plan.view();
            pending.note_exception(repo_id, &action.action_id, ask.plan);
            pending.note_exception_reasons(repo_id, &action.action_id, ask.reasons);
            pending.note_exception_drivers(repo_id, &action.action_id, ask.drivers);
            action.exception = Some(plan.clone());
            GuardReply::Exception(Box::new(GuardExceptionResult {
                pending: Some(action),
                plan,
                issued: None,
            }))
        }
        Err(reason) => GuardReply::ExceptionRefused(refused(reason.into())),
    }
}

/// Call 2: the same requester, the window closed and not expired; the token goes to the
/// requester only. The entry taken is returned for the `applied` audit.
///
/// `drivers` are the textconv drivers read again for call 2 (`None` when they could not be
/// read): a set that is not the announced one ends the announcement without a token, since a
/// driver added since would run unneutralized.
pub(super) fn issue(
    registry: &GuardRegistry,
    git: &SystemGit,
    repo_id: &str,
    now: i64,
    action_id: &str,
    requester: Requester,
    drivers: Option<&[String]>,
) -> (GuardReply, Option<Entry>) {
    let due =
        registry
            .pending()
            .take_due(repo_id, action_id, PendingKind::Exception, requester, now);
    let action = match due {
        Ok(action) => action,
        Err(r) => return (GuardReply::ExceptionRefused(refused(r)), None),
    };
    let Some(plan) = action.exception.clone() else {
        return (refusal(ExceptionRefusal::NoPending), None);
    };
    let same_drivers = drivers.is_some_and(|now| {
        now.len() == action.exception_drivers.len()
            && now.iter().all(|d| action.exception_drivers.contains(d))
    });
    if !same_drivers {
        return (refusal(ExceptionRefusal::UnsupportedForm), Some(action));
    }
    let view = plan.view();
    let bound = Bound {
        action_id: action.action_id.clone(),
        repo_id: repo_id.to_owned(),
        requester,
        plan,
    };
    let tokens = registry.exceptions();
    let Some(token) = tokens.issue(bound, monotonic_ms()) else {
        // Without randomness there is no token: the announcement ends without effect.
        return (GuardReply::Failed, Some(action));
    };
    let issued = IssuedException {
        token,
        git: gitraptor_api::Untrusted::from_os(git.path.as_os_str()),
        config: issued_config(&action.exception_drivers),
        ttl_ms: u64::try_from(tokens.ttl().as_millis()).unwrap_or(u64::MAX),
    };
    (
        GuardReply::Exception(Box::new(GuardExceptionResult {
            pending: None,
            plan: view,
            issued: Some(issued),
        })),
        Some(action),
    )
}

/// The `exception-cancelled` entry of a cancelled exception, with the actor who cancelled.
pub(super) fn cancelled_entry(
    common: &Path,
    action: &Entry,
    actor: Option<AgentKind>,
) -> Option<LogEntry> {
    let plan = action.exception.as_ref()?;
    let (at_ms, utc_offset_s) = crate::watch::wall_now();
    let ctx = LogContext {
        actor,
        worktree: Some(plan.worktree.clone()),
        at_ms,
        utc_offset_s,
        ..LogContext::default()
    };
    Some(exception_entry(
        ExceptionLog {
            kind: LogKind::ExceptionCancelled,
            common_dir: common.to_string_lossy().into_owned(),
            action_id: Some(action.action_id.clone()),
            operation: plan.binding.operation.clone(),
            reasons: action.exception_reasons.clone(),
            effect: Effect::Deny,
            applied_effect: Effect::Deny,
            decision_id: crate::guardrails::evaluate::decision_id(),
            layer: LogLayer::Hooks,
        },
        &ctx,
    ))
}
