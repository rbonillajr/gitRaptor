//! The process table the detector reads (S1 and S3 of ADR-GRP-012).
//!
//! Only what identifies a process and places it: pid, parent, start time,
//! executable path and, on request, the working folder. Never the command
//! line nor the environment (SEC-04): `claude -p "..."` carries the prompt
//! in its arguments, so not even the platform APIs that would load them are
//! called.

use std::path::PathBuf;

/// One process of the user, as the kernel reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcEntry {
    pub pid: u32,
    pub ppid: u32,
    /// Start time, microseconds since the epoch. With `pid`, the identity
    /// that survives pid reuse.
    pub start_us: u64,
    pub exe: Option<PathBuf>,
}

/// Reads the processes of the current user. Behind a trait so the rules of
/// the detector are tested with a synthetic table.
pub trait ProcLister: Send + Sync {
    /// Every process of the current user; `None` when the platform cannot
    /// list them (the detector stays idle).
    fn list(&self) -> Option<Vec<ProcEntry>>;
    /// Like [`list`](Self::list), but `exe` may be `None` until [`exe`](Self::exe) asks for it:
    /// the S1 scan reads the whole table every second and only needs the path of the processes
    /// it has not classified yet, so reading it for all of them was most of the daemon's CPU at
    /// rest (RES-01).
    fn list_bare(&self) -> Option<Vec<ProcEntry>> {
        self.list()
    }
    /// Executable path of `entry`, only while `(pid, start_us)` is still that process: `None`
    /// when it ended or its pid now names another one.
    fn exe(&self, entry: &ProcEntry) -> Option<PathBuf> {
        entry.exe.clone()
    }
    /// Working folder of `pid`, without symbolic links.
    fn cwd(&self, pid: u32) -> Option<PathBuf>;
}

/// The running OS.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemProcLister;

/// Whether this platform can list processes for the detector. Windows:
/// Pendiente: etapa de validación multiplataforma.
pub const fn detection_supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "linux"))
}

#[cfg(target_os = "macos")]
mod mac {
    use std::path::PathBuf;

    use libproc::libproc::bsd_info::BSDInfo;
    use libproc::libproc::proc_pid::{pidinfo, pidpath};
    use libproc::processes::{ProcFilter, pids_by_type};

    use super::ProcEntry;

    /// Pid, parent and start of `pid`; `None` when it is gone.
    pub(super) fn entry(pid: u32) -> Option<ProcEntry> {
        let raw = i32::try_from(pid).ok()?;
        let bsd = pidinfo::<BSDInfo>(raw, 0).ok()?;
        (bsd.pbi_pid == pid).then(|| ProcEntry {
            pid,
            ppid: bsd.pbi_ppid,
            start_us: bsd
                .pbi_start_tvsec
                .saturating_mul(1_000_000)
                .saturating_add(bsd.pbi_start_tvusec),
            exe: None,
        })
    }

    /// One `sysctl` for the whole table; one `proc_pidinfo` per process when it cannot be read
    /// or does not pass [`sane`]: a failure costs CPU, it never blinds the detector.
    pub(super) fn table() -> Option<Vec<ProcEntry>> {
        let uid = crate::channel::peer::current_uid();
        if let Some(table) = gitraptor_macsys::process::user_processes(uid).filter(|t| sane(t)) {
            return Some(
                table
                    .into_iter()
                    .map(|p| ProcEntry {
                        pid: p.pid,
                        ppid: p.ppid,
                        start_us: p.start_us,
                        exe: None,
                    })
                    .collect(),
            );
        }
        let pids = pids_by_type(ProcFilter::ByUID { uid }).ok()?;
        Some(pids.into_iter().filter_map(entry).collect())
    }

    /// The offsets of `kinfo_proc` are written by hand (`libc` does not declare it): the table
    /// must hold this very process with its parent, or its layout is not the one checked.
    fn sane(table: &[gitraptor_macsys::process::ProcBrief]) -> bool {
        let (me, parent) = (std::process::id(), std::os::unix::process::parent_id());
        table.iter().any(|p| p.pid == me && p.ppid == parent)
    }

    /// The path first and the start after it: a pid reused between the two reads never lends
    /// its new executable to the old process.
    pub(super) fn exe(e: &ProcEntry) -> Option<PathBuf> {
        let path = pidpath(i32::try_from(e.pid).ok()?).ok()?;
        entry(e.pid)
            .is_some_and(|now| now.start_us == e.start_us)
            .then(|| PathBuf::from(path))
    }
}

#[cfg(target_os = "macos")]
impl ProcLister for SystemProcLister {
    fn list(&self) -> Option<Vec<ProcEntry>> {
        let mut table = mac::table()?;
        for e in &mut table {
            e.exe = mac::exe(e);
        }
        Some(table)
    }

    fn list_bare(&self) -> Option<Vec<ProcEntry>> {
        mac::table()
    }

    fn exe(&self, entry: &ProcEntry) -> Option<PathBuf> {
        mac::exe(entry)
    }

    fn cwd(&self, pid: u32) -> Option<PathBuf> {
        crate::channel::peer::process_cwd(pid)
    }
}

#[cfg(target_os = "linux")]
impl ProcLister for SystemProcLister {
    fn list(&self) -> Option<Vec<ProcEntry>> {
        use crate::channel::peer::{ProcSource, SystemProcs, current_uid};
        let uid = current_uid();
        let pids = SystemProcs.pids_of(uid)?;
        Some(
            pids.into_iter()
                .filter_map(|pid| SystemProcs.read(pid).ok())
                .map(|p| ProcEntry {
                    pid: p.pid,
                    ppid: p.ppid,
                    start_us: p.start_us,
                    exe: p.exe,
                })
                .collect(),
        )
    }

    // `list_bare` stays `list`: `SystemProcs::read` reads the link with the rest. Pendiente:
    // etapa de validación multiplataforma (the cost of the scan on Linux is not measured).
    fn exe(&self, entry: &ProcEntry) -> Option<PathBuf> {
        use crate::channel::peer::{ProcSource, SystemProcs};
        let now = SystemProcs.read(entry.pid).ok()?;
        (now.start_us == entry.start_us)
            .then_some(now.exe)
            .flatten()
    }

    fn cwd(&self, pid: u32) -> Option<PathBuf> {
        crate::channel::peer::process_cwd(pid)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
impl ProcLister for SystemProcLister {
    fn list(&self) -> Option<Vec<ProcEntry>> {
        None
    }

    fn cwd(&self, _pid: u32) -> Option<PathBuf> {
        None
    }
}

#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod tests {
    use super::*;

    #[test]
    fn lists_a_child_with_its_parent_and_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .current_dir(dir.path())
            .spawn()
            .unwrap();
        let table = SystemProcLister.list().unwrap();
        let bare = SystemProcLister.list_bare().unwrap();
        let cwd = SystemProcLister.cwd(child.id());
        let found = bare.iter().find(|e| e.pid == child.id()).unwrap().clone();
        let exe = SystemProcLister.exe(&found);
        child.kill().unwrap();
        child.wait().unwrap();
        // Ended: its path is no longer read.
        assert_eq!(SystemProcLister.exe(&found), None);
        assert!(exe.unwrap().ends_with("sleep"));
        let entry = table.iter().find(|e| e.pid == child.id()).unwrap();
        assert_eq!((found.ppid, found.start_us), (entry.ppid, entry.start_us));
        // Another start under the same pid is another process.
        let me = bare.iter().find(|e| e.pid == std::process::id()).unwrap();
        let other = ProcEntry {
            start_us: me.start_us + 1,
            ..me.clone()
        };
        assert_eq!(SystemProcLister.exe(&other), None);
        assert_eq!(entry.ppid, std::process::id());
        let me = table.iter().find(|e| e.pid == std::process::id()).unwrap();
        assert!(me.start_us <= entry.start_us);
        assert!(entry.exe.as_ref().unwrap().ends_with("sleep"));
        assert_eq!(cwd, Some(dir.path().canonicalize().unwrap()));
    }

    /// The one-call table agrees with `proc_pidinfo`, start included, and leaves zombies out.
    #[cfg(target_os = "macos")]
    #[test]
    fn the_table_agrees_with_pidinfo_and_skips_zombies() {
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let mut zombie = std::process::Command::new("true").spawn().unwrap();
        // Ended and not reaped: a zombie until `wait`, as `ps` shows it.
        let stat = |pid: u32| {
            let out = std::process::Command::new("/bin/ps")
                .args(["-o", "stat=", "-p", &pid.to_string()])
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_owned()
        };
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !stat(zombie.id()).starts_with('Z') {
            assert!(std::time::Instant::now() < deadline, "no zombie");
            std::thread::yield_now();
        }
        let uid = crate::channel::peer::current_uid();
        let raw = gitraptor_macsys::process::user_processes(uid).unwrap();
        let table = SystemProcLister.list_bare().unwrap();
        for pid in [std::process::id(), child.id()] {
            let brief = raw.iter().find(|p| p.pid == pid).unwrap();
            let info = mac::entry(pid).unwrap();
            assert_eq!((brief.ppid, brief.start_us), (info.ppid, info.start_us));
            assert!(table.contains(&info));
        }
        assert!(
            raw.iter().all(|p| p.pid != zombie.id()),
            "zombies are not listed"
        );
        child.kill().unwrap();
        child.wait().unwrap();
        zombie.wait().unwrap();
    }
}
