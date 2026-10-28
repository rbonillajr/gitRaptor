//! Exception tokens (US-GRD-006): issued per repo, presented by the git child, consumed once.
//!
//! The store keeps only the SHA-256 of each token, compared in constant time. A token is valid
//! for [`TOKEN_TTL`] until its first presentation; after that only the git child it was bound to
//! reuses it, for the transitions it was bound to, until `end` or [`CONSUMED_MAX`].

use std::collections::HashMap;
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
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
/// Debug builds only: another token TTL for the end-to-end suites.
pub const TTL_ENV: &str = "GITRAPTOR_TEST_GUARD_EXCEPTION_TTL_MS";
/// Debug builds only: another child wait for the end-to-end suites.
pub const CHILD_WAIT_ENV: &str = "GITRAPTOR_TEST_GUARD_EXCEPTION_CHILD_WAIT_MS";

fn test_override(name: &str) -> Option<Duration> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_millis)
}

/// The token TTL of this daemon.
pub fn token_ttl() -> Duration {
    test_override(TTL_ENV).unwrap_or(TOKEN_TTL)
}

/// The child wait of this daemon.
pub fn child_wait() -> Duration {
    test_override(CHILD_WAIT_ENV).unwrap_or(CHILD_WAIT)
}

/// Milliseconds on the monotonic clock of this process, the one every token time is read on:
/// a jump of the wall clock neither expires nor extends a token. Never compared with the wall
/// clock of the pending actions (`pending::Entry::applies_at_ms`).
pub fn monotonic_ms() -> i64 {
    static START: OnceLock<Instant> = OnceLock::new();
    let start = *START.get_or_init(Instant::now);
    i64::try_from(start.elapsed().as_millis()).unwrap_or(i64::MAX)
}

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
    /// Whether the bound child already presented, and had accepted, the bound rebase: only
    /// then does a local movement of the rebased ref count as covered.
    rebase_matched: bool,
}

impl Slot {
    fn stale(&self, now_ms: i64, ttl: Duration) -> bool {
        match self.consumed_ms {
            None => now_ms.saturating_sub(self.issued_ms) > ttl.as_millis() as i64,
            Some(at) => now_ms.saturating_sub(at) > CONSUMED_MAX.as_millis() as i64,
        }
    }

    /// Whether `presented` is one of the bound transitions: the same ref movement, or the same
    /// ref rebased onto the same base.
    fn covers(&self, presented: &Transition) -> bool {
        if self.plan.binding.transitions.contains(presented) {
            return true;
        }
        // The rebased branch moves in its `reference-transaction`: the same ref the bound
        // rebase names, locally, and only once this same git presented that rebase with its
        // base at `pre-rebase` and it was accepted.
        if !self.rebase_matched {
            return false;
        }
        let Transition::Ref {
            remote: None,
            refname,
            ..
        } = presented
        else {
            return false;
        };
        self.plan
            .binding
            .transitions
            .iter()
            .any(|t| matches!(t, Transition::Rebase { refname: bound, .. } if bound == refname))
    }
}

/// The tokens by repo id: one pending exception per repo.
pub struct Tokens {
    slots: Mutex<HashMap<String, Slot>>,
    /// Signalled when a child is bound or a token ends.
    bound: Condvar,
    child_wait: Duration,
    ttl: Duration,
}

/// The daemon's store: the child wait and the TTL of this build (debug builds read the overrides).
impl Default for Tokens {
    fn default() -> Self {
        Self::with_limits(child_wait(), token_ttl())
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
        Self::with_limits(child_wait, TOKEN_TTL)
    }

    /// Tokens valid for `ttl` until their first presentation.
    pub fn with_limits(child_wait: Duration, ttl: Duration) -> Self {
        Self {
            slots: Mutex::default(),
            bound: Condvar::new(),
            child_wait,
            ttl,
        }
    }

    /// The TTL of the tokens this store issues.
    pub fn ttl(&self) -> Duration {
        self.ttl
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
            rebase_matched: false,
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
        let started = Instant::now();
        let deadline = started + self.child_wait;
        let mut slots = self.lock();
        loop {
            // The clock advances while waiting for the child: the TTL is checked against now,
            // not against when the presentation arrived.
            let elapsed = i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
            let now_ms = now_ms.saturating_add(elapsed);
            let Some(slot) = slots
                .get_mut(p.repo_id)
                .filter(|s| ct_eq(&s.digest, &wanted))
            else {
                return Presented::Rejected(Rejection::Unknown);
            };
            if slot.stale(now_ms, self.ttl) {
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
            if p.transitions
                .iter()
                .any(|t| matches!(t, Transition::Rebase { .. }))
            {
                slot.rebase_matched = true;
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
        let ttl = self.ttl;
        self.lock().retain(|_, slot| {
            let stale = slot.stale(now_ms, ttl);
            if stale {
                dropped.push(slot.action_id.clone());
            }
            !stale
        });
        dropped
    }

    /// The plan the token of `action_id` was issued for.
    pub fn plan_of(&self, action_id: &str) -> Option<ExceptionPlan> {
        self.lock()
            .values()
            .find(|s| s.action_id == action_id)
            .map(|s| s.plan.clone())
    }

    /// The plan of the token whose digest is `token`'s in `repo_id`, whatever its state: what a
    /// rejected presentation of a known token is logged with.
    pub fn plan_for_token(
        &self,
        repo_id: &str,
        token: &ExceptionToken,
    ) -> Option<(String, ExceptionPlan)> {
        let wanted = digest(token.expose());
        self.lock()
            .get(repo_id)
            .filter(|s| ct_eq(&s.digest, &wanted))
            .map(|s| (s.action_id.clone(), s.plan.clone()))
    }

    /// Forgets the token of `action_id` without ending it: its `used` audit could not be
    /// written, so the exception does not apply, now or on a later presentation.
    pub fn revoke(&self, action_id: &str) {
        let mut slots = self.lock();
        let repo = Self::by_action(&mut slots, action_id).map(|(repo, _)| repo.clone());
        if let Some(repo) = repo {
            slots.remove(&repo);
        }
        drop(slots);
        self.bound.notify_all();
    }

    /// The digest of the token of `action_id`.
    pub fn digest_of(&self, action_id: &str) -> Option<[u8; 32]> {
        self.lock()
            .values()
            .find(|s| s.action_id == action_id)
            .map(|s| s.digest)
    }
}

#[cfg(test)]
mod tests {
    use gitraptor_api::guard::LoggedOperation;

    use super::*;

    const REPO: &str = "repo";
    const WT: &str = "/wt";
    const PARENT: Requester = Requester {
        pid: 10,
        start_us: 1,
    };
    const CHILD: (u32, u64) = (11, 2);

    fn rebase() -> Transition {
        Transition::Rebase {
            refname: "refs/heads/feat".to_owned(),
            base: "b".repeat(40),
        }
    }

    fn local_move() -> Transition {
        Transition::Ref {
            remote: None,
            refname: "refs/heads/feat".to_owned(),
            old: Some("a".repeat(40)),
            new: Some("c".repeat(40)),
        }
    }

    fn issued() -> (Tokens, ExceptionToken) {
        let tokens = Tokens::with_limits(Duration::ZERO, TOKEN_TTL);
        let plan = ExceptionPlan {
            worktree: WT.to_owned(),
            binding: Binding {
                operation: LoggedOperation::Rebase {
                    upstream: None,
                    branch: None,
                },
                transitions: vec![rebase()],
            },
            rules: Vec::new(),
        };
        let bound = Bound {
            action_id: "act".to_owned(),
            repo_id: REPO.to_owned(),
            requester: PARENT,
            plan,
        };
        let token = tokens.issue(bound, 0).expect("the OS gives randomness");
        tokens
            .bind_child("act", PARENT, CHILD)
            .expect("the child binds");
        (tokens, token)
    }

    fn present(tokens: &Tokens, token: &ExceptionToken, transitions: &[Transition]) -> Presented {
        tokens.present(
            &Presentation {
                repo_id: REPO,
                worktree: WT,
                token,
                git: CHILD,
                git_parent: PARENT,
                transitions,
                reasons: &[],
            },
            1,
        )
    }

    #[test]
    fn a_rebased_ref_moves_only_after_its_rebase_was_accepted() {
        let (tokens, token) = issued();
        assert_eq!(
            present(&tokens, &token, &[local_move()]),
            Presented::Rejected(Rejection::OtherTransition),
            "no rebase was presented by this git yet"
        );
        assert!(matches!(
            present(&tokens, &token, &[rebase()]),
            Presented::Applied { first: true, .. }
        ));
        assert!(matches!(
            present(&tokens, &token, &[local_move()]),
            Presented::Applied { first: false, .. }
        ));
    }

    #[test]
    fn a_rejected_rebase_does_not_open_the_rebased_ref() {
        let (tokens, token) = issued();
        let other = Transition::Rebase {
            refname: "refs/heads/feat".to_owned(),
            base: "d".repeat(40),
        };
        assert_eq!(
            present(&tokens, &token, &[other]),
            Presented::Rejected(Rejection::OtherTransition)
        );
        assert_eq!(
            present(&tokens, &token, &[local_move()]),
            Presented::Rejected(Rejection::OtherTransition)
        );
    }
}
