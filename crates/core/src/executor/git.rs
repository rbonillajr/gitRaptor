//! How a step of the executor runs `git` (ADR-CKP-002 § 6): only through the invocation of user
//! operations of `crates/git`, as a marked child of the protected operation, and interruptible
//! like a Ctrl-C to its process group, never killed mid-write.

use std::process::ExitStatus;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use gitraptor_git::user_ops::UserGitCommand;

use crate::timemachine::protected::StepCtx;

/// Why a running operation was interrupted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interrupt {
    /// Cancel from a client with layer `cockpit` (BR-CKP-WF-008).
    Cancelled,
    /// The time limit of layer `mcp` (`time-limit`).
    TimeLimit,
}

/// Shared by the step and whoever may interrupt it.
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicU8>);

impl CancelToken {
    pub fn cancel(&self, why: Interrupt) {
        let code = match why {
            Interrupt::Cancelled => 1,
            Interrupt::TimeLimit => 2,
        };
        let _ = self
            .0
            .compare_exchange(0, code, Ordering::SeqCst, Ordering::SeqCst);
    }

    pub fn reason(&self) -> Option<Interrupt> {
        match self.0.load(Ordering::SeqCst) {
            1 => Some(Interrupt::Cancelled),
            2 => Some(Interrupt::TimeLimit),
            _ => None,
        }
    }
}

/// Sets `token` to [`Interrupt::TimeLimit`] after `limit`, unless dropped first.
pub struct TimeLimit {
    done: Arc<AtomicBool>,
}

impl TimeLimit {
    pub fn start(token: &CancelToken, limit: Duration) -> Self {
        let done = Arc::new(AtomicBool::new(false));
        let (flag, token) = (Arc::clone(&done), token.clone());
        let _ = std::thread::Builder::new()
            .name("raptor-time-limit".into())
            .spawn(move || {
                let deadline = Instant::now() + limit;
                while !flag.load(Ordering::SeqCst) {
                    if Instant::now() >= deadline {
                        token.cancel(Interrupt::TimeLimit);
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            });
        Self { done }
    }
}

impl Drop for TimeLimit {
    fn drop(&mut self) {
        self.done.store(true, Ordering::SeqCst);
    }
}

/// What one `git` left.
#[derive(Debug)]
pub struct GitRun {
    pub status: ExitStatus,
    /// Untrusted, capped.
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub interrupted: Option<Interrupt>,
}

#[cfg(unix)]
fn interrupt_group(pid: u32) {
    if let Some(pid) = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    {
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::INT);
    }
}

/// Pendiente: etapa de validación multiplataforma (console Ctrl-C to the group on Windows).
#[cfg(not(unix))]
fn interrupt_group(_pid: u32) {}

/// Runs one user operation as a marked child of the step, interrupting it when `cancel` is set.
pub fn run_git(
    ctx: &mut StepCtx<'_>,
    cmd: UserGitCommand,
    cancel: &CancelToken,
) -> std::io::Result<GitRun> {
    let mut collector = None;
    let mut marked = ctx.spawn_with(|| {
        let (child, out) = cmd.spawn()?;
        collector = Some(out);
        Ok(child)
    })?;
    let mut signalled = None;
    let status = loop {
        if let Some(status) = marked.child.try_wait()? {
            break status;
        }
        if signalled.is_none()
            && let Some(why) = cancel.reason()
        {
            // The group of `git` is its pid: it was launched with its own group.
            interrupt_group(marked.pid);
            signalled = Some(why);
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    ctx.child_ended(marked.pid);
    let (stdout, stderr) = collector.map(|c| c.collect()).unwrap_or_default();
    Ok(GitRun {
        status,
        stdout,
        stderr,
        interrupted: signalled,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_time_limit_interrupts_with_its_reason() {
        let token = CancelToken::default();
        let limit = TimeLimit::start(&token, Duration::from_millis(30));
        let start = Instant::now();
        while token.reason().is_none() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(token.reason(), Some(Interrupt::TimeLimit));
        // The first reason stays.
        token.cancel(Interrupt::Cancelled);
        assert_eq!(token.reason(), Some(Interrupt::TimeLimit));
        drop(limit);

        let other = CancelToken::default();
        drop(TimeLimit::start(&other, Duration::from_millis(30)));
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!(
            other.reason(),
            None,
            "a finished operation is not interrupted"
        );
    }
}
