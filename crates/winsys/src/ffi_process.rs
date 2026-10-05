//! Raw Win32 calls of [`crate::process`]. The pseudo-handle of the current process needs no
//! closing, and only plain numbers leave this module.

use std::mem;

use windows_sys::Win32::Foundation::FILETIME;
use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetProcessHandleCount, GetProcessTimes,
};

fn current() -> windows_sys::Win32::Foundation::HANDLE {
    // SAFETY: no preconditions; it returns a constant pseudo-handle that needs no closing.
    unsafe { GetCurrentProcess() }
}

fn filetime(t: FILETIME) -> u64 {
    (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime)
}

/// User plus kernel time of the current process, in 100 ns units.
pub(crate) fn cpu_time_100ns() -> Option<u64> {
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut creation, mut exit, mut kernel, mut user) = (zero, zero, zero, zero);
    let process = current();
    // SAFETY: the current-process pseudo-handle is always valid, and the four out pointers
    // point to live, writable `FILETIME`s of this frame.
    let ok = unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) };
    (ok != 0).then(|| filetime(kernel).saturating_add(filetime(user)))
}

/// Working set of the current process, in bytes.
pub(crate) fn working_set_bytes() -> Option<u64> {
    // SAFETY: `PROCESS_MEMORY_COUNTERS` is plain data; all zeros is a valid value.
    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { mem::zeroed() };
    let size = u32::try_from(mem::size_of::<PROCESS_MEMORY_COUNTERS>()).ok()?;
    counters.cb = size;
    let process = current();
    // SAFETY: the pseudo-handle is valid and `counters` is a live, writable struct of `size`
    // bytes.
    let ok = unsafe { K32GetProcessMemoryInfo(process, &mut counters, size) };
    (ok != 0).then_some(counters.WorkingSetSize as u64)
}

/// Open handles of the current process.
pub(crate) fn handle_count() -> Option<u64> {
    let mut count = 0u32;
    let process = current();
    // SAFETY: the pseudo-handle is valid and `count` is a live, writable `u32`.
    let ok = unsafe { GetProcessHandleCount(process, &mut count) };
    (ok != 0).then_some(u64::from(count))
}
