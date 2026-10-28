//! The Guardrails engine of the executor's governed plans, behind [`GuardrailsGate`]: it decides
//! a plan with the same function the hooks use and leaves at most one log entry per plan.
//!
//! The engine never takes a repo's write lock and never waits for the daemon loop: it reads the
//! repo on the caller's thread and hands its entries to the record it was attached to, which
//! never waits either.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use gitraptor_api::catalog::Layer;
use gitraptor_api::guard::{
    AuthorshipFacts, Decision, Effect, LogKind, LogLayer, LoggedAuthorship, LoggedReason,
    Operation, Rule,
};
use gitraptor_api::{Actor, AgentKind};
use gitraptor_git::{Hide, PathLimits};
use gitraptor_policy::guard::plan::{PlanShape, Preset, normalize};
use gitraptor_policy::guard::policies::Touched;

use super::evaluate::{DecideInput, RuleSet, decide_all, system_deny};
use super::log::LogEntry;
use super::registry::GuardRegistry;
use crate::channel::requester::Who;
use crate::executor::{GateDecision, GateRequest, GuardrailsGate, PlanBinding, PlanClose, Verdict};
use crate::timemachine::oplog::{Requester, RequesterOrigin};

/// Plans whose state the engine keeps at once, from the preview to their close (D8).
pub const MAX_PLANS: usize = 1024;

/// Where the engine writes: the channel's registry and the record of its log.
type Record = Arc<dyn Fn(LogEntry) -> bool + Send + Sync>;

struct Attached {
    registry: Arc<GuardRegistry>,
    record: Record,
}

/// What the single entry of a plan is made of.
struct PlanState {
    plan_id: String,
    /// The last decision taken for the plan (the preview, then the one that counts).
    decision: Decision,
    /// Present while the plan is allowed: shared with the marks and the executor.
    binding: Option<Arc<PlanBinding>>,
    /// The entry the plan leaves if it leaves one, without its kind, reasons and decision.
    entry: LogEntry,
}

impl PlanState {
    /// Whether someone other than the engine still holds the plan's binding (a running plan).
    fn alive(&self) -> bool {
        self.binding
            .as_ref()
            .is_some_and(|b| Arc::strong_count(b) > 1)
    }
}

/// A plan's decision with what its binding and entry need.
struct Decided {
    decision: Decision,
    /// The first evaluated operation: the one the entry names.
    first: Option<Operation>,
    registered: Vec<gitraptor_policy::guard::plan::Expected>,
    /// The authorship policy applied to a commit.
    policy: Option<&'static str>,
}

/// The real decision engine: one per daemon, attached once to the channel's registry and log.
#[derive(Default)]
pub struct GuardrailsEngine {
    attached: OnceLock<Attached>,
    /// In arrival order, so the oldest state without a live binding goes first.
    plans: Mutex<VecDeque<PlanState>>,
}

impl std::fmt::Debug for GuardrailsEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GuardrailsEngine")
            .field("attached", &self.attached.get().is_some())
            .field("plans", &self.lock().len())
            .finish()
    }
}

/// The log's layer for the executor's layer.
pub fn log_layer(layer: Layer) -> LogLayer {
    match layer {
        Layer::Cockpit => LogLayer::Cockpit,
        Layer::Mcp => LogLayer::Mcp,
    }
}

fn agent_of(actor: &Actor) -> Option<AgentKind> {
    match actor {
        Actor::Agent { kind, .. } => Some(*kind),
        Actor::Unattributed => None,
    }
}

/// The plan's requester as far as the gate knows it: its actor. The session lives in the marks,
/// which are what a child's classification reads.
fn who_of(actor: &Actor) -> Who {
    let requester = match actor {
        Actor::Agent { kind, .. } => Requester::Agent {
            name: serde_json::to_value(kind)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
            origin: RequesterOrigin::Detected,
            session_id: String::new(),
        },
        Actor::Unattributed => Requester::Unattributed,
    };
    Who {
        actor: actor.clone(),
        requester,
    }
}

/// The paths of the rebased commits: the commits of the tip that `onto` does not hold, hiding
/// only `onto` (D18). `None` when the repo cannot be opened.
fn rebase_preset(common: &Path, branch: &str, old_tip: &str, onto: &str) -> Option<Preset> {
    let reader = super::evaluate::open(common)?;
    let touched = match reader.fresh_commit_paths(
        Some(onto),
        old_tip,
        &[branch],
        Hide::OldOnly,
        &PathLimits::default(),
    ) {
        Ok(found) if !found.unverifiable => Touched {
            paths: found.paths,
            unverifiable: false,
        },
        _ => Touched {
            paths: Vec::new(),
            unverifiable: true,
        },
    };
    Some(Preset {
        touched: Some(touched.clone()),
        config_touched: Some(touched),
    })
}

/// What the executor sees of a decision. `ask` is applied as `deny` until the confirmation queue
/// exists, but the executor still tells the two apart: it refuses an `ask` with its own reason.
fn view_of(decision: &Decision) -> GateDecision {
    if decision.effect == Effect::Allow && decision.applied_effect == Effect::Allow {
        GateDecision::Allow
    } else if decision.effect == Effect::Ask && decision.applied_effect == Effect::Deny {
        GateDecision::Ask
    } else {
        GateDecision::Deny
    }
}

/// The entry of a plan, without what only its close knows.
fn base_entry(req: &GateRequest, first: Option<&Operation>, shape: Option<&PlanShape>) -> LogEntry {
    let (at_ms, utc_offset_s) = crate::watch::wall_now();
    let operation = first.map_or(
        gitraptor_api::guard::LoggedOperation::RefTransaction { refs: Vec::new() },
        |op| super::log::normalize(op, &[]),
    );
    let authorship = match shape {
        Some(PlanShape::Commit { authorship, .. }) => Some(LoggedAuthorship {
            coauthors: authorship.coauthors.clone(),
            agent_trailer: authorship.coauthors.iter().any(Option::is_some),
            unreadable: authorship.unreadable,
            policy: None,
        }),
        _ => None,
    };
    LogEntry {
        common_dir: req.common_dir.to_string_lossy().into_owned(),
        at_ms,
        utc_offset_s,
        worktree: Some(
            gitraptor_policy::guard::fastpath::simplified(req.worktree.clone())
                .to_string_lossy()
                .into_owned(),
        ),
        branch: req.branch.clone(),
        actor: agent_of(&req.actor),
        operation,
        kind: LogKind::Denial,
        effect: Effect::Deny,
        applied_effect: Effect::Deny,
        reasons: Vec::new(),
        decision_id: String::new(),
        authorship,
        layer: log_layer(req.layer),
        action_id: None,
    }
}

/// The single entry of a plan closed with `decision`, or `None` when it leaves none: a denial
/// whatever happened to the plan, a notice only if it ran.
fn closing_entry(mut entry: LogEntry, decision: &Decision, ran: bool) -> Option<LogEntry> {
    let (kind, reasons) = if decision.applied_effect != Effect::Allow {
        (LogKind::Denial, &decision.reasons)
    } else if ran && !decision.notices.is_empty() {
        (LogKind::Notice, &decision.notices)
    } else {
        return None;
    };
    entry.kind = kind;
    entry.effect = decision.effect;
    entry.applied_effect = decision.applied_effect;
    entry.reasons = reasons
        .iter()
        .map(|r| LoggedReason {
            rule: r.rule,
            level: r.level,
            cause: r.cause,
        })
        .collect();
    entry.decision_id = decision.decision_id.clone();
    Some(entry)
}

/// The decision the plan closes with: a denial a direct child met later (D6) wins over an
/// allowed plan.
fn final_decision(state: &PlanState) -> Decision {
    let late = state.binding.as_ref().and_then(|b| {
        b.late
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    });
    match late {
        Some(late) if late.applied_effect != Effect::Allow => late,
        _ => state.decision.clone(),
    }
}

impl GuardrailsEngine {
    fn lock(&self) -> MutexGuard<'_, VecDeque<PlanState>> {
        self.plans
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write(&self, entry: Option<LogEntry>) {
        if let (Some(entry), Some(attached)) = (entry, self.attached.get()) {
            // Never waits: over the cap the record counts the occurrence instead.
            let _ = (attached.record)(entry);
        }
    }

    /// The decision of `req`'s plan and what it was made of.
    fn decide(&self, attached: &Attached, req: &GateRequest) -> Decided {
        let failed = || Decided {
            decision: system_deny(Rule::InternalError),
            first: None,
            registered: Vec::new(),
            policy: None,
        };
        let Some(shape) = &req.shape else {
            return failed();
        };
        let normalized = normalize(shape);
        let mut preset = normalized.preset.clone();
        if let PlanShape::Rebase {
            branch,
            old_tip,
            onto,
            ..
        } = shape
        {
            match rebase_preset(&req.common_dir, branch, old_tip, onto) {
                Some(p) if preset.len() > 1 => preset[1] = p,
                _ => return failed(),
            }
        }
        let authorship = match shape {
            PlanShape::Commit { authorship, .. } => Some(authorship),
            _ => None,
        };
        let input = DecideInput {
            common_dir: &req.common_dir,
            repo_id: &req.repo_id,
            actor: agent_of(&req.actor),
            worktree: Some(&req.worktree),
            layer: log_layer(req.layer),
            rules: RuleSet::all(),
            authorship,
            // D12: `main` and the main branch, for everyone, while the repo is unregistered.
            unregistered_bases: Some(&[]),
        };
        let served = decide_all(&attached.registry, &input, &normalized.evaluated, &preset);
        Decided {
            decision: served.decision,
            first: normalized.evaluated.into_iter().next(),
            registered: normalized.registered,
            policy: served.authorship_policy,
        }
    }
}

impl GuardrailsGate for GuardrailsEngine {
    fn evaluate(&self, req: &GateRequest) -> GateDecision {
        self.evaluate_detailed(req).view
    }

    fn evaluate_detailed(&self, req: &GateRequest) -> Verdict {
        let denied = |decision: Decision| Verdict {
            view: GateDecision::Deny,
            decision: Some(decision),
            binding: None,
        };
        // D16: an engine nobody attached cannot read the registry nor record: closed.
        let Some(attached) = self.attached.get() else {
            return denied(system_deny(Rule::InternalError));
        };
        let Decided {
            decision,
            first,
            registered,
            policy,
        } = self.decide(attached, req);
        let view = view_of(&decision);
        let binding = (view == GateDecision::Allow).then(|| {
            Arc::new(PlanBinding {
                plan_id: req.plan_id.clone(),
                decision: decision.clone(),
                layer: req.layer,
                who: who_of(&req.actor),
                registered: Arc::from(registered),
                late: Mutex::new(None),
            })
        });
        let mut entry = base_entry(req, first.as_ref(), req.shape.as_ref());
        if let Some(logged) = entry.authorship.as_mut() {
            logged.policy = policy.map(str::to_owned);
        }
        let mut evicted = None;
        {
            let mut plans = self.lock();
            if let Some(state) = plans.iter_mut().find(|s| s.plan_id == req.plan_id) {
                state.decision = decision.clone();
                state.binding = binding.clone();
                state.entry = entry;
            } else {
                if plans.len() >= MAX_PLANS {
                    match plans.iter().position(|s| !s.alive()) {
                        Some(i) => evicted = plans.remove(i),
                        None => {
                            drop(plans);
                            // Every plan is running: this one cannot be held, and its denial
                            // is written now, as no close will find it.
                            let refused = system_deny(Rule::InternalError);
                            self.write(closing_entry(entry, &refused, false));
                            return denied(refused);
                        }
                    }
                }
                plans.push_back(PlanState {
                    plan_id: req.plan_id.clone(),
                    decision: decision.clone(),
                    binding: binding.clone(),
                    entry,
                });
            }
        }
        // An expelled state closes as dropped: its denial, if it denied.
        if let Some(old) = evicted {
            let last = final_decision(&old);
            self.write(closing_entry(old.entry, &last, false));
        }
        Verdict {
            view,
            decision: Some(decision),
            binding,
        }
    }

    fn record_close(&self, req: &GateRequest, close: PlanClose) {
        let state = {
            let mut plans = self.lock();
            let Some(i) = plans.iter().position(|s| s.plan_id == req.plan_id) else {
                return;
            };
            plans.remove(i)
        };
        let Some(state) = state else {
            return;
        };
        let last = final_decision(&state);
        let ran = matches!(close, PlanClose::Ran(_));
        self.write(closing_entry(state.entry, &last, ran));
    }

    fn attach(&self, registry: &Arc<GuardRegistry>, record: Record) {
        // Once per daemon: a second attach changes nothing.
        let _ = self.attached.set(Attached {
            registry: Arc::clone(registry),
            record,
        });
    }
}

/// The authorship facts of a commit `message`, read with the repo's trailer configuration under
/// `common` (`commit.cleanup`, `core.commentChar`, as the `commit-msg` hook reads them). The text
/// of the message never leaves this function.
pub fn commit_facts(common: &Path, message: &str) -> AuthorshipFacts {
    use gitraptor_policy::authorship::{self, Cleanup, MAX_MESSAGE_BYTES, MessageOptions};
    if message.len() > MAX_MESSAGE_BYTES {
        return authorship::unreadable();
    }
    let mut options = MessageOptions::default();
    if let Some(reader) = super::evaluate::open(common) {
        let (cleanup, comment) = reader.commit_message_config();
        if let Some(c) = cleanup {
            options.cleanup = Cleanup::from_config(&c);
        }
        // `auto` picks a character absent from the message: `#` unless it is used.
        match comment.as_deref() {
            Some("auto") | None => {}
            Some(c) if !c.is_empty() => options.comment = c.to_owned(),
            Some(_) => {}
        }
    }
    authorship::facts(message.as_bytes(), &options)
}

#[cfg(test)]
#[path = "plan_gate_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "plan_gate_ask_tests.rs"]
mod ask_tests;
