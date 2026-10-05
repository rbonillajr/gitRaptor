//! The one-use confirmation challenge (ADR-TMC-005 § 3, SEC-TMC-03).
//!
//! The daemon issues it only to a caller that passes the confirmation
//! checks, bound to the connection, the caller's `(pid, start)` and the hash
//! of the plan the daemon computed and showed. It expires after 60 s, each
//! connection holds at most one, and it is consumed by the first redeem,
//! successful or not. On redeem the checks run again: a caller that became
//! ineligible in between is refused.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use gitraptor_api::messages::RefusalReason;
use sha2::{Digest, Sha256};

/// Lifetime of a challenge.
pub const CHALLENGE_TTL: Duration = Duration::from_secs(60);
/// Spent tokens remembered to report a reuse.
const SPENT_MEMORY: usize = 256;

/// SHA-256 of the daemon's own plan, serialized canonically by the caller.
pub fn plan_hash(canonical_plan: &[u8]) -> [u8; 32] {
    Sha256::digest(canonical_plan).into()
}

/// What a challenge is bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub connection: u64,
    pub pid: u32,
    pub start_us: u64,
    pub plan_hash: [u8; 32],
}

/// Why a challenge was not issued or not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeError {
    /// The caller may not confirm.
    NotEligible(RefusalReason),
    /// No live challenge on this connection matches.
    Unknown,
    /// Already redeemed.
    Reused,
    /// Issued to another connection.
    OtherConnection,
    /// Issued to another process.
    OtherProcess,
    /// The plan changed since it was issued.
    PlanChanged,
    Expired,
    /// No randomness available (fail-closed).
    NoRandomness,
}

#[derive(Debug, Clone)]
struct Live {
    token: [u8; 16],
    binding: Binding,
    issued: Instant,
}

#[derive(Debug, Default)]
struct Book {
    /// At most one per connection.
    live: HashMap<u64, Live>,
    spent: VecDeque<[u8; 16]>,
}

/// The challenges of the daemon.
#[derive(Debug, Default)]
pub struct ChallengeBook {
    book: Mutex<Book>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<[u8; 16]> {
    if text.len() != 32 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

impl ChallengeBook {
    fn lock(&self) -> std::sync::MutexGuard<'_, Book> {
        self.book.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Issues a challenge: `refusal` is the result of the confirmation
    /// checks run now. Replaces any live challenge of the connection.
    pub fn issue(
        &self,
        binding: Binding,
        refusal: Option<RefusalReason>,
        now: Instant,
    ) -> Result<String, ChallengeError> {
        if let Some(reason) = refusal {
            return Err(ChallengeError::NotEligible(reason));
        }
        let mut token = [0u8; 16];
        getrandom::fill(&mut token).map_err(|_| ChallengeError::NoRandomness)?;
        self.lock().live.insert(
            binding.connection,
            Live {
                token,
                binding,
                issued: now,
            },
        );
        Ok(hex(&token))
    }

    /// Redeems a challenge. `refusal` is the result of the confirmation
    /// checks run again now. Any matching challenge is consumed, accepted
    /// or not.
    pub fn redeem(
        &self,
        binding: Binding,
        token: &str,
        refusal: Option<RefusalReason>,
        now: Instant,
    ) -> Result<(), ChallengeError> {
        let token = unhex(&token.to_ascii_lowercase()).ok_or(ChallengeError::Unknown)?;
        let mut book = self.lock();
        if book.spent.contains(&token) {
            return Err(ChallengeError::Reused);
        }
        // Issued to another connection: consume it there too.
        let owner = book
            .live
            .iter()
            .find(|(_, l)| l.token == token)
            .map(|(c, _)| *c);
        let Some(owner) = owner else {
            return Err(ChallengeError::Unknown);
        };
        let live = book.live.remove(&owner).expect("found above");
        book.spent.push_back(token);
        if book.spent.len() > SPENT_MEMORY {
            book.spent.pop_front();
        }
        drop(book);
        if owner != binding.connection {
            return Err(ChallengeError::OtherConnection);
        }
        if (live.binding.pid, live.binding.start_us) != (binding.pid, binding.start_us) {
            return Err(ChallengeError::OtherProcess);
        }
        if now.saturating_duration_since(live.issued) > CHALLENGE_TTL {
            return Err(ChallengeError::Expired);
        }
        if live.binding.plan_hash != binding.plan_hash {
            return Err(ChallengeError::PlanChanged);
        }
        if let Some(reason) = refusal {
            return Err(ChallengeError::NotEligible(reason));
        }
        Ok(())
    }

    /// Drops the challenge of a closed connection.
    pub fn forget(&self, connection: u64) {
        self.lock().live.remove(&connection);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(connection: u64, plan: &[u8]) -> Binding {
        Binding {
            connection,
            pid: 30,
            start_us: 300,
            plan_hash: plan_hash(plan),
        }
    }

    #[test]
    fn a_valid_challenge_is_accepted_once() {
        let book = ChallengeBook::default();
        let now = Instant::now();
        let token = book.issue(binding(1, b"plan"), None, now).unwrap();
        assert_eq!(token.len(), 32);
        assert_eq!(book.redeem(binding(1, b"plan"), &token, None, now), Ok(()));
        assert_eq!(
            book.redeem(binding(1, b"plan"), &token, None, now),
            Err(ChallengeError::Reused)
        );
    }

    #[test]
    fn ineligible_callers_get_no_challenge() {
        let book = ChallengeBook::default();
        assert_eq!(
            book.issue(
                binding(1, b"p"),
                Some(RefusalReason::NoControllingTerminal),
                Instant::now()
            ),
            Err(ChallengeError::NotEligible(
                RefusalReason::NoControllingTerminal
            ))
        );
    }

    #[test]
    fn mismatches_are_refused_and_consume_the_challenge() {
        let book = ChallengeBook::default();
        let now = Instant::now();

        let token = book.issue(binding(1, b"plan"), None, now).unwrap();
        assert_eq!(
            book.redeem(binding(2, b"plan"), &token, None, now),
            Err(ChallengeError::OtherConnection)
        );
        assert_eq!(
            book.redeem(binding(1, b"plan"), &token, None, now),
            Err(ChallengeError::Reused)
        );

        let token = book.issue(binding(1, b"plan"), None, now).unwrap();
        assert_eq!(
            book.redeem(binding(1, b"plan v2"), &token, None, now),
            Err(ChallengeError::PlanChanged)
        );

        let token = book.issue(binding(1, b"plan"), None, now).unwrap();
        let mut other = binding(1, b"plan");
        other.start_us = 301;
        assert_eq!(
            book.redeem(other, &token, None, now),
            Err(ChallengeError::OtherProcess)
        );

        let token = book.issue(binding(1, b"plan"), None, now).unwrap();
        assert_eq!(
            book.redeem(
                binding(1, b"plan"),
                &token,
                None,
                now + CHALLENGE_TTL + Duration::from_millis(1)
            ),
            Err(ChallengeError::Expired)
        );

        // Eligible when issued, not when redeemed (a tmux pane gained an
        // agent in between).
        let token = book.issue(binding(1, b"plan"), None, now).unwrap();
        assert_eq!(
            book.redeem(
                binding(1, b"plan"),
                &token,
                Some(RefusalReason::AgentAncestry),
                now
            ),
            Err(ChallengeError::NotEligible(RefusalReason::AgentAncestry))
        );
        assert_eq!(
            book.redeem(binding(1, b"plan"), "not-a-token", None, now),
            Err(ChallengeError::Unknown)
        );
    }

    #[test]
    fn one_live_challenge_per_connection() {
        let book = ChallengeBook::default();
        let now = Instant::now();
        let first = book.issue(binding(1, b"plan"), None, now).unwrap();
        let second = book.issue(binding(1, b"plan"), None, now).unwrap();
        assert_ne!(first, second);
        assert_eq!(
            book.redeem(binding(1, b"plan"), &first, None, now),
            Err(ChallengeError::Unknown)
        );
        assert_eq!(book.redeem(binding(1, b"plan"), &second, None, now), Ok(()));
        let third = book.issue(binding(1, b"plan"), None, now).unwrap();
        book.forget(1);
        assert_eq!(
            book.redeem(binding(1, b"plan"), &third, None, now),
            Err(ChallengeError::Unknown)
        );
    }
}
