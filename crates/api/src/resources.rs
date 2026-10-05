//! What GitRaptor consumes on the user's machine: the result of
//! `engine.resources` (US-GRP-017, ADR-GRP-015 § 4).
//!
//! Only numbers, booleans and enums: no presentation text (NFR-10). A value
//! the system does not give is `None` ("not available"), never a zero.
//! The definitions are those of the footprint gate of INF-GRP-002: CPU is
//! the process CPU time (user + system) over wall time, in % of one core;
//! RSS is the resident memory of the daemon process; descriptors are the
//! open descriptors of the process (handles on Windows).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const MIB: u64 = 1024 * 1024;

/// The targets of RES-01, RES-02, RES-04, RES-05 and RES-09
/// (`non-functional.md` § Consumo de recursos). The single source of the
/// figures for the product; the gate of INF-GRP-002 uses the same ones.
pub const TARGETS: ResourceTargets = ResourceTargets {
    cpu_pct: 1.0,
    rss_bytes: 150 * MIB,
    open_fds: Some(256),
    inotify_share_pct: 50.0,
    profile_bytes: 250 * MIB,
    time_machine_bytes: 10 * 1024 * MIB,
};

/// The window of the mean CPU (RES-01): 10 minutes.
pub const CPU_WINDOW_S: u64 = 600;

/// Result of `engine.resources`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourcesResult {
    pub process: ProcessUsage,
    pub watches: WatchUsage,
    pub disk: DiskUsage,
    /// Active class of each pool (TS-GRP-005). `None` until it exists.
    pub pools: Option<Vec<PoolClass>>,
    /// Power saving mode (US-GRP-019). `None` until it exists.
    pub power_saving: Option<PowerSavingView>,
    /// What each value is measured against.
    pub targets: ResourceTargets,
}

/// The daemon process.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProcessUsage {
    pub cpu: CpuUsage,
    pub rss_bytes: Option<u64>,
    pub open_fds: Option<u64>,
}

/// CPU of the daemon over its window (RES-01).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CpuUsage {
    /// Mean over `window_s`, in % of one core.
    pub mean_pct: Option<f64>,
    /// Highest mean of one sampling interval of the window.
    pub peak_pct: Option<f64>,
    /// Seconds the mean really covers: up to [`CPU_WINDOW_S`], less while
    /// the daemon is younger.
    pub window_s: u64,
}

/// What the observer watches (RES-04).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WatchUsage {
    /// Roots watched recursively: worktrees and common directories.
    pub roots: u64,
    /// Linux only: the inotify watches of the process.
    pub inotify: Option<InotifyUsage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InotifyUsage {
    pub watches: u64,
    /// `/proc/sys/fs/inotify/max_user_watches`; `None` if unreadable.
    pub max_user_watches: Option<u64>,
}

/// Disk the profile takes, as allocated on disk (RES-05, RES-09).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiskUsage {
    /// The whole profile without the Time Machine.
    pub profile_bytes: u64,
    /// The Time Machine of each repo (store and oplog), retired ones too.
    pub time_machine: Vec<RepoDisk>,
    /// `false` when the read hit its time or entry bound: the sizes are
    /// then lower bounds.
    pub complete: bool,
}

impl DiskUsage {
    pub fn time_machine_bytes(&self) -> u64 {
        self.time_machine.iter().map(|r| r.bytes).sum()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepoDisk {
    pub repo_id: String,
    pub bytes: u64,
}

/// A pool of work of the daemon (ADR-GRP-015 § 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Pool {
    Observer,
    Executor,
    TimeMachineWriter,
    Capture,
    Maintenance,
    Predictor,
    Reconciliation,
}

/// The class of work a pool runs in (ADR-GRP-015 § 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum WorkClass {
    UserInitiated,
    Default,
    Utility,
    Background,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PoolClass {
    pub pool: Pool,
    pub class: WorkClass,
}

/// `engine.powerSaving` (ADR-GRP-015 § 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum PowerSavingSetting {
    Auto,
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PowerSavingView {
    pub setting: PowerSavingSetting,
    pub active: bool,
}

/// The targets each value is measured against.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResourceTargets {
    /// RES-01: mean CPU below this, in % of one core.
    pub cpu_pct: f64,
    /// RES-02: RSS below this.
    pub rss_bytes: u64,
    /// RES-04: at most this many open descriptors. `None` where the count
    /// has no baseline yet (Windows handles).
    pub open_fds: Option<u64>,
    /// RES-04: inotify watches at most this share of `max_user_watches`.
    pub inotify_share_pct: f64,
    /// RES-05: profile without the Time Machine at most this.
    pub profile_bytes: u64,
    /// RES-09: default `timeMachine.maxDiskSizeGiB`, for every repo
    /// together. A reference until US-TMC-022 enforces it.
    pub time_machine_bytes: u64,
}

impl ResourceTargets {
    pub fn cpu_within(&self, pct: f64) -> bool {
        pct < self.cpu_pct
    }

    pub fn rss_within(&self, bytes: u64) -> bool {
        bytes < self.rss_bytes
    }

    /// `None` when there is no target.
    pub fn fds_within(&self, fds: u64) -> Option<bool> {
        self.open_fds.map(|max| fds <= max)
    }

    /// `None` when the maximum is unknown.
    pub fn inotify_within(&self, usage: &InotifyUsage) -> Option<bool> {
        let max = usage.max_user_watches.filter(|m| *m > 0)?;
        Some(usage.watches as f64 * 100.0 <= self.inotify_share_pct * max as f64)
    }

    pub fn profile_within(&self, bytes: u64) -> bool {
        bytes <= self.profile_bytes
    }

    pub fn time_machine_within(&self, bytes: u64) -> bool {
        bytes <= self.time_machine_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same figures as the footprint gate of INF-GRP-002: 1 %, 150 MiB
    /// and 256 descriptors.
    #[test]
    fn targets_are_those_of_the_gate() {
        assert_eq!(TARGETS.cpu_pct, 1.0);
        assert_eq!(TARGETS.rss_bytes, 150 * 1024 * 1024);
        assert_eq!(TARGETS.open_fds, Some(256));
        assert!(TARGETS.cpu_within(0.99) && !TARGETS.cpu_within(1.0));
        assert!(TARGETS.rss_within(150 * MIB - 1) && !TARGETS.rss_within(150 * MIB));
        assert_eq!(TARGETS.fds_within(256), Some(true));
        assert_eq!(TARGETS.fds_within(257), Some(false));
        let half = InotifyUsage {
            watches: 50,
            max_user_watches: Some(100),
        };
        assert_eq!(TARGETS.inotify_within(&half), Some(true));
        let over = InotifyUsage {
            watches: 51,
            ..half
        };
        assert_eq!(TARGETS.inotify_within(&over), Some(false));
        let unknown = InotifyUsage {
            max_user_watches: None,
            ..half
        };
        assert_eq!(TARGETS.inotify_within(&unknown), None);
    }

    fn sample() -> ResourcesResult {
        ResourcesResult {
            process: ProcessUsage {
                cpu: CpuUsage {
                    mean_pct: Some(0.1),
                    peak_pct: Some(0.4),
                    window_s: 600,
                },
                rss_bytes: Some(40 * MIB),
                open_fds: Some(30),
            },
            watches: WatchUsage {
                roots: 3,
                inotify: None,
            },
            disk: DiskUsage {
                profile_bytes: 4096,
                time_machine: vec![RepoDisk {
                    repo_id: "r1".into(),
                    bytes: 8192,
                }],
                complete: true,
            },
            pools: None,
            power_saving: None,
            targets: TARGETS,
        }
    }

    /// NFR-10: the only strings of the result are repo ids; everything else
    /// is a number, a boolean, an enum or null.
    #[test]
    fn the_result_carries_no_presentation_text() {
        fn strings(v: &serde_json::Value, path: &str, out: &mut Vec<String>) {
            match v {
                serde_json::Value::String(_) => out.push(path.to_owned()),
                serde_json::Value::Array(a) => a.iter().for_each(|v| strings(v, path, out)),
                serde_json::Value::Object(o) => {
                    for (k, v) in o {
                        strings(v, &format!("{path}.{k}"), out);
                    }
                }
                _ => {}
            }
        }
        let mut result = sample();
        result.pools = Some(vec![PoolClass {
            pool: Pool::Observer,
            class: WorkClass::Default,
        }]);
        result.power_saving = Some(PowerSavingView {
            setting: PowerSavingSetting::Auto,
            active: false,
        });
        let mut found = Vec::new();
        strings(&serde_json::to_value(&result).unwrap(), "", &mut found);
        assert_eq!(
            found,
            [
                ".disk.time_machine.repo_id",
                ".pools.class",
                ".pools.pool",
                ".power_saving.setting",
            ]
        );
    }

    #[test]
    fn the_result_rejects_unknown_fields() {
        let mut value = serde_json::to_value(sample()).unwrap();
        let back: ResourcesResult = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(back, sample());
        value["process"]["label"] = "x".into();
        assert!(serde_json::from_value::<ResourcesResult>(value).is_err());
    }
}
