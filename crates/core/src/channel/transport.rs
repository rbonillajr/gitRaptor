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

#[cfg(unix)]
use std::io;
use std::path::Path;

#[cfg(unix)]
use gitraptor_api::SOCKET_FILE;

#[cfg(unix)]
use crate::profile::{ProfileError, fsperm};

// The client side and what both sides share live once, in the client library
// (INF-CKP-001 Entrega 2b): the socket's path, the private-folder check behind
// L-06, the pipe's name and the server checks of `connect`.
#[cfg(unix)]
use gitraptor_api::client::transport::in_dir;
pub use gitraptor_api::client::transport::{MAX_SOCKET_PATH, Stream, connect, socket_path};
#[cfg(windows)]
pub use gitraptor_api::client::transport::{pipe_path, runs_as_this_user};

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    use std::os::unix::net::UnixListener;

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
}

#[cfg(unix)]
pub use unix::bind;

#[cfg(windows)]
mod windows {
    use super::*;
    use gitraptor_winsys::pipe::PipeListener;
    use std::path::PathBuf;

    use crate::profile::ProfileError;

    /// Creates the channel pipe: owner and only entry of a protected DACL,
    /// the user's SID; first instance, so a name already taken (by anyone)
    /// fails closed; at most `max_instances` instances.
    pub fn bind(runtime: &Path, max_instances: u32) -> Result<PipeListener, ProfileError> {
        let sid = gitraptor_api::client::transport::user_sid()?;
        let name = gitraptor_api::client::transport::pipe_name(&sid, runtime);
        let sddl = format!("O:{sid}D:P(A;;GA;;;{sid})");
        PipeListener::bind(&name, &sddl, max_instances).map_err(|err| ProfileError::InsecureDir {
            path: PathBuf::from(&name),
            reason: format!("the channel pipe cannot be created as its first instance: {err}"),
        })
    }
}

#[cfg(windows)]
pub use windows::bind;
