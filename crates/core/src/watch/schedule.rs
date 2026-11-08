//! The schedule of the periodic reconciliation under power saving (ADR-GRP-015 § 2;
//! DS-TS-GRP-005 D13), as pure functions of the configuration and the mode.

use std::time::Duration;

use super::WatchConfig;

/// The period of the full reconciliation: `periodic`, or `max(periodic, saving_periodic)` while
/// saving, so saving never shortens it.
#[must_use]
pub fn periodic_period(config: &WatchConfig, saving: bool) -> Duration {
    if saving {
        config.periodic.max(config.saving_periodic)
    } else {
        config.periodic
    }
}

/// Dormant sweep interval while saving power, at least (ADR-GRP-015, TS-GRP-006 D12).
pub const SAVING_DORMANT_POLL: Duration = Duration::from_secs(300);
/// Shortest slow reconciliation interval while saving power, at least.
pub const SAVING_DORMANT_RECONCILE: Duration = Duration::from_secs(180 * 60);

/// The dormant safety nets, `(sweep, slow reconciliation)`, from their configured values: while
/// saving each is at least its saving value, never shorter than configured.
#[must_use]
pub fn dormant_intervals(
    poll: Duration,
    reconcile: Duration,
    saving: bool,
) -> (Duration, Duration) {
    if saving {
        (
            poll.max(SAVING_DORMANT_POLL),
            reconcile.max(SAVING_DORMANT_RECONCILE),
        )
    } else {
        (poll, reconcile)
    }
}

/// How many periodic reconciliations are due in `(0, window]`.
///
/// `timeline` lists the mode changes as `(offset from the start, saving)`, the first at 0 being
/// the initial mode; an empty timeline means no saving. Each reconciliation is due one period
/// after the previous one (or the start), the period being that of the mode in force when the
/// previous one fired.
#[must_use]
pub fn reconciliations_in(
    config: &WatchConfig,
    window: Duration,
    timeline: &[(Duration, bool)],
) -> u32 {
    let saving_at = |at: Duration| {
        timeline
            .iter()
            .filter(|(offset, _)| *offset <= at)
            .max_by_key(|(offset, _)| *offset)
            .is_some_and(|(_, saving)| *saving)
    };
    let mut count = 0u32;
    let mut at = Duration::ZERO;
    loop {
        let period = periodic_period(config, saving_at(at));
        if period.is_zero() {
            // A zero period would never advance: nothing meaningful to count.
            return count;
        }
        at = at.saturating_add(period);
        if at > window {
            return count;
        }
        count = count.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: Duration = Duration::from_secs(60);

    #[test]
    fn sixty_minutes_have_4_reconciliations_on_battery_and_12_on_ac() {
        let config = WatchConfig::default();
        assert_eq!(
            reconciliations_in(&config, 60 * MIN, &[(Duration::ZERO, true)]),
            4
        );
        assert_eq!(
            reconciliations_in(&config, 60 * MIN, &[(Duration::ZERO, false)]),
            12
        );
    }

    #[test]
    fn a_change_applies_from_the_reconciliation_after_it() {
        let config = WatchConfig::default();
        let timeline = [(Duration::ZERO, true), (30 * MIN, false)];
        assert_eq!(reconciliations_in(&config, 60 * MIN, &timeline), 8);
        assert_eq!(reconciliations_in(&config, 60 * MIN, &[]), 12);
    }
}
