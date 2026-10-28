//! How a step of the executor runs `git` (ADR-CKP-002 § 6): only through the invocation of user
//! operations of `crates/git`, as a marked child of the protected operation, and interruptible
//! like a Ctrl-C to its process group, never killed mid-write.

use std::process::ExitStatus;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::{Duration, Instant};

use gitraptor_git::user_ops::{OutputCollector, UserGitCommand};

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
    /// Untrusted, capped; partial when a descendant kept the pipe open past [`COLLECT_GRACE`].
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub interrupted: Option<Interrupt>,
    /// Descendants of the interrupted `git` (its hooks) outlived it and were terminated or killed:
    /// one of them may have died mid-write, so the step cannot claim the repo is unchanged.
    pub forced: bool,
}

/// How often an interrupted `git` that has not ended gets Ctrl-C again.
const INTERRUPT_REPEAT: Duration = Duration::from_secs(1);

/// How long the descendants of an interrupted `git` get after it ended, per signal, to let go
/// of its pipes before the next one: Ctrl-C again, then a terminate, then a kill.
#[cfg(unix)]
const LEFTOVER_GRACE: Duration = Duration::from_secs(3);

/// How long the output is waited for once `git` ended and its group was dealt with: a hook's
/// background job (or a descendant that left the group) can hold the pipes open indefinitely.
const COLLECT_GRACE: Duration = Duration::from_secs(3);

#[cfg(unix)]
fn signal_group(pid: u32, signal: rustix::process::Signal) {
    if let Some(pid) = i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    {
        let _ = rustix::process::kill_process_group(pid, signal);
    }
}

#[cfg(unix)]
fn interrupt_group(pid: u32) {
    signal_group(pid, rustix::process::Signal::INT);
}

/// Pendiente: etapa de validación multiplataforma (console Ctrl-C to the group on Windows).
#[cfg(not(unix))]
fn interrupt_group(_pid: u32) {}

/// Whether `git` ended, without reaping it: while it is a zombie its pid, which is also its
/// group's id, cannot be reused, so the group can still be signalled safely.
#[cfg(unix)]
fn has_ended(child: &mut std::process::Child) -> std::io::Result<bool> {
    use rustix::process::{Pid, WaitId, WaitIdOptions, waitid};
    let pid = i32::try_from(child.id())
        .ok()
        .and_then(Pid::from_raw)
        .ok_or_else(|| std::io::Error::other("invalid pid"))?;
    let options = WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT;
    loop {
        match waitid(WaitId::Pid(pid), options) {
            Ok(status) => return Ok(status.is_some()),
            Err(rustix::io::Errno::INTR) => {}
            Err(e) => return Err(e.into()),
        }
    }
}

#[cfg(not(unix))]
fn has_ended(child: &mut std::process::Child) -> std::io::Result<bool> {
    Ok(child.try_wait()?.is_some())
}

/// After an interrupted `git` ended: what is left of its group are its hooks' descendants, which
/// may hold its pipes (a shell can survive a single Ctrl-C). They get Ctrl-C again, then a
/// terminate, then a kill, each after [`LEFTOVER_GRACE`] with the pipes still open. `git` itself
/// is never signalled past its interrupt (ADR-CKP-002 § 6). `true` when it took a terminate or a
/// kill.
#[cfg(unix)]
fn end_leftovers(pid: u32, out: Option<&OutputCollector>) -> bool {
    use rustix::process::Signal;
    let Some(out) = out else { return false };
    let mut forced = false;
    for signal in [Signal::INT, Signal::TERM, Signal::KILL] {
        // Ctrl-C again even with the pipes closed: a member that let go of them gets it too.
        if signal != Signal::INT && out.is_closed() {
            break;
        }
        signal_group(pid, signal);
        forced |= signal != Signal::INT;
        let deadline = Instant::now() + LEFTOVER_GRACE;
        while !out.is_closed() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    forced
}

/// Pendiente: etapa de validación multiplataforma (Job Object on Windows): the leftovers are
/// not signalled; the bounded collect still keeps the step from waiting for them.
#[cfg(not(unix))]
fn end_leftovers(_pid: u32, _out: Option<&OutputCollector>) -> bool {
    false
}

/// Runs one user operation as a marked child of the step, interrupting it when `cancel` is set.
pub fn run_git(
    ctx: &mut StepCtx<'_>,
    cmd: UserGitCommand,
    cancel: &CancelToken,
) -> std::io::Result<GitRun> {
    run_git_with(ctx, cmd, cancel, &[])
}

/// [`run_git`] for a `git` that also makes `transitions` of its own besides the plan's (the
/// abort of an atomic rebase moving the branch back): registered with its identity.
pub fn run_git_with(
    ctx: &mut StepCtx<'_>,
    cmd: UserGitCommand,
    cancel: &CancelToken,
    transitions: &[gitraptor_policy::guard::plan::Expected],
) -> std::io::Result<GitRun> {
    let mut collector = None;
    let mut marked = ctx.spawn_with_transitions(
        || {
            let (child, out) = cmd.spawn()?;
            collector = Some(out);
            Ok(child)
        },
        transitions,
    )?;
    let mut signalled = None;
    let mut next_interrupt = Instant::now();
    while !has_ended(&mut marked.child)? {
        if let Some(why) = signalled.or_else(|| cancel.reason())
            && Instant::now() >= next_interrupt
        {
            // The group of `git` is its pid: it was launched with its own group. Again while
            // `git` lives, as a user would press Ctrl-C again.
            interrupt_group(marked.pid);
            signalled = Some(why);
            next_interrupt = Instant::now() + INTERRUPT_REPEAT;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let forced = signalled.is_some() && end_leftovers(marked.pid, collector.as_ref());
    let status = marked.child.wait()?;
    ctx.child_ended(marked.pid);
    let (stdout, stderr) = collector
        .map(|c| c.collect(COLLECT_GRACE))
        .unwrap_or_default();
    Ok(GitRun {
        status,
        stdout,
        stderr,
        interrupted: signalled,
        forced,
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
