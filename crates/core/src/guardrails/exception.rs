//! Exception tokens (US-GRD-006): issued per repo, presented by the git child, consumed once.
//!
//! The store keeps only the SHA-256 of each token, compared in constant time. A token is valid
//! for [`TOKEN_TTL`] until its first presentation; after that only the git child it was bound to
//! reuses it, for the transitions it was bound to, until `end` or [`CONSUMED_MAX`].

use std::collections::HashMap;
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use gitraptor_api::guard::ExceptionPlanView;
pub use gitraptor_api::guard::ExceptionToken;
use gitraptor_api::guard::{ExceptionRefusal, Rule};
use gitraptor_policy::guard::exception::{Binding, Transition, covers};

use super::pending::Requester;

/// How long an issued token is valid.
pub const TOKEN_TTL: Duration = Duration::from_secs(60);
/// How long `present` waits for the git child to be bound.
pub const CHILD_WAIT: Duration = Duration::from_secs(2);
/// How long a consumed token is remembered.
pub const CONSUMED_MAX: Duration = Duration::from_secs(600);

/// What an exception covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExceptionPlan {
    pub worktree: String,
    pub binding: Binding,
    pub rules: Vec<Rule>,
}

impl ExceptionPlan {
    /// The wire view of this plan.
    pub fn view(&self) -> ExceptionPlanView {
        ExceptionPlanView {
            operation: self.binding.operation.clone(),
            rules: self.rules.clone(),
        }
    }
}

/// An announced exception ready to be issued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bound {
    pub action_id: String,
    pub repo_id: String,
    pub requester: Requester,
    pub plan: ExceptionPlan,
}

/// What a git child presents when it evaluates.
#[derive(Debug, Clone)]
pub struct Presentation<'a> {
    pub repo_id: &'a str,
    pub worktree: &'a str,
    pub token: &'a ExceptionToken,
    /// The git child: pid and start time.
    pub git: (u32, u64),
    pub git_parent: Requester,
    pub transitions: &'a [Transition],
    pub reasons: &'a [Rule],
}

/// The outcome of a presentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presented {
    Applied { action_id: String, first: bool },
    Rejected(Rejection),
}

/// Why a presentation was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    Unknown,
    Expired,
    OtherWorktree,
    ChildNotBound,
    NotTheChild,
    OtherTransition,
    RuleNotAnnounced,
}

/// One repo's issued token: its digest, never the token itself.
#[derive(Debug)]
struct Slot {
    digest: [u8; 32],
    action_id: String,
    requester: Requester,
    plan: ExceptionPlan,
    issued_ms: i64,
    /// The git child the requester started: pid and start time.
    child: Option<(u32, u64)>,
    /// When the first valid presentation consumed it.
    consumed_ms: Option<i64>,
}

impl Slot {
    fn stale(&self, now_ms: i64) -> bool {
        match self.consumed_ms {
            None => now_ms > self.issued_ms + TOKEN_TTL.as_millis() as i64,
            Some(at) => now_ms > at + CONSUMED_MAX.as_millis() as i64,
        }
    }

    /// Whether `presented` is one of the bound transitions: the same ref movement, or the same
    /// ref rebased onto the same base.
    fn covers(&self, presented: &Transition) -> bool {
        self.plan
            .binding
            .transitions
            .iter()
            .any(|bound| match (bound, presented) {
                (
                    Transition::Rebase { refname, base },
                    Transition::Rebase {
                        refname: other_ref,
                        base: other_base,
                    },
                ) => refname == other_ref && base == other_base,
                (Transition::Ref { .. }, Transition::Ref { .. }) => bound == presented,
                _ => false,
            })
    }
}

/// The tokens by repo id: one pending exception per repo.
pub struct Tokens {
    slots: Mutex<HashMap<String, Slot>>,
    /// Signalled when a child is bound or a token ends.
    bound: Condvar,
    child_wait: Duration,
}

impl Default for Tokens {
    fn default() -> Self {
        Self::new(CHILD_WAIT)
    }
}

impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens")
            .field("issued", &self.lock().len())
            .finish_non_exhaustive()
    }
}

/// SHA-256 of `token`.
pub fn digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

/// Equality of two digests in constant time: every byte is read, with no early exit.
pub fn ct_eq(a: &[u8; 32], b: &[u8; 32]) -> bool {
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

impl Tokens {
    /// Tokens whose `present` waits up to `child_wait` for the child.
    pub fn new(child_wait: Duration) -> Self {
        Self {
            slots: Mutex::default(),
            bound: Condvar::new(),
            child_wait,
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Slot>> {
        self.slots.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn by_action<'a>(
        slots: &'a mut HashMap<String, Slot>,
        action_id: &str,
    ) -> Option<(&'a String, &'a mut Slot)> {
        slots.iter_mut().find(|(_, s)| s.action_id == action_id)
    }

    /// Issues a token for `bound`, replacing any other of its repo. `None` when the OS has no
    /// randomness to give: without a CSPRNG there is no token.
    pub fn issue(&self, bound: Bound, now_ms: i64) -> Option<ExceptionToken> {
        let mut secret = [0u8; 16];
        getrandom::fill(&mut secret).ok()?;
        let hex: String = secret.iter().map(|b| format!("{b:02x}")).collect();
        let token = ExceptionToken::from_secret(hex)?;
        let slot = Slot {
            digest: digest(token.expose()),
            action_id: bound.action_id,
            requester: bound.requester,
            plan: bound.plan,
            issued_ms: now_ms,
            child: None,
            consumed_ms: None,
        };
        self.lock().insert(bound.repo_id, slot);
        self.bound.notify_all();
        Some(token)
    }

    /// Binds the git `child` of the exception `action_id` to its `requester`.
    ///
    /// # Errors
    ///
    /// `no-pending` for an unknown action, `not-the-requester` from another process and
    /// `child-not-ours` when a child is already bound.
    pub fn bind_child(
        &self,
        action_id: &str,
        requester: Requester,
        child: (u32, u64),
    ) -> Result<(), ExceptionRefusal> {
        let mut slots = self.lock();
        let (_, slot) =
            Self::by_action(&mut slots, action_id).ok_or(ExceptionRefusal::NoPending)?;
        if slot.requester != requester {
            return Err(ExceptionRefusal::NotTheRequester);
        }
        if slot.child.is_some() {
            return Err(ExceptionRefusal::ChildNotOurs);
        }
        slot.child = Some(child);
        drop(slots);
        self.bound.notify_all();
        Ok(())
    }

    /// Checks a presentation and consumes the token on the first success. Waits on the
    /// connection thread, at most the child wait, for the child to be bound.
    pub fn present(&self, p: &Presentation<'_>, now_ms: i64) -> Presented {
        let wanted = digest(p.token.expose());
        let deadline = Instant::now() + self.child_wait;
        let mut slots = self.lock();
        loop {
            let Some(slot) = slots
                .get_mut(p.repo_id)
                .filter(|s| ct_eq(&s.digest, &wanted))
            else {
                return Presented::Rejected(Rejection::Unknown);
            };
            if slot.stale(now_ms) {
                return Presented::Rejected(Rejection::Expired);
            }
            if slot.plan.worktree != p.worktree {
                return Presented::Rejected(Rejection::OtherWorktree);
            }
            let Some(child) = slot.child else {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Presented::Rejected(Rejection::ChildNotBound);
                }
                slots = self
                    .bound
                    .wait_timeout(slots, left)
                    .unwrap_or_else(PoisonError::into_inner)
                    .0;
                continue;
            };
            if p.git != child || p.git_parent != slot.requester {
                return Presented::Rejected(Rejection::NotTheChild);
            }
            if p.transitions.is_empty() || !p.transitions.iter().all(|t| slot.covers(t)) {
                return Presented::Rejected(Rejection::OtherTransition);
            }
            if !covers(&slot.plan.rules, p.reasons) {
                return Presented::Rejected(Rejection::RuleNotAnnounced);
            }
            let first = slot.consumed_ms.is_none();
            if first {
                slot.consumed_ms = Some(now_ms);
            }
            return Presented::Applied {
                action_id: slot.action_id.clone(),
                first,
            };
        }
    }

    /// Ends the exception `action_id` and forgets its token; whether it had been consumed.
    ///
    /// # Errors
    ///
    /// `no-pending` for an unknown action and `not-the-requester` from another process.
    pub fn end(&self, action_id: &str, requester: Requester) -> Result<bool, ExceptionRefusal> {
        let mut slots = self.lock();
        let (repo, slot) =
            Self::by_action(&mut slots, action_id).ok_or(ExceptionRefusal::NoPending)?;
        if slot.requester != requester {
            return Err(ExceptionRefusal::NotTheRequester);
        }
        let consumed = slot.consumed_ms.is_some();
        let repo = repo.clone();
        slots.remove(&repo);
        drop(slots);
        self.bound.notify_all();
        Ok(consumed)
    }

    /// Drops the tokens never presented within their TTL and those consumed more than
    /// [`CONSUMED_MAX`] ago; the action ids dropped.
    pub fn sweep(&self, now_ms: i64) -> Vec<String> {
        let mut dropped = Vec::new();
        self.lock().retain(|_, slot| {
            let stale = slot.stale(now_ms);
            if stale {
                dropped.push(slot.action_id.clone());
            }
            !stale
        });
        dropped
    }

    /// The digest of the token of `action_id`.
    pub fn digest_of(&self, action_id: &str) -> Option<[u8; 32]> {
        self.lock()
            .values()
            .find(|s| s.action_id == action_id)
            .map(|s| s.digest)
    }
}
