//! Corrections of the attribution (US-GRP-010).

use super::{Group, pending};

pub const ATTRIBUTION_CORRECT: &str = "attribution.correct";
pub const ATTRIBUTION_WITHDRAW: &str = "attribution.withdraw-correction";

pub(super) const GROUP: Group = Group {
    methods: &[
        pending(ATTRIBUTION_CORRECT, "US-GRP-010"),
        pending(ATTRIBUTION_WITHDRAW, "US-GRP-010"),
    ],
    ..Group::new("attribution")
};
