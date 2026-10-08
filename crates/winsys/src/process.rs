//! Processes as the Windows kernel reports them: parent, creation time,
//! image path and whether the owner is the current user.
//!
//! The identity of a process is `(pid, creation time)`. A process is opened
//! before anything else is read, and the handle pins it: while it is open
//! the pid cannot be reused, so the snapshot entry with that pid is the same
//! process.

use std::path::PathBuf;

use crate::ffi_process::{created, ended, image, open, owner, snapshot};

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

/// Whose live process `pid` is, read through its handle. `Owner::Unknown`
/// when its token cannot be read.
pub fn owner_of(pid: u32) -> Result<Owner, Error> {
    let handle = open(pid)?;
    if ended(&handle) {
        return Err(Error::Gone);
    }
    Ok(owner(&handle))
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
    })
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
        // Still pinned by `child`'s handle, but no longer in the list.
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(process(pid), Err(Error::Gone));
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
