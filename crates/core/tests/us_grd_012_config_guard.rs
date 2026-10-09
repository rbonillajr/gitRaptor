//! The protection of the Guardrails configuration on the daemon's side, without a channel: what a
//! connection without `guard.config-protection` sees, and what an agent's movement that cannot be
//! verified gets. Temporary repos only (NFR-01).

mod common;

use common::git;
use gitraptor_api::AgentKind;
use gitraptor_api::guard::{
    Cause, Decision, Effect, EvaluateParams, ExceptionState, GuardLogEntry, GuardLogResult,
    GuardLogSummary, Hook, Level, LogDetail, LogKind, LogLayer, LogOrigin, LoggedOperation,
    LoggedReason, Operation, Param, ParamKind, Reason, RefUpdate, RefValue, Rule,
};
use gitraptor_core::guardrails::config_guard::{legacy_decision, legacy_log};
use gitraptor_core::guardrails::evaluate::{Caller, serve_as};
use gitraptor_core::guardrails::{GuardEntry, GuardRegistry};

fn denial(reasons: Vec<Reason>) -> Decision {
    Decision {
        decision_id: "d".into(),
        effect: Effect::Deny,
        applied_effect: Effect::Deny,
        reasons,
        exception: ExceptionState::None,
        config_status: Vec::new(),
        config_ref: Vec::new(),
        notices: Vec::new(),
    }
}

fn entry(kind: LogKind, rule: Rule, id: &str) -> GuardLogEntry {
    GuardLogEntry {
        at_ms: 1,
        utc_offset_s: 0,
        last_ms: 1,
        count: 1,
        worktree: None,
        branch: None,
        actor: Some(AgentKind::ClaudeCode),
        operation: LoggedOperation::RefTransaction { refs: Vec::new() },
        kind,
        detail: LogDetail::Full,
        effect: if kind == LogKind::Denial {
            Effect::Deny
        } else {
            Effect::Allow
        },
        applied_effect: if kind == LogKind::Denial {
            Effect::Deny
        } else {
            Effect::Allow
        },
        reasons: vec![LoggedReason {
            rule,
            level: Level::Minimum,
            cause: None,
        }],
        layer: LogLayer::Hooks,
        origin: LogOrigin::Daemon,
        decision_id: id.into(),
        authorship: None,
    }
}

/// D9: a connection that did not ask for `guard.config-protection` keeps the protection, but it
/// reads as a forbidden path of `/.gitraptor/`, and the `relax-ignored` notices never reach it.
/// A denial is never downgraded to an allow.
#[test]
fn without_the_capability_the_reason_reads_as_a_forbidden_path() {
    // The decision.
    let mut d = denial(vec![
        Reason {
            rule: Rule::ConfigProtected,
            level: Level::Minimum,
            cause: None,
            params: vec![
                Param::new(ParamKind::Path, ".gitraptor/settings.json"),
                Param::new(ParamKind::Pattern, "/.gitraptor/"),
            ],
        },
        Reason {
            rule: Rule::ConfigProtected,
            level: Level::Minimum,
            cause: Some(Cause::Unverifiable),
            params: Vec::new(),
        },
    ]);
    legacy_decision(&mut d);
    assert_eq!(d.applied_effect, Effect::Deny, "{d:?}");
    assert_eq!(d.effect, Effect::Deny, "{d:?}");
    assert!(
        d.reasons.iter().all(|r| r.rule == Rule::ForbiddenPath),
        "{d:?}"
    );
    assert!(
        d.reasons[0]
            .params
            .contains(&Param::new(ParamKind::Pattern, "/.gitraptor/")),
        "{d:?}"
    );
    assert!(
        d.reasons[0]
            .params
            .contains(&Param::new(ParamKind::Path, ".gitraptor/settings.json")),
        "{d:?}"
    );
    assert_eq!(d.reasons[1].cause, Some(Cause::Unverifiable), "{d:?}");

    // The log page.
    let mut log = GuardLogResult {
        since_ms: 0,
        summary: GuardLogSummary::default(),
        entries: vec![
            entry(LogKind::Denial, Rule::ConfigProtected, "a"),
            entry(LogKind::Notice, Rule::RelaxIgnored, "b"),
            entry(LogKind::Denial, Rule::ForbiddenPath, "c"),
        ],
        unlogged_periods: Vec::new(),
    };
    legacy_log(&mut log);
    let ids: Vec<&str> = log.entries.iter().map(|e| e.decision_id.as_str()).collect();
    assert_eq!(ids, ["a", "c"], "the notice of `relax-ignored` is dropped");
    assert!(
        log.entries
            .iter()
            .flat_map(|e| &e.reasons)
            .all(|r| r.rule == Rule::ForbiddenPath),
        "{log:?}"
    );
    assert!(
        log.entries.iter().all(|e| e.applied_effect == Effect::Deny),
        "{log:?}"
    );
}

/// D5: what cannot be verified within the bounds is denied to an agent even when no path rule
/// exists, and the person is never held.
#[test]
fn an_unverifiable_movement_by_an_agent_is_denied() {
    let repo = tempfile::tempdir().unwrap();
    let path = repo.path();
    git(path, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(path.join(".gitraptor")).unwrap();
    // Protected branches only: no forbidden-path rule governs anyone.
    std::fs::write(
        path.join(".gitraptor/settings.json"),
        r#"{"policies":{"protectedBranches":{"patterns":["main"]}}}"#,
    )
    .unwrap();
    git(path, &["add", "."]);
    git(path, &["commit", "-q", "-m", "a"]);
    let base = git(path, &["rev-parse", "HEAD"]);
    let tree = git(path, &["rev-parse", "HEAD^{tree}"]);
    // More new commits than the bound (256): a chain written with plumbing, nothing moved.
    let mut tip = base.clone();
    for n in 0..300 {
        tip = git(
            path,
            &["commit-tree", &tree, "-p", &tip, "-m", &format!("c{n}")],
        );
        assert_eq!(tip.len(), 40, "commit-tree failed: {tip:?}");
    }
    let common = path.join(".git").canonicalize().unwrap();
    let registry = GuardRegistry::default();
    registry.set(
        "0000-ffff",
        GuardEntry {
            common_dir: common.to_string_lossy().into_owned(),
            bases: vec!["main".into()],
            confirmed: None,
        },
    );
    let params = EvaluateParams {
        repo_id: "0000-ffff".into(),
        common_dir: common.to_string_lossy().into_owned(),
        hook: Hook::ReferenceTransaction,
        operation: Operation::RefTransaction {
            updates: vec![RefUpdate {
                refname: "refs/heads/feat-x".into(),
                old: RefValue::Oid(base),
                new: RefValue::Oid(tip),
            }],
            orphan_head: None,
        },
        authorship: None,
    };
    let caller = |agent: bool| Caller {
        actor: agent.then_some(AgentKind::ClaudeCode),
        cwd: Some(path.to_path_buf()),
        policies: true,
        ..Caller::default()
    };

    let d = serve_as(&registry, &params, &caller(true));
    assert_eq!(d.applied_effect, Effect::Deny, "{d:?}");
    assert_eq!(d.reasons[0].rule, Rule::ConfigProtected, "{d:?}");
    assert_eq!(d.reasons[0].cause, Some(Cause::Unverifiable), "{d:?}");
    assert!(d.reasons[0].params.is_empty(), "{d:?}");

    let d = serve_as(&registry, &params, &caller(false));
    assert_eq!(d.applied_effect, Effect::Allow, "{d:?}");
}
