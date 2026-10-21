//! `guard.confirm` in the daemon (ADR-GRD-004 § 3–4, ADR-GRD-007 D8): the plan of confirming
//! the team baseline of the main branch, read now, its view and its fingerprint on the wire, and
//! the write of what the developer accepted.
//!
//! Nothing here confirms on a read: only [`apply`] writes, and only a plan whose fingerprint the
//! caller already compared with the one the developer saw.

use std::path::Path;

use gitraptor_api::guard::{
    BaseChangeView, ConfirmPlanView, ConfirmSourceView, GuardConfirmResult, RelaxationView,
    RelaxedKey as WireKey,
};
use gitraptor_api::{Untrusted, UntrustedName};
use gitraptor_policy::team::Confirmed;
use gitraptor_policy::team::confirm::{self, ConfirmPlan, Fingerprint, Refusal, RelaxedKey};
use sha2::{Digest, Sha256};

use super::evaluate;
use super::journal::Stage;
use super::registry::GuardRegistry;
use crate::profile::{ProfileDirs, RepoStore};

/// Names shown in a plan are cut to this many characters.
const MAX_NAME_CHARS: usize = 100;

/// The step of a `guard.confirm` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Only the plan: nothing is written or announced.
    Plan,
    /// The fingerprint of the plan the developer accepted.
    Accept(String),
    /// The `action_id` of the window, once it closed.
    Apply(String),
}

/// The fingerprint as it travels: the SHA-256 of its canonical encoding, in hex.
pub fn wire(fingerprint: &Fingerprint) -> String {
    Sha256::digest(fingerprint.as_str().as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn name(text: &str) -> UntrustedName {
    UntrustedName::new(text).capped_chars(MAX_NAME_CHARS)
}

fn key(k: RelaxedKey) -> WireKey {
    match k {
        RelaxedKey::PermissionsAllow => WireKey::PermissionsAllow,
        RelaxedKey::PermissionsAsk => WireKey::PermissionsAsk,
        RelaxedKey::PermissionsDeny => WireKey::PermissionsDeny,
        RelaxedKey::DisableSafeMinimum => WireKey::DisableSafeMinimum,
        RelaxedKey::ProtectedBranches => WireKey::ProtectedBranches,
        RelaxedKey::ForbiddenPaths => WireKey::ForbiddenPaths,
        RelaxedKey::CommitAuthorship => WireKey::CommitAuthorship,
    }
}

fn values(list: &[String]) -> Vec<Untrusted> {
    list.iter()
        .map(|v| Untrusted::new(v.as_str()).capped_chars(MAX_NAME_CHARS))
        .collect()
}

/// The plan as the developer sees it; the repo's text stays untrusted.
pub fn view(plan: &ConfirmPlan) -> ConfirmPlanView {
    ConfirmPlanView {
        source: plan.source.as_ref().map(|copy| ConfirmSourceView {
            reference: name(copy.reference.as_str()),
            commit: copy.commit.clone(),
            floor_blob: plan.floor_blob.clone(),
        }),
        base: BaseChangeView {
            from: plan.base.from.as_ref().map(|b| name(b.as_str())),
            to: name(plan.base.to.as_str()),
        },
        relaxations: plan
            .relaxations
            .iter()
            .map(|r| RelaxationView {
                key: key(r.key),
                before: values(&r.before),
                after: values(&r.after),
            })
            .collect(),
        relaxes: plan.needs_window(),
    }
}

/// The answer of a refused plan.
pub fn refusal(r: Refusal) -> GuardConfirmResult {
    match r {
        Refusal::NothingToConfirm => GuardConfirmResult::NothingToConfirm,
        Refusal::BaseInvalid => GuardConfirmResult::BaseInvalid,
        Refusal::FloorUnreadable => GuardConfirmResult::FloorUnreadable,
        Refusal::UnshowableRelaxation => GuardConfirmResult::UnshowableRelaxation,
    }
}

/// The plan of confirming what the main branch of the repo at `common` resolves now, against
/// what `store` confirmed, with its wire fingerprint.
///
/// # Errors
///
/// The [`Refusal`] of the plan; a team level that cannot be read is
/// [`Refusal::FloorUnreadable`]: nothing is confirmed blind.
pub fn current(common: &Path, store: &RepoStore) -> Result<(ConfirmPlan, String), Refusal> {
    let confirmed = store
        .confirmed_team_baseline()
        .map_err(|_| Refusal::FloorUnreadable)?;
    let reader = evaluate::open(common).ok_or(Refusal::FloorUnreadable)?;
    let team = super::team_status::read(&reader, confirmed.as_ref())
        .map_err(|_| Refusal::FloorUnreadable)?;
    let plan = confirm::plan(&team, confirmed.as_ref())?;
    let fingerprint = wire(&plan.fingerprint);
    Ok((plan, fingerprint))
}

/// Why [`apply`] did not record the confirmation.
#[derive(Debug)]
pub struct NotRecorded;

/// Records what `plan` confirms and, when the repo is protected, publishes it (the registry
/// and the snapshot the hooks read). Returns what was confirmed.
///
/// # Errors
///
/// [`NotRecorded`] when the store could not be written; nothing changed then.
pub fn apply(
    dirs: &ProfileDirs,
    repo_id: &str,
    store: &mut RepoStore,
    registry: &GuardRegistry,
    plan: &ConfirmPlan,
) -> Result<Confirmed, NotRecorded> {
    let confirmed = plan.confirmed();
    store
        .set_confirmed_team_baseline(&confirmed)
        .map_err(|_| NotRecorded)?;
    let journal = store
        .guard_keys()
        .ok()
        .as_ref()
        .and_then(super::install::journal)
        .filter(|j| j.stage == Stage::Confirmed);
    if let Some(journal) = journal {
        super::install::publish(dirs, repo_id, &journal, registry, Some(confirmed.clone()));
    }
    Ok(confirmed)
}
