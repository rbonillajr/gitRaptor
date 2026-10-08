//! Who is on the other end of a channel connection, as the kernel reports it (ADR-GRP-005
//! § 5, L-06). Shared by the client, which checks the server before sending anything, and
//! by the daemon, which reads its clients; the process details of the daemon side (ancestry,
//! start times) stay in `crates/core`.

/// Credentials of the peer of a connected socket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCred {
    pub uid: u32,
    pub pid: u32,
}

/// The uid of processes of another user on Windows, which has SIDs instead.
pub const FOREIGN_UID: u32 = u32::MAX;

/// Effective uid of this process. Windows: 0, the stand-in for "this user".
pub fn current_uid() -> u32 {
    #[cfg(unix)]
    {
        rustix::process::geteuid().as_raw()
    }
    #[cfg(not(unix))]
    {
        0
    }
}

/// macOS: `LOCAL_PEERCRED` and `LOCAL_PEERPID`, cross-checked with the audit token.
#[cfg(target_os = "macos")]
pub fn peer_cred(stream: &std::os::unix::net::UnixStream) -> std::io::Result<PeerCred> {
    use nix::sys::socket::{getsockopt, sockopt};
    let cred = getsockopt(stream, sockopt::LocalPeerCred).map_err(std::io::Error::from)?;
    let pid = getsockopt(stream, sockopt::LocalPeerPid).map_err(std::io::Error::from)?;
    let pid = u32::try_from(pid).map_err(|_| std::io::Error::other("invalid peer pid"))?;
    // The audit token carries the same pid and euid; a mismatch means the
    // kernel views disagree, so nothing about the peer is trusted.
    let token = getsockopt(stream, sockopt::LocalPeerToken).map_err(std::io::Error::from)?;
    if token.val[5] != pid || token.val[1] != cred.uid() {
        return Err(std::io::Error::other(
            "peer token disagrees with credentials",
        ));
    }
    Ok(PeerCred {
        uid: cred.uid(),
        pid,
    })
}

/// Linux: `SO_PEERCRED`. Pendiente: etapa de validación multiplataforma (pidfd via
/// `SO_PEERPIDFD` is the stronger identifier).
#[cfg(target_os = "linux")]
pub fn peer_cred(stream: &std::os::unix::net::UnixStream) -> std::io::Result<PeerCred> {
    let cred = rustix::net::sockopt::socket_peercred(stream)?;
    Ok(PeerCred {
        uid: cred.uid.as_raw(),
        pid: cred.pid.as_raw_nonzero().get().unsigned_abs(),
    })
}

/// The process at the other end of a channel pipe: its pid as the kernel
/// reports it, and whether its token is this user's (W4). An owner that
/// cannot be read is an error, never "this user" (fail-closed).
#[cfg(windows)]
pub fn peer_cred(stream: &gitraptor_winsys::pipe::PipeStream) -> std::io::Result<PeerCred> {
    use gitraptor_winsys::process::{self, Error, Owner};
    let pid = stream.peer_pid()?;
    let uid = match process::owner_of(pid) {
        Ok(Owner::Current) => current_uid(),
        Ok(Owner::Other) | Err(Error::Denied) => FOREIGN_UID,
        Ok(Owner::Unknown) | Err(Error::Gone | Error::Unavailable) => {
            return Err(std::io::Error::other("the peer's owner cannot be read"));
        }
    };
    Ok(PeerCred { uid, pid })
}
