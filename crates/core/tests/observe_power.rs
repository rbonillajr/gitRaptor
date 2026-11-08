//! TS-GRP-005 T009 (SAV1, guards 9.1 to 9.3): the observer follows the power mode. The
//! schedule is checked with the pure functions of `watch::schedule` and a simulated timeline;
//! the live observer with a temporary repo (NFR-01). Waits are on signals (the sink), never a
//! fixed sleep to see something happen; the one wait for an absence is bounded by the period.
//!
//! Signatures expected from the plan (T009, A3):
//! - `WatchConfig.saving_periodic: Duration` (15 min by default);
//! - `watch::schedule::periodic_period(config: &WatchConfig, saving: bool) -> Duration`;
//! - `watch::schedule::reconciliations_in(config: &WatchConfig, window: Duration,
//!   timeline: &[(Duration, bool)]) -> u32`: `timeline` lists the mode changes as (offset from
//!   the start, saving); the first entry, at 0, is the initial mode; each reconciliation is due
//!   one period after the previous one (or the start), the period being that of the mode when
//!   the previous one fired; it counts the ones due in `(0, window]`;
//! - `Observer::set_power_saving(&self, active: bool)`;
//! - `TierConfig::with_power_saving(self, active: bool) -> TierConfig`, the rule of
//!   `from_settings(.., true)` over the base.

use std::time::Duration;

use gitraptor_core::daemon::TierConfig;
use gitraptor_core::watch::schedule::{periodic_period, reconciliations_in};
use gitraptor_core::watch::{Observer, WatchConfig};

const MIN: Duration = Duration::from_secs(60);

#[test]
fn sixty_minutes_have_4_reconciliations_on_battery_and_12_on_ac() {
    let config = WatchConfig::default();
    assert_eq!(
        config.saving_periodic,
        15 * MIN,
        "WatchConfig.saving_periodic defaults to 15 min"
    );
    assert_eq!(
        reconciliations_in(&config, 60 * MIN, &[(Duration::ZERO, true)]),
        4,
        "60 simulated minutes on battery: one reconciliation every 15 min"
    );
    assert_eq!(
        reconciliations_in(&config, 60 * MIN, &[(Duration::ZERO, false)]),
        12,
        "60 simulated minutes on AC: one reconciliation every 5 min"
    );
}

#[test]
fn the_saving_period_is_the_longer_of_periodic_and_saving_periodic() {
    let config = WatchConfig::default();
    assert_eq!(periodic_period(&config, false), 5 * MIN);
    assert_eq!(periodic_period(&config, true), 15 * MIN);
    let long = WatchConfig {
        periodic: 20 * MIN,
        ..config
    };
    assert_eq!(
        periodic_period(&long, true),
        20 * MIN,
        "saving never shortens a periodic longer than saving_periodic: max(periodic, saving_periodic)"
    );
    assert_eq!(periodic_period(&long, false), 20 * MIN);
}

#[test]
fn back_on_ac_the_period_in_the_timeline_returns_to_five_minutes() {
    let config = WatchConfig::default();
    // 30 min on battery (2 reconciliations: 15, 30), then 30 min on AC (6: 35 .. 60).
    let timeline = [(Duration::ZERO, true), (30 * MIN, false)];
    assert_eq!(
        reconciliations_in(&config, 60 * MIN, &timeline),
        8,
        "battery for the first half and AC for the second: 2 + 6 reconciliations"
    );
}

#[test]
fn power_saving_lengthens_the_dormant_safety_nets() {
    let base = TierConfig {
        sweep_every: Duration::from_secs(120),
        reconcile_every: 60 * MIN,
        ..TierConfig::default()
    };
    let saving = base.with_power_saving(true);
    assert!(
        saving.sweep_every >= Duration::from_secs(300),
        "saving lengthens the dormant sweep to >= 300 s, got {:?}",
        saving.sweep_every
    );
    assert!(
        saving.reconcile_every >= 180 * MIN,
        "saving lengthens the slow reconciliation to >= 180 min, got {:?}",
        saving.reconcile_every
    );
    assert_eq!(
        base.with_power_saving(false),
        base,
        "without saving the base config is untouched"
    );
}

#[test]
fn the_observer_follows_the_mode_in_its_dormant_safety_nets() {
    let observer = Observer::start(WatchConfig::default(), std::sync::Arc::new(|_| {}));
    let usage = observer.usage_source();
    let sweep = |o: &Observer| {
        let _ = o;
        usage().expect("the observer reports its tiers").dormant
    };
    let before = sweep(&observer);
    assert_eq!(before.sweep_interval_s, 120);
    observer.set_power_saving(true);
    let saving = sweep(&observer);
    assert!(
        saving.sweep_interval_s >= 300,
        "in saving the dormant sweep interval published is >= 300 s, got {}",
        saving.sweep_interval_s
    );
    assert!(
        observer.reconcile_interval() >= 180 * MIN,
        "in saving the slow reconciliation interval is >= 180 min, got {:?}",
        observer.reconcile_interval()
    );
    observer.set_power_saving(false);
    assert_eq!(
        sweep(&observer).sweep_interval_s,
        120,
        "back on AC the dormant sweep returns to its configured value"
    );
    assert_eq!(observer.reconcile_interval(), 60 * MIN);
}

#[cfg(target_os = "macos")]
mod live {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use gitraptor_core::observe::{self, reconcile};
    use gitraptor_core::profile::GapCause;
    use gitraptor_core::watch::{Observer, WatchBackend, WatchConfig};
    use gitraptor_testkit::Fixture;
    use gitraptor_testkit::fixture::git_from_path;

    /// (is a periodic-reconciliation batch, when).
    type Seen = Arc<Mutex<Vec<(bool, Instant)>>>;

    fn wait_for(
        what: &str,
        seen: &Seen,
        limit: Duration,
        done: impl Fn(&[(bool, Instant)]) -> bool,
    ) {
        let start = Instant::now();
        while !done(&seen.lock().unwrap()) {
            assert!(start.elapsed() < limit, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn back_on_ac_the_next_reconciliation_moves_within_60_s() {
        let f = Fixture::new(&git_from_path());
        f.write("login.txt", "user\n");
        f.git(&["add", "login.txt"]);
        f.git(&["commit", "-q", "-m", "login"]);
        f.git(&["branch", "feat-login"]);
        let wt = f
            .add_worktree("feat-login", "feat-login")
            .canonicalize()
            .unwrap();

        let seen: Seen = Arc::default();
        let sink_seen = seen.clone();
        let config = WatchConfig {
            backend: WatchBackend::Fsevents,
            periodic: Duration::from_secs(8),
            saving_periodic: Duration::from_secs(3600),
            ..WatchConfig::default()
        };
        let observer = Observer::start(
            config,
            Arc::new(move |b| {
                let periodic = b
                    .gap
                    .is_some_and(|g| g.cause == GapCause::PeriodicReconciliation);
                sink_seen.lock().unwrap().push((periodic, Instant::now()));
            }),
        );
        observer.set_power_saving(true);
        let common = observe::locate(&f.repo).unwrap();
        let read = reconcile(&common, &observe::base_branch(None)).unwrap();
        observer.watch_repo("r", &common, &read);

        std::fs::write(wt.join("ready.txt"), "r\n").unwrap();
        wait_for(
            "the streams to be live",
            &seen,
            Duration::from_secs(15),
            |s| s.iter().any(|(p, _)| !*p),
        );
        std::thread::sleep(Duration::from_millis(500));
        // A change whose event is lost: only a periodic reconciliation finds it.
        observer.simulate_lost_events(true);
        std::fs::write(wt.join("login.txt"), "lost\n").unwrap();
        std::thread::sleep(Duration::from_millis(600));
        observer.simulate_lost_events(false);
        assert!(
            !seen.lock().unwrap().iter().any(|(p, _)| *p),
            "in saving no periodic reconciliation may be due yet"
        );

        let switched = Instant::now();
        observer.set_power_saving(false);
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            !seen.lock().unwrap().iter().any(|(p, _)| *p),
            "switching the mode must not fire a reconciliation: it only moves the next deadline"
        );
        wait_for(
            "the periodic reconciliation after the switch",
            &seen,
            Duration::from_secs(60),
            |s| s.iter().any(|(p, _)| *p),
        );
        let fired = seen.lock().unwrap().iter().find(|(p, _)| *p).unwrap().1;
        assert!(
            fired.duration_since(switched) <= Duration::from_secs(60),
            "back on AC the next reconciliation must move within 60 s"
        );
    }
}
