//! The process table the detector reads (S1 and S3 of ADR-GRP-012).
//!
//! Only what identifies a process and places it: pid, parent, start time,
//! executable path and, on request, the working folder. The command line
//! and the environment are read for one thing only: whether a foreign `git`
//! redirects its target ([`ProcLister::git_redirect`]), and only that
//! boolean leaves the reader; an argument or a variable's value is never
//! kept, logged nor returned (SEC-04: `claude -p "..."` carries the prompt
//! in its arguments). The agent's own command line is never read.

use std::ffi::{OsStr, OsString};
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
    /// Whether the `git` `pid` redirects its target (`-C`, `--git-dir`,
    /// `--work-tree` or `GIT_DIR`, `GIT_WORK_TREE`, `GIT_COMMON_DIR` in its
    /// environment). `None` when its arguments or environment cannot be
    /// read. Only this boolean leaves the reader (SEC-04).
    fn git_redirect(&self, pid: u32) -> Option<bool> {
        let _ = pid;
        None
    }
}

/// Whether a `git` with these arguments (after the program name) and these
/// environment variable names redirects its target: a global option `-C`,
/// `--git-dir` or `--work-tree` before the subcommand, or `GIT_DIR`,
/// `GIT_WORK_TREE` or `GIT_COMMON_DIR` in the environment.
///
/// The first argument that is not an option is the subcommand and nothing
/// after it is read: `git log -C` (copy detection) does not redirect. The
/// value of a global option that takes one (`-c k=v`) is skipped.
pub fn git_redirects<'a>(
    args: &[OsString],
    mut env_names: impl Iterator<Item = &'a OsStr>,
) -> bool {
    global_options_redirect(args.iter().map(|a| a.as_encoded_bytes()))
        || env_names.any(|n| REDIRECT_ENV.contains(&n.as_encoded_bytes()))
}

/// The environment variables that move a `git`'s repository or worktree.
const REDIRECT_ENV: [&[u8]; 3] = [b"GIT_DIR", b"GIT_WORK_TREE", b"GIT_COMMON_DIR"];

/// The global options of `git` whose value is the next argument (in the `=`
/// form they are one argument). `--exec-path` only takes one with `=`.
const GLOBAL_WITH_VALUE: [&[u8]; 7] = [
    b"-C",
    b"-c",
    b"--git-dir",
    b"--work-tree",
    b"--namespace",
    b"--config-env",
    b"--super-prefix",
];

/// Whether the global options (before the subcommand, program name
/// excluded) hold `-C`, `--git-dir` or `--work-tree`, separate or with `=`.
fn global_options_redirect<'a>(mut args: impl Iterator<Item = &'a [u8]>) -> bool {
    while let Some(arg) = args.next() {
        if !arg.starts_with(b"-") || arg == b"--" {
            return false;
        }
        let name = arg.split(|b| *b == b'=').next().unwrap_or_default();
        if matches!(name, b"-C" | b"--git-dir" | b"--work-tree") {
            return true;
        }
        if name.len() == arg.len() && GLOBAL_WITH_VALUE.contains(&arg) {
            // Its value, which may itself start with `-`.
            args.next();
        }
    }
    false
}

/// Most bytes read from `/proc/<pid>/cmdline` or `/proc/<pid>/environ`; a
/// longer one is unreadable.
#[cfg(target_os = "linux")]
const PROC_TEXT_CAP: u64 = 256 * 1024;

/// Linux: [`git_redirects`] over `/proc/<pid>/cmdline` (without `argv[0]`)
/// and the names of `/proc/<pid>/environ`. `None` when either cannot be read,
/// is empty (the process is exiting) or exceeds [`PROC_TEXT_CAP`].
#[cfg(target_os = "linux")]
fn linux_git_redirect(pid: u32) -> Option<bool> {
    use std::io::Read as _;
    use std::os::unix::ffi::OsStrExt as _;

    let read = |name: &str| -> Option<Vec<u8>> {
        let file = std::fs::File::open(format!("/proc/{pid}/{name}")).ok()?;
        let mut buf = Vec::new();
        file.take(PROC_TEXT_CAP + 1).read_to_end(&mut buf).ok()?;
        (u64::try_from(buf.len()).ok()? <= PROC_TEXT_CAP).then_some(buf)
    };
    let cmdline = read("cmdline").filter(|c| !c.is_empty())?;
    let environ = read("environ")?;
    let args: Vec<OsString> = cmdline
        .strip_suffix(b"\0")
        .unwrap_or(&cmdline[..])
        .split(|b| *b == 0)
        .skip(1)
        .map(|a| OsStr::from_bytes(a).to_owned())
        .collect();
    let names = environ
        .split(|b| *b == 0)
        .filter(|kv| !kv.is_empty())
        .map(|kv| OsStr::from_bytes(kv.split(|b| *b == b'=').next().unwrap_or_default()));
    Some(git_redirects(&args, names))
}

/// The running OS.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemProcLister;

/// Whether this platform can list processes for the detector.
pub const fn detection_supported() -> bool {
    cfg!(any(target_os = "macos", target_os = "linux", windows))
}

/// Windows: processes of the current user from one snapshot, the image path read per process
/// only when asked, and the working folder through the PEB (`gitraptor-winsys`, SEC-04: only
/// that folder, never a command line). The identity is `(pid, creation time)`; pid reuse is
/// caught by comparing the creation time while each piece is read.
#[cfg(windows)]
mod win {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    use gitraptor_winsys::process;

    use super::ProcEntry;

    /// 100 ns intervals between 1601-01-01 and 1970-01-01.
    const EPOCH_DIFF_100NS: u64 = 116_444_736_000_000_000;

    /// Microseconds since the epoch, as the other platforms keep it.
    fn to_epoch_us(created_100ns: u64) -> u64 {
        created_100ns.saturating_sub(EPOCH_DIFF_100NS) / 10
    }

    /// Back to the clock of the kernel, to the microsecond (see `winsys::process`).
    fn to_created_100ns(start_us: u64) -> u64 {
        start_us.saturating_mul(10).saturating_add(EPOCH_DIFF_100NS)
    }

    /// The creation time of each process of the last scan: the working folder of a pid is read
    /// only while it is still the process that scan saw, even if Windows reused the pid since.
    fn seen() -> &'static Mutex<HashMap<u32, u64>> {
        static SEEN: OnceLock<Mutex<HashMap<u32, u64>>> = OnceLock::new();
        SEEN.get_or_init(Mutex::default)
    }

    pub(super) fn table() -> Option<Vec<ProcEntry>> {
        let table = process::current_user_processes()?;
        *seen().lock().unwrap_or_else(|e| e.into_inner()) =
            table.iter().map(|p| (p.pid, p.created_100ns)).collect();
        Some(
            table
                .into_iter()
                .map(|p| ProcEntry {
                    pid: p.pid,
                    ppid: p.ppid,
                    start_us: to_epoch_us(p.created_100ns),
                    exe: None,
                })
                .collect(),
        )
    }

    pub(super) fn exe(entry: &ProcEntry) -> Option<PathBuf> {
        process::image_of(entry.pid, to_created_100ns(entry.start_us))
    }

    /// The working folder of `pid` in the drive form the detector compares worktrees in, with
    /// the links of the path resolved as on the other platforms. `None` when it cannot be read.
    pub(super) fn cwd(pid: u32) -> Option<PathBuf> {
        // The one the last scan saw; a process born since (a short `git`) is read as it is now.
        let seen = seen()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&pid)
            .copied();
        let created = match seen {
            Some(created) => created,
            None => process::created_100ns(pid).ok()?,
        };
        let folder = process::cwd(pid, created).ok()?;
        let resolved = std::fs::canonicalize(folder).ok()?;
        Some(gitraptor_policy::guard::fastpath::simplified(resolved))
    }
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
    /// must hold this very process with its parent and the start `proc_pidinfo` gives, or its
    /// layout is not the one checked (a wrong start would make every path read refuse).
    fn sane(table: &[gitraptor_macsys::process::ProcBrief]) -> bool {
        let (me, parent) = (std::process::id(), std::os::unix::process::parent_id());
        let Some(start) = entry(me).map(|e| e.start_us) else {
            return false;
        };
        table
            .iter()
            .any(|p| p.pid == me && p.ppid == parent && p.start_us == start)
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

    fn git_redirect(&self, pid: u32) -> Option<bool> {
        gitraptor_macsys::process::process_git_redirect(pid)
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

    fn git_redirect(&self, pid: u32) -> Option<bool> {
        linux_git_redirect(pid)
    }
}

// Windows and the rest keep the default `git_redirect` (`None`, unknown): a
// foreign `git` counts in the whole repo, as before. Pendiente: etapa de
// validación multiplataforma.
#[cfg(windows)]
impl ProcLister for SystemProcLister {
    fn list(&self) -> Option<Vec<ProcEntry>> {
        let mut table = win::table()?;
        for e in &mut table {
            e.exe = win::exe(e);
        }
        Some(table)
    }

    fn list_bare(&self) -> Option<Vec<ProcEntry>> {
        win::table()
    }

    fn exe(&self, entry: &ProcEntry) -> Option<PathBuf> {
        win::exe(entry)
    }

    fn cwd(&self, pid: u32) -> Option<PathBuf> {
        win::cwd(pid)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
impl ProcLister for SystemProcLister {
    fn list(&self) -> Option<Vec<ProcEntry>> {
        None
    }

    fn cwd(&self, _pid: u32) -> Option<PathBuf> {
        None
    }
}

#[cfg(test)]
#[path = "procs_redirect_tests.rs"]
mod redirect_tests;

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

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    fn plain(path: &std::path::Path) -> String {
        let text = std::fs::canonicalize(path)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_lowercase()
    }

    /// A child with its own working folder is listed with its parent, start and path, its
    /// folder is read, and once it ends none of that is read for it.
    #[test]
    fn lists_a_child_with_its_parent_path_and_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new("cmd")
            .args(["/C", "ping -n 30 127.0.0.1 >NUL"])
            .current_dir(dir.path())
            .spawn()
            .unwrap();
        let bare = SystemProcLister.list_bare().unwrap();
        let found = bare.iter().find(|e| e.pid == child.id()).unwrap().clone();
        assert_eq!(found.ppid, std::process::id());
        assert_eq!(found.exe, None);
        let exe = SystemProcLister.exe(&found).unwrap();
        assert!(exe.to_string_lossy().to_lowercase().ends_with("cmd.exe"));
        let table = SystemProcLister.list().unwrap();
        let entry = table.iter().find(|e| e.pid == child.id()).unwrap();
        assert_eq!((entry.ppid, entry.start_us), (found.ppid, found.start_us));
        assert_eq!(entry.exe.as_deref(), Some(exe.as_path()));
        let cwd = SystemProcLister.cwd(child.id());
        assert_eq!(cwd.as_deref().map(plain), Some(plain(dir.path())));
        // Another start under the same pid is another process.
        let other = ProcEntry {
            start_us: found.start_us + 1_000,
            ..found.clone()
        };
        assert_eq!(SystemProcLister.exe(&other), None);
        // This process is in the table too, with a start before its child's.
        let me = table.iter().find(|e| e.pid == std::process::id()).unwrap();
        assert!(me.start_us <= entry.start_us);
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(SystemProcLister.exe(&found), None);
        assert_eq!(SystemProcLister.cwd(child.id()), None);
        assert!(detection_supported());
    }
}
