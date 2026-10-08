//! What the engine consumes on the machine (US-GRP-017, ADR-GRP-015 § 4):
//! the measurements behind `engine.resources`.
//!
//! The daemon measures itself in process, with the same definitions as the
//! footprint gate of INF-GRP-002, and never starts a process to do it
//! (SEC-08). A thread samples the CPU every 10 s to keep the 10-minute
//! window of RES-01; everything else is read when asked.

pub mod disk;
pub mod meter;
pub mod window;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use gitraptor_api::clock::monotonic_ns;
use gitraptor_api::resources::{
    CPU_WINDOW_S, CpuUsage, DiskUsage, ProcessUsage, ResourceTargets, ResourcesResult, TARGETS,
    WatchUsage,
};

use crate::profile::ProfileDirs;
pub use disk::DiskLimits;
use window::{CpuPoint, CpuWindow};

/// Debug-build test hook: lowers resource targets so a scenario can put a
/// real daemon over one (US-GRP-017, escenario 2). `key=value` pairs
/// separated by `,`, keys `cpu_pct`, `rss_bytes`, `open_fds` and
/// `profile_bytes`; every value must be at most the real target, and any
/// error discards the whole override. Release builds do not read it, like
/// `GITRAPTOR_AGENT_EXECUTABLES`.
pub const RESOURCE_TARGETS_ENV: &str = "GITRAPTOR_RESOURCE_TARGETS";

/// How the engine measures itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResourceConfig {
    /// CPU sampling interval; the peak is the highest of these intervals.
    pub sample_every: Duration,
    /// Window of the mean CPU (RES-01).
    pub window: Duration,
    pub disk: DiskLimits,
    /// How long a disk reading is reused: walking the profile costs CPU,
    /// which would show in the next reading.
    pub disk_cache: Duration,
    pub targets: ResourceTargets,
}

impl Default for ResourceConfig {
    fn default() -> Self {
        let mut targets = TARGETS;
        // Windows handles have no baseline yet: shown, not judged.
        if cfg!(windows) {
            targets.open_fds = None;
        }
        Self {
            sample_every: Duration::from_secs(10),
            window: Duration::from_secs(CPU_WINDOW_S),
            disk: DiskLimits::default(),
            disk_cache: Duration::from_secs(60),
            targets,
        }
    }
}

impl ResourceConfig {
    /// The defaults with the debug-only override of the targets applied.
    pub fn from_env() -> Self {
        let mut config = Self::default();
        if cfg!(debug_assertions)
            && let Some(targets) = std::env::var(RESOURCE_TARGETS_ENV)
                .ok()
                .and_then(|v| parse_targets(&v, config.targets))
        {
            config.targets = targets;
        }
        config
    }
}

/// Strict: an unknown key, a bad number or a value above the real target
/// discards the override.
fn parse_targets(text: &str, base: ResourceTargets) -> Option<ResourceTargets> {
    let mut t = base;
    for pair in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (key, value) = pair.split_once('=')?;
        match key.trim() {
            "cpu_pct" => {
                let v: f64 = value.trim().parse().ok()?;
                (v.is_finite() && v >= 0.0 && v <= base.cpu_pct).then_some(())?;
                t.cpu_pct = v;
            }
            "rss_bytes" => {
                let v: u64 = value.trim().parse().ok()?;
                (v <= base.rss_bytes).then_some(())?;
                t.rss_bytes = v;
            }
            "open_fds" => {
                let v: u64 = value.trim().parse().ok()?;
                (v <= base.open_fds?).then_some(())?;
                t.open_fds = Some(v);
            }
            "profile_bytes" => {
                let v: u64 = value.trim().parse().ok()?;
                (v <= base.profile_bytes).then_some(())?;
                t.profile_bytes = v;
            }
            _ => return None,
        }
    }
    Some(t)
}

struct Inner {
    config: ResourceConfig,
    dirs: ProfileDirs,
    window: Mutex<CpuWindow>,
    disk: Mutex<Option<(Instant, DiskUsage)>>,
    roots: Arc<AtomicU64>,
    /// The observer's tiers (TS-GRP-006), once it runs.
    observation: Mutex<Option<ObservationSource>>,
}

/// Reads the observation tiers when `engine.resources` is asked.
pub type ObservationSource =
    Arc<dyn Fn() -> Option<gitraptor_api::resources::ObservationUsage> + Send + Sync>;

impl Inner {
    fn point() -> Option<CpuPoint> {
        let cpu_ns = meter::sample().cpu_ns?;
        Some(CpuPoint {
            at_ns: monotonic_ns(),
            cpu_ns,
        })
    }

    fn record(&self) {
        if let Some(point) = Self::point() {
            self.window
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(point);
        }
    }
}

/// The engine's own measurements, shared by the daemon and the channel.
pub struct ResourceMonitor {
    inner: Arc<Inner>,
    sampler: Mutex<Option<(Sender<()>, JoinHandle<()>)>>,
}

impl ResourceMonitor {
    pub fn new(config: ResourceConfig, dirs: ProfileDirs, roots: Arc<AtomicU64>) -> Self {
        let inner = Arc::new(Inner {
            window: Mutex::new(CpuWindow::new(
                u64::try_from(config.window.as_nanos()).unwrap_or(u64::MAX),
            )),
            config,
            dirs,
            disk: Mutex::new(None),
            roots,
            observation: Mutex::new(None),
        });
        inner.record();
        Self {
            inner,
            sampler: Mutex::new(None),
        }
    }

    /// The counter the observer keeps of its watched roots.
    pub fn roots_counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.inner.roots)
    }

    /// Where the observation tiers are read from (TS-GRP-006).
    pub fn set_observation(&self, source: ObservationSource) {
        *self
            .inner
            .observation
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(source);
    }

    /// Starts the sampling thread. It wakes once per interval and ends as
    /// soon as [`ResourceMonitor::stop`] is called.
    pub fn start(&self) {
        let mut sampler = self.sampler.lock().unwrap_or_else(|e| e.into_inner());
        if sampler.is_some() {
            return;
        }
        let (tx, rx) = channel::<()>();
        let inner = Arc::clone(&self.inner);
        let every = inner.config.sample_every;
        let spawned = std::thread::Builder::new()
            .name("raptor-resources".into())
            .spawn(move || {
                while let Err(RecvTimeoutError::Timeout) = rx.recv_timeout(every) {
                    inner.record();
                }
            });
        if let Ok(handle) = spawned {
            *sampler = Some((tx, handle));
        }
    }

    pub fn stop(&self) {
        let taken = self
            .sampler
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some((tx, handle)) = taken {
            drop(tx);
            let _ = handle.join();
        }
    }

    /// The result of `engine.resources`. The live CPU sample is taken
    /// before the disk is walked, so the walk does not count in it.
    pub fn read(&self) -> ResourcesResult {
        let live = meter::sample();
        let now = live.cpu_ns.map(|cpu_ns| CpuPoint {
            at_ns: monotonic_ns(),
            cpu_ns,
        });
        let cpu = match now {
            Some(now) => {
                let stats = self
                    .inner
                    .window
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .stats(now);
                CpuUsage {
                    mean_pct: stats.mean_pct,
                    peak_pct: stats.peak_pct,
                    window_s: stats.window_s,
                }
            }
            None => CpuUsage {
                mean_pct: None,
                peak_pct: None,
                window_s: 0,
            },
        };
        ResourcesResult {
            process: ProcessUsage {
                cpu,
                rss_bytes: live.rss_bytes,
                open_fds: live.open_fds,
            },
            watches: WatchUsage {
                roots: self.inner.roots.load(Ordering::Relaxed),
                inotify: meter::inotify(),
            },
            disk: self.disk(),
            pools: None,
            power_saving: None,
            observation: self
                .inner
                .observation
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .as_ref()
                .and_then(|read| read()),
            targets: self.inner.config.targets,
        }
    }

    fn disk(&self) -> DiskUsage {
        let mut cache = self.inner.disk.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((at, usage)) = cache.as_ref()
            && at.elapsed() < self.inner.config.disk_cache
        {
            return usage.clone();
        }
        let usage = disk::measure(&self.inner.dirs, self.inner.config.disk);
        *cache = Some((Instant::now(), usage.clone()));
        usage
    }
}

impl Drop for ResourceMonitor {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_target_override_is_strict_and_only_lowers() {
        let t = parse_targets("rss_bytes=1", TARGETS).unwrap();
        assert_eq!(t.rss_bytes, 1);
        assert_eq!(t.cpu_pct, TARGETS.cpu_pct);
        let t = parse_targets("cpu_pct=0.5, open_fds=3,profile_bytes=0", TARGETS).unwrap();
        assert_eq!((t.cpu_pct, t.open_fds, t.profile_bytes), (0.5, Some(3), 0));
        for bad in [
            "rss_bytes=99999999999",
            "cpu_pct=2",
            "cpu_pct=NaN",
            "watts=1",
            "rss_bytes",
            "rss_bytes=-1",
            "rss_bytes=1,x=2",
        ] {
            assert_eq!(parse_targets(bad, TARGETS), None, "{bad}");
        }
    }

    #[test]
    fn a_reading_has_every_value_and_the_targets() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = ProfileDirs::under_root(tmp.path());
        std::fs::create_dir_all(dirs.data.join("tm/r1")).unwrap();
        std::fs::write(dirs.data.join("tm/r1/oplog.sqlite"), [1u8; 4096]).unwrap();
        let roots = Arc::new(AtomicU64::new(3));
        let monitor = ResourceMonitor::new(ResourceConfig::default(), dirs, roots);
        monitor.start();
        let r = monitor.read();
        monitor.stop();
        assert_eq!(r.watches.roots, 3);
        assert_eq!(r.disk.time_machine.len(), 1);
        assert_eq!(r.disk.time_machine[0].repo_id, "r1");
        assert_eq!(r.targets.rss_bytes, TARGETS.rss_bytes);
        assert!(r.pools.is_none() && r.power_saving.is_none());
        #[cfg(any(target_os = "macos", target_os = "linux", windows))]
        {
            assert!(r.process.rss_bytes.unwrap() > 0);
            assert!(r.process.open_fds.unwrap() > 0);
        }
    }

    /// The sampler ends at once when stopped, not at its next wake-up.
    #[test]
    fn the_sampler_stops_without_waiting_for_its_interval() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ResourceConfig {
            sample_every: Duration::from_secs(3600),
            ..ResourceConfig::default()
        };
        let monitor =
            ResourceMonitor::new(config, ProfileDirs::under_root(tmp.path()), Arc::default());
        monitor.start();
        let start = Instant::now();
        monitor.stop();
        assert!(start.elapsed() < Duration::from_secs(60));
    }
}
