//! Raw Win32 calls of [`crate::acl`]: the only `unsafe` code of the ACL checks.
//!
//! Every function copies what Windows returns into owned bytes before releasing it, so no
//! pointer or handle leaves this module.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use crate::ffi_handle::Handle;
use windows_sys::Win32::Foundation::{ERROR_SUCCESS, HANDLE, INVALID_HANDLE_VALUE, LocalFree};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
    SE_FILE_OBJECT, SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetLengthSid, GetTokenInformation, IsValidSid,
    OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER, TokenUser,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, GetFileInformationByHandle, GetVolumeInformationByHandleW, OPEN_EXISTING,
    READ_CONTROL,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

/// Owner and DACL of an object, copied out of its security descriptor.
pub(crate) struct RawSecurity {
    /// Bytes of the owner SID.
    pub owner: Vec<u8>,
    /// Bytes of the DACL; `None` for a missing or NULL DACL (full access to everyone).
    pub dacl: Option<Vec<u8>>,
    /// The object is a reparse point (symlink, junction, mount point).
    pub reparse_point: bool,
    /// Its volume keeps ACLs (`FILE_PERSISTENT_ACLS`).
    pub persistent_acls: bool,
}

/// `FILE_PERSISTENT_ACLS` (in `Win32_System_SystemServices`, not worth the feature).
const FILE_PERSISTENT_ACLS: u32 = 0x0000_0008;

/// A block allocated by Windows with `LocalAlloc`, freed on drop.
struct Local(*mut core::ffi::c_void);

impl Drop for Local {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: the pointer came from an API documented to allocate with `LocalAlloc`,
            // and it is freed only here.
            unsafe { LocalFree(self.0) };
        }
    }
}

fn wide(path: &Path) -> Vec<u16> {
    OsStr::new(path).encode_wide().chain(Some(0)).collect()
}

/// Copies a SID that Windows owns into owned bytes.
///
/// `sid` must point to memory that stays valid during the call.
fn copy_sid(sid: PSID) -> io::Result<Vec<u8>> {
    if sid.is_null() {
        return Err(io::Error::other("no SID"));
    }
    // SAFETY: `sid` is non-null and points to a SID returned by Windows; `IsValidSid` only
    // reads it.
    if unsafe { IsValidSid(sid) } == 0 {
        return Err(io::Error::other("invalid SID"));
    }
    // SAFETY: `sid` was just validated by `IsValidSid`.
    let len = unsafe { GetLengthSid(sid) } as usize;
    // SAFETY: a valid SID is exactly `GetLengthSid` bytes long and stays alive during the call.
    let bytes = unsafe { std::slice::from_raw_parts(sid.cast::<u8>(), len) };
    Ok(bytes.to_vec())
}

/// Copies an ACL that Windows owns into owned bytes, using the size in its header.
fn copy_acl(acl: *const ACL) -> Vec<u8> {
    // SAFETY: `acl` is non-null (checked by the caller) and points to an ACL header; this only
    // computes the address of its `AclSize` field.
    let field = unsafe { &raw const (*acl).AclSize };
    // SAFETY: the header is readable, so its `AclSize` field is too.
    let size = unsafe { field.read_unaligned() } as usize;
    // SAFETY: Windows guarantees that an ACL occupies `AclSize` bytes from its start.
    let bytes = unsafe { std::slice::from_raw_parts(acl.cast::<u8>(), size) };
    bytes.to_vec()
}

/// Reads owner and DACL of `path` through a handle opened without following a reparse point,
/// so the path cannot change between opening and reading.
pub(crate) fn read_security(path: &Path) -> io::Result<RawSecurity> {
    let name = wide(path);
    // SAFETY: `name` is a NUL-terminated UTF-16 string alive for the call; the other pointers
    // are null, which the API accepts.
    let raw = unsafe {
        CreateFileW(
            name.as_ptr(),
            READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let handle = Handle::new(raw).ok_or_else(|| io::Error::other("invalid handle"))?;

    // SAFETY: zeroed is a valid value of this plain C struct.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `handle.raw()` is open and `info` is a local the call writes into.
    if unsafe { GetFileInformationByHandle(handle.raw(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut fs_flags = 0u32;
    // SAFETY: `handle.raw()` is open; null buffers with size 0 and null out pointers are accepted
    // for the values not wanted; `fs_flags` is a local.
    let ok = unsafe {
        GetVolumeInformationByHandleW(
            handle.raw(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut fs_flags,
            ptr::null_mut(),
            0,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut owner: PSID = ptr::null_mut();
    let mut dacl: *mut ACL = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `handle.raw()` is an open handle with READ_CONTROL; every out pointer points to a
    // local that lives through the call. `owner` and `dacl` point into `descriptor`.
    let status = unsafe {
        GetSecurityInfo(
            handle.raw(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    // Frees the descriptor, and with it `owner` and `dacl`, after both are copied.
    let _descriptor = Local(descriptor);
    let owner = copy_sid(owner)?;
    let dacl = (!dacl.is_null()).then(|| copy_acl(dacl));
    Ok(RawSecurity {
        owner,
        dacl,
        reparse_point: info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0,
        persistent_acls: fs_flags & FILE_PERSISTENT_ACLS != 0,
    })
}

/// Bytes of the SID of the user the process runs as (`TokenUser`).
pub(crate) fn current_user_sid() -> io::Result<Vec<u8>> {
    let mut raw: HANDLE = ptr::null_mut();
    // SAFETY: `GetCurrentProcess` returns a pseudo-handle that needs no closing.
    let process = unsafe { GetCurrentProcess() };
    // SAFETY: `raw` is a local out pointer that lives through the call.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut raw) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = Handle::new(raw).ok_or_else(|| io::Error::other("invalid token handle"))?;

    let mut needed = 0u32;
    // SAFETY: a null buffer of length 0 asks only for the size, written to `needed`.
    unsafe { GetTokenInformation(token.raw(), TokenUser, ptr::null_mut(), 0, &mut needed) };
    if needed == 0 {
        return Err(io::Error::last_os_error());
    }
    // `u64` elements keep the buffer aligned for `TOKEN_USER`.
    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: `buffer` holds at least `needed` writable bytes and lives through the call.
    let ok = unsafe {
        GetTokenInformation(
            token.raw(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call succeeded, so `buffer` starts with an aligned `TOKEN_USER` whose SID
    // points inside `buffer`, which outlives the copy below.
    let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
    copy_sid(sid)
}

/// A security descriptor built from SDDL, owned and freed on drop.
pub(crate) struct Descriptor(Local);

// SAFETY: the descriptor is a block of memory owned only by this value, never written after
// `from_sddl`; Windows reads it from whichever thread passes it.
unsafe impl Send for Descriptor {}
// SAFETY: as above: shared access only reads it.
unsafe impl Sync for Descriptor {}

impl Descriptor {
    pub(crate) fn from_sddl(sddl: &str) -> io::Result<Self> {
        let sddl: Vec<u16> = OsStr::new(sddl).encode_wide().chain(Some(0)).collect();
        let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
        // SAFETY: `sddl` is NUL-terminated and alive for the call; `descriptor` is a local out
        // pointer; the size out pointer may be null.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(Local(descriptor)))
    }

    /// Non-inheritable attributes pointing to this descriptor, valid while `self` lives.
    pub(crate) fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: self.0.0,
            bInheritHandle: 0,
        }
    }
}

/// Creates one folder with the security descriptor given in SDDL, so it never exists with
/// other permissions.
pub(crate) fn create_dir_with_sddl(path: &Path, sddl: &str) -> io::Result<()> {
    let descriptor = Descriptor::from_sddl(sddl)?;
    let attributes = descriptor.attributes();
    let name = wide(path);
    // SAFETY: `name` is NUL-terminated and `attributes` points to a descriptor alive for the
    // call.
    if unsafe { CreateDirectoryW(name.as_ptr(), &attributes) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Owner and DACL of a kernel object through its open handle (a pipe instance, say).
pub(crate) fn handle_security(handle: &Handle) -> io::Result<(Vec<u8>, Option<Vec<u8>>)> {
    let mut owner: PSID = ptr::null_mut();
    let mut dacl: *mut ACL = ptr::null_mut();
    let mut descriptor: PSECURITY_DESCRIPTOR = ptr::null_mut();
    // SAFETY: `handle.raw()` is open; every out pointer points to a local that lives through
    // the call. `owner` and `dacl` point into `descriptor`.
    let status = unsafe {
        GetSecurityInfo(
            handle.raw(),
            SE_KERNEL_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let _descriptor = Local(descriptor);
    let owner = copy_sid(owner)?;
    let dacl = (!dacl.is_null()).then(|| copy_acl(dacl));
    Ok((owner, dacl))
}
