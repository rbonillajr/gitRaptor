//! The one owner of kernel handles for every FFI module of the crate.

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};

/// An owned kernel handle, closed once on drop. Never a sentinel: APIs fail
/// with NULL (`OpenProcess`) or `INVALID_HANDLE_VALUE` (`CreateFileW`,
/// `CreateToolhelp32Snapshot`), and neither is ever wrapped.
pub(crate) struct Handle(HANDLE);

impl Handle {
    pub(crate) fn new(raw: HANDLE) -> Option<Self> {
        (!raw.is_null() && raw != INVALID_HANDLE_VALUE).then_some(Self(raw))
    }

    /// The raw value, valid while `self` lives. Never closed by the caller.
    pub(crate) fn raw(&self) -> HANDLE {
        self.0
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a valid handle (not a sentinel, see `new`) owned
        // only by this guard, so it is closed exactly once.
        unsafe { CloseHandle(self.0) };
    }
}
