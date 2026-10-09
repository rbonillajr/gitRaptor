//! The decision log entry (US-GRD-005, ADR-GRD-006 § 1 and § 2): what the connection thread
//! builds from a decision, and the sink that keeps the hook from ever waiting on the log.
//!
//! - [`entry`]: a denial, a warning or an agent's `flexible` commit becomes an entry; an
//!   allowed operation without a rule, or anything under an executor operation (one entry per
//!   plan, ADR-GRD-006 Enmienda Cockpit), does not.
//! - The operation is normalized: refs and their change, the remote without userinfo, query or
//!   fragment. Never argv, oids or a message (M-06).
//! - [`LogSink`]: entries in flight to the daemon loop are capped (D4). Over the cap an entry
//!   is not queued: its occurrence is added to a shared counter the loop writes as a row over
//!   the cap, so the KPI never loses it.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};

use gitraptor_api::guard::{
    CommitStage, Decision, Effect, EvaluateParams, LogKind, LoggedAuthorship, LoggedOperation,
    LoggedReason, LoggedRef, Operation, ParamKind, Reason, RefChange, RefValue, Rule,
};
use gitraptor_api::{AgentKind, Untrusted};

/// Entries in flight from the connections to the loop (D4).
pub const MAX_IN_FLIGHT: usize = 1024;
/// Refs kept per logged operation (D5).
pub const MAX_REFS: usize = 16;
/// How often the daemon purges the expired entries (ADR-GRD-006 § 3, ASSUMPTION).
pub const PURGE_EVERY_MS: i64 = 24 * 60 * 60 * 1000;

/// What the connection knows of the caller beyond the request (D3).
#[derive(Debug, Clone, Default)]
pub struct LogContext {
    /// The agent behind the hook client; `None` = unattributed.
    pub actor: Option<AgentKind>,
    /// The hook client runs under an operation of the executor: its plan logs, not the hook.
    pub under_executor: bool,
    /// Canonical worktree (the hook client's working directory).
    pub worktree: Option<String>,
    /// The worktree's branch (`HEAD`).
    pub branch: Option<String>,
    /// The authorship policy applied to a commit (`agents-commit`, `human-author`,
    /// `flexible`), only when it was.
    pub authorship_policy: Option<String>,
    pub at_ms: i64,
    pub utc_offset_s: i32,
}

/// One occurrence to log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// The repo, as the dispatcher fixed it: the loop finds the observed repo by it.
    pub common_dir: String,
    pub at_ms: i64,
    pub utc_offset_s: i32,
    pub worktree: Option<String>,
    pub branch: Option<String>,
    pub actor: Option<AgentKind>,
    pub operation: LoggedOperation,
    pub kind: LogKind,
    pub effect: Effect,
    pub applied_effect: Effect,
    pub reasons: Vec<LoggedReason>,
    pub decision_id: String,
    pub authorship: Option<LoggedAuthorship>,
}

impl LogEntry {
    /// Two occurrences with the same key are the same entry (ADR-GRD-006 § 2).
    pub fn agg_key(&self) -> String {
        serde_json::to_string(&(
            "full",
            &self.worktree,
            &self.branch,
            &self.actor,
            &self.operation,
            &self.kind,
            &self.reasons,
            &self.authorship,
        ))
        .unwrap_or_default()
    }

    /// This occurrence as a row over the cap: kind, operation without refs or remote, and the
    /// first rule.
    pub fn overflow(&self) -> OverflowRow {
        OverflowRow {
            common_dir: self.common_dir.clone(),
            kind: self.kind,
            operation: stripped(&self.operation),
            reason: self.reasons.first().cloned(),
            effect: self.effect,
            applied_effect: self.applied_effect,
            count: 1,
            first_ms: self.at_ms,
            last_ms: self.at_ms,
            utc_offset_s: self.utc_offset_s,
        }
    }
}

/// Occurrences counted without their detail (ADR-GRD-006 § 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverflowRow {
    pub common_dir: String,
    pub kind: LogKind,
    pub operation: LoggedOperation,
    pub reason: Option<LoggedReason>,
    pub effect: Effect,
    pub applied_effect: Effect,
    pub count: u64,
    pub first_ms: i64,
    pub last_ms: i64,
    pub utc_offset_s: i32,
}

impl OverflowRow {
    pub fn agg_key(&self) -> String {
        serde_json::to_string(&("rate-limited", &self.kind, &self.operation, &self.reason))
            .unwrap_or_default()
    }
}

fn stripped(op: &LoggedOperation) -> LoggedOperation {
    match op {
        LoggedOperation::Push { .. } => LoggedOperation::Push {
            remote: None,
            refs: vec![],
        },
        LoggedOperation::RefTransaction { .. } => LoggedOperation::RefTransaction { refs: vec![] },
        LoggedOperation::Rebase { .. } => LoggedOperation::Rebase {
            upstream: None,
            branch: None,
        },
        LoggedOperation::Commit { stage } => LoggedOperation::Commit { stage: *stage },
        // Nothing in it is a ref, a remote or an oid.
        LoggedOperation::ProtectionState { .. } => op.clone(),
    }
}

/// The entry of a change of state of the hook layer (US-GRD-004, ADR-GRD-005 § 5): no actor,
/// no operation of Git, nothing but the two states and the cause. Repeats of the same
/// transition aggregate like any entry.
pub fn protection_entry(
    common_dir: &str,
    at_ms: i64,
    utc_offset_s: i32,
    from: gitraptor_api::guard::HooksStatus,
    to: &gitraptor_api::guard::HooksLayer,
    expected: bool,
) -> LogEntry {
    LogEntry {
        common_dir: common_dir.to_owned(),
        at_ms,
        utc_offset_s,
        worktree: None,
        branch: None,
        actor: None,
        operation: LoggedOperation::ProtectionState {
            from,
            to: to.status,
            cause: to.cause,
            expected,
        },
        kind: LogKind::ProtectionState,
        effect: Effect::Allow,
        applied_effect: Effect::Allow,
        reasons: Vec::new(),
        decision_id: super::evaluate::decision_id(),
        authorship: None,
    }
}

/// The entry of a decision, or `None` when nothing is logged.
pub fn entry(params: &EvaluateParams, decision: &Decision, ctx: &LogContext) -> Option<LogEntry> {
    if ctx.under_executor {
        return None;
    }
    let (kind, reasons) = if decision.applied_effect != Effect::Allow {
        (LogKind::Denial, &decision.reasons)
    } else if !decision.notices.is_empty() || flexible_agent_commit(params, ctx) {
        (LogKind::Notice, &decision.notices)
    } else {
        return None;
    };
    let authorship = match params.operation {
        Operation::Commit { .. } => params.authorship.as_ref().map(|f| LoggedAuthorship {
            coauthors: f.coauthors.clone(),
            agent_trailer: f.coauthors.iter().any(Option::is_some),
            unreadable: f.unreadable,
            policy: ctx.authorship_policy.clone(),
        }),
        _ => None,
    };
    Some(LogEntry {
        common_dir: params.common_dir.clone(),
        at_ms: ctx.at_ms,
        utc_offset_s: ctx.utc_offset_s,
        worktree: ctx.worktree.clone(),
        branch: ctx.branch.clone(),
        actor: ctx.actor,
        operation: normalize(&params.operation, reasons),
        kind,
        effect: decision.effect,
        applied_effect: decision.applied_effect,
        reasons: reasons
            .iter()
            .map(|r| LoggedReason {
                rule: r.rule,
                level: r.level,
                cause: r.cause,
            })
            .collect(),
        decision_id: decision.decision_id.clone(),
        authorship,
    })
}

/// An agent's commit under `flexible` passes with no rule, and BR-AUTH-005 still has it
/// recorded: where the commit is decided, never at `pre-commit`.
fn flexible_agent_commit(params: &EvaluateParams, ctx: &LogContext) -> bool {
    ctx.actor.is_some()
        && ctx.authorship_policy.as_deref() == Some("flexible")
        && matches!(
            params.operation,
            Operation::Commit {
                stage: CommitStage::CommitMsg | CommitStage::SecondLine
            }
        )
}

fn short(name: &str) -> &str {
    name.strip_prefix("refs/heads/").unwrap_or(name)
}

/// The refs the reasons name, short.
fn named(reasons: &[Reason]) -> Vec<String> {
    reasons
        .iter()
        .flat_map(|r| &r.params)
        .filter(|p| matches!(p.kind, ParamKind::Branch | ParamKind::Ref | ParamKind::Base))
        .map(|p| short(p.value.raw()).to_owned())
        .collect()
}

/// The refs the reasons name, or the first ones when they name none; never more than
/// [`MAX_REFS`], and never a walk over every update of a huge operation.
fn keep<T>(
    items: &[T],
    name: impl Fn(&T) -> &str,
    to_ref: impl Fn(&T) -> LoggedRef,
    named: &[String],
) -> Vec<LoggedRef> {
    let wanted: Vec<LoggedRef> = items
        .iter()
        .filter(|i| named.iter().any(|n| n == short(name(i))))
        .take(MAX_REFS)
        .map(&to_ref)
        .collect();
    if wanted.is_empty() {
        items.iter().take(MAX_REFS).map(to_ref).collect()
    } else {
        wanted
    }
}

pub(crate) fn normalize(op: &Operation, reasons: &[Reason]) -> LoggedOperation {
    let named = named(reasons);
    let forced = reasons.iter().any(|r| r.rule == Rule::MinimumForcePush);
    match op {
        Operation::Push { remote, updates } => {
            let to_ref = |u: &gitraptor_api::guard::PushUpdate| {
                let name = short(&u.remote_ref);
                let change = if u.local.is_zero() {
                    RefChange::Delete
                } else if u.remote.is_zero() {
                    RefChange::Create
                } else if forced && named.iter().any(|n| n == name) {
                    RefChange::Force
                } else {
                    RefChange::Update
                };
                LoggedRef {
                    name: Untrusted::new(name),
                    change,
                }
            };
            let remote = sanitize_remote(remote.raw());
            LoggedOperation::Push {
                remote: (!remote.is_empty()).then(|| Untrusted::new(remote)),
                refs: keep(updates, |u| &u.remote_ref, to_ref, &named),
            }
        }
        Operation::RefTransaction { updates, .. } => {
            let to_ref = |u: &gitraptor_api::guard::RefUpdate| LoggedRef {
                name: Untrusted::new(short(&u.refname)),
                change: match (&u.old, &u.new) {
                    (_, RefValue::Zero) => RefChange::Delete,
                    (RefValue::Zero, _) => RefChange::Create,
                    _ => RefChange::Update,
                },
            };
            LoggedOperation::RefTransaction {
                refs: keep(updates, |u| &u.refname, to_ref, &named),
            }
        }
        // Its upstream and branch come from argv and may be oids (M-06): not kept.
        Operation::Rebase { .. } => LoggedOperation::Rebase {
            upstream: None,
            branch: None,
        },
        Operation::Commit { stage } => LoggedOperation::Commit { stage: *stage },
    }
}

/// A remote as it may be logged: the name, or the URL without userinfo, query or fragment
/// (M-06). A token in any of the three never reaches the profile.
pub fn sanitize_remote(remote: &str) -> String {
    // Userinfo first, up to the last `@`: a password may hold `:`, `/`, `?` or `#`.
    let (scheme, rest) = match remote.split_once("://") {
        Some((scheme, rest)) => (Some(scheme), rest),
        None => (None, remote),
    };
    let rest = rest.rsplit_once('@').map_or(rest, |(_, host)| host);
    let rest = &rest[..rest.find(['?', '#']).unwrap_or(rest.len())];
    match scheme {
        Some(scheme) => format!("{scheme}://{rest}"),
        None => rest.to_owned(),
    }
}

/// Entries in flight to the loop, and the occurrences over the cap (D4). Shared by every
/// connection and the loop through the registry.
#[derive(Debug)]
pub struct LogSink {
    capacity: usize,
    in_flight: AtomicUsize,
    overflow: Mutex<HashMap<(String, String), OverflowRow>>,
    /// When the expired entries were last purged; `i64::MIN` before the first time.
    last_purge: AtomicI64,
}

impl Default for LogSink {
    fn default() -> Self {
        Self::with_capacity(MAX_IN_FLIGHT)
    }
}

impl LogSink {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            in_flight: AtomicUsize::new(0),
            overflow: Mutex::default(),
            last_purge: AtomicI64::new(i64::MIN),
        }
    }

    /// Whether a purge is due at `now_ms` (the first time, then every 24 h); marks it done.
    pub fn purge_due(&self, now_ms: i64) -> bool {
        let last = self.last_purge.load(Ordering::Acquire);
        (last == i64::MIN || now_ms.saturating_sub(last) >= PURGE_EVERY_MS)
            && self
                .last_purge
                .compare_exchange(last, now_ms, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
    }

    /// Takes one slot to queue an entry; `false` when the cap is reached. Never waits.
    pub fn try_reserve(&self) -> bool {
        self.update(|n| (n < self.capacity).then_some(n + 1))
    }

    /// Gives back a slot: the loop wrote (or dropped) a queued entry.
    pub fn release(&self) {
        self.update(|n| n.checked_sub(1));
    }

    /// Applies `f` to the slots in flight; `false` when it refuses.
    fn update(&self, f: impl Fn(usize) -> Option<usize>) -> bool {
        let mut current = self.in_flight.load(Ordering::Acquire);
        while let Some(next) = f(current) {
            match self.in_flight.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(seen) => current = seen,
            }
        }
        false
    }

    /// Counts an occurrence that was not queued.
    pub fn overflow(&self, entry: &LogEntry) {
        let row = entry.overflow();
        let key = (row.common_dir.clone(), row.agg_key());
        let Ok(mut map) = self.overflow.lock() else {
            return;
        };
        map.entry(key)
            .and_modify(|r| {
                r.count += 1;
                r.first_ms = r.first_ms.min(row.first_ms);
                r.last_ms = r.last_ms.max(row.last_ms);
            })
            .or_insert(row);
    }

    /// Takes every counted occurrence, for the loop to write.
    pub fn drain(&self) -> Vec<OverflowRow> {
        self.overflow
            .lock()
            .map(|mut map| map.drain().map(|(_, row)| row).collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scp_remotes_lose_the_user() {
        assert_eq!(
            sanitize_remote("git@example.com:o/r.git"),
            "example.com:o/r.git"
        );
        assert_eq!(sanitize_remote("/srv/remote.git"), "/srv/remote.git");
        assert_eq!(
            sanitize_remote("file:///srv/r.git?x=1"),
            "file:///srv/r.git"
        );
    }

    #[test]
    fn the_sink_releases_without_underflow() {
        let sink = LogSink::with_capacity(1);
        sink.release();
        assert!(sink.try_reserve());
        assert!(!sink.try_reserve());
    }
}
