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
    console_host_raw, created, current_directory, drive_is_fixed, ended, image, open, open_reading,
    owner, session_id, snapshot,
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

/// A live process of the current user, as little as the detector's scan needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Brief {
    pub pid: u32,
    /// Parent pid at creation (see [`Process::ppid`]).
    pub ppid: u32,
    /// Creation time, 100 ns intervals since 1601-01-01 UTC.
    pub created_100ns: u64,
}

/// The live processes of the current user from one snapshot: pid, parent and creation time of
/// each, without reading their image paths. Another user's, SYSTEM's and the ones whose token
/// cannot be read are left out. `None` if the list cannot be read.
pub fn current_user_processes() -> Option<Vec<Brief>> {
    Some(
        snapshot()?
            .into_iter()
            .filter(|(pid, _)| *pid != 0 && *pid != 4)
            .filter_map(|(pid, ppid)| {
                let handle = open(pid).ok()?;
                let created_100ns = created(&handle)?;
                (!ended(&handle) && owner(&handle) == Owner::Current).then_some(Brief {
                    pid,
                    ppid,
                    created_100ns,
                })
            })
            .collect(),
    )
}

/// Whether the process behind `handle` is the one created at `created_100ns` and still runs.
/// Compared at the microsecond: a caller that keeps the time in microseconds (the detector)
/// loses the last digit, and two processes of one pid never start within one microsecond.
fn is_still(handle: &Handle, created_100ns: u64) -> bool {
    created(handle).is_some_and(|c| c / 10 == created_100ns / 10) && !ended(handle)
}

/// The image path of `pid`, only while it is still the process created at `created_100ns`: a
/// pid reused meanwhile never lends its executable to the old one.
pub fn image_of(pid: u32, created_100ns: u64) -> Option<PathBuf> {
    let handle = open(pid).ok()?;
    if !is_still(&handle, created_100ns) {
        return None;
    }
    let path = image(&handle)?;
    // Read once more: still the same process after the path was read.
    is_still(&handle, created_100ns).then_some(path)
}

/// The working folder of `pid`, only while it is still the process created at `created_100ns`,
/// as the process set it (`C:\work\repo`, never with a trailing separator but at a drive root).
///
/// Reads another process's memory, so it asks for as little as it can: the handle has the right
/// to read memory only for this call; the process must be the current user's (checked on its
/// token before anything is read: another user's is [`Error::Denied`]); a 32-bit process is
/// refused, not guessed at; and only the folder is read, never its command line or environment
/// (SEC-04). **Unreadable is unknown, never "not an agent"**: every failure to read, an
/// elevated process included, is [`Error::Denied`], and the caller keeps the process in doubt.
pub fn cwd(pid: u32, created_100ns: u64) -> Result<PathBuf, Error> {
    let handle = open_reading(pid)?;
    if !is_still(&handle, created_100ns) {
        return Err(Error::Gone);
    }
    if owner(&handle) != Owner::Current {
        return Err(Error::Denied);
    }
    let units = current_directory(&handle).ok_or(Error::Denied)?;
    // The folder belongs to the process that was checked, not to a pid reused during the read.
    if !is_still(&handle, created_100ns) {
        return Err(Error::Gone);
    }
    let folder = folder_from(&units).ok_or(Error::Denied)?;
    // Callers open this path to resolve its links: only a local fixed disk, never a network
    // share, a mapped drive or a letter a process redefined for itself.
    let letter = folder.to_string_lossy().as_bytes()[0];
    if !drive_is_fixed(letter) {
        return Err(Error::Denied);
    }
    Ok(folder)
}

/// A drive-absolute folder (`C:\\work\\repo`) of plain components, without the trailing separator
/// Windows keeps on it (a drive root keeps its own). Whatever else a process could write in its
/// own PEB is refused: a UNC or device path, a `/`, a `.` or `..`, a component that ends in a
/// dot or a space (Win32 would read another folder) or has a character no folder name has.
/// The text is only ever compared with the paths of the worktrees; a caller that opens it asks
/// first for [`cwd`]'s own guarantee that its drive is a local fixed disk.
fn folder_from(units: &[u16]) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let mut text = std::ffi::OsString::from_wide(units).into_string().ok()?;
    let b = text.as_bytes();
    if !(b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'\\') {
        return None;
    }
    if text.len() > 3 && text.ends_with('\\') {
        text.pop();
    }
    let plain = |c: &str| {
        !c.is_empty()
            && c != "."
            && c != ".."
            && !c.ends_with(['.', ' '])
            && !c
                .chars()
                .any(|ch| ch.is_control() || "<>:\"|?*/".contains(ch))
    };
    text[3..]
        .split('\\')
        .all(|c| plain(c) || text.len() == 3)
        .then(|| PathBuf::from(text))
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

    /// The folder as the kernel keeps it, in drive form, for comparing with another path.
    fn plain(path: &std::path::Path) -> String {
        let text = std::fs::canonicalize(path)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        text.strip_prefix(r"\\?\").unwrap_or(&text).to_lowercase()
    }

    fn sleeper(dir: &std::path::Path, program: &str) -> std::process::Child {
        Command::new(program)
            .args(["/C", "ping -n 30 127.0.0.1 >NUL"])
            .current_dir(dir)
            .spawn()
            .unwrap()
    }

    /// The working folder of a child of this user, read while it lives and refused once it ends
    /// or when the creation time is not the one the caller saw.
    #[test]
    fn the_working_folder_of_a_child_is_read_only_for_the_process_that_was_seen() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = sleeper(dir.path(), "cmd");
        let created = process(child.id()).unwrap().created_100ns;
        let folder = cwd(child.id(), created).unwrap();
        assert_eq!(plain(&folder), plain(dir.path()));
        assert!(!folder.to_string_lossy().ends_with('\\'));
        // Another creation time under the same pid is another process.
        assert_eq!(cwd(child.id(), created + 20), Err(Error::Gone));
        // This process too.
        let me = process(std::process::id()).unwrap();
        assert_eq!(
            plain(&cwd(me.pid, me.created_100ns).unwrap()),
            plain(&std::env::current_dir().unwrap())
        );
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(cwd(child.id(), created), Err(Error::Gone));
    }

    /// A 32-bit process keeps its folder in another structure: refused, never guessed.
    #[test]
    fn a_32_bit_process_is_refused() {
        let wow = std::path::Path::new(r"C:\Windows\SysWOW64\cmd.exe");
        if !wow.is_file() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let mut child = sleeper(dir.path(), wow.to_str().unwrap());
        let created = process(child.id()).unwrap().created_100ns;
        assert_eq!(cwd(child.id(), created), Err(Error::Denied));
        child.kill().unwrap();
        child.wait().unwrap();
    }

    /// Another user's process and System are never read: unknown, not "not an agent".
    #[test]
    fn processes_of_other_users_are_never_read() {
        assert!(matches!(cwd(4, 1), Err(Error::Denied | Error::Gone)));
        let foreign = pids().unwrap().into_iter().find_map(|pid| {
            let p = process(pid).ok()?;
            (p.owner == Owner::Other).then_some(p)
        });
        if let Some(p) = foreign {
            assert_eq!(cwd(p.pid, p.created_100ns), Err(Error::Denied));
        }
    }

    #[test]
    fn the_listing_has_this_user_with_parent_and_creation_and_never_system() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = sleeper(dir.path(), "cmd");
        let table = current_user_processes().unwrap();
        let me = table.iter().find(|b| b.pid == std::process::id()).unwrap();
        let kid = table.iter().find(|b| b.pid == child.id()).unwrap();
        assert_eq!(kid.ppid, me.pid);
        assert_eq!(
            Some(kid.created_100ns),
            process(child.id()).ok().map(|p| p.created_100ns)
        );
        assert!(table.iter().all(|b| b.pid != 0 && b.pid != 4));
        let exe = image_of(me.pid, me.created_100ns).unwrap();
        assert_eq!(
            exe.file_name(),
            std::env::current_exe().unwrap().file_name()
        );
        assert_eq!(image_of(me.pid, me.created_100ns + 20), None);
        child.kill().unwrap();
        child.wait().unwrap();
        assert_eq!(image_of(child.id(), kid.created_100ns), None);
    }

    #[test]
    fn a_folder_has_the_shape_of_a_path_or_is_refused() {
        let wide = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        let ok = |s: &str| folder_from(&wide(s)).map(|p| p.to_string_lossy().into_owned());
        assert_eq!(ok(r"C:\work\repo\").as_deref(), Some(r"C:\work\repo"));
        assert_eq!(ok(r"C:\work").as_deref(), Some(r"C:\work"));
        assert_eq!(ok(r"C:\").as_deref(), Some(r"C:\"));
        assert_eq!(
            ok(r"C:\path with spaces\a.b").as_deref(),
            Some(r"C:\path with spaces\a.b")
        );
        for bad in [
            r"relative\x",
            r"\\srv\share\x",
            r"\\?\C:\x",
            r"\\.\pipe\x",
            r"\\./pipe/x",
            "C:x",
            "",
            "C:\\a\0b",
            r"C:\w\repo ",
            r"C:\w\repo.",
            r"C:\w\..\x",
            r"C:\w\.\x",
            r"C:\w\a|b",
            "C:/w/x",
            r"C:\w\\x",
        ] {
            assert_eq!(ok(bad), None, "{bad:?}");
        }
    }

    /// Only a local fixed disk is a drive whose folders may be opened.
    #[test]
    fn a_drive_that_is_not_a_fixed_disk_is_refused() {
        assert!(drive_is_fixed(b'C'));
        // A letter that points nowhere (the last one, `Z`, is not assigned on a test machine
        // unless someone mapped it: then the test is not about it).
        if !std::path::Path::new(r"Z:\").exists() {
            assert!(!drive_is_fixed(b'Z'));
        }
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
