//! Raw Win32 calls of [`crate::pipe`]: named-pipe instances, client opens and overlapped I/O.
//!
//! Every operation is overlapped with its own event and `OVERLAPPED`, and is always waited to
//! its end (completed, failed or cancelled) before returning, so no buffer or `OVERLAPPED`
//! outlives the call that Windows writes into. Handles are owned by [`Handle`] on every path.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use std::time::Duration;

use crate::ffi_acl::Descriptor;
use crate::ffi_handle::Handle;
use windows_sys::Win32::Foundation::{
    ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_NO_DATA, ERROR_OPERATION_ABORTED, ERROR_PIPE_BUSY,
    ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GENERIC_WRITE, GetLastError,
    HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, OPEN_EXISTING,
    PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, WriteFile,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId,
    PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT, WaitNamedPipeW,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects, WaitForSingleObject,
};

/// Bytes of each direction's buffer of a pipe instance.
const BUFFER: u32 = 64 * 1024;

pub(crate) fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain(Some(0)).collect()
}

fn last_error() -> u32 {
    // SAFETY: reads the calling thread's last-error value; no arguments.
    unsafe { GetLastError() }
}

/// A manual-reset event, born unsignaled.
pub(crate) struct Event(Handle);

impl Event {
    pub(crate) fn new() -> io::Result<Self> {
        // SAFETY: null attributes and name are accepted; plain values otherwise. The result is
        // checked by `Handle::new`.
        let raw = unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) };
        Handle::new(raw)
            .map(Self)
            .ok_or_else(io::Error::last_os_error)
    }

    pub(crate) fn set(&self) {
        // SAFETY: `self.0` is an open event handle.
        unsafe { SetEvent(self.0.raw()) };
    }

    /// Waits up to `timeout` for the event; `true` if it is signaled.
    pub(crate) fn wait(&self, timeout: Duration) -> bool {
        // SAFETY: `self.0` is an open event handle.
        unsafe { WaitForSingleObject(self.0.raw(), timeout_ms(Some(timeout))) == WAIT_OBJECT_0 }
    }
}

/// Every instance of the pipe is in use (`ERROR_PIPE_BUSY`).
pub(crate) fn is_busy(err: &io::Error) -> bool {
    err.raw_os_error() == Some(ERROR_PIPE_BUSY as i32)
}

/// The other end closed or disconnected the pipe.
pub(crate) fn is_closed(err: &io::Error) -> bool {
    [ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_NOT_CONNECTED]
        .iter()
        .any(|code| err.raw_os_error() == Some(*code as i32))
}

/// How an overlapped operation ended.
pub(crate) enum Outcome {
    /// It completed, with the bytes transferred.
    Done(u32),
    /// The deadline passed first; the operation was cancelled.
    TimedOut,
    /// The stop event was signaled first; the operation was cancelled.
    Stopped,
}

/// How starting an operation went.
enum Start {
    /// Queued or already complete: its event will be (or is) signaled.
    Queued,
    /// Complete without touching the event (`ERROR_PIPE_CONNECTED`).
    Done,
    Failed(u32),
}

fn timeout_ms(timeout: Option<Duration>) -> u32 {
    timeout.map_or(INFINITE, |d| {
        u32::try_from(d.as_millis()).map_or(INFINITE - 1, |ms| ms.min(INFINITE - 1))
    })
}

/// Starts one overlapped operation on `handle` with `start`, then waits for it, for `stop` or
/// for `timeout`. A stopped or late operation is cancelled and waited to its end, so `ov` is
/// never written after this returns. An operation that completed while being cancelled counts
/// as done: its bytes were transferred.
fn run(
    handle: &Handle,
    stop: &Event,
    timeout: Option<Duration>,
    start: impl FnOnce(*mut OVERLAPPED) -> Start,
) -> io::Result<Outcome> {
    let done = Event::new()?;
    // SAFETY: zeroed is a valid value of this plain C struct.
    let mut ov: OVERLAPPED = unsafe { std::mem::zeroed() };
    ov.hEvent = done.0.raw();
    match start(&mut ov) {
        Start::Queued => {}
        Start::Done => return Ok(Outcome::Done(0)),
        Start::Failed(code) => return Err(io::Error::from_raw_os_error(code as i32)),
    }
    let waits: [HANDLE; 2] = [done.0.raw(), stop.0.raw()];
    // SAFETY: both handles are open events alive through the call; the count matches the array.
    let woke = unsafe { WaitForMultipleObjects(2, waits.as_ptr(), 0, timeout_ms(timeout)) };
    let early = match woke {
        WAIT_OBJECT_0 => None,
        w if w == WAIT_OBJECT_0 + 1 => Some(Outcome::Stopped),
        WAIT_TIMEOUT => Some(Outcome::TimedOut),
        _ => Some(Outcome::Stopped),
    };
    if early.is_some() {
        // SAFETY: `handle` is open and `ov` is the `OVERLAPPED` of the operation started above,
        // still alive; cancelling an operation that already ended is harmless.
        unsafe { CancelIoEx(handle.raw(), &ov) };
    }
    let mut transferred = 0u32;
    // SAFETY: `handle` is open, `ov` belongs to the operation started on it and lives through
    // the call; `bWait = TRUE` returns only once the operation has ended.
    let ok = unsafe { GetOverlappedResult(handle.raw(), &ov, &mut transferred, 1) };
    if ok != 0 {
        return Ok(Outcome::Done(transferred));
    }
    match (last_error(), early) {
        (ERROR_OPERATION_ABORTED, Some(outcome)) => Ok(outcome),
        (code, _) => Err(io::Error::from_raw_os_error(code as i32)),
    }
}

fn queued(ok: i32) -> Start {
    if ok != 0 {
        return Start::Queued;
    }
    match last_error() {
        ERROR_IO_PENDING => Start::Queued,
        code => Start::Failed(code),
    }
}

/// Reads into `buf`.
pub(crate) fn read(
    handle: &Handle,
    buf: &mut [u8],
    stop: &Event,
    timeout: Option<Duration>,
) -> io::Result<Outcome> {
    let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
    let data = buf.as_mut_ptr();
    run(handle, stop, timeout, |ov| {
        // SAFETY: `data` points to `len` writable bytes of `buf`, which outlives `run`; `ov` is
        // the live `OVERLAPPED` of `run`, which waits for the read to end before returning.
        queued(unsafe { ReadFile(handle.raw(), data, len, ptr::null_mut(), ov) })
    })
}

/// Writes from `buf`.
pub(crate) fn write(
    handle: &Handle,
    buf: &[u8],
    stop: &Event,
    timeout: Option<Duration>,
) -> io::Result<Outcome> {
    let len = u32::try_from(buf.len()).unwrap_or(u32::MAX);
    let data = buf.as_ptr();
    run(handle, stop, timeout, |ov| {
        // SAFETY: `data` points to `len` readable bytes of `buf`, which outlives `run`; `ov` is
        // the live `OVERLAPPED` of `run`, which waits for the write to end before returning.
        queued(unsafe { WriteFile(handle.raw(), data, len, ptr::null_mut(), ov) })
    })
}

/// Creates one server instance of the pipe `name` (NUL-terminated), protected by `descriptor`,
/// byte mode, overlapped, refusing remote clients. With `first`, creation fails if any instance
/// of that name already exists (`ERROR_ACCESS_DENIED`).
pub(crate) fn create_instance(
    name: &[u16],
    descriptor: &Descriptor,
    first: bool,
    max_instances: u32,
) -> io::Result<Handle> {
    let attributes = descriptor.attributes();
    let mut open_mode = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED;
    if first {
        open_mode |= FILE_FLAG_FIRST_PIPE_INSTANCE;
    }
    // SAFETY: `name` is NUL-terminated and alive for the call; `attributes` points to a
    // descriptor owned by `descriptor`, alive for the call; the rest are plain values.
    let raw = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            open_mode,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            max_instances,
            BUFFER,
            BUFFER,
            0,
            &attributes,
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Handle::new(raw).ok_or_else(|| io::Error::other("invalid pipe handle"))
}

/// Waits for a client on a server instance, or for `stop`.
pub(crate) fn accept(handle: &Handle, stop: &Event) -> io::Result<Outcome> {
    run(handle, stop, None, |ov| {
        // SAFETY: `handle` is a server instance opened overlapped; `ov` is the live
        // `OVERLAPPED` of `run`, which waits for the connection to end before returning.
        let ok = unsafe { ConnectNamedPipe(handle.raw(), ov) };
        if ok == 0 && last_error() == ERROR_PIPE_CONNECTED {
            // A client connected between creation and this call.
            return Start::Done;
        }
        queued(ok)
    })
}

/// Opens the client end of `name` (NUL-terminated), overlapped, letting the server only
/// identify this process, never impersonate it (SQOS).
pub(crate) fn open_client(name: &[u16]) -> io::Result<Handle> {
    // SAFETY: `name` is NUL-terminated and alive for the call; null attributes and template are
    // accepted; the rest are plain values.
    let raw = unsafe {
        CreateFileW(
            name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
            ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    Handle::new(raw).ok_or_else(|| io::Error::other("invalid pipe handle"))
}

/// Waits up to `timeout` for a free instance of `name`. `false` on timeout or error.
pub(crate) fn wait_free(name: &[u16], timeout: Duration) -> bool {
    // SAFETY: `name` is NUL-terminated and alive for the call.
    unsafe { WaitNamedPipeW(name.as_ptr(), timeout_ms(Some(timeout)).max(1)) != 0 }
}

/// The pid of the client process connected to a server instance.
pub(crate) fn client_pid(handle: &Handle) -> io::Result<u32> {
    let mut pid = 0u32;
    // SAFETY: `handle` is an open server instance; `pid` is a local out value.
    if unsafe { GetNamedPipeClientProcessId(handle.raw(), &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(pid)
}

/// The pid of the server process of a client end.
pub(crate) fn server_pid(handle: &Handle) -> io::Result<u32> {
    let mut pid = 0u32;
    // SAFETY: `handle` is an open client end; `pid` is a local out value.
    if unsafe { GetNamedPipeServerProcessId(handle.raw(), &mut pid) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(pid)
}
