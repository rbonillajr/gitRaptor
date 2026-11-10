//! LocalAuthentication FFI. Every `unsafe` block of the LA binding lives here,
//! mirroring how a private `ffi_local_auth` module would sit under
//! `crates/macsys` (ADR-GRP-002 exception, SPIKE-GRD-002 § 4).
#![allow(unsafe_code)]

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_foundation::{NSError, NSString};
use objc2_local_authentication::{LAContext, LAPolicy};

/// What the reply block reports back. Plain data only: nothing Objective-C
/// crosses the channel, so the receiver never touches framework objects.
#[derive(Debug, Clone)]
pub struct Reply {
    pub success: bool,
    pub code: Option<i64>,
    pub domain: Option<String>,
    pub message: Option<String>,
    /// `pthread_main_np()` on the thread that ran the reply block.
    pub on_main_thread: bool,
    pub thread_id: u64,
    pub at: Instant,
}

#[derive(Debug, Clone, Copy)]
pub enum Policy {
    Owner,
    BiometricsOrWatch,
    Biometrics,
}

impl Policy {
    fn la(self) -> LAPolicy {
        match self {
            Policy::Owner => LAPolicy::DeviceOwnerAuthentication,
            Policy::BiometricsOrWatch => {
                LAPolicy::DeviceOwnerAuthenticationWithBiometricsOrCompanion
            }
            Policy::Biometrics => LAPolicy::DeviceOwnerAuthenticationWithBiometrics,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Policy::Owner => "deviceOwnerAuthentication",
            Policy::BiometricsOrWatch => "deviceOwnerAuthenticationWithBiometricsOrWatch",
            Policy::Biometrics => "deviceOwnerAuthenticationWithBiometrics",
        }
    }
}

fn thread_info() -> (bool, u64) {
    let mut tid: u64 = 0;
    // SAFETY: `pthread_main_np` takes no arguments; `pthread_threadid_np` with a
    // null thread means "current thread" and writes into a valid `u64`.
    unsafe {
        libc::pthread_threadid_np(0 as libc::pthread_t, &mut tid);
        (libc::pthread_main_np() == 1, tid)
    }
}

fn error_parts(err: &NSError) -> (i64, String, String) {
    (
        err.code() as i64,
        err.domain().to_string(),
        err.localizedDescription().to_string(),
    )
}

/// A fresh context per request, reuse duration forced to 0 (F3, H6).
pub struct Context {
    ctx: Retained<LAContext>,
}

#[derive(Debug, Clone)]
pub struct Preflight {
    pub ok: bool,
    pub code: Option<i64>,
    pub message: Option<String>,
    pub biometry_type: i64,
    pub reuse_duration: f64,
}

impl Context {
    pub fn new() -> Self {
        // SAFETY: `+[LAContext new]` has no preconditions.
        let ctx = unsafe { LAContext::new() };
        // SAFETY: plain property setter on a live context.
        unsafe { ctx.setTouchIDAuthenticationAllowableReuseDuration(0.0) };
        Context { ctx }
    }

    pub fn preflight(&self, policy: Policy) -> Preflight {
        // SAFETY: `canEvaluatePolicy:error:` on a live context; the error out-param
        // is handled by the binding (Result).
        let res = unsafe { self.ctx.canEvaluatePolicy_error(policy.la()) };
        // SAFETY: plain property getters, valid after canEvaluatePolicy.
        let (bt, reuse) = unsafe {
            (
                self.ctx.biometryType().0 as i64,
                self.ctx.touchIDAuthenticationAllowableReuseDuration(),
            )
        };
        match res {
            Ok(()) => Preflight {
                ok: true,
                code: None,
                message: None,
                biometry_type: bt,
                reuse_duration: reuse,
            },
            Err(e) => {
                let (c, _, m) = error_parts(&e);
                Preflight {
                    ok: false,
                    code: Some(c),
                    message: Some(m),
                    biometry_type: bt,
                    reuse_duration: reuse,
                }
            }
        }
    }

    /// Starts the evaluation; the reply arrives on the returned channel from a
    /// framework-private queue. The block owns only a `Sender` (Send + Sync).
    pub fn start(&self, policy: Policy, reason: &str) -> Receiver<Reply> {
        let (tx, rx): (Sender<Reply>, Receiver<Reply>) = channel();
        let block = RcBlock::new(move |ok: Bool, err: *mut NSError| {
            let (on_main_thread, thread_id) = thread_info();
            // SAFETY: the framework passes either null or a valid NSError that it
            // keeps alive for the duration of the block call.
            let err = unsafe { err.as_ref() };
            let (code, domain, message) = match err {
                Some(e) => {
                    let (c, d, m) = error_parts(e);
                    (Some(c), Some(d), Some(m))
                }
                None => (None, None, None),
            };
            // A late reply after the receiver is gone just fails to send: no UB.
            let _ = tx.send(Reply {
                success: ok.as_bool(),
                code,
                domain,
                message,
                on_main_thread,
                thread_id,
                at: Instant::now(),
            });
        });
        let reason = NSString::from_str(reason);
        // SAFETY: the block only captures a `Sender`, which is Send + Sync, so it is
        // "sendable" as the method requires. The framework copies (retains) the
        // block, so dropping our `RcBlock` after the call is fine. `reason` is a
        // non-empty NSString (callers validate) to avoid NSInvalidArgumentException.
        unsafe {
            self.ctx
                .evaluatePolicy_localizedReason_reply(policy.la(), &reason, &block)
        };
        rx
    }

    /// Cancels any pending evaluation; the pending reply fails with appCancel (-9).
    pub fn invalidate(&self) {
        // SAFETY: `invalidate` is idempotent on a live context.
        unsafe { self.ctx.invalidate() };
    }

    /// Opaque domain state after an evaluation (F4/H7): only a hash of the
    /// enrolled biometry, never the method used.
    pub fn domain_state_len(&self) -> Option<usize> {
        // SAFETY: plain property getter.
        #[allow(deprecated)]
        let d = unsafe { self.ctx.evaluatedPolicyDomainState() };
        d.map(|d| d.len())
    }
}

/// Waits for a reply up to `timeout`.
pub fn wait(rx: &Receiver<Reply>, timeout: Duration) -> Option<Reply> {
    rx.recv_timeout(timeout).ok()
}
