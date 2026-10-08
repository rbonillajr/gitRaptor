//! The local transport: a Unix socket in the profile's runtime folder
//! (ADR-GRP-005 § 5, SEC-01). No network listener exists (NFR-03).
//!
//! The folder is created or verified 0700 and owned by the user; the socket
//! is created under umask 077, so no other user has access at any moment,
//! and ends 0600.
//! A path longer than the `sun_path` limit is reached relative to the
//! runtime folder, changing the working folder under a process-wide mutex.
//!
//! Windows: a named pipe per user and profile (`gitraptor-<SID>-<hash of the
//! runtime folder>`) with a protected DACL that grants only the user's SID,
//! created as the first instance (a taken name fails closed), refusing
//! remote clients; the client opens it with SQOS identification and checks
//! that the server runs as this same user (DS-TS-GRP-004 § 8). The Win32
//! calls live in `gitraptor-winsys`.

use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::{io, sync::Mutex};

use gitraptor_api::SOCKET_FILE;

#[cfg(unix)]
use crate::profile::{ProfileError, fsperm};

/// Longest socket path used as is. `sun_path` holds 104 bytes on macOS and
/// 108 on Linux, including the terminating NUL.
pub const MAX_SOCKET_PATH: usize = 100;

/// Serializes the working-folder changes of long-path connects and binds.
#[cfg(unix)]
static CWD_LOCK: Mutex<()> = Mutex::new(());

/// Full path of the socket in `runtime`.
pub fn socket_path(runtime: &Path) -> PathBuf {
    runtime.join(SOCKET_FILE)
}

/// Runs `op` with the working folder in `dir`, restoring it afterwards.
/// Process-wide: no other thread may rely on relative paths meanwhile.
#[cfg(unix)]
fn in_dir<T>(dir: &Path, op: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::env::current_dir()?;
    std::env::set_current_dir(dir)?;
    // The folder we landed in is the private one, not a swapped symlink.
    let verified = fsperm::verify_private_dir(Path::new("."))
        .map_err(|err| io::Error::new(io::ErrorKind::PermissionDenied, err.to_string()));
    let result = verified.and_then(|()| op());
    std::env::set_current_dir(previous)?;
    result
}

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};

    /// Binds the channel socket. The caller holds the instance lock, so a
    /// leftover socket file is stale: it is removed only if it is a socket
    /// of this user.
    pub fn bind(runtime: &Path) -> Result<UnixListener, ProfileError> {
        fsperm::set_restrictive_umask();
        fsperm::ensure_private_dir(runtime)?;
        let path = socket_path(runtime);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) => {
                let euid = rustix::process::geteuid().as_raw();
                if !meta.file_type().is_socket() || meta.uid() != euid {
                    return Err(ProfileError::InsecureDir {
                        path: path.clone(),
                        reason: "is not a socket of the current user".into(),
                    });
                }
                std::fs::remove_file(&path)?;
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
        let listener = if path.as_os_str().len() > MAX_SOCKET_PATH {
            in_dir(runtime, || UnixListener::bind(SOCKET_FILE))?
        } else {
            UnixListener::bind(&path)?
        };
        // Under umask 077 the socket is born without any group or other
        // bit (macOS gives sockets 0777 & !umask = 0700), so no other user
        // ever had access. Dropping the owner's execute bit to reach 0600
        // only narrows it further.
        let mode = std::fs::symlink_metadata(&path)?.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(ProfileError::InsecureDir {
                path,
                reason: format!("socket mode is {mode:o}, expected 600"),
            });
        }
        if mode != 0o600 {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(listener)
    }

    /// Connects to the channel socket in `runtime`, and checks that its
    /// folder is private and that the server runs as this same user before
    /// sending anything. Both refusals are `PermissionDenied`.
    pub fn connect(runtime: &Path) -> io::Result<UnixStream> {
        // L-06: the socket's folder must be this user's and private (0700,
        // not a symbolic link) before anything is sent; a missing folder
        // means no daemon.
        std::fs::symlink_metadata(runtime)?;
        crate::profile::fsperm::verify_private_dir(runtime).map_err(|err| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("channel rejected: {err}"),
            )
        })?;
        let path = socket_path(runtime);
        let stream = if path.as_os_str().len() > MAX_SOCKET_PATH {
            in_dir(runtime, || UnixStream::connect(SOCKET_FILE))?
        } else {
            UnixStream::connect(&path)?
        };
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        {
            let server = super::super::peer::peer_cred(&stream)?;
            if server.uid != rustix::process::geteuid().as_raw() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "the channel server runs as another user",
                ));
            }
        }
        Ok(stream)
    }
}

#[cfg(unix)]
pub use unix::{bind, connect};

/// The connected stream of the channel.
#[cfg(unix)]
pub type Stream = std::os::unix::net::UnixStream;
/// The connected stream of the channel.
#[cfg(windows)]
pub type Stream = gitraptor_winsys::pipe::PipeStream;

/// FNV-1a, 64 bits: a stable fingerprint of the runtime folder for the pipe
/// name. Not a secret: the DACL and the checks guard the pipe.
#[cfg_attr(not(windows), allow(dead_code))]
fn fingerprint(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The pipe name of the channel of `runtime` for the user `sid`: one per
/// user and per profile, like the socket on Unix. The folder is compared
/// without case, as Windows paths are.
#[cfg_attr(not(windows), allow(dead_code))]
fn pipe_name(sid: &str, runtime: &Path) -> String {
    let folder = runtime.to_string_lossy().to_lowercase();
    format!(r"\\.\pipe\gitraptor-{sid}-{:016x}", fingerprint(&folder))
}

#[cfg(windows)]
mod windows {
    use super::*;
    use gitraptor_winsys::pipe::{PipeListener, PipeStream};
    use gitraptor_winsys::process::{self, Owner};
    use std::io;
    use std::time::Duration;

    use crate::profile::ProfileError;

    /// How long a client waits for a free instance while all are busy.
    const BUSY_WAIT: Duration = Duration::from_secs(2);

    fn user_sid() -> io::Result<String> {
        Ok(gitraptor_winsys::acl::current_user_sid()?.to_string())
    }

    /// The pipe name of the channel of `runtime` for this user.
    pub fn pipe_path(runtime: &Path) -> io::Result<String> {
        Ok(pipe_name(&user_sid()?, runtime))
    }

    /// Creates the channel pipe: owner and only entry of a protected DACL,
    /// the user's SID; first instance, so a name already taken (by anyone)
    /// fails closed; at most `max_instances` instances.
    pub fn bind(runtime: &Path, max_instances: u32) -> Result<PipeListener, ProfileError> {
        let sid = user_sid()?;
        let name = pipe_name(&sid, runtime);
        let sddl = format!("O:{sid}D:P(A;;GA;;;{sid})");
        PipeListener::bind(&name, &sddl, max_instances).map_err(|err| ProfileError::InsecureDir {
            path: PathBuf::from(&name),
            reason: format!("the channel pipe cannot be created as its first instance: {err}"),
        })
    }

    /// Whether `pid` runs as this user. An unreadable owner is a refusal.
    pub fn runs_as_this_user(pid: u32) -> bool {
        matches!(process::owner_of(pid), Ok(Owner::Current))
    }

    /// Connects to the channel pipe of `runtime` and checks, before sending
    /// anything, that its server runs as this same user. A refusal, or an
    /// owner that cannot be read, is `PermissionDenied`.
    pub fn connect(runtime: &Path) -> io::Result<PipeStream> {
        let stream = PipeStream::connect(&pipe_path(runtime)?, BUSY_WAIT)?;
        let server = stream.peer_pid()?;
        if !runs_as_this_user(server) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the channel server runs as another user",
            ));
        }
        Ok(stream)
    }
}

#[cfg(windows)]
pub use windows::{bind, connect, pipe_path, runs_as_this_user};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pipe_name_is_per_user_and_per_profile() {
        let a = pipe_name(
            "S-1-5-21-1-2-3-1001",
            Path::new(r"C:\Users\a\AppData\gitraptor\run"),
        );
        assert!(
            a.starts_with(r"\\.\pipe\gitraptor-S-1-5-21-1-2-3-1001-"),
            "{a}"
        );
        assert_eq!(
            a,
            pipe_name(
                "S-1-5-21-1-2-3-1001",
                Path::new(r"c:\users\A\appdata\GitRaptor\RUN")
            )
        );
        assert_ne!(
            a,
            pipe_name(
                "S-1-5-21-1-2-3-1002",
                Path::new(r"C:\Users\a\AppData\gitraptor\run")
            )
        );
        assert_ne!(
            a,
            pipe_name("S-1-5-21-1-2-3-1001", Path::new(r"C:\Users\a\other\run"))
        );
        assert!(a.len() < 256);
    }
}
