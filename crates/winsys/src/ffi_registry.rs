//! String values of `HKEY_CURRENT_USER`, one call each: no key handle is
//! ever opened, so there is nothing to close.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
};

/// `text` as a NUL-terminated UTF-16 string.
fn wide(text: &OsStr) -> Vec<u16> {
    text.encode_wide().chain(std::iter::once(0)).collect()
}

fn error(code: u32) -> io::Error {
    io::Error::from_raw_os_error(i32::try_from(code).unwrap_or(i32::MAX))
}

pub(crate) fn get(subkey: &Path, name: &str) -> io::Result<Option<String>> {
    let subkey = wide(subkey.as_os_str());
    let name = wide(OsStr::new(name));
    let mut buf: Vec<u16> = vec![0; 260];
    loop {
        let mut bytes = u32::try_from(buf.len() * 2).unwrap_or(u32::MAX);
        // SAFETY: both strings are NUL-terminated and outlive the call;
        // `buf` holds `bytes` writable bytes and `bytes` is a valid u32.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                subkey.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        match status {
            ERROR_SUCCESS => {
                let len = (bytes as usize / 2).min(buf.len());
                let text = &buf[..len];
                let text = text.strip_suffix(&[0]).unwrap_or(text);
                return Ok(Some(String::from_utf16_lossy(text)));
            }
            ERROR_FILE_NOT_FOUND => return Ok(None),
            ERROR_MORE_DATA => buf.resize(bytes as usize / 2 + 1, 0),
            other => return Err(error(other)),
        }
    }
}

pub(crate) fn set(subkey: &Path, name: &str, value: &str) -> io::Result<()> {
    let subkey = wide(subkey.as_os_str());
    let name = wide(OsStr::new(name));
    let data = wide(OsStr::new(value));
    let bytes = u32::try_from(data.len() * 2)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "value too long"))?;
    // SAFETY: the strings are NUL-terminated and outlive the call; `data`
    // holds exactly `bytes` readable bytes, terminator included (REG_SZ).
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            name.as_ptr(),
            REG_SZ,
            data.as_ptr().cast(),
            bytes,
        )
    };
    match status {
        ERROR_SUCCESS => Ok(()),
        other => Err(error(other)),
    }
}

pub(crate) fn delete(subkey: &Path, name: &str) -> io::Result<bool> {
    let subkey = wide(subkey.as_os_str());
    let name = wide(OsStr::new(name));
    // SAFETY: both strings are NUL-terminated and outlive the call.
    let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, subkey.as_ptr(), name.as_ptr()) };
    match status {
        ERROR_SUCCESS => Ok(true),
        ERROR_FILE_NOT_FOUND => Ok(false),
        other => Err(error(other)),
    }
}
