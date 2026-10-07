//! The command line of another process of the user (DS-US-GRD-018 § 5.3): read only to classify
//! a Git subcommand, never stored.

use std::ffi::OsString;

/// Most arguments returned; a longer command line is unreadable.
pub const MAX_ARGS: usize = 4096;

/// The arguments (`argv`, program name first) of `pid`, or `None` when they cannot be read.
/// macOS only; elsewhere always `None`.
pub fn process_args(pid: u32) -> Option<Vec<OsString>> {
    #[cfg(target_os = "macos")]
    {
        parse_procargs2(&crate::ffi_procargs::procargs2(pid)?)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = pid;
        None
    }
}

/// Parses a `KERN_PROCARGS2` area: a native-endian `int` argc, the executable path, NUL padding,
/// then `argc` NUL-terminated strings. The environment that follows is never read. Anything
/// malformed is `None`.
pub fn parse_procargs2(area: &[u8]) -> Option<Vec<OsString>> {
    let (count, rest) = area.split_first_chunk::<4>()?;
    let argc = usize::try_from(i32::from_ne_bytes(*count)).ok()?;
    if argc == 0 || argc > MAX_ARGS {
        return None;
    }
    // The executable path, then its NUL padding.
    let end = rest.iter().position(|b| *b == 0)?;
    let mut rest = &rest[end..];
    while let [0, tail @ ..] = rest {
        rest = tail;
    }
    let mut out = Vec::with_capacity(argc);
    for _ in 0..argc {
        let end = rest.iter().position(|b| *b == 0)?;
        out.push(os(&rest[..end]));
        rest = &rest[end + 1..];
    }
    Some(out)
}

#[cfg(unix)]
fn os(bytes: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::OsStr::from_bytes(bytes).to_owned()
}

#[cfg(not(unix))]
fn os(bytes: &[u8]) -> OsString {
    String::from_utf8_lossy(bytes).into_owned().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(argc: i32, body: &[u8]) -> Vec<u8> {
        let mut v = argc.to_ne_bytes().to_vec();
        v.extend_from_slice(body);
        v
    }

    #[test]
    fn reads_argv_and_never_the_environment() {
        let a = area(
            3,
            b"/usr/bin/git\0\0\0\0git\0commit\0--no-verify\0SECRET=1\0",
        );
        let args = parse_procargs2(&a).unwrap();
        assert_eq!(args, ["git", "commit", "--no-verify"]);
    }

    #[test]
    fn malformed_areas_are_unreadable() {
        assert_eq!(parse_procargs2(&[]), None);
        assert_eq!(parse_procargs2(&area(0, b"/x\0x\0")), None);
        assert_eq!(parse_procargs2(&area(-1, b"/x\0x\0")), None);
        // Fewer strings than argc.
        assert_eq!(parse_procargs2(&area(3, b"/x\0\0git\0commit")), None);
        // No terminated executable path.
        assert_eq!(parse_procargs2(&area(1, b"/x")), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reads_this_process() {
        let args = process_args(std::process::id()).unwrap();
        let me: Vec<OsString> = std::env::args_os().collect();
        assert_eq!(args, me);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_gone_pid_is_unreadable() {
        assert_eq!(process_args(u32::MAX), None);
        assert_eq!(process_args(0x7fff_fff0), None);
    }
}
