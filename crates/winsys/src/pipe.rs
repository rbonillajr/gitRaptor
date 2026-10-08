//! Named pipes for the local channel (ADR-GRP-005 § 5, DS-TS-GRP-004 § 8).
//!
//! The server creates the first instance with `FILE_FLAG_FIRST_PIPE_INSTANCE`, so a name taken
//! by anyone else fails closed, and always keeps one free instance waiting, so the name is never
//! free while it serves. Every instance refuses remote clients and carries the descriptor given
//! at bind (the channel passes a protected DACL with only the user's SID). The client opens with
//! SQOS identification, so the server can identify it but never impersonate it.
//!
//! Reads and writes are overlapped with deadlines (`set_read_timeout`, `set_write_timeout`,
//! `TimedOut` when they pass) and `shutdown` wakes a blocked read or write, so a
//! [`PipeStream`] behaves like the `UnixStream` the channel uses on Unix. Clones share the
//! pipe, its deadlines and its shutdown state.

use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::acl::{Ace, Sid, parse_acl};
use crate::ffi_acl::{Descriptor, handle_security};
use crate::ffi_handle::Handle;
use crate::ffi_pipe::{self, Event, Outcome};

/// How long `accept` waits before trying again when the instance cap is reached.
const RETRY_AT_CAP: Duration = Duration::from_millis(50);

/// Consecutive failed connections after which `accept` gives up.
const MAX_ACCEPT_FAILURES: u32 = 16;

/// The server side of a named pipe.
pub struct PipeListener {
    name: Vec<u16>,
    descriptor: Descriptor,
    max_instances: u32,
    /// The free instance a client connects to next.
    pending: Mutex<Option<Handle>>,
    stop: Arc<Event>,
}

/// Wakes a [`PipeListener::accept`] for good: every later call returns `None` too.
#[derive(Clone)]
pub struct PipeWaker(Arc<Event>);

impl PipeWaker {
    pub fn wake(&self) {
        self.0.set();
    }
}

impl PipeListener {
    /// Creates the first instance of `name` (`\\.\pipe\…`) with the security descriptor `sddl`.
    /// Fails if any instance of that name already exists, whoever created it. At most
    /// `max_instances` instances (1 to 255) exist at once.
    pub fn bind(name: &str, sddl: &str, max_instances: u32) -> io::Result<Self> {
        let name = ffi_pipe::wide(name);
        let descriptor = Descriptor::from_sddl(sddl)?;
        let max_instances = max_instances.clamp(1, 255);
        let first = ffi_pipe::create_instance(&name, &descriptor, true, max_instances)?;
        Ok(Self {
            name,
            descriptor,
            max_instances,
            pending: Mutex::new(Some(first)),
            stop: Arc::new(Event::new()?),
        })
    }

    pub fn waker(&self) -> PipeWaker {
        PipeWaker(Arc::clone(&self.stop))
    }

    /// Owner and DACL entries of the pipe, read from the free instance. `None` entries: a NULL
    /// DACL (access for everyone).
    pub fn security(&self) -> io::Result<(Sid, Option<Vec<Ace>>)> {
        let pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let instance = pending
            .as_ref()
            .ok_or_else(|| io::Error::other("no free instance"))?;
        let (owner, dacl) = handle_security(instance)?;
        let owner = Sid::from_bytes(&owner).ok_or_else(|| io::Error::other("invalid owner"))?;
        let dacl = dacl
            .map(|bytes| parse_acl(&bytes))
            .transpose()
            .map_err(|_| io::Error::other("invalid DACL"))?;
        Ok((owner, dacl))
    }

    fn new_instance(&self) -> io::Result<Handle> {
        ffi_pipe::create_instance(&self.name, &self.descriptor, false, self.max_instances)
    }

    /// Waits for the next client. `None` once woken by a [`PipeWaker`]. The next free instance
    /// is created before the connected one is handed over, so the name is never free.
    pub fn accept(&self) -> io::Result<Option<PipeStream>> {
        let mut failures = 0;
        loop {
            let taken = self
                .pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            let instance = match taken {
                Some(instance) => instance,
                None => match self.new_instance() {
                    Ok(instance) => instance,
                    // Every instance is in use: wait for one to close, or for the waker.
                    Err(err) if ffi_pipe::is_busy(&err) => {
                        if self.stop.wait(RETRY_AT_CAP) {
                            return Ok(None);
                        }
                        continue;
                    }
                    Err(err) => return Err(err),
                },
            };
            match ffi_pipe::accept(&instance, &self.stop) {
                Ok(Outcome::Done(_)) => {
                    let next = self.new_instance().ok();
                    *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = next;
                    return Ok(Some(PipeStream::new(instance, Side::Server)?));
                }
                Ok(Outcome::Stopped | Outcome::TimedOut) => {
                    *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(instance);
                    return Ok(None);
                }
                // A client that came and went before the connection completed: this instance
                // is spent; a fresh one is created.
                Err(err) => {
                    failures += 1;
                    if failures >= MAX_ACCEPT_FAILURES {
                        return Err(err);
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Server,
    Client,
}

struct Inner {
    handle: Handle,
    side: Side,
    read_stop: Event,
    write_stop: Event,
    read_shut: AtomicBool,
    write_shut: AtomicBool,
    read_timeout: Mutex<Option<Duration>>,
    write_timeout: Mutex<Option<Duration>>,
}

/// One connected end of a named pipe.
#[derive(Clone)]
pub struct PipeStream {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for PipeStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PipeStream")
            .field("side", &self.inner.side)
            .finish_non_exhaustive()
    }
}

impl PipeStream {
    fn new(handle: Handle, side: Side) -> io::Result<Self> {
        Ok(Self {
            inner: Arc::new(Inner {
                handle,
                side,
                read_stop: Event::new()?,
                write_stop: Event::new()?,
                read_shut: AtomicBool::new(false),
                write_shut: AtomicBool::new(false),
                read_timeout: Mutex::new(None),
                write_timeout: Mutex::new(None),
            }),
        })
    }

    /// Connects to `name` with SQOS identification. While every instance is busy, waits up to
    /// `busy_wait` for a free one. A missing pipe is `NotFound`.
    pub fn connect(name: &str, busy_wait: Duration) -> io::Result<Self> {
        let wide = ffi_pipe::wide(name);
        let deadline = Instant::now() + busy_wait;
        loop {
            match ffi_pipe::open_client(&wide) {
                Ok(handle) => return Self::new(handle, Side::Client),
                Err(err) if ffi_pipe::is_busy(&err) => {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return Err(err);
                    }
                    ffi_pipe::wait_free(&wide, left);
                }
                Err(err) => return Err(err),
            }
        }
    }

    /// The pid of the process at the other end, as the kernel reports it: the client of a
    /// server end, the server of a client end.
    pub fn peer_pid(&self) -> io::Result<u32> {
        match self.inner.side {
            Side::Server => ffi_pipe::client_pid(&self.inner.handle),
            Side::Client => ffi_pipe::server_pid(&self.inner.handle),
        }
    }

    pub fn try_clone(&self) -> io::Result<Self> {
        Ok(self.clone())
    }

    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        *self
            .inner
            .read_timeout
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = timeout;
        Ok(())
    }

    pub fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        *self
            .inner
            .write_timeout
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = timeout;
        Ok(())
    }

    /// Ends reading (a blocked read returns end of stream), writing (a blocked write fails) or
    /// both, for every clone. The pipe closes when the last clone is dropped.
    pub fn shutdown(&self, how: Shutdown) -> io::Result<()> {
        if matches!(how, Shutdown::Read | Shutdown::Both) {
            self.inner.read_shut.store(true, Ordering::SeqCst);
            self.inner.read_stop.set();
        }
        if matches!(how, Shutdown::Write | Shutdown::Both) {
            self.inner.write_shut.store(true, Ordering::SeqCst);
            self.inner.write_stop.set();
        }
        Ok(())
    }

    fn read_timeout(&self) -> Option<Duration> {
        *self
            .inner
            .read_timeout
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn write_timeout(&self) -> Option<Duration> {
        *self
            .inner
            .write_timeout
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }
}

impl Read for &PipeStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.inner.read_shut.load(Ordering::SeqCst) {
            return Ok(0);
        }
        let timeout = self.read_timeout();
        match ffi_pipe::read(&self.inner.handle, buf, &self.inner.read_stop, timeout) {
            Ok(Outcome::Done(n)) => Ok(n as usize),
            Ok(Outcome::Stopped) => Ok(0),
            Ok(Outcome::TimedOut) => Err(io::ErrorKind::TimedOut.into()),
            Err(err) if ffi_pipe::is_closed(&err) => Ok(0),
            Err(err) => Err(err),
        }
    }
}

impl Read for PipeStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        (&*self).read(buf)
    }
}

impl Write for &PipeStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.inner.write_shut.load(Ordering::SeqCst) {
            return Err(io::ErrorKind::BrokenPipe.into());
        }
        if buf.is_empty() {
            return Ok(0);
        }
        let timeout = self.write_timeout();
        match ffi_pipe::write(&self.inner.handle, buf, &self.inner.write_stop, timeout) {
            Ok(Outcome::Done(n)) => Ok(n as usize),
            Ok(Outcome::Stopped) => Err(io::ErrorKind::BrokenPipe.into()),
            Ok(Outcome::TimedOut) => Err(io::ErrorKind::TimedOut.into()),
            Err(err) if ffi_pipe::is_closed(&err) => Err(io::ErrorKind::BrokenPipe.into()),
            Err(err) => Err(err),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Write for PipeStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        (&*self).write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acl::{AceKind, current_user_sid};

    fn unique_name(tag: &str) -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!(
            r"\\.\pipe\gitraptor-test-{tag}-{}-{nanos}",
            std::process::id()
        )
    }

    fn private_sddl() -> String {
        let user = current_user_sid().unwrap();
        format!("O:{user}D:P(A;;GA;;;{user})")
    }

    fn serve(name: &str) -> (Arc<PipeListener>, std::thread::JoinHandle<PipeStream>) {
        let listener = Arc::new(PipeListener::bind(name, &private_sddl(), 4).unwrap());
        let l = Arc::clone(&listener);
        let server = std::thread::spawn(move || l.accept().unwrap().unwrap());
        (listener, server)
    }

    #[test]
    fn the_pipe_dacl_names_only_the_user() {
        let name = unique_name("dacl");
        let listener = PipeListener::bind(&name, &private_sddl(), 2).unwrap();
        let (owner, dacl) = listener.security().unwrap();
        let user = current_user_sid().unwrap();
        assert_eq!(owner, user);
        let dacl = dacl.expect("a DACL, never a NULL one");
        assert_eq!(dacl.len(), 1, "{dacl:?}");
        assert_eq!(dacl[0].kind, AceKind::Allow);
        assert_eq!(dacl[0].sid.as_ref(), Some(&user));
    }

    #[test]
    fn a_taken_name_fails_closed() {
        let name = unique_name("squat");
        // Someone else's pipe, open to everyone, created first.
        let _squatter = PipeListener::bind(&name, "D:(A;;GA;;;WD)", 2).unwrap();
        assert!(PipeListener::bind(&name, &private_sddl(), 2).is_err());
    }

    #[test]
    fn round_trip_and_peer_pids() {
        let name = unique_name("rt");
        let (_listener, server) = serve(&name);
        let client = PipeStream::connect(&name, Duration::from_secs(2)).unwrap();
        let server = server.join().unwrap();
        assert_eq!(client.peer_pid().unwrap(), std::process::id());
        assert_eq!(server.peer_pid().unwrap(), std::process::id());
        (&client).write_all(b"hello\n").unwrap();
        let mut buf = [0u8; 6];
        (&server).read_exact(&mut buf).unwrap();
        assert_eq!(&buf, b"hello\n");
        (&server).write_all(b"bye").unwrap();
        drop(server);
        let mut rest = Vec::new();
        (&client).read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"bye", "data written before the close is still read");
    }

    #[test]
    fn a_read_times_out() {
        let name = unique_name("timeout");
        let (_listener, server) = serve(&name);
        let _client = PipeStream::connect(&name, Duration::from_secs(2)).unwrap();
        let server = server.join().unwrap();
        server
            .set_read_timeout(Some(Duration::from_millis(50)))
            .unwrap();
        let err = (&server).read(&mut [0u8; 8]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn shutdown_wakes_a_blocked_read() {
        let name = unique_name("shut");
        let (_listener, server) = serve(&name);
        let _client = PipeStream::connect(&name, Duration::from_secs(2)).unwrap();
        let server = server.join().unwrap();
        let reader = server.try_clone().unwrap();
        let blocked = std::thread::spawn(move || (&reader).read(&mut [0u8; 8]).unwrap());
        server.shutdown(Shutdown::Read).unwrap();
        assert_eq!(blocked.join().unwrap(), 0);
    }

    #[test]
    fn the_waker_ends_accept() {
        let name = unique_name("wake");
        let listener = Arc::new(PipeListener::bind(&name, &private_sddl(), 2).unwrap());
        let waker = listener.waker();
        let l = Arc::clone(&listener);
        let accepting = std::thread::spawn(move || l.accept().unwrap().is_none());
        waker.wake();
        assert!(accepting.join().unwrap());
    }

    #[test]
    fn a_missing_pipe_is_not_found() {
        let err = PipeStream::connect(&unique_name("none"), Duration::ZERO).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn instances_are_capped() {
        let name = unique_name("cap");
        let listener = Arc::new(PipeListener::bind(&name, &private_sddl(), 2).unwrap());
        let l = Arc::clone(&listener);
        let first = std::thread::spawn(move || l.accept().unwrap().unwrap());
        let c1 = PipeStream::connect(&name, Duration::from_secs(2)).unwrap();
        let _s1 = first.join().unwrap();
        // The free instance is the second and last one.
        let c2 = PipeStream::connect(&name, Duration::from_secs(2)).unwrap();
        // No third instance exists, so a third client finds the pipe busy.
        let err = PipeStream::connect(&name, Duration::from_millis(50)).unwrap_err();
        assert!(ffi_pipe::is_busy(&err), "{err:?}");
        drop((c1, c2));
    }
}
