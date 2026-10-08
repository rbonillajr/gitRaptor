//! Every Win32 call of the crate, one per `unsafe` block, behind safe
//! functions. Nothing here is public outside the crate.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::OnceLock;

use windows_sys::Wdk::System::Threading::{NtQueryInformationProcess, PROCESSINFOCLASS};
use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_NO_MORE_FILES, FILETIME, HANDLE, HANDLE_FLAG_INHERIT,
    INVALID_HANDLE_VALUE, STILL_ACTIVE, SetHandleInformation,
};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::Console::{
    GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::RemoteDesktop::{
    ProcessIdToSessionId, WTS_CONNECTSTATE_CLASS, WTS_CURRENT_SERVER_HANDLE, WTSActive,
    WTSConnectState, WTSFreeMemory, WTSQuerySessionInformationW,
};
use windows_sys::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, GetProcessTimes, OpenProcess, OpenProcessToken,
    PROCESS_BASIC_INFORMATION, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_VM_READ, QueryFullProcessImageNameW,
};

use crate::ffi_handle::Handle;
use crate::process::{Error, Owner};

/// Longest image path read, in UTF-16 units (the `\\?\` limit).
const MAX_PATH_UNITS: usize = 32_768;

/// Largest `TOKEN_USER` accepted, in bytes (a SID is at most 68 bytes).
const MAX_TOKEN_USER: u32 = 1024;

fn last_error() -> Option<u32> {
    std::io::Error::last_os_error()
        .raw_os_error()
        .map(|e| e as u32)
}

/// `(pid, parent pid)` of every live process.
pub(crate) fn snapshot() -> Option<Vec<(u32, u32)>> {
    // SAFETY: plain values; the result is checked by `Handle::new`.
    let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    let snap = Handle::new(raw)?;
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut out = Vec::new();
    // SAFETY: `snap` is a valid snapshot handle and `entry` an initialized
    // `PROCESSENTRY32W` with `dwSize` set, borrowed only for the call.
    let mut more = unsafe { Process32FirstW(snap.raw(), &mut entry) } != 0;
    while more {
        out.push((entry.th32ProcessID, entry.th32ParentProcessID));
        // SAFETY: as above.
        more = unsafe { Process32NextW(snap.raw(), &mut entry) } != 0;
    }
    // The list ends with `ERROR_NO_MORE_FILES`; anything else is a failure.
    (last_error() == Some(ERROR_NO_MORE_FILES)).then_some(out)
}

pub(crate) fn open(pid: u32) -> Result<Handle, Error> {
    // SAFETY: plain values; the result is checked by `Handle::new`.
    let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    // The last error is read right after the failed call.
    Handle::new(raw).ok_or_else(|| match last_error() {
        Some(ERROR_ACCESS_DENIED) => Error::Denied,
        _ => Error::Gone,
    })
}

/// Like [`open`], with the right to read the process's memory: only to read its working folder
/// ([`current_directory`]), and only after the caller checked the owner.
pub(crate) fn open_reading(pid: u32) -> Result<Handle, Error> {
    // SAFETY: plain values; the result is checked by `Handle::new`.
    let raw = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid) };
    // The last error is read right after the failed call.
    Handle::new(raw).ok_or_else(|| match last_error() {
        Some(ERROR_ACCESS_DENIED) => Error::Denied,
        _ => Error::Gone,
    })
}

pub(crate) fn created(process: &Handle) -> Option<u64> {
    let mut times = [FILETIME::default(); 4];
    let [creation, exit, kernel, user] = &mut times;
    // SAFETY: `process` is a valid handle with query rights and the four
    // pointers are distinct, writable `FILETIME`s that outlive the call.
    let ok = unsafe { GetProcessTimes(process.raw(), creation, exit, kernel, user) };
    (ok != 0).then(|| u64::from(creation.dwHighDateTime) << 32 | u64::from(creation.dwLowDateTime))
}

/// Whether the process has ended. One that ended with the code 259 (`STILL_ACTIVE`) or whose
/// code cannot be read counts as running here; the process list leaves it out once it is torn
/// down.
pub(crate) fn ended(process: &Handle) -> bool {
    let mut code = 0u32;
    // SAFETY: `process` is a valid handle with query rights and `code` a
    // writable out pointer that outlives the call.
    let ok = unsafe { GetExitCodeProcess(process.raw(), &mut code) };
    ok != 0 && code != STILL_ACTIVE as u32
}

pub(crate) fn image(process: &Handle) -> Option<PathBuf> {
    let mut buf = vec![0u16; MAX_PATH_UNITS];
    let mut len = MAX_PATH_UNITS as u32;
    // SAFETY: `buf` holds `len` writable UTF-16 units and `len` is a valid
    // in/out pointer; the API writes at most `len` units.
    let ok = unsafe {
        QueryFullProcessImageNameW(
            process.raw(),
            PROCESS_NAME_WIN32,
            buf.as_mut_ptr(),
            &mut len,
        )
    };
    let len = len as usize;
    (ok != 0 && len > 0 && len < MAX_PATH_UNITS)
        .then(|| PathBuf::from(OsString::from_wide(&buf[..len])))
}

/// The `TOKEN_USER` of a process, in an 8-aligned buffer that also holds
/// the SID it points to.
struct TokenUserBuf(Vec<u64>);

impl TokenUserBuf {
    fn read(process: HANDLE) -> Option<Self> {
        let mut raw: HANDLE = null_mut();
        // SAFETY: `process` is a valid process handle (or this process's
        // pseudo-handle) and `raw` a writable out pointer.
        let ok = unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut raw) };
        if ok == 0 {
            return None;
        }
        let token = Handle::new(raw)?;
        let mut needed = 0u32;
        // SAFETY: a null buffer of length 0 only asks for the size, written
        // to `needed`; the call fails by design.
        unsafe { GetTokenInformation(token.raw(), TokenUser, null_mut(), 0, &mut needed) };
        if needed < std::mem::size_of::<TOKEN_USER>() as u32 || needed > MAX_TOKEN_USER {
            return None;
        }
        let mut buf = vec![0u64; (needed as usize).div_ceil(8)];
        // SAFETY: `buf` is 8-aligned and holds at least `needed` writable
        // bytes; `needed` is a valid out pointer.
        let ok = unsafe {
            GetTokenInformation(
                token.raw(),
                TokenUser,
                buf.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        };
        (ok != 0).then_some(Self(buf))
    }

    fn same_user(&self, other: &Self) -> bool {
        // SAFETY: both buffers were filled by `GetTokenInformation(TokenUser)`,
        // so each starts with an aligned `TOKEN_USER` whose SID lies inside
        // the same buffer, which is alive and unchanged for this call.
        let a = unsafe { &*self.0.as_ptr().cast::<TOKEN_USER>() };
        // SAFETY: as above, for `other`.
        let b = unsafe { &*other.0.as_ptr().cast::<TOKEN_USER>() };
        // SAFETY: both SIDs are valid (see above) and only read.
        unsafe { EqualSid(a.User.Sid, b.User.Sid) != 0 }
    }
}

/// `ProcessConsoleHostProcess`: not in the SDK headers, stable since Windows 8 (Process
/// Explorer, System Informer and Task Manager read it).
const PROCESS_CONSOLE_HOST_PROCESS: PROCESSINFOCLASS = 49;

/// The raw `ProcessConsoleHostProcess` value of a process: for a console client, the pid of
/// the process hosting its console with bit 0 set. `None` if the kernel refuses the query.
pub(crate) fn console_host_raw(process: &Handle) -> Option<usize> {
    let mut value = 0usize;
    let mut len = 0u32;
    // SAFETY: `process` is a valid handle with query rights; `value` is a writable `usize`
    // whose size is the length passed, and `len` a writable out pointer, both outliving the call.
    let status = unsafe {
        NtQueryInformationProcess(
            process.raw(),
            PROCESS_CONSOLE_HOST_PROCESS,
            (&raw mut value).cast(),
            std::mem::size_of::<usize>() as u32,
            &mut len,
        )
    };
    (status >= 0 && len as usize == std::mem::size_of::<usize>()).then_some(value)
}

/// The Windows session of a live process. `None` if it cannot be read.
pub(crate) fn session_id(pid: u32) -> Option<u32> {
    let mut id = 0u32;
    // SAFETY: plain value and a writable out pointer that outlives the call.
    let ok = unsafe { ProcessIdToSessionId(pid, &mut id) };
    (ok != 0).then_some(id)
}

/// Whether the Windows session `id` has a user connected (`WTSActive`): the physical console
/// or a connected Remote Desktop session. Any failure answers no.
pub(crate) fn session_active(id: u32) -> bool {
    let mut buf = null_mut();
    let mut bytes = 0u32;
    // SAFETY: the local server's pseudo-handle, plain values and two writable out pointers that
    // outlive the call; on success `buf` is a buffer the API allocated, freed below.
    let ok = unsafe {
        WTSQuerySessionInformationW(
            WTS_CURRENT_SERVER_HANDLE,
            id,
            WTSConnectState,
            &mut buf,
            &mut bytes,
        )
    };
    if ok == 0 || buf.is_null() {
        return false;
    }
    let state = (bytes as usize >= std::mem::size_of::<WTS_CONNECTSTATE_CLASS>()).then(|| {
        // SAFETY: `buf` holds at least `bytes` readable bytes, enough for one
        // `WTS_CONNECTSTATE_CLASS`, and stays allocated until the free below.
        unsafe { buf.cast::<WTS_CONNECTSTATE_CLASS>().read_unaligned() }
    });
    // SAFETY: `buf` was allocated by `WTSQuerySessionInformationW` and is freed once.
    unsafe { WTSFreeMemory(buf.cast()) };
    state == Some(WTSActive)
}

/// The current user's `TOKEN_USER`, read once.
fn current_user() -> Option<&'static TokenUserBuf> {
    static CURRENT: OnceLock<Option<TokenUserBuf>> = OnceLock::new();
    CURRENT
        .get_or_init(|| {
            // SAFETY: returns a pseudo-handle that needs no closing.
            let me = unsafe { GetCurrentProcess() };
            TokenUserBuf::read(me)
        })
        .as_ref()
}

pub(crate) fn owner(process: &Handle) -> Owner {
    match (TokenUserBuf::read(process.raw()), current_user()) {
        (Some(theirs), Some(mine)) if theirs.same_user(mine) => Owner::Current,
        (Some(_), Some(_)) => Owner::Other,
        _ => Owner::Unknown,
    }
}

/// The shared Windows folder (`C:\\Windows`), from the kernel.
pub(crate) fn windows_dir() -> Option<PathBuf> {
    let mut buf = vec![0u16; MAX_PATH_UNITS];
    // SAFETY: `buf` holds `MAX_PATH_UNITS` writable UTF-16 units, the size
    // passed; the API writes at most that many.
    let len =
        unsafe { GetSystemWindowsDirectoryW(buf.as_mut_ptr(), MAX_PATH_UNITS as u32) } as usize;
    (len > 0 && len < MAX_PATH_UNITS).then(|| PathBuf::from(OsString::from_wide(&buf[..len])))
}

/// Makes this process's standard handles non-inheritable. A detached child (the daemon) would
/// otherwise inherit them and keep the caller's pipes open after this process exits.
/// `std::process::Command` duplicates them anew for children that inherit their stdio, so
/// those are unaffected.
pub(crate) fn keep_std_handles_private() {
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        clear_inherit(which);
    }
}

fn clear_inherit(which: STD_HANDLE) {
    // SAFETY: plain value; returns a borrowed handle of this process, or null or
    // `INVALID_HANDLE_VALUE`, which are skipped.
    let handle = unsafe { GetStdHandle(which) };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return;
    }
    // SAFETY: `handle` is a standard handle of this process, open while it runs; only its
    // inherit flag changes.
    unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
}

/// `ProcessBasicInformation` of `NtQueryInformationProcess`.
const PROCESS_BASIC_INFORMATION_CLASS: PROCESSINFOCLASS = 0;

/// `ProcessWow64Information`: not zero for a 32-bit process on 64-bit Windows.
const PROCESS_WOW64_INFORMATION: PROCESSINFOCLASS = 26;

/// Offset of `ProcessParameters` in the 64-bit `PEB` (stable since Windows Vista).
const PEB_PROCESS_PARAMETERS: usize = 0x20;

/// Offset of `CurrentDirectory.DosPath`, a `UNICODE_STRING`, in the 64-bit
/// `RTL_USER_PROCESS_PARAMETERS` (stable since Windows Vista).
const PARAMETERS_CURRENT_DIRECTORY: usize = 0x38;

/// Reads exactly `out.len()` bytes of the memory of `process` at `address`.
fn read_memory(process: &Handle, address: usize, out: &mut [u8]) -> bool {
    let mut read = 0usize;
    // SAFETY: `process` is a valid handle with the right to read memory; `out` is a writable
    // buffer of the length passed and `read` a writable out pointer, both outliving the call.
    // Whatever address of the other process is bad, the call fails instead of faulting here.
    let ok = unsafe {
        ReadProcessMemory(
            process.raw(),
            address as *const _,
            out.as_mut_ptr().cast(),
            out.len(),
            &mut read,
        )
    };
    ok != 0 && read == out.len()
}

fn read_usize(process: &Handle, address: usize) -> Option<usize> {
    let mut bytes = [0u8; std::mem::size_of::<usize>()];
    read_memory(process, address, &mut bytes).then(|| usize::from_le_bytes(bytes))
}

/// The working folder of a 64-bit process as UTF-16 units (`DosPath` of its current directory),
/// at most [`MAX_PATH_UNITS`]. It reads three small pieces of the process's memory and nothing
/// else: the pointer to its parameters, the `UNICODE_STRING` of the folder and the folder text.
/// Never its command line or its environment (SEC-04). `None` for a 32-bit process (its folder
/// lives in another structure: refused, not guessed), when the memory cannot be read, or when
/// what is read does not have the shape of a path. Only for a 64-bit reader, whose offsets these
/// are.
pub(crate) fn current_directory(process: &Handle) -> Option<Vec<u16>> {
    if !cfg!(target_pointer_width = "64") {
        return None;
    }
    let mut wow64 = 0usize;
    let mut len = 0u32;
    // SAFETY: `process` is a valid handle with query rights; `wow64` is a writable `usize` whose
    // size is the length passed, and `len` a writable out pointer, both outliving the call.
    let status = unsafe {
        NtQueryInformationProcess(
            process.raw(),
            PROCESS_WOW64_INFORMATION,
            (&raw mut wow64).cast(),
            std::mem::size_of::<usize>() as u32,
            &mut len,
        )
    };
    if status < 0 || wow64 != 0 {
        return None;
    }
    let mut basic = PROCESS_BASIC_INFORMATION::default();
    // SAFETY: `process` is a valid handle with query rights; `basic` is a writable
    // `PROCESS_BASIC_INFORMATION` whose size is the length passed, and `len` a writable out
    // pointer, both outliving the call.
    let status = unsafe {
        NtQueryInformationProcess(
            process.raw(),
            PROCESS_BASIC_INFORMATION_CLASS,
            (&raw mut basic).cast(),
            std::mem::size_of::<PROCESS_BASIC_INFORMATION>() as u32,
            &mut len,
        )
    };
    let peb = basic.PebBaseAddress as usize;
    if status < 0 || peb == 0 {
        return None;
    }
    let parameters = read_usize(process, peb.checked_add(PEB_PROCESS_PARAMETERS)?)?;
    if parameters == 0 {
        return None;
    }
    // `UNICODE_STRING`: `Length` and `MaximumLength` (bytes), padding, then `Buffer`.
    let mut string = [0u8; 16];
    if !read_memory(
        process,
        parameters.checked_add(PARAMETERS_CURRENT_DIRECTORY)?,
        &mut string,
    ) {
        return None;
    }
    let length = usize::from(u16::from_le_bytes([string[0], string[1]]));
    let maximum = usize::from(u16::from_le_bytes([string[2], string[3]]));
    let buffer = usize::from_le_bytes(string[8..16].try_into().ok()?);
    if length == 0
        || length % 2 != 0
        || length > maximum
        || length > MAX_PATH_UNITS * 2
        || buffer == 0
    {
        return None;
    }
    let mut bytes = vec![0u8; length];
    if !read_memory(process, buffer, &mut bytes) {
        return None;
    }
    Some(
        bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect(),
    )
}
