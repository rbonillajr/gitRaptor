//! What the daemon reads about the process on the other end of the channel
//! and its ancestors (ADR-GRP-005 § 5 and § 6.1).
//!
//! Every value comes from the kernel, never from the client. The identifier
//! that is never reused is `(pid, start time)` (ADR-GRP-012): on macOS the
//! audit token's `pidversion` cannot be checked against another process
//! without `unsafe` (Enmienda TS-GRP-004 a ADR-GRP-005), so the walk checks
//! start times instead.

use std::path::PathBuf;

// What the client checks too lives once, in the client library (L-06).
#[cfg(any(target_os = "macos", target_os = "linux", windows))]
pub use gitraptor_api::client::peer::peer_cred;
pub use gitraptor_api::client::peer::{FOREIGN_UID, PeerCred, current_uid};

/// One process as the kernel reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: u32,
    pub ppid: u32,
    /// Effective uid. Windows: [`current_uid`] for this user's processes,
    /// [`FOREIGN_UID`] for another user's.
    pub uid: u32,
    /// Start time, microseconds since the epoch. With `pid`, the identity.
    pub start_us: u64,
    pub exe: Option<PathBuf>,
    /// Unix: a controlling terminal. Windows: a console (see `session`) in
    /// an interactive Windows session (DS-TS-GRP-004 § 9, C1 and C3).
    pub controlling_terminal: bool,
    /// Session id: the pid of the session leader. Windows: the pid of the
    /// process hosting its console, or its own pid without one.
    pub session: u32,
    /// Process group id.
    pub pgid: u32,
    /// Windows session (`None` on Unix), recorded in the audit.
    pub desktop_session: Option<u32>,
}

/// Why a process could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcError {
    /// It does not exist (any more).
    Gone,
    /// It exists but the kernel refuses to describe it (another user's
    /// process, such as `login` or `launchd`).
    Denied,
    Unsupported,
}

/// Reads processes. Behind a trait so the ancestry rules are tested with a
/// synthetic process tree.
pub trait ProcSource {
    fn read(&self, pid: u32) -> Result<ProcInfo, ProcError>;
    /// `Some(true)` when `pid` exists and its effective uid is not `uid`;
    /// `Some(false)` when it is; `None` when unknown.
    fn foreign_to(&self, pid: u32, uid: u32) -> Option<bool>;
    /// Every process whose effective uid is `uid`; `None` when the list
    /// cannot be read (callers fail closed).
    fn pids_of(&self, _uid: u32) -> Option<Vec<u32>> {
        None
    }
    /// Whether `info` is the root of the user's interactive desktop, where
    /// an ancestry ends cleanly even though its parent is gone (Windows:
    /// `explorer.exe` in the Windows folder, whose parent `userinit` exits).
    fn is_session_root(&self, _info: &ProcInfo) -> bool {
        false
    }
    /// Whether `session` names the process hosting a console (Windows)
    /// rather than a session leader. Then the console's creator, the host's
    /// parent, must be the caller or one of its ancestors (DS-TS-GRP-004
    /// § 9, C8).
    fn console_hosts(&self) -> bool {
        false
    }
    /// The command line of `pid` (program name first), read only to classify a Git subcommand
    /// and never stored (DS-US-GRD-018 § 5.3). `None` when it cannot be read, and on Windows
    /// (Pendiente: etapa de validación multiplataforma): callers evaluate (fail-closed).
    fn args(&self, _pid: u32) -> Option<Vec<std::ffi::OsString>> {
        None
    }
}

/// The running OS.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemProcs;

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use libproc::libproc::proc_pid::{pidinfo, pidpath};
    use libproc::libproc::task_info::TaskAllInfo;
    use libproc::processes::{ProcFilter, pids_by_type};

    /// `PROC_FLAG_CONTROLT`: the process has a controlling terminal.
    const PROC_FLAG_CONTROLT: u32 = 0x80;

    fn exists(pid: u32) -> bool {
        pids_by_type(ProcFilter::All).is_ok_and(|pids| pids.contains(&pid))
    }

    impl ProcSource for SystemProcs {
        fn args(&self, pid: u32) -> Option<Vec<std::ffi::OsString>> {
            gitraptor_macsys::process::process_args(pid)
        }

        fn read(&self, pid: u32) -> Result<ProcInfo, ProcError> {
            let raw = i32::try_from(pid).map_err(|_| ProcError::Gone)?;
            let info = match pidinfo::<TaskAllInfo>(raw, 0) {
                Ok(info) => info,
                Err(_) if exists(pid) => return Err(ProcError::Denied),
                Err(_) => return Err(ProcError::Gone),
            };
            let bsd = info.pbsd;
            if bsd.pbi_pid != pid {
                return Err(ProcError::Gone);
            }
            let session = rustix::process::Pid::from_raw(raw)
                .and_then(|p| rustix::process::getsid(Some(p)).ok())
                .map_or(0, |sid| sid.as_raw_nonzero().get().unsigned_abs());
            Ok(ProcInfo {
                pid,
                ppid: bsd.pbi_ppid,
                uid: bsd.pbi_uid,
                start_us: bsd
                    .pbi_start_tvsec
                    .saturating_mul(1_000_000)
                    .saturating_add(bsd.pbi_start_tvusec),
                exe: pidpath(raw).ok().map(PathBuf::from),
                controlling_terminal: bsd.pbi_flags & PROC_FLAG_CONTROLT != 0,
                desktop_session: None,
                session,
                pgid: bsd.pbi_pgid,
            })
        }

        fn foreign_to(&self, pid: u32, uid: u32) -> Option<bool> {
            if !exists(pid) {
                return None;
            }
            // Effective uid: `login` is setuid root with the user's real uid,
            // and runs with root's rights, not the user's.
            let mine = pids_by_type(ProcFilter::ByUID { uid }).ok()?;
            Some(!mine.contains(&pid))
        }

        fn pids_of(&self, uid: u32) -> Option<Vec<u32>> {
            pids_by_type(ProcFilter::ByUID { uid }).ok()
        }
    }

    /// `struct vnode_info_path`: a `vnode_info` (152 bytes, unused here)
    /// and the path (`MAXPATHLEN`).
    #[repr(C)]
    struct VnodeInfoPath {
        _info: [u8; 152],
        path: [u8; 1024],
    }

    /// `struct proc_vnodepathinfo`: current and root directories. The
    /// kernel fills it through libproc's `pidinfo`, so this crate stays
    /// free of `unsafe` (US-GRP-007).
    #[repr(C)]
    struct VnodePathInfo {
        cdir: VnodeInfoPath,
        _rdir: VnodeInfoPath,
    }

    // `pidinfo` zero-fills a `VnodePathInfo` and passes its size to the
    // kernel: only byte arrays, and exactly the C size.
    const _: () = assert!(std::mem::size_of::<VnodePathInfo>() == 2352);

    impl libproc::libproc::proc_pid::PIDInfo for VnodePathInfo {
        fn flavor() -> libproc::libproc::proc_pid::PidInfoFlavor {
            libproc::libproc::proc_pid::PidInfoFlavor::VNodePathInfo
        }
    }

    /// Working folder of `pid` (`PROC_PIDVNODEPATHINFO`): the path the
    /// kernel resolved, without symbolic links. `None` if it cannot be read.
    pub fn process_cwd(pid: u32) -> Option<PathBuf> {
        use std::os::unix::ffi::OsStrExt;
        let raw = i32::try_from(pid).ok()?;
        let info = pidinfo::<VnodePathInfo>(raw, 0).ok()?;
        let path = &info.cdir.path;
        let len = path.iter().position(|b| *b == 0).unwrap_or(path.len());
        (len > 0).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(&path[..len])))
    }
}

/// Linux: `SO_PEERCRED` and `/proc`. Pendiente: etapa de validación
/// multiplataforma (pidfd via `SO_PEERPIDFD` is the stronger identifier).
#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    /// Fields of `/proc/<pid>/stat` after the command name.
    fn stat_fields(pid: u32) -> Result<Vec<String>, ProcError> {
        let text =
            std::fs::read_to_string(format!("/proc/{pid}/stat")).map_err(|_| ProcError::Gone)?;
        let after = text.rfind(')').ok_or(ProcError::Gone)?;
        Ok(text[after + 1..]
            .split_whitespace()
            .map(str::to_owned)
            .collect())
    }

    fn boot_time_s() -> Option<u64> {
        let stat = std::fs::read_to_string("/proc/stat").ok()?;
        stat.lines()
            .find_map(|l| l.strip_prefix("btime "))
            .and_then(|v| v.trim().parse().ok())
    }

    /// [`super::proc_clock_us`]: `/proc/uptime` counts the same boot clock as `starttime`, in
    /// hundredths rounded down, so both share the boot second and the tick.
    pub(super) fn clock_us() -> Option<u64> {
        let boot = boot_time_s()?;
        let uptime = std::fs::read_to_string("/proc/uptime").ok()?;
        let (secs, hundredths) = uptime.split_whitespace().next()?.split_once('.')?;
        let ticks = secs
            .parse::<u64>()
            .ok()?
            .checked_mul(100)?
            .checked_add(hundredths.parse::<u64>().ok()?)?;
        Some(
            boot.saturating_mul(1_000_000)
                .saturating_add(ticks.saturating_mul(10_000)),
        )
    }

    impl ProcSource for SystemProcs {
        fn args(&self, pid: u32) -> Option<Vec<std::ffi::OsString>> {
            use std::os::unix::ffi::OsStrExt;
            let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
            let raw = raw.strip_suffix(&[0]).unwrap_or(&raw);
            if raw.is_empty() {
                return None;
            }
            Some(
                raw.split(|b| *b == 0)
                    .map(|a| std::ffi::OsStr::from_bytes(a).to_owned())
                    .collect(),
            )
        }

        fn read(&self, pid: u32) -> Result<ProcInfo, ProcError> {
            let meta = std::fs::metadata(format!("/proc/{pid}")).map_err(|_| ProcError::Gone)?;
            let f = stat_fields(pid)?;
            let num = |i: usize| f.get(i).and_then(|v| v.parse::<u64>().ok());
            // After the name: state(0) ppid(1) pgrp(2) session(3) tty_nr(4)
            // ... starttime(19), in clock ticks since boot (assumed 100 Hz).
            let ticks = num(19).ok_or(ProcError::Gone)?;
            let start_us = boot_time_s()
                .unwrap_or(0)
                .saturating_mul(1_000_000)
                .saturating_add(ticks.saturating_mul(10_000));
            Ok(ProcInfo {
                pid,
                ppid: num(1).ok_or(ProcError::Gone)? as u32,
                uid: meta.uid(),
                start_us,
                exe: std::fs::read_link(format!("/proc/{pid}/exe")).ok(),
                controlling_terminal: num(4).is_some_and(|t| t != 0),
                desktop_session: None,
                session: num(3).ok_or(ProcError::Gone)? as u32,
                pgid: num(2).ok_or(ProcError::Gone)? as u32,
            })
        }

        fn foreign_to(&self, pid: u32, uid: u32) -> Option<bool> {
            let meta = std::fs::metadata(format!("/proc/{pid}")).ok()?;
            Some(meta.uid() != uid)
        }

        fn pids_of(&self, uid: u32) -> Option<Vec<u32>> {
            let entries = std::fs::read_dir("/proc").ok()?;
            Some(
                entries
                    .filter_map(Result::ok)
                    .filter_map(|e| {
                        let pid = e.file_name().to_str()?.parse::<u32>().ok()?;
                        (e.metadata().ok()?.uid() == uid).then_some(pid)
                    })
                    .collect(),
            )
        }
    }

    pub fn process_cwd(pid: u32) -> Option<PathBuf> {
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
    }
}

/// Windows: a process is read through its handle (`gitraptor-winsys`), so
/// the pid is pinned while it is read. The identity is `(pid, creation
/// time)`; the parent pid is never updated by Windows, so the walk's "parent
/// started before its child" check is what tells a reused pid apart.
///
/// There is no uid: [`ProcInfo::uid`] is [`current_uid`] (0) when the
/// process token's user is this process's user and [`FOREIGN_UID`] when it
/// is another; a token that cannot be read makes the process `Denied`. The
/// terminal is the console (TQ-14, DS-TS-GRP-004 § 9): `session` is the
/// process hosting it and `controlling_terminal` says it is in an
/// interactive Windows session. There is no process group (`pgid` is 0).
#[cfg(windows)]
mod imp {
    use super::*;
    use gitraptor_winsys::process::{self, Error, Owner};

    /// 100 ns intervals between 1601-01-01 and 1970-01-01.
    const EPOCH_DIFF_100NS: u64 = 116_444_736_000_000_000;

    /// The System process: always another user's.
    const SYSTEM_PID: u32 = 4;

    /// Microseconds since the epoch. The last 100 ns are truncated: the
    /// walk compares with `<=`, so two times that collapse are still in
    /// order.
    fn to_epoch_us(created_100ns: u64) -> u64 {
        created_100ns.saturating_sub(EPOCH_DIFF_100NS) / 10
    }

    impl ProcSource for SystemProcs {
        fn read(&self, pid: u32) -> Result<ProcInfo, ProcError> {
            if pid == 0 || pid == SYSTEM_PID {
                return Err(ProcError::Denied);
            }
            let p = process::process(pid).map_err(|e| match e {
                Error::Gone => ProcError::Gone,
                Error::Denied => ProcError::Denied,
                Error::Unavailable => ProcError::Unsupported,
            })?;
            let uid = match p.owner {
                Owner::Current => current_uid(),
                Owner::Other => FOREIGN_UID,
                Owner::Unknown => return Err(ProcError::Denied),
            };
            // Without a console there is no leader to walk: the process is
            // its own session, and it has no terminal.
            let (session, controlling_terminal) = match p.console_host {
                Some(host) => (
                    host,
                    p.session_id
                        .is_some_and(gitraptor_winsys::system::session_is_interactive),
                ),
                None => (pid, false),
            };
            Ok(ProcInfo {
                pid,
                ppid: p.ppid,
                uid,
                start_us: to_epoch_us(p.created_100ns),
                exe: p.exe,
                controlling_terminal,
                session,
                pgid: 0,
                desktop_session: p.session_id,
            })
        }

        fn console_hosts(&self) -> bool {
            true
        }

        fn foreign_to(&self, pid: u32, uid: u32) -> Option<bool> {
            if pid == 0 || pid == SYSTEM_PID {
                return Some(uid == current_uid());
            }
            match process::process(pid).ok()?.owner {
                Owner::Current => Some(uid != current_uid()),
                Owner::Other => Some(uid == current_uid()),
                Owner::Unknown => None,
            }
        }

        /// The Windows folder comes from the kernel, not from `SystemRoot`,
        /// which whoever launched the daemon could have set.
        fn is_session_root(&self, info: &ProcInfo) -> bool {
            let (Some(exe), Some(windows)) =
                (info.exe.as_deref(), gitraptor_winsys::system::windows_dir())
            else {
                return false;
            };
            info.uid == current_uid()
                && exe
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&windows.join("explorer.exe").to_string_lossy())
        }

        fn pids_of(&self, uid: u32) -> Option<Vec<u32>> {
            if uid != current_uid() {
                return None;
            }
            Some(
                process::pids()?
                    .into_iter()
                    .filter(|pid| process::process(*pid).is_ok_and(|p| p.owner == Owner::Current))
                    .collect(),
            )
        }
    }

    /// Pendiente: the working folder of another process needs its PEB.
    pub fn process_cwd(_pid: u32) -> Option<PathBuf> {
        None
    }
}

/// Other platforms: nothing about a peer can be read, so every reserved
/// command is refused (fail-closed).
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
mod imp {
    use super::*;

    impl ProcSource for SystemProcs {
        fn read(&self, _pid: u32) -> Result<ProcInfo, ProcError> {
            Err(ProcError::Unsupported)
        }
        fn foreign_to(&self, _pid: u32, _uid: u32) -> Option<bool> {
            None
        }
    }

    pub fn process_cwd(_pid: u32) -> Option<PathBuf> {
        None
    }
}

pub use imp::process_cwd;

/// Now, on the clock of [`ProcInfo::start_us`]: a process started at or after this call reads a
/// `start_us` not below it. Linux derives start times from the boot second and 100 Hz ticks, so
/// a wall-clock instant compares wrongly with them (the boot second alone is up to 1 s off);
/// there this is the boot second plus the uptime in whole ticks. Elsewhere, the wall clock.
pub fn proc_clock_us() -> u64 {
    #[cfg(target_os = "linux")]
    if let Some(us) = imp::clock_us() {
        return us;
    }
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(u64::MAX))
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;

    #[test]
    fn reads_this_process_and_its_parent() {
        let me = SystemProcs.read(std::process::id()).unwrap();
        assert_eq!(me.uid, current_uid());
        assert!(me.start_us > 1_600_000_000_000_000, "after 2020");
        let parent = SystemProcs.read(me.ppid).unwrap();
        assert!(parent.start_us <= me.start_us);
        assert_eq!(SystemProcs.foreign_to(me.pid, me.uid), Some(false));
        assert!(SystemProcs.pids_of(me.uid).unwrap().contains(&me.pid));
        assert!(me.pgid > 0);
    }

    /// The executor's marks and the accepted peer compare start times with this clock: a child
    /// started after it must never read as older, whatever the fraction of the boot second.
    #[test]
    fn a_child_started_after_the_clock_is_not_older() {
        assert!(proc_clock_us() >= SystemProcs.read(std::process::id()).unwrap().start_us);
        for _ in 0..20 {
            let before = proc_clock_us();
            let mut child = std::process::Command::new("/bin/sleep")
                .arg("5")
                .spawn()
                .unwrap();
            let read = SystemProcs.read(child.id());
            child.kill().unwrap();
            child.wait().unwrap();
            let start = read.unwrap().start_us;
            assert!(start >= before, "child {start} before the clock {before}");
            assert!(start <= proc_clock_us());
        }
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn reads_this_process_and_its_parent() {
        let me = SystemProcs.read(std::process::id()).unwrap();
        assert_eq!(me.uid, current_uid());
        assert!(me.start_us > 0);
        assert_eq!(
            me.exe.as_deref().map(|p| p.file_name()),
            std::env::current_exe()
                .ok()
                .as_deref()
                .map(|p| p.file_name())
        );
        let parent = SystemProcs.read(me.ppid).unwrap();
        assert!(parent.start_us <= me.start_us);
        assert_eq!(SystemProcs.foreign_to(me.pid, me.uid), Some(false));
        assert!(SystemProcs.pids_of(me.uid).unwrap().contains(&me.pid));
        assert!(me.pgid > 0);
    }

    #[test]
    fn reads_the_working_folder_of_a_child() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new("/bin/sleep")
            .arg("5")
            .current_dir(dir.path())
            .spawn()
            .unwrap();
        let cwd = process_cwd(child.id());
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(cwd, Some(dir.path().canonicalize().unwrap()));
        assert_eq!(process_cwd(child.id()), None);
    }

    #[test]
    fn launchd_is_foreign_and_a_dead_pid_is_gone() {
        assert_eq!(SystemProcs.foreign_to(1, current_uid()), Some(true));
        let mut child = std::process::Command::new("/usr/bin/true").spawn().unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert_eq!(SystemProcs.read(pid), Err(ProcError::Gone));
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn reads_this_process_and_its_parent() {
        let me = SystemProcs.read(std::process::id()).unwrap();
        assert_eq!(me.uid, current_uid());
        assert!(me.start_us > 1_600_000_000_000_000, "after 2020");
        assert_eq!(
            me.exe.as_deref().and_then(|p| p.file_name()),
            std::env::current_exe()
                .ok()
                .as_deref()
                .and_then(|p| p.file_name())
        );
        let parent = SystemProcs.read(me.ppid).unwrap();
        assert!(parent.start_us <= me.start_us);
        assert_eq!(SystemProcs.foreign_to(me.pid, me.uid), Some(false));
        assert!(SystemProcs.pids_of(me.uid).unwrap().contains(&me.pid));
        assert!(!SystemProcs.is_session_root(&me));
        assert!(SystemProcs.console_hosts());
        assert_eq!(me.pgid, 0);
        // OpenSSH and services run in session 0: never a terminal there.
        let session = me.desktop_session.expect("the Windows session");
        if session == 0 {
            assert!(!me.controlling_terminal);
        }
        // `session` is the console host, or the process itself without one.
        if me.session == me.pid {
            assert!(!me.controlling_terminal);
        } else {
            let host = SystemProcs.read(me.session).unwrap();
            let name = host.exe.unwrap().file_name().unwrap().to_ascii_lowercase();
            assert!(
                name == "conhost.exe" || name == "openconsole.exe",
                "{name:?}"
            );
            assert_eq!(host.desktop_session, Some(session));
        }
        // The CLI explains a refusal by the same reading (C10).
        let issue = super::super::authz::console_issue(&SystemProcs, me.uid, me.pid);
        if !me.controlling_terminal {
            assert!(issue.is_some());
        }
        if session == 0 {
            use super::super::authz::ConsoleIssue;
            let want = if me.session == me.pid {
                ConsoleIssue::NoConsole
            } else {
                ConsoleIssue::NotInteractive
            };
            assert_eq!(issue, Some(want));
        }
    }

    #[test]
    fn system_is_foreign_and_a_dead_pid_is_gone() {
        assert_eq!(SystemProcs.read(4), Err(ProcError::Denied));
        assert_eq!(SystemProcs.foreign_to(4, current_uid()), Some(true));
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "exit 0"])
            .spawn()
            .unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert_eq!(SystemProcs.read(pid), Err(ProcError::Gone));
        assert!(!SystemProcs.pids_of(current_uid()).unwrap().contains(&4));
    }
}
