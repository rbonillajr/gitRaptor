//! US-GRD-006 PR-B (DS-US-GRD-006 § 5, S1, S3, S4): the wire contract the exception flow
//! completes. Driven through JSON strings and the method table, so it compiles before the new
//! enum variants exist.

use gitraptor_api::guard::{GuardExceptionParams, LogKind, PendingKind};
use gitraptor_api::methods::{
    GROUPS, GUARD_EXCEPTION, GUARD_EXCEPTION_CHILD, GUARD_EXCEPTION_END, GUARD_EXCEPTION_REFUSED,
    spec,
};
use serde_json::json;

#[test]
fn exception_pending_and_log_kinds_round_trip() {
    {
        let (wire, what) = ("exception", "PendingKind");
        let read = serde_json::from_value::<PendingKind>(json!(wire)).unwrap_or_else(|e| {
            panic!("GRD006-API-KINDS: {what} should read {wire:?} (PendingKind::Exception): {e}")
        });
        assert_eq!(
            serde_json::to_value(read).unwrap(),
            json!(wire),
            "GRD006-API-KINDS: {what} should write {wire:?} back"
        );
    }
    for wire in ["exception", "exception-rejected", "exception-cancelled"] {
        let read = serde_json::from_value::<LogKind>(json!(wire)).unwrap_or_else(|e| {
            panic!("GRD006-API-KINDS: LogKind should read {wire:?} (the exception entries): {e}")
        });
        assert_eq!(
            serde_json::to_value(read).unwrap(),
            json!(wire),
            "GRD006-API-KINDS: LogKind should write {wire:?} back in kebab-case"
        );
    }
}

#[test]
fn exception_methods_are_implemented_and_their_error_registered() {
    for name in [GUARD_EXCEPTION, GUARD_EXCEPTION_CHILD, GUARD_EXCEPTION_END] {
        let m = spec(name).unwrap_or_else(|| panic!("{name} should be declared"));
        assert_eq!(
            m.implemented_by, None,
            "GRD006-API-REGISTERED: {name} is implemented by PR-B: drop `.ahead(\"US-GRD-006\")`"
        );
    }
    let guard = GROUPS.iter().find(|g| g.module == "guard").unwrap();
    assert!(
        guard
            .errors
            .iter()
            .any(|e| e.name == GUARD_EXCEPTION_REFUSED.name),
        "GRD006-API-REGISTERED: GUARD_EXCEPTION_REFUSED should be in the guard GROUP.errors (its en/es messages)"
    );
}

#[test]
fn exception_params_debug_redacts_argv() {
    let params = GuardExceptionParams {
        path: "/repo".into(),
        argv: vec![
            "git".into(),
            "push".into(),
            "--force".into(),
            "secret-branch-name".into(),
        ],
        confirm: None,
    };
    let shown = format!("{params:?}");
    for leak in ["secret-branch-name", "--force", "push"] {
        assert!(
            !shown.contains(leak),
            "GRD006-API-ARGV-REDACTED: the Debug of GuardExceptionParams shows {leak:?} of the argv: {shown}"
        );
    }
}
