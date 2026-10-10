//! The protection of the Guardrails configuration on the daemon's side: the
//! `config.relax-ignored` notice and the downgrade for a connection without
//! `guard.config-protection`.
//!
//! Nothing here decides anything: the denial is `policy::guard::config::protect_config`, and the
//! notice only observes what [`gitraptor_policy::layers::ignored_relaxations`] found. A notice
//! says which level declared the relaxation, never what it declared and never who wrote the
//! file: the level's file is not attributed.

use gitraptor_api::guard::{
    Cause, Decision, EvaluateParams, GuardLogResult, Level, LogKind, LoggedReason, Operation,
    Param, ParamKind, Reason, Rule,
};
use gitraptor_policy::guard::config::CONFIG_PATTERN;
use gitraptor_policy::layers::IgnoredRelaxation;

use super::log::{LogContext, LogEntry, normalize};

/// The `config.relax-ignored` notice of an evaluation: `None` without an agent actor, under the
/// executor, for an operation that is not a branch movement or a push, or with nothing ignored.
/// Same normalized operation, decision id and effects as the decision; one reason per distinct
/// level, no params. It is a separate entry (kind `notice`), so it never counts as a block and
/// is never sent to the hook client.
pub fn relax_entry(
    params: &EvaluateParams,
    decision: &Decision,
    ignored: &[IgnoredRelaxation],
    ctx: &LogContext,
) -> Option<LogEntry> {
    if ctx.under_executor || ctx.actor.is_none() || ignored.is_empty() {
        return None;
    }
    if !matches!(
        params.operation,
        Operation::RefTransaction { .. } | Operation::Push { .. }
    ) {
        return None;
    }
    let mut levels: Vec<Level> = Vec::new();
    for r in ignored {
        if !levels.contains(&r.level) {
            levels.push(r.level);
        }
    }
    Some(LogEntry {
        common_dir: params.common_dir.clone(),
        at_ms: ctx.at_ms,
        utc_offset_s: ctx.utc_offset_s,
        worktree: ctx.worktree.clone(),
        branch: ctx.branch.clone(),
        actor: ctx.actor,
        operation: normalize(&params.operation, &decision.reasons),
        kind: LogKind::Notice,
        effect: decision.effect,
        applied_effect: decision.applied_effect,
        reasons: levels
            .into_iter()
            .map(|level| LoggedReason {
                rule: Rule::RelaxIgnored,
                level,
                cause: None,
            })
            .collect(),
        decision_id: decision.decision_id.clone(),
        authorship: None,
    })
}

/// `policy.config-protected` as the `policy.forbidden-path` a client without the capability
/// knows: the same effect, `path` and `pattern` = `/.gitraptor/`. An unverifiable one keeps its
/// cause, which `policy.forbidden-path` also has.
fn legacy_reason(reason: &mut Reason) {
    if reason.rule != Rule::ConfigProtected {
        return;
    }
    reason.rule = Rule::ForbiddenPath;
    if reason.cause != Some(Cause::Unverifiable)
        && !reason.params.iter().any(|p| p.kind == ParamKind::Pattern)
    {
        reason
            .params
            .push(Param::new(ParamKind::Pattern, CONFIG_PATTERN));
    }
}

/// A decision for a connection without `guard.config-protection`: the protection is applied all
/// the same (a denial is never downgraded to an allow), and it reads as a forbidden path.
pub fn legacy_decision(decision: &mut Decision) {
    decision.reasons.iter_mut().for_each(legacy_reason);
    decision.notices.iter_mut().for_each(legacy_reason);
    // Defense in depth: a notice of the configuration never travels to an old client.
    decision.notices.retain(|r| r.rule != Rule::RelaxIgnored);
}

/// A `guard.log` page for a connection without `guard.config-protection`: the reasons read as a
/// forbidden path. The `relax-ignored` notices were already left out by the query (page and
/// totals alike, `Store::guard_log`); dropping them here again is defense in depth and leaves the
/// totals alone, since they never counted them.
pub fn legacy_log(log: &mut GuardLogResult) {
    log.entries
        .retain(|e| e.reasons.iter().all(|r| r.rule != Rule::RelaxIgnored));
    for reason in log.entries.iter_mut().flat_map(|e| &mut e.reasons) {
        if reason.rule == Rule::ConfigProtected {
            reason.rule = Rule::ForbiddenPath;
        }
    }
}

#[cfg(test)]
mod tests {
    use gitraptor_api::AgentKind;
    use gitraptor_api::guard::{Effect, ExceptionState, Hook, LogDetail, RefUpdate, RefValue};
    use gitraptor_policy::layers::RelaxKey;

    use super::*;

    fn params() -> EvaluateParams {
        EvaluateParams {
            repo_id: "0000-ffff".into(),
            common_dir: "/repo/.git".into(),
            hook: Hook::ReferenceTransaction,
            operation: Operation::RefTransaction {
                updates: vec![RefUpdate {
                    refname: "refs/heads/feat-x".into(),
                    old: RefValue::Zero,
                    new: RefValue::Zero,
                }],
                orphan_head: None,
            },
            authorship: None,
        }
    }

    fn decision() -> Decision {
        Decision {
            decision_id: "d1".into(),
            effect: Effect::Deny,
            applied_effect: Effect::Deny,
            reasons: Vec::new(),
            notices: Vec::new(),
            exception: ExceptionState::None,
            config_status: Vec::new(),
            config_ref: Vec::new(),
            prior_snapshot: None,
        }
    }

    fn ctx(actor: Option<AgentKind>, under_executor: bool) -> LogContext {
        LogContext {
            actor,
            under_executor,
            ..LogContext::default()
        }
    }

    fn ignored(level: Level) -> IgnoredRelaxation {
        IgnoredRelaxation {
            level,
            key: RelaxKey::BaseBranch,
        }
    }

    #[test]
    fn the_notice_is_only_for_an_agent_outside_the_executor_with_something_ignored() {
        let (p, d) = (params(), decision());
        let agent = Some(AgentKind::ClaudeCode);
        let one = [ignored(Level::Local)];
        assert!(relax_entry(&p, &d, &one, &ctx(None, false)).is_none());
        assert!(relax_entry(&p, &d, &one, &ctx(agent, true)).is_none());
        assert!(relax_entry(&p, &d, &[], &ctx(agent, false)).is_none());
        let mut commit = params();
        commit.operation = Operation::Commit {
            stage: gitraptor_api::guard::CommitStage::CommitMsg,
        };
        assert!(relax_entry(&commit, &d, &one, &ctx(agent, false)).is_none());

        // One reason per distinct level, the decision's id, and never a block.
        let two = [
            ignored(Level::Worktree),
            ignored(Level::Worktree),
            ignored(Level::Local),
        ];
        let e = relax_entry(&p, &d, &two, &ctx(agent, false)).expect("a notice");
        assert_eq!(e.kind, LogKind::Notice);
        assert_eq!(e.decision_id, "d1");
        let levels: Vec<Level> = e.reasons.iter().map(|r| r.level).collect();
        assert_eq!(levels, [Level::Worktree, Level::Local]);
        assert!(e.reasons.iter().all(|r| r.rule == Rule::RelaxIgnored));
        // Distinct from the decision's own entry in the aggregation.
        assert!(e.agg_key().contains("notice"));
    }

    #[test]
    fn legacy_log_rewrites_the_config_denials_and_leaves_the_totals_alone() {
        let e = |kind, rule| gitraptor_api::guard::GuardLogEntry {
            at_ms: 1,
            utc_offset_s: 0,
            last_ms: 1,
            count: 3,
            worktree: None,
            branch: None,
            actor: None,
            operation: gitraptor_api::guard::LoggedOperation::RefTransaction { refs: Vec::new() },
            kind,
            detail: LogDetail::Full,
            effect: Effect::Allow,
            applied_effect: Effect::Allow,
            reasons: vec![LoggedReason {
                rule,
                level: Level::Minimum,
                cause: None,
            }],
            layer: gitraptor_api::guard::LogLayer::Hooks,
            origin: gitraptor_api::guard::LogOrigin::Daemon,
            decision_id: "x".into(),
            authorship: None,
        };
        let mut log = GuardLogResult {
            since_ms: 0,
            summary: gitraptor_api::guard::GuardLogSummary {
                blocked: 3,
                notices: 5,
                rate_limited: 0,
            },
            entries: vec![
                e(LogKind::Denial, Rule::ConfigProtected),
                e(LogKind::Notice, Rule::RelaxIgnored),
            ],
            unlogged_periods: Vec::new(),
        };
        legacy_log(&mut log);
        assert_eq!(log.entries.len(), 1);
        assert_eq!(log.entries[0].reasons[0].rule, Rule::ForbiddenPath);
        // The totals are the query's: the rewrite leaves them alone.
        assert_eq!(log.summary.blocked, 3);
        assert_eq!(log.summary.notices, 5);
    }
}
