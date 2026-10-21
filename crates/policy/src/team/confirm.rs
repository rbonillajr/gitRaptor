//! The plan of `guard confirm` (ADR-GRD-004 § 3–4): what confirming the team baseline of the
//! main branch would change, shown before any effect.
//!
//! Pure: no I/O. The plan compares the floor of the copy of the main branch with a **reference**:
//! what is confirmed or, with nothing confirmed, `main` with no team settings (what the install
//! would have confirmed). The base branch change is reported apart from the relaxations; both
//! decide whether the confirmation waits for the cancellable window.
//!
//! [`relaxations`] is the single criterion of "this floor relaxes that one": the team loader uses
//! it for `floor-relax-pending` and the plan for the list it shows, so they never disagree.

use std::collections::BTreeSet;

use gitraptor_git::RefName;

use super::{
    Confirmed, ConfirmedFloor, DEFAULT_BRANCH, MainCopy, ResolvedBase, TeamConfig, TeamSource,
    combine,
};
use crate::guard::authorship::Policy;
use crate::settings::diagnostic::{Code, SourceKind};
use crate::settings::document::SourceStatus;
use crate::settings::model::{AppliesTo, Operation, PatternPolicy, Settings};

/// A key of the closed list of relaxations the plan knows how to show (D3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelaxedKey {
    PermissionsAllow,
    PermissionsAsk,
    PermissionsDeny,
    DisableSafeMinimum,
    ProtectedBranches,
    ForbiddenPaths,
    CommitAuthorship,
}

impl RelaxedKey {
    /// The key path in the settings document.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PermissionsAllow => "permissions.allow",
            Self::PermissionsAsk => "permissions.ask",
            Self::PermissionsDeny => "permissions.deny",
            Self::DisableSafeMinimum => "permissions.disableSafeMinimum",
            Self::ProtectedBranches => "policies.protectedBranches",
            Self::ForbiddenPaths => "policies.forbiddenPaths",
            Self::CommitAuthorship => "policies.commitAuthorship",
        }
    }
}

/// One key that relaxes, with its value in the reference and in the new floor. Values are the
/// repo's text: untrusted, sanitised by whoever shows them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relaxation {
    pub key: RelaxedKey,
    pub before: Vec<String>,
    pub after: Vec<String>,
}

/// The base branch of the reference and the resolved one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseChange {
    pub from: Option<RefName>,
    pub to: RefName,
}

/// What the developer saw: source ref and commit, floor blob, resolved base and what was
/// confirmed before. Two plans with the same fingerprint confirm the same thing (D5).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Fingerprint(String);

impl Fingerprint {
    /// The canonical encoding (length-prefixed fields); hash it before putting it on a wire.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A floor relaxes something the plan cannot show: the confirmation fails closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unshowable;

/// Why there is no plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The resolved baseline already is the confirmed one, or the floor only hardens it.
    NothingToConfirm,
    /// `engine.baseBranch` is not a valid branch name.
    BaseInvalid,
    /// The floor is ignored or partial: confirming it would confirm something nobody can read.
    FloorUnreadable,
    /// The floor relaxes something the plan cannot show.
    UnshowableRelaxation,
}

/// The plan of one confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmPlan {
    /// The copy of the main branch the floor was read from.
    pub source: Option<MainCopy>,
    /// The blob of the floor; `None` with no team settings.
    pub floor_blob: Option<String>,
    pub base: BaseChange,
    pub relaxations: Vec<Relaxation>,
    pub fingerprint: Fingerprint,
    /// The confirmed floor could not be read: the reference fell back to no settings.
    reference_missing: bool,
}

impl ConfirmPlan {
    /// The confirmation waits for the cancellable window (D4): the base differs from the
    /// reference, or something relaxes. A missing confirmed floor always waits.
    pub fn needs_window(&self) -> bool {
        self.base.from.as_ref() != Some(&self.base.to)
            || !self.relaxations.is_empty()
            || self.reference_missing
    }

    /// What applying the plan stores.
    pub fn confirmed(&self) -> Confirmed {
        Confirmed {
            base_branch: self.base.to.clone(),
            floor: self
                .floor_blob
                .clone()
                .map_or(ConfirmedFloor::Absent, ConfirmedFloor::Blob),
        }
    }
}

/// One floor as the relaxation criterion reads it: its settings and whether it is fully
/// readable (only a readable floor turns the safe minimum off).
#[derive(Debug, Clone, Copy, Default)]
pub struct FloorView<'a> {
    pub settings: Option<&'a Settings>,
    pub readable: bool,
}

impl<'a> FloorView<'a> {
    /// The view of one team source.
    pub fn of(source: &'a TeamSource) -> Self {
        Self {
            settings: source.settings(),
            readable: source.status() == SourceStatus::Readable,
        }
    }
}

fn op_list(settings: Option<&Settings>, key: RelaxedKey) -> Vec<Operation> {
    let Some(p) = settings.and_then(|s| s.permissions.as_ref()) else {
        return Vec::new();
    };
    let list = match key {
        RelaxedKey::PermissionsAllow => &p.allow,
        RelaxedKey::PermissionsAsk => &p.ask,
        _ => &p.deny,
    };
    let mut out: Vec<Operation> = Vec::new();
    for op in list.iter().flatten() {
        if !out.contains(op) {
            out.push(*op);
        }
    }
    out
}

fn names(ops: &[Operation]) -> Vec<String> {
    ops.iter().map(|o| o.as_str().to_owned()).collect()
}

fn pattern_policy(settings: Option<&Settings>, key: RelaxedKey) -> Option<&PatternPolicy> {
    let p = settings?.policies.as_ref()?;
    match key {
        RelaxedKey::ProtectedBranches => p.protected_branches.as_ref(),
        _ => p.forbidden_paths.as_ref(),
    }
}

/// Patterns plus `appliesTo: everyone` when the person is held too.
fn pattern_values(policy: Option<&PatternPolicy>) -> (Vec<String>, bool) {
    let patterns = policy.and_then(|p| p.patterns.clone()).unwrap_or_default();
    let everyone = policy.and_then(|p| p.applies_to) == Some(AppliesTo::Everyone);
    (patterns, everyone)
}

fn shown_patterns(patterns: &[String], everyone: bool) -> Vec<String> {
    let mut out = patterns.to_vec();
    if everyone {
        out.push("appliesTo: everyone".to_owned());
    }
    out
}

fn authorship(settings: Option<&Settings>) -> Policy {
    settings
        .and_then(|s| s.policies.as_ref())
        .and_then(|p| p.commit_authorship.as_ref())
        .and_then(Policy::of)
        .unwrap_or(Policy::AgentsCommit)
}

fn authorship_value(policy: Policy) -> Vec<String> {
    let mode = match policy {
        Policy::HumanAuthorWarn => "human-author (warn)",
        other => other.mode(),
    };
    vec![mode.to_owned()]
}

/// Every key of the closed list in which `after` is less restrictive than `before`, in key
/// order. `Err(Unshowable)` when an effective permission relaxes and no key of the list
/// explains it (fail-closed).
pub fn relaxations(
    before: FloorView<'_>,
    after: FloorView<'_>,
) -> Result<Vec<Relaxation>, Unshowable> {
    let floor =
        |v: FloorView<'_>| combine(v.settings.map(|s| (SourceKind::Floor, s)), v.readable, &[]);
    let (eb, ea) = (floor(before), floor(after));
    let mut keys = BTreeSet::new();
    let minimum_off = eb.safe_minimum_active && !ea.safe_minimum_active;
    if minimum_off {
        keys.insert(RelaxedKey::DisableSafeMinimum);
    }
    let lists = |s| {
        [
            RelaxedKey::PermissionsAllow,
            RelaxedKey::PermissionsAsk,
            RelaxedKey::PermissionsDeny,
        ]
        .map(|k| (k, op_list(s, k)))
    };
    let (lb, la) = (lists(before.settings), lists(after.settings));
    for op in Operation::ALL {
        if ea.permission(op) >= eb.permission(op) {
            continue;
        }
        let mut explained = op == Operation::ForcePush && minimum_off;
        for ((key, b), (_, a)) in lb.iter().zip(&la) {
            let changed = if *key == RelaxedKey::PermissionsAllow {
                a.contains(&op) && !b.contains(&op)
            } else {
                b.contains(&op) && !a.contains(&op)
            };
            if changed {
                keys.insert(*key);
                explained = true;
            }
        }
        if !explained {
            return Err(Unshowable);
        }
    }

    let mut out = Vec::new();
    for key in keys {
        let (before_v, after_v) = match key {
            RelaxedKey::DisableSafeMinimum => (
                vec![(!eb.safe_minimum_active).to_string()],
                vec![(!ea.safe_minimum_active).to_string()],
            ),
            k => (
                names(&op_list(before.settings, k)),
                names(&op_list(after.settings, k)),
            ),
        };
        out.push(Relaxation {
            key,
            before: before_v,
            after: after_v,
        });
    }

    for key in [RelaxedKey::ProtectedBranches, RelaxedKey::ForbiddenPaths] {
        let (pb, everyone_b) = pattern_values(pattern_policy(before.settings, key));
        let (pa, everyone_a) = pattern_values(pattern_policy(after.settings, key));
        let removed = pb.iter().any(|p| !pa.contains(p));
        if removed || (everyone_b && !everyone_a) {
            out.push(Relaxation {
                key,
                before: shown_patterns(&pb, everyone_b),
                after: shown_patterns(&pa, everyone_a),
            });
        }
    }

    let (ab, aa) = (authorship(before.settings), authorship(after.settings));
    if aa < ab {
        out.push(Relaxation {
            key: RelaxedKey::CommitAuthorship,
            before: authorship_value(ab),
            after: authorship_value(aa),
        });
    }
    Ok(out)
}

/// Whether `after` relaxes `before` anywhere, or in a way that cannot be shown.
pub fn relaxes(before: FloorView<'_>, after: FloorView<'_>) -> bool {
    !matches!(relaxations(before, after), Ok(v) if v.is_empty())
}

fn push_field(out: &mut String, field: Option<&str>) {
    match field {
        None => out.push('-'),
        Some(s) => {
            out.push_str(&s.len().to_string());
            out.push(':');
            out.push_str(s);
        }
    }
    out.push(';');
}

fn fingerprint(
    config: &TeamConfig,
    resolved: &RefName,
    previous: Option<&Confirmed>,
) -> Fingerprint {
    let mut out = String::from("v1;");
    let copy = config.main_branch.copy.as_ref();
    push_field(&mut out, copy.map(|c| c.reference.as_str()));
    push_field(&mut out, copy.map(|c| c.commit.as_str()));
    push_field(&mut out, config.floor.blob.as_deref());
    push_field(&mut out, Some(resolved.as_str()));
    match previous {
        None => push_field(&mut out, None),
        Some(c) => {
            push_field(&mut out, Some(c.base_branch.as_str()));
            match &c.floor {
                ConfirmedFloor::Absent => push_field(&mut out, Some("absent")),
                ConfirmedFloor::Blob(id) => push_field(&mut out, Some(&format!("blob {id}"))),
            }
        }
    }
    Fingerprint(out)
}

/// The plan of confirming what `config` resolves, against `confirmed` (`None`: nothing
/// confirmed yet). `config` must have been loaded with the same `confirmed`.
///
/// # Errors
///
/// [`Refusal::FloorUnreadable`] with an ignored or partial floor, [`Refusal::BaseInvalid`] with
/// an invalid `engine.baseBranch` (the rest of the floor is not confirmed either),
/// [`Refusal::UnshowableRelaxation`] when a relaxation cannot be shown and
/// [`Refusal::NothingToConfirm`] when the resolved baseline is the confirmed one or only hardens
/// it.
pub fn plan(config: &TeamConfig, confirmed: Option<&Confirmed>) -> Result<ConfirmPlan, Refusal> {
    if matches!(
        config.floor.status(),
        SourceStatus::Ignored | SourceStatus::Partial
    ) {
        return Err(Refusal::FloorUnreadable);
    }
    let to = match &config.resolved_base {
        ResolvedBase::Valid(r) => r.clone(),
        ResolvedBase::Invalid => return Err(Refusal::BaseInvalid),
    };
    let missing = config
        .team_diagnostics()
        .iter()
        .any(|d| d.code == Code::ConfirmedFloorMissing);
    let main = RefName::new(DEFAULT_BRANCH).expect("valid default");
    let from = confirmed.map_or(main, |c| c.base_branch.clone());
    // The reference floor: none (no settings), the floor itself, or the confirmed one read by id.
    let (reference, identical): (Option<&TeamSource>, bool) = match confirmed {
        None => (None, false),
        Some(_) if missing => (None, false),
        Some(c) => match &c.floor {
            ConfirmedFloor::Absent => (None, config.floor.blob.is_none()),
            ConfirmedFloor::Blob(id) if config.floor.blob.as_deref() == Some(id.as_str()) => {
                (Some(&config.floor), true)
            }
            ConfirmedFloor::Blob(_) => (config.confirmed_floor.as_ref(), false),
        },
    };
    let before = reference.map_or(
        FloorView {
            settings: None,
            readable: true,
        },
        FloorView::of,
    );
    let relaxations = relaxations(before, FloorView::of(&config.floor))
        .map_err(|_| Refusal::UnshowableRelaxation)?;
    if confirmed.is_some() && !missing && from == to && (identical || relaxations.is_empty()) {
        return Err(Refusal::NothingToConfirm);
    }
    Ok(ConfirmPlan {
        source: config.main_branch.copy.clone(),
        floor_blob: config.floor.blob.clone(),
        fingerprint: fingerprint(config, &to, confirmed),
        base: BaseChange {
            from: Some(from),
            to,
        },
        relaxations,
        reference_missing: missing,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::document::parse_document;
    use crate::settings::model::Level;

    fn settings(json: &str) -> Settings {
        parse_document(json.as_bytes(), Level::Team, SourceKind::Floor)
            .settings
            .map(|s| (*s).clone())
            .expect("valid test document")
    }

    fn view(s: &Settings) -> FloorView<'_> {
        FloorView {
            settings: Some(s),
            readable: true,
        }
    }

    #[test]
    fn relaxation_compares_effective_decisions() {
        let deny_fp = settings(r#"{"permissions":{"deny":["force-push","push"]}}"#);
        let only_push = settings(r#"{"permissions":{"deny":["push"]}}"#);
        // Dropping a deny the minimum already covers is not a relaxation.
        assert!(!relaxes(view(&deny_fp), view(&only_push)));
        assert!(relaxes(view(&only_push), view(&Settings::default())));
        let off = settings(r#"{"permissions":{"deny":["push"],"disableSafeMinimum":true}}"#);
        assert!(relaxes(view(&only_push), view(&off)));
        assert!(!relaxes(view(&only_push), view(&deny_fp)));
    }

    #[test]
    fn a_partial_floor_never_turns_the_minimum_off() {
        let off = settings(r#"{"permissions":{"disableSafeMinimum":true}}"#);
        let partial = FloorView {
            settings: Some(&off),
            readable: false,
        };
        assert!(!relaxes(view(&Settings::default()), partial));
    }

    #[test]
    fn applies_to_everyone_dropped_is_a_relaxation() {
        let everyone = settings(
            r#"{"policies":{"protectedBranches":{"patterns":["release/*"],"appliesTo":"everyone"}}}"#,
        );
        let agents = settings(r#"{"policies":{"protectedBranches":{"patterns":["release/*"]}}}"#);
        let got = relaxations(view(&everyone), view(&agents)).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].key, RelaxedKey::ProtectedBranches);
        assert!(!relaxes(view(&agents), view(&everyone)));
    }
}
