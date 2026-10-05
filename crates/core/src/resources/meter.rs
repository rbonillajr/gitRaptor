//! The daemon measures itself, in process, without starting any process
//! (SEC-08). Same definitions as the footprint gate of INF-GRP-002, which
//! reads the same values from outside (`ps`, `lsof`, `/proc/<pid>`).

/// One reading of the current process. A value the OS does not give is
/// `None`, never a zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProcessSample {
    /// User plus system CPU time since the process started, in ns.
    pub cpu_ns: Option<u64>,
    /// Resident memory, in bytes.
    pub rss_bytes: Option<u64>,
    /// Open descriptors (handles on Windows).
    pub open_fds: Option<u64>,
}

/// Reads the current process.
pub fn sample() -> ProcessSample {
    imp::sample()
}

/// inotify watches of the process and the user's maximum (Linux only).
pub fn inotify() -> Option<gitraptor_api::resources::InotifyUsage> {
    imp::inotify()
}

/// Entries of a descriptor folder (`/dev/fd`, `/proc/self/fd`), minus the
/// one the listing itself opens.
#[cfg(unix)]
fn count_fds(dir: &str) -> Option<u64> {
    let n = std::fs::read_dir(dir).ok()?.count() as u64;
    Some(n.saturating_sub(1))
}

#[cfg(unix)]
fn cpu_ns() -> Option<u64> {
    use nix::sys::resource::{UsageWho, getrusage};
    let usage = getrusage(UsageWho::RUSAGE_SELF).ok()?;
    let ns = |t: nix::sys::time::TimeVal| {
        u64::try_from(t.tv_sec()).unwrap_or(0) * 1_000_000_000
            + u64::try_from(t.tv_usec()).unwrap_or(0) * 1_000
    };
    Some(ns(usage.user_time()) + ns(usage.system_time()))
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use libproc::libproc::proc_pid::pidinfo;
    use libproc::libproc::task_info::TaskInfo;

    pub(super) fn sample() -> ProcessSample {
        let rss = i32::try_from(std::process::id())
            .ok()
            .and_then(|pid| pidinfo::<TaskInfo>(pid, 0).ok())
            .map(|info| info.pti_resident_size);
        ProcessSample {
            cpu_ns: cpu_ns(),
            rss_bytes: rss,
            open_fds: count_fds("/dev/fd"),
        }
    }

    pub(super) fn inotify() -> Option<gitraptor_api::resources::InotifyUsage> {
        None
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use gitraptor_api::resources::InotifyUsage;

    /// `VmRSS` of `/proc/self/status`, as the gate reads it.
    fn rss_bytes() -> Option<u64> {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        parse_vm_rss(&status)
    }

    pub(super) fn sample() -> ProcessSample {
        ProcessSample {
            cpu_ns: cpu_ns(),
            rss_bytes: rss_bytes(),
            open_fds: count_fds("/proc/self/fd"),
        }
    }

    /// The `inotify wd:` lines of every inotify descriptor of the process.
    pub(super) fn inotify() -> Option<InotifyUsage> {
        let mut watches = 0u64;
        let mut any = false;
        for entry in std::fs::read_dir("/proc/self/fdinfo").ok()?.flatten() {
            let Ok(info) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            let n = info
                .lines()
                .filter(|l| l.starts_with("inotify wd:"))
                .count() as u64;
            if n > 0 || info.contains("inotify") {
                any = true;
            }
            watches += n;
        }
        let max = std::fs::read_to_string("/proc/sys/fs/inotify/max_user_watches")
            .ok()
            .and_then(|s| s.trim().parse().ok());
        Some(InotifyUsage {
            watches: if any { watches } else { 0 },
            max_user_watches: max,
        })
    }
}

#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
mod imp {
    use super::*;

    pub(super) fn sample() -> ProcessSample {
        ProcessSample {
            cpu_ns: cpu_ns(),
            rss_bytes: None,
            open_fds: count_fds("/dev/fd"),
        }
    }

    pub(super) fn inotify() -> Option<gitraptor_api::resources::InotifyUsage> {
        None
    }
}

#[cfg(windows)]
mod imp {
    use super::*;

    pub(super) fn sample() -> ProcessSample {
        let u = gitraptor_winsys::usage::current();
        ProcessSample {
            cpu_ns: u.cpu_ns,
            rss_bytes: u.resident_bytes,
            open_fds: u.handles,
        }
    }

    pub(super) fn inotify() -> Option<gitraptor_api::resources::InotifyUsage> {
        None
    }
}

/// `VmRSS:   1234 kB` → bytes.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_vm_rss(status: &str) -> Option<u64> {
    let line = status.lines().find(|l| l.starts_with("VmRSS:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_rss_is_read_in_kib() {
        let status = "Name:\traptor\nVmRSS:\t   2048 kB\nThreads:\t4\n";
        assert_eq!(parse_vm_rss(status), Some(2048 * 1024));
        assert_eq!(parse_vm_rss("Name:\traptor\n"), None);
    }

    /// The test process has resident memory, CPU time that grows and at
    /// least stdin, stdout and stderr open.
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    #[test]
    fn the_current_process_is_measured() {
        let first = sample();
        assert!(first.rss_bytes.unwrap() > 0);
        assert!(first.open_fds.unwrap() >= 3, "{first:?}");
        let mut x = 0u64;
        for i in 0..20_000_000u64 {
            x = x.wrapping_add(i.wrapping_mul(i));
        }
        std::hint::black_box(x);
        assert!(sample().cpu_ns.unwrap() >= first.cpu_ns.unwrap());
    }
}
