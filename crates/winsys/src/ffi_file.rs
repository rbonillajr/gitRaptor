//! Byte-range locks (`LockFileEx`, `UnlockFileEx`), the identity of an open file
//! (`GetFileInformationByHandle`) and renames that never replace (`MoveFileExW`), one call per
//! `unsafe` block. Nothing here is public outside the crate.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;

use windows_sys::Win32::Foundation::{ERROR_LOCK_VIOLATION, HANDLE};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_BASIC_INFO, FILE_DISPOSITION_INFO, FileBasicInfo,
    FileDispositionInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
    GetVolumeInformationByHandleW, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx,
    MOVEFILE_WRITE_THROUGH, MoveFileExW, SetFileInformationByHandle, UnlockFileEx,
};
use windows_sys::Win32::System::IO::OVERLAPPED;

/// An `OVERLAPPED` that only carries the start of the range.
fn at(offset: u64) -> OVERLAPPED {
    let mut overlapped = OVERLAPPED::default();
    overlapped.Anonymous.Anonymous.Offset = offset as u32;
    overlapped.Anonymous.Anonymous.OffsetHigh = (offset >> 32) as u32;
    overlapped
}

/// Locks the byte at `offset` without waiting. `Ok(false)`: another handle holds a
/// conflicting lock on it.
pub(crate) fn try_lock_byte(file: &File, offset: u64, exclusive: bool) -> io::Result<bool> {
    let handle = file.as_raw_handle() as HANDLE;
    let mut flags = LOCKFILE_FAIL_IMMEDIATELY;
    if exclusive {
        flags |= LOCKFILE_EXCLUSIVE_LOCK;
    }
    let mut overlapped = at(offset);
    // SAFETY: `handle` is the open handle of `file`, borrowed for the call; `overlapped` is an
    // initialized `OVERLAPPED` borrowed only for the call. A handle std opens is synchronous, so
    // with `LOCKFILE_FAIL_IMMEDIATELY` the call never returns pending.
    let ok = unsafe { LockFileEx(handle, flags, 0, 1, 0, &mut overlapped) } != 0;
    if ok {
        return Ok(true);
    }
    let err = io::Error::last_os_error();
    if err.raw_os_error() == Some(ERROR_LOCK_VIOLATION as i32) {
        Ok(false)
    } else {
        Err(err)
    }
}

/// Unlocks the byte at `offset`, locked before through the same handle.
pub(crate) fn unlock_byte(file: &File, offset: u64) -> io::Result<()> {
    let handle = file.as_raw_handle() as HANDLE;
    let mut overlapped = at(offset);
    // SAFETY: as in `try_lock_byte`.
    let ok = unsafe { UnlockFileEx(handle, 0, 1, 0, &mut overlapped) } != 0;
    if ok {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// `(volume serial number, file index)` of an open file.
pub(crate) fn file_index(file: &File) -> io::Result<(u32, u64)> {
    let handle = file.as_raw_handle() as HANDLE;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `handle` is the open handle of `file`, borrowed for the call; `info` is an
    // initialized `BY_HANDLE_FILE_INFORMATION` borrowed only for the call.
    let ok = unsafe { GetFileInformationByHandle(handle, &mut info) } != 0;
    if !ok {
        return Err(io::Error::last_os_error());
    }
    let index = (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow);
    Ok((info.dwVolumeSerialNumber, index))
}

/// `FILE_BASIC_INFO` of an open file: its times and attributes.
fn basic_info(file: &File) -> io::Result<FILE_BASIC_INFO> {
    let handle = file.as_raw_handle() as HANDLE;
    let mut info = FILE_BASIC_INFO::default();
    // SAFETY: `handle` is the open handle of `file`, borrowed for the call; `info` is an
    // initialized `FILE_BASIC_INFO` whose exact size is passed, borrowed only for the call.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            handle,
            FileBasicInfo,
            (&raw mut info).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    } != 0;
    if ok {
        Ok(info)
    } else {
        Err(io::Error::last_os_error())
    }
}

/// `ChangeTime` of an open file (100 ns since 1601): it moves on every write, rename or
/// attribute change, and nobody can set it back, unlike the write time.
pub(crate) fn change_time(file: &File) -> io::Result<i64> {
    basic_info(file).map(|info| info.ChangeTime)
}

/// `(CreationTime, FileAttributes)` of an open file, the time in 100 ns since 1601.
pub(crate) fn creation_and_attributes(file: &File) -> io::Result<(i64, u32)> {
    basic_info(file).map(|info| (info.CreationTime, info.FileAttributes))
}

/// Longest file system name read, in UTF-16 units (`MAX_PATH + 1`, what the API documents).
const FS_NAME_UNITS: usize = 261;

/// Name of the file system of the volume an open file lives on (`NTFS`, `ReFS`, `FAT32`…).
pub(crate) fn file_system_name(file: &File) -> io::Result<String> {
    let handle = file.as_raw_handle() as HANDLE;
    let mut name = [0u16; FS_NAME_UNITS];
    // SAFETY: `handle` is the open handle of `file`, borrowed for the call; `name` holds
    // `FS_NAME_UNITS` writable UTF-16 units and that length is passed. Every other buffer is
    // null with size 0, which the API documents as "not requested".
    let ok = unsafe {
        GetVolumeInformationByHandleW(
            handle,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            name.as_mut_ptr(),
            FS_NAME_UNITS as u32,
        )
    } != 0;
    if !ok {
        return Err(io::Error::last_os_error());
    }
    let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    Ok(String::from_utf16_lossy(&name[..len]))
}

/// Marks the file of an open handle (opened with `DELETE` access) to be deleted when the last
/// handle closes: the entry deleted is exactly the one that was read through this handle.
pub(crate) fn delete_on_close(file: &File) -> io::Result<()> {
    let handle = file.as_raw_handle() as HANDLE;
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: `handle` is the open handle of `file`, borrowed for the call; `info` is an
    // initialized `FILE_DISPOSITION_INFO` whose exact size is passed, borrowed only for the call.
    let ok = unsafe {
        SetFileInformationByHandle(
            handle,
            FileDispositionInfo,
            (&raw const info).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } != 0;
    if ok {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

/// UTF-16 with NUL. An absolute drive path gets the `\\?\` prefix, so Windows takes every name
/// as is (no trailing dots or spaces dropped, no device names, no 260-character limit).
fn wide(path: &Path) -> Vec<u16> {
    let raw = OsStr::new(path);
    let bytes = raw.as_encoded_bytes();
    let drive = bytes.len() > 2 && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/');
    let prefix: &[u16] = if drive {
        &[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16]
    } else {
        &[]
    };
    prefix
        .iter()
        .copied()
        .chain(raw.encode_wide().map(|c| {
            if drive && c == u16::from(b'/') {
                u16::from(b'\\')
            } else {
                c
            }
        }))
        .chain(Some(0))
        .collect()
}

/// Renames the entry `from` to `to` on the same volume, flushed before returning. Never replaces:
/// fails with `ERROR_ALREADY_EXISTS` or `ERROR_FILE_EXISTS` if `to` exists. A link or junction at
/// `from` is renamed itself, never followed.
pub(crate) fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    let from = wide(from);
    let to = wide(to);
    // SAFETY: both buffers are NUL-terminated UTF-16 strings alive for the call. Without
    // `MOVEFILE_COPY_ALLOWED` the call is a rename on one volume and never copies.
    let ok = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } != 0;
    if ok {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
