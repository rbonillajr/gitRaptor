//! The personal levels over the team's (BR-CONS-001): the profile's `settings.json` and the
//! repo's `settings.local.json` harden the team rules and never relax them.
//!
//! stub: replaced by the implementation slice. The signatures are the contract; the bodies
//! return the neutral value (no hardening, nothing ignored), which is today's behavior.

use gitraptor_api::guard::Level;

use crate::settings::document::Parsed;
use crate::settings::model::{Operation, Settings};
use crate::team::{EffectivePermissions, Permission, TeamConfig};

/// The two personal levels as read (BR-CONS-001): the profile's `settings.json` and the repo's
/// `settings.local.json`. `None` = absent or ignored.
#[derive(Debug, Clone, Copy, Default)]
pub struct Personal<'a> {
    pub profile: Option<&'a Settings>,
    pub local: Option<&'a Settings>,
}

/// The permission `settings` declares for `op`: the most restrictive of its lists that names
/// it; `None` when none does.
pub fn declared(_settings: &Settings, _op: Operation) -> Option<Permission> {
    // stub: replaced by the implementation slice
    None
}

/// The team permissions hardened by the personal levels: per operation the personal value is
/// the local one when the local declares it, else the profile one; the effective one is the
/// maximum of the team and that value. Never below `team`.
pub fn harden(team: &EffectivePermissions, _personal: Personal<'_>) -> EffectivePermissions {
    // stub: replaced by the implementation slice
    team.clone()
}

/// What a level that only hardens tried to relax.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelaxKey {
    Permission(Operation),
    SafeMinimum,
    BaseBranch,
    CommitAuthorship,
}

/// One relaxation that was ignored, and the level that declared it (`Worktree`, `Profile` or
/// `Local`).
// stub: the Brief derives `PartialOrd, Ord` here, but `api::guard::Level` has no `Ord` yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IgnoredRelaxation {
    pub level: Level,
    pub key: RelaxKey,
}

/// Every relaxation the worktree and the personal level in force declared against the team
/// and that was ignored. Sorted and deduplicated. Pure: never changes a decision.
/// `floor_may_relax`: the floor is the confirmed one and fully readable.
pub fn ignored_relaxations(
    _team: &TeamConfig,
    _profile: &Parsed,
    _local: &Parsed,
    _floor_may_relax: bool,
) -> Vec<IgnoredRelaxation> {
    // stub: replaced by the implementation slice
    Vec::new()
}
