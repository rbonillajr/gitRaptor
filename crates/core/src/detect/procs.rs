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
impl ProcLister for SystemProcLister {
    fn list(&self) -> Option<Vec<ProcEntry>> {
        use libproc::libproc::bsd_info::BSDInfo;
        use libproc::libproc::proc_pid::{pidinfo, pidpath};
        use libproc::processes::{ProcFilter, pids_by_type};
        let uid = crate::channel::peer::current_uid();
        let pids = pids_by_type(ProcFilter::ByUID { uid }).ok()?;
        Some(
            pids.into_iter()
                .filter_map(|pid| {
                    let raw = i32::try_from(pid).ok()?;
                    let bsd = pidinfo::<BSDInfo>(raw, 0).ok()?;
                    (bsd.pbi_pid == pid).then(|| ProcEntry {
                        pid,
                        ppid: bsd.pbi_ppid,
                        start_us: bsd
                            .pbi_start_tvsec
                            .saturating_mul(1_000_000)
                            .saturating_add(bsd.pbi_start_tvusec),
                        exe: pidpath(raw).ok().map(PathBuf::from),
                    })
                })
                .collect(),
        )
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
        let cwd = SystemProcLister.cwd(child.id());
        child.kill().unwrap();
        child.wait().unwrap();
        let entry = table.iter().find(|e| e.pid == child.id()).unwrap();
        assert_eq!(entry.ppid, std::process::id());
        let me = table.iter().find(|e| e.pid == std::process::id()).unwrap();
        assert!(me.start_us <= entry.start_us);
        assert!(entry.exe.as_ref().unwrap().ends_with("sleep"));
        assert_eq!(cwd, Some(dir.path().canonicalize().unwrap()));
    }
}
