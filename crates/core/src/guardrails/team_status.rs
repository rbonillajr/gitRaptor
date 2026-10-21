//! The base branches Guardrails protects and what the protection status says about the team
//! baseline (ADR-GRD-004 § 3, ADR-GRD-005 § 1): both read from the team loader and the
//! confirmation in the store, never from the install journal.
//!
//! Read-only. When the team level cannot be read the bases fall back to the union of `main`,
//! the main branch, the confirmed base and the bases the install recorded (§ 3.6): never fewer.

use gitraptor_api::guard::{ConfirmAction, ConfirmHint, Diagnostic};
use gitraptor_git::{ReadError, RepoReader};
use gitraptor_policy::settings::Code;
use gitraptor_policy::team::{Confirmed, DEFAULT_BRANCH, TeamConfig, confirm};

use super::evaluate;

/// The team level of the repo `reader` was opened on, with what the store confirmed.
pub fn read(reader: &RepoReader, confirmed: Option<&Confirmed>) -> Result<TeamConfig, ReadError> {
    super::layers::LOADER.load(reader, confirmed)
}

fn push_unique(out: &mut Vec<String>, name: &str) {
    if !out.iter().any(|b| b == name) {
        out.push(name.to_owned());
    }
}

/// The bases Guardrails protects: the loader's `guarded_base_branches()`, or, when it could not
/// read the team level (`team` is `None`), the fail-safe union of `main`, the main branch (when `reader` resolves
/// it), the confirmed base and `recorded`.
pub fn guarded_bases(
    team: Option<&TeamConfig>,
    reader: Option<&RepoReader>,
    confirmed: Option<&Confirmed>,
    recorded: &[String],
) -> Vec<String> {
    if let Some(team) = team {
        return team
            .guarded_base_branches()
            .iter()
            .map(|b| b.as_str().to_owned())
            .collect();
    }
    let mut out = match reader {
        Some(reader) => evaluate::default_bases(reader),
        None => vec![DEFAULT_BRANCH.to_owned()],
    };
    if let Some(c) = confirmed {
        push_unique(&mut out, c.base_branch.as_str());
    }
    for b in recorded {
        push_unique(&mut out, b);
    }
    out
}

/// The diagnostics of the status that the team level produces.
pub fn diagnostics(team: &TeamConfig) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for d in team.team_diagnostics() {
        let mapped = match d.code {
            Code::BaseUnconfirmed => Diagnostic::BaseUnconfirmed,
            Code::BaseChangePending { invalid: false } => Diagnostic::BaseChangePending,
            Code::BaseChangePending { invalid: true } => Diagnostic::BaseChangePendingInvalid,
            Code::FloorRelaxPending => Diagnostic::FloorRelaxPending,
            _ => continue,
        };
        if !out.contains(&mapped) {
            out.push(mapped);
        }
    }
    out
}

/// The hint to confirm: present exactly when `guard confirm` would have a plan to show. The
/// same function decides it and the plan itself (no hint with an invalid base).
pub fn hint(team: &TeamConfig, confirmed: Option<&Confirmed>) -> Option<ConfirmHint> {
    confirm::plan(team, confirmed).ok().map(|_| ConfirmHint {
        action: ConfirmAction::GuardConfirm,
    })
}

/// What the status of a protected repo says about its bases.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedView {
    pub bases: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
    pub confirm: Option<ConfirmHint>,
}

/// The bases, diagnostics and hint of a protected repo; `recorded` are the bases its install
/// recorded.
pub fn protected(
    team: Option<&TeamConfig>,
    reader: Option<&RepoReader>,
    confirmed: Option<&Confirmed>,
    recorded: &[String],
) -> ProtectedView {
    let bases = guarded_bases(team, reader, confirmed, recorded);
    match team {
        Some(team) => ProtectedView {
            bases,
            diagnostics: diagnostics(team),
            confirm: hint(team, confirmed),
        },
        // Nothing can be shown to be confirmed: say so, offer nothing.
        None => ProtectedView {
            bases,
            diagnostics: if confirmed.is_none() {
                vec![Diagnostic::BaseUnconfirmed]
            } else {
                Vec::new()
            },
            confirm: None,
        },
    }
}
