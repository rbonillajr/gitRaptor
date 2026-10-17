//! The client side of the local transport, and what the daemon's side shares with it
//! (ADR-GRP-005 § 5, SEC-01, L-06): the socket's path, the check of a private folder and
//! the pipe's name. One implementation: `crates/core` binds with these and does not keep a
//! copy of them.
//!
//! Unix: a socket in the profile's runtime folder. Before sending anything, the client
//! checks that the folder is this user's and private (0700, not a symbolic link) and that
//! the server runs as this same user. A path longer than the `sun_path` limit is reached
//! relative to the runtime folder, changing the working folder under a process-wide mutex.
//!
//! Windows: a named pipe per user and profile (`gitraptor-<SID>-<hash of the runtime
//! folder>`); the client checks that its server runs as this same user (DS-TS-GRP-004 § 8).
//! The Win32 calls live in `gitraptor-winsys`.

use std::io;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::Mutex;

use crate::SOCKET_FILE;

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

/// Why a folder is not private.
#[derive(Debug)]
pub enum PrivateDirError {
    Io(io::Error),
    /// It exists but must not be trusted: the reason, for the message.
    Insecure(String),
}

impl From<io::Error> for PrivateDirError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl std::fmt::Display for PrivateDirError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Insecure(reason) => f.write_str(reason),
        }
    }
}

/// Fails unless `path` is a real folder owned by the current user with mode
/// exactly 0700 (Unix), or owned by and accessible only to the user, SYSTEM
/// and Administrators (Windows, counting what children would inherit).
pub fn verify_private_dir(path: &Path) -> Result<(), PrivateDirError> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(PrivateDirError::Insecure("is a symbolic link".into()));
    }
    if !meta.is_dir() {
        return Err(PrivateDirError::Insecure("is not a directory".into()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let euid = rustix::process::geteuid().as_raw();
        if meta.uid() != euid {
            return Err(PrivateDirError::Insecure(format!(
                "owned by uid {}, expected {euid}",
                meta.uid()
            )));
        }
        let mode = meta.mode() & 0o777;
        if mode != 0o700 {
            return Err(PrivateDirError::Insecure(format!(
                "mode is {mode:o}, expected 700"
            )));
        }
    }
    #[cfg(windows)]
    gitraptor_winsys::acl::verify_private_dir(path)
        .map_err(|e| PrivateDirError::Insecure(e.to_string()))?;
    Ok(())
}

/// Runs `op` with the working folder in `dir`, restoring it afterwards.
/// Process-wide: no other thread may rely on relative paths meanwhile.
#[cfg(unix)]
pub fn in_dir<T>(dir: &Path, op: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    let _guard = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::env::current_dir()?;
    std::env::set_current_dir(dir)?;
    // The folder we landed in is the private one, not a swapped symlink.
    let verified = verify_private_dir(Path::new("."))
        .map_err(|err| io::Error::new(io::ErrorKind::PermissionDenied, err.to_string()));
    let result = verified.and_then(|()| op());
    std::env::set_current_dir(previous)?;
    result
}

/// The connected stream of the channel.
#[cfg(unix)]
pub type Stream = std::os::unix::net::UnixStream;
/// The connected stream of the channel.
#[cfg(windows)]
pub type Stream = gitraptor_winsys::pipe::PipeStream;

/// Connects to the channel socket in `runtime`, and checks that its
/// folder is private and that the server runs as this same user before
/// sending anything. Both refusals are `PermissionDenied`.
#[cfg(unix)]
pub fn connect(runtime: &Path) -> io::Result<Stream> {
    connect_checked(runtime, rustix::process::geteuid().as_raw())
}

/// [`connect`], with the uid the server must run as given instead of this process's
/// effective uid. The check is the same; tests use it to play a server of another user,
/// which an unprivileged test cannot create. Only with the `test-support` feature.
#[cfg(all(unix, feature = "test-support"))]
pub fn connect_expecting(runtime: &Path, server_uid: u32) -> io::Result<Stream> {
    connect_checked(runtime, server_uid)
}

/// The L-06 checks of [`connect`], against the uid the server must run as.
#[cfg(unix)]
fn connect_checked(runtime: &Path, server_uid: u32) -> io::Result<Stream> {
    use std::os::unix::net::UnixStream;
    // L-06: the socket's folder must be this user's and private (0700,
    // not a symbolic link) before anything is sent; a missing folder
    // means no daemon.
    std::fs::symlink_metadata(runtime)?;
    verify_private_dir(runtime).map_err(|err| {
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
        let server = super::peer::peer_cred(&stream)?;
        if server.uid != server_uid {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "the channel server runs as another user",
            ));
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let _ = server_uid;
    Ok(stream)
}

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
pub fn pipe_name(sid: &str, runtime: &Path) -> String {
    let folder = runtime.to_string_lossy().to_lowercase();
    format!(r"\\.\pipe\gitraptor-{sid}-{:016x}", fingerprint(&folder))
}

#[cfg(windows)]
mod windows {
    use super::*;
    use gitraptor_winsys::pipe::PipeStream;
    use gitraptor_winsys::process::{self, Owner};
    use std::time::Duration;

    /// How long a client waits for a free instance while all are busy.
    const BUSY_WAIT: Duration = Duration::from_secs(2);

    /// This user's SID, as text.
    pub fn user_sid() -> io::Result<String> {
        Ok(gitraptor_winsys::acl::current_user_sid()?.to_string())
    }

    /// The pipe name of the channel of `runtime` for this user.
    pub fn pipe_path(runtime: &Path) -> io::Result<String> {
        Ok(pipe_name(&user_sid()?, runtime))
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
pub use windows::{connect, pipe_path, runs_as_this_user, user_sid};

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
