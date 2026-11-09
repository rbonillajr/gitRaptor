//! Shell commands the app offers the user to paste: quoting and the `raptor` they run.

use std::path::{Path, PathBuf};

use super::wire::{ConfirmCommandDto, NotOfferedReason};

/// `args` as one POSIX command line with each argument single-quoted; `None` when an argument
/// could act on the terminal it is pasted in.
pub fn posix(_args: &[&str]) -> Option<String> {
    None
}

/// The `raptor` the app may name in a command: the daemon's executable unless it hangs from a
/// watched path or is not an installed `raptor`, else the bundled one.
pub fn raptor_path(
    _daemon_exe: Option<&Path>,
    _bundled: Option<&Path>,
    _watched: &[&Path],
) -> Option<PathBuf> {
    None
}

/// The command that confirms the base of `repo`, when `raptor` is known.
pub fn confirm(_raptor: Option<&Path>, _repo: &str) -> ConfirmCommandDto {
    ConfirmCommandDto::NotOffered {
        reason: NotOfferedReason::RaptorUnknown,
    }
}
