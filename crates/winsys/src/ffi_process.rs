//! Every Win32 call of the crate, one per `unsafe` block, behind safe
//! functions. Nothing here is public outside the crate.

use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_NO_MORE_FILES, FILETIME, HANDLE, STILL_ACTIVE,
};
use windows_sys::Win32::Security::{
    EqualSid, GetTokenInformation, TOKEN_QUERY, TOKEN_USER, TokenUser,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, GetProcessTimes, OpenProcess, OpenProcessToken,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
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
