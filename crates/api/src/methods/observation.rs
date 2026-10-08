//! Observation tiers (TS-GRP-006, ADR-GRP-010, Enmienda 2026-10-07, N8).

use super::Group;
use crate::capability::Capability;

/// The observation tier of each repo: `RepoView::tier` and
/// `RepoView::checked_utc_ms`, `RepoSummaryView::tier`, the `repo.tier`
/// event and the `observation` block of `engine.resources`. A connection
/// without it never sees them.
pub const CAP_OBSERVATION_TIERS: Capability = Capability::new("observation.tiers");

pub(super) const GROUP: Group = Group {
    capabilities: &[CAP_OBSERVATION_TIERS],
    ..Group::new("observation")
};
