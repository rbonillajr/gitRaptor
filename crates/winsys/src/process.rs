//! Processes as the Windows kernel reports them: parent, creation time,
//! image path, whether the owner is the current user, Windows session and
//! the process hosting their console.
//!
//! The identity of a process is `(pid, creation time)`. A process is opened
//! before anything else is read, and the handle pins it: while it is open
//! the pid cannot be reused, so the snapshot entry with that pid is the same
//! process.

use std::path::PathBuf;

use crate::ffi_handle::Handle;
use crate::ffi_process::{
    console_host_raw, created, ended, image, open, owner, session_id, snapshot,
};

/// Whose a process is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// Its token's user is the user of this process.
    Current,
    /// Another user (SYSTEM, a service account, another person).
    Other,
    /// The token cannot be read.
    Unknown,
}

/// One live process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    /// Parent pid at creation. Windows never updates it: the parent may have
    /// ended and its pid been reused, so callers compare creation times.
    pub ppid: u32,
    /// Creation time, 100 ns intervals since 1601-01-01 UTC.
    pub created_100ns: u64,
    /// Win32 path of the image.
    pub exe: Option<PathBuf>,
    pub owner: Owner,
    /// Windows session (`ProcessIdToSessionId`); `None` when it cannot be read.
    pub session_id: Option<u32>,
    /// The live process hosting its console (`conhost.exe` of the Windows folder, or
    /// `OpenConsole.exe` of Windows Terminal and ConPTY), when it is attached to one. `None`
    /// without a console or when the host cannot be read or is not one of those (fail-closed).
    pub console_host: Option<u32>,
}

/// Why a process could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// It does not exist (any more).
    Gone,
    /// It exists but this user may not open it.
    Denied,
    /// The process list could not be read.
    Unavailable,
}

/// Pids of every live process. `None` if the list cannot be read.
pub fn pids() -> Option<Vec<u32>> {
    snapshot().map(|s| s.into_iter().map(|(pid, _)| pid).collect())
}

/// Makes this process's standard handles non-inheritable, so a detached child (the daemon)
/// does not keep the caller's pipes open. Children that inherit their stdio through
/// `std::process::Command` get fresh duplicates and are unaffected.
pub fn keep_std_handles_private() {
    crate::ffi_process::keep_std_handles_private();
}

/// Whose live process `pid` is, read through its handle. `Owner::Unknown`
/// when its token cannot be read.
pub fn owner_of(pid: u32) -> Result<Owner, Error> {
    let handle = open(pid)?;
    if ended(&handle) {
        return Err(Error::Gone);
    }
    Ok(owner(&handle))
}

/// Creation time of the live process `pid`, 100 ns intervals since 1601-01-01 UTC: with the pid,
/// its identity. A process that ended but is still pinned by some handle counts as gone.
pub fn created_100ns(pid: u32) -> Result<u64, Error> {
    let handle = open(pid)?;
    let created_100ns = created(&handle).ok_or(Error::Gone)?;
    if ended(&handle) {
        return Err(Error::Gone);
    }
    Ok(created_100ns)
}

/// Reads one live process.
pub fn process(pid: u32) -> Result<Process, Error> {
    // Opened first: the handle pins the process, so the pid is not reused
    // while the rest is read.
    let handle = open(pid)?;
    let created_100ns = created(&handle).ok_or(Error::Gone)?;
    // A process that ended but is still pinned by some handle (its parent's
    // `Child`, say) counts as gone: right after the end it can still be in
    // the list for a moment, and once torn down it is not.
    if ended(&handle) {
        return Err(Error::Gone);
    }
    let ppid = snapshot()
        .ok_or(Error::Unavailable)?
        .into_iter()
        .find_map(|(p, parent)| (p == pid).then_some(parent))
        .ok_or(Error::Gone)?;
    Ok(Process {
        pid,
        ppid,
        created_100ns,
        exe: image(&handle),
        owner: owner(&handle),
        // The open handle pins the pid, so these read this same process.
        session_id: session_id(pid),
        console_host: console_host(pid, &handle),
    })
}

/// The pid of the process hosting the console of `process`, checked to be a console host.
fn console_host(pid: u32, process: &Handle) -> Option<u32> {
    let raw = console_host_raw(process)?;
    // Bit 0 marks a console client; without it the value is something else (for a GUI
    // process, the pid of its creator).
    if raw & 1 == 0 {
        return None;
    }
    let host = u32::try_from(raw & !3).ok()?;
    if host == 0 || host == pid {
        return None;
    }
    let handle = open(host).ok()?;
    if ended(&handle) {
        return None;
    }
    is_console_host(&image(&handle)?).then_some(host)
}

/// `conhost.exe` of the Windows folder (read from the kernel), or `OpenConsole.exe`, which
/// Windows Terminal and VS Code ship in their own folders.
fn is_console_host(exe: &std::path::Path) -> bool {
    let name = exe
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if name == "openconsole.exe" {
        return true;
    }
    let Some(windows) = crate::system::windows_dir() else {
        return false;
    };
    let conhost = windows.join("System32").join("conhost.exe");
    exe.to_string_lossy()
        .eq_ignore_ascii_case(&conhost.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn reads_this_process_and_its_parent() {
        let me = process(std::process::id()).unwrap();
        assert_eq!(me.owner, Owner::Current);
        assert!(me.created_100ns > 0);
        let exe = std::env::current_exe().unwrap();
        assert_eq!(
            me.exe.as_deref().and_then(|p| p.file_name()),
            exe.file_name()
        );
        let parent = process(me.ppid).unwrap();
        assert!(parent.created_100ns <= me.created_100ns);
        assert!(pids().unwrap().contains(&me.pid));
    }

    #[test]
    fn a_child_is_read_and_is_gone_once_it_ends() {
        let mut child = Command::new("cmd")
            .args(["/C", "ping -n 3 127.0.0.1 >NUL"])
            .spawn()
            .unwrap();
        let pid = child.id();
        let info = process(pid).unwrap();
        assert_eq!(info.ppid, std::process::id());
        assert_eq!(info.owner, Owner::Current);
        assert!(info.created_100ns >= process(std::process::id()).unwrap().created_100ns);
        assert_eq!(created_100ns(pid), Ok(info.created_100ns));
        // Still pinned by `child`'s handle, but no longer in the list.
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(process(pid), Err(Error::Gone));
        assert_eq!(created_100ns(pid), Err(Error::Gone));
    }

    /// A child given its own console (`CREATE_NEW_CONSOLE`) is hosted by a `conhost.exe` of
    /// the Windows folder that it created; a detached one (`DETACHED_PROCESS`) has none.
    #[test]
    fn console_hosts_are_read_from_the_kernel() {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_CONSOLE: u32 = 0x10;
        const DETACHED_PROCESS: u32 = 0x08;
        let spawn = |flags| {
            Command::new("cmd")
                .args(["/C", "ping -n 30 127.0.0.1 >NUL"])
                .creation_flags(flags)
                .spawn()
                .unwrap()
        };
        let mut with = spawn(CREATE_NEW_CONSOLE);
        let mut without = spawn(DETACHED_PROCESS);
        // The console is attached while the child initializes: wait for it, bounded.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let host = loop {
            if let Some(host) = process(with.id()).unwrap().console_host {
                break host;
            }
            assert!(std::time::Instant::now() < deadline, "no console host");
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let info = process(host).unwrap();
        assert_eq!(info.owner, Owner::Current);
        let exe = info.exe.unwrap();
        assert!(is_console_host(&exe), "{exe:?}");
        // Its creator is the child, as C8 of DS-TS-GRP-004 § 9 relies on.
        assert_eq!(info.ppid, with.id());
        let me = process(std::process::id()).unwrap();
        assert_eq!(process(with.id()).unwrap().session_id, me.session_id);
        assert_eq!(process(without.id()).unwrap().console_host, None);
        for child in [&mut with, &mut without] {
            child.kill().unwrap();
            child.wait().unwrap();
        }
        assert!(!is_console_host(std::path::Path::new(
            "C:\\tmp\\conhost.exe"
        )));
    }

    #[test]
    fn system_processes_are_not_the_current_user() {
        // pid 4 is System: denied to a user, or another owner when elevated.
        match process(4) {
            Err(Error::Denied) => {}
            Ok(p) => assert_ne!(p.owner, Owner::Current),
            Err(e) => panic!("unexpected {e:?}"),
        }
        assert_eq!(process(u32::MAX - 3), Err(Error::Gone));
    }
}
