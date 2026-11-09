//! Shell commands the app offers the user to paste: quoting and the `raptor` they run.

use std::path::{Path, PathBuf};

use gitraptor_api::client::installed::is_installed_raptor;
use gitraptor_api::display::must_escape;

use super::wire::{ConfirmCommandDto, NotOfferedReason};

/// `args` as one POSIX command line with each argument single-quoted; `None` when an argument
/// could act on the terminal it is pasted in.
pub fn posix(args: &[&str]) -> Option<String> {
    if args.iter().any(|arg| arg.chars().any(must_escape)) {
        return None;
    }
    let quoted: Vec<String> = args
        .iter()
        .map(|arg| format!("'{}'", arg.replace('\'', "'\\''")))
        .collect();
    Some(quoted.join(" "))
}

/// The `raptor` the app may name in a command: the daemon's executable unless it hangs from a
/// watched path or is not an installed `raptor`, else the bundled one. Never `PATH`.
///
/// A path that cannot be canonicalized is not trusted (fails closed).
pub fn raptor_path(
    daemon_exe: Option<&Path>,
    bundled: Option<&Path>,
    watched: &[&Path],
) -> Option<PathBuf> {
    let daemon = daemon_exe.and_then(|exe| {
        let canonical = exe.canonicalize().ok()?;
        let inside = watched.iter().any(|root| {
            root.canonicalize().map_or_else(
                |_| exe.starts_with(root),
                |root| canonical.starts_with(root),
            )
        });
        (!inside && is_installed_raptor(&canonical)).then_some(canonical)
    });
    daemon.or_else(|| bundled.map(Path::to_path_buf))
}

/// The command that confirms the base of `repo`, when `raptor` is known. A repo path that cannot
/// be pasted safely leaves the command without it.
pub fn confirm(raptor: Option<&Path>, repo: &str) -> ConfirmCommandDto {
    let Some(raptor) = raptor.and_then(Path::to_str) else {
        return ConfirmCommandDto::NotOffered {
            reason: NotOfferedReason::RaptorUnknown,
        };
    };
    let base = ["guard", "confirm"];
    let with = |extra: &[&str]| {
        let mut args = vec![raptor];
        args.extend(base);
        args.extend(extra);
        posix(&args)
    };
    if let Some(command) = with(&[repo]) {
        return ConfirmCommandDto::Offered {
            command,
            with_path: true,
        };
    }
    match with(&[]) {
        Some(command) => ConfirmCommandDto::Offered {
            command,
            with_path: false,
        },
        None => ConfirmCommandDto::NotOffered {
            reason: NotOfferedReason::RaptorUnknown,
        },
    }
}
