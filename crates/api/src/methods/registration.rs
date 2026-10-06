//! Explicit registration of agents (US-GRP-009).

use super::{Group, method};

/// Registers an agent in a worktree (US-GRP-009): not reserved, the
/// daemon decides who asks and where (ADR-GRP-005 § 6.6).
pub const REGISTRATION_REGISTER: &str = "registration.register";
/// Withdraws a registration (reserved, ADR-GRP-005 § 6).
pub const REGISTRATION_WITHDRAW: &str = "registration.withdraw";

pub(super) const GROUP: Group = Group {
    methods: &[
        // Its result carries no paths (SEC-12); an agent registers itself in
        // the worktree of its working folder.
        method(REGISTRATION_REGISTER, false, true),
        method(REGISTRATION_WITHDRAW, true, false),
    ],
    ..Group::new("registration")
};
