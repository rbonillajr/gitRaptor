//! US-GRD-006 PR-A (DS-US-GRD-006 § 5, D4, D15, D18, D19, D20): the wire contract of the conscious
//! exception: its types, its three methods, its capability and its error. Existing types that gain
//! a field are driven with JSON strings, so this file compiles before those fields exist.

use gitraptor_api::Untrusted;
use gitraptor_api::capability;
use gitraptor_api::capability::CAPABILITIES_PROTOCOL;
use gitraptor_api::guard::{
    EvaluateParams, ExceptionRefusal, ExceptionToken, GuardExceptionChildParams,
    GuardExceptionEndParams, GuardExceptionEndResult, GuardExceptionParams,
    GuardExceptionRefusedData, GuardExceptionResult, GuardLogSummary, IssuedException,
    MAX_EXCEPTION_ARG_BYTES, MAX_EXCEPTION_ARGV, PendingAction,
};
use gitraptor_api::methods::{
    CAP_GUARD_EXCEPTION, GROUPS, GUARD_EXCEPTION, GUARD_EXCEPTION_CHILD, GUARD_EXCEPTION_END,
    GUARD_EXCEPTION_REFUSED, GUARD_UNINSTALL_REFUSED, RepoWrite, spec,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const SECRET: &str = "0123456789abcdef0123456789abcdef";

/// `value` is read into `T` and written back to the same JSON; one more field is refused.
fn same_both_ways<T: Serialize + DeserializeOwned + std::fmt::Debug>(name: &str, value: Value) {
    let read: T = serde_json::from_value(value.clone())
        .unwrap_or_else(|e| panic!("{name} should read {value}: {e}"));
    assert_eq!(
        serde_json::to_value(&read).unwrap(),
        value,
        "{name} should write back the JSON it read (names, omitted optionals)"
    );
    let mut extra = value;
    extra
        .as_object_mut()
        .expect("an object")
        .insert("unknown_field".into(), json!(1));
    assert!(
        serde_json::from_value::<T>(extra).is_err(),
        "{name} should refuse an unknown field"
    );
}

#[test]
fn exception_refusal_codes_are_kebab_case_both_ways() {
    let all = [
        (ExceptionRefusal::NotInstalled, "not-installed"),
        (ExceptionRefusal::OutdatedInstall, "outdated-install"),
        (ExceptionRefusal::AlreadyPending, "already-pending"),
        (ExceptionRefusal::WindowOpen, "window-open"),
        (ExceptionRefusal::NoPending, "no-pending"),
        (ExceptionRefusal::NotTheRequester, "not-the-requester"),
        (ExceptionRefusal::UnsupportedForm, "unsupported-form"),
        (
            ExceptionRefusal::UnsupportedOperation,
            "unsupported-operation",
        ),
        (ExceptionRefusal::NotDenied, "not-denied"),
        (ExceptionRefusal::NotExceptable, "not-exceptable"),
        (ExceptionRefusal::ChildNotOurs, "child-not-ours"),
    ];
    assert_eq!(all.len(), 11, "the contract has eleven refusals");
    for (refusal, code) in all {
        assert_eq!(
            serde_json::to_value(refusal).unwrap(),
            json!(code),
            "{refusal:?} should be written as {code}"
        );
        assert_eq!(
            serde_json::from_value::<ExceptionRefusal>(json!(code)).ok(),
            Some(refusal),
            "{code} should be read as {refusal:?}"
        );
    }
    same_both_ways::<GuardExceptionRefusedData>(
        "GuardExceptionRefusedData",
        json!({"reason": "window-open", "remainingMs": 5000}),
    );
    same_both_ways::<GuardExceptionRefusedData>(
        "GuardExceptionRefusedData without remainingMs",
        json!({"reason": "not-denied"}),
    );
}

#[test]
fn exception_params_and_results_have_the_documented_shape() {
    same_both_ways::<GuardExceptionParams>(
        "GuardExceptionParams",
        json!({"path": "/r", "argv": ["git", "push", "origin", "main"]}),
    );
    same_both_ways::<GuardExceptionParams>(
        "GuardExceptionParams with confirm",
        json!({"path": "/r", "argv": ["git", "rebase"], "confirm": "a1"}),
    );

    let plan = json!({
        "operation": {
            "kind": "rebase",
            "upstream": {"untrusted": "origin/main"},
            "branch": {"untrusted": "feat"}
        },
        "rules": ["permission.rebase", "minimum.force-push"]
    });
    same_both_ways::<gitraptor_api::guard::ExceptionPlanView>("ExceptionPlanView", plan.clone());

    same_both_ways::<GuardExceptionResult>(
        "GuardExceptionResult with only the plan",
        json!({"plan": plan}),
    );
    let pending = json!({
        "action_id": "a1",
        "action": "uninstall",
        "applies_at_ms": 5,
        "window_ms": 10
    });
    let issued = json!({
        "token": SECRET,
        "git": {"untrusted": "/usr/bin/git"},
        "config": ["core.hooksPath=/x"],
        "ttlMs": 60000
    });
    same_both_ways::<GuardExceptionResult>(
        "GuardExceptionResult",
        json!({"pending": pending, "plan": plan, "issued": issued}),
    );
    same_both_ways::<IssuedException>("IssuedException", issued);

    same_both_ways::<GuardExceptionChildParams>(
        "GuardExceptionChildParams",
        json!({"path": "/r", "actionId": "a1", "pid": 42}),
    );
    same_both_ways::<GuardExceptionEndParams>(
        "GuardExceptionEndParams",
        json!({"path": "/r", "actionId": "a1"}),
    );
    same_both_ways::<GuardExceptionEndResult>("GuardExceptionEndResult", json!({"consumed": true}));
}

#[test]
fn existing_types_gain_the_exception_fields_without_changing_shape() {
    // EvaluateParams: snake_case; `exception_token` only when present.
    let base = json!({
        "repo_id": "r1",
        "common_dir": "/c",
        "hook": "pre-rebase",
        "operation": {"kind": "rebase"}
    });
    same_both_ways::<EvaluateParams>("EvaluateParams of today", base.clone());
    let mut with_token = base;
    with_token["exception_token"] = json!(SECRET);
    same_both_ways::<EvaluateParams>("EvaluateParams with exception_token", with_token);

    // PendingAction: `exception` only when present.
    let today = json!({
        "action_id": "a1",
        "action": "uninstall",
        "applies_at_ms": 5,
        "window_ms": 10
    });
    same_both_ways::<PendingAction>("PendingAction of today", today.clone());
    let mut with_plan = today;
    with_plan["exception"] = json!({
        "operation": {"kind": "rebase"},
        "rules": ["minimum.force-push"]
    });
    same_both_ways::<PendingAction>("PendingAction with exception", with_plan);

    // GuardLogSummary: camelCase; the two counters are omitted at zero.
    let today = json!({"blocked": 1, "notices": 2, "rateLimited": 0});
    same_both_ways::<GuardLogSummary>("GuardLogSummary of today", today.clone());
    let mut with_counts = today.clone();
    with_counts["exceptions"] = json!(3);
    with_counts["exceptionsRejected"] = json!(1);
    same_both_ways::<GuardLogSummary>("GuardLogSummary with exceptions", with_counts);
    let mut zero = today.clone();
    zero["exceptions"] = json!(0);
    zero["exceptionsRejected"] = json!(0);
    let read: GuardLogSummary = serde_json::from_value(zero)
        .expect("GuardLogSummary should read exceptions and exceptionsRejected at zero");
    assert_eq!(
        serde_json::to_value(read).unwrap(),
        today,
        "GuardLogSummary should omit exceptions and exceptionsRejected at zero"
    );
}

#[test]
fn exception_token_is_redacted_in_debug_and_validated_on_read() {
    let token = ExceptionToken::from_secret(SECRET.to_owned())
        .expect("32 lowercase hex characters should be a token");
    assert_eq!(token.expose(), SECRET);
    assert!(
        !format!("{token:?}").contains(SECRET),
        "the Debug of ExceptionToken shows the token"
    );

    let bad = [
        "0123456789ABCDEF0123456789abcdef".to_owned(),
        SECRET[..31].to_owned(),
        format!("{SECRET}0"),
        "g123456789abcdef0123456789abcdef".to_owned(),
        String::new(),
    ];
    for text in &bad {
        assert!(
            ExceptionToken::from_secret(text.clone()).is_none(),
            "from_secret should refuse {text:?}"
        );
        assert!(
            serde_json::from_value::<ExceptionToken>(json!(text)).is_err(),
            "reading an ExceptionToken should refuse {text:?}"
        );
        let params = json!({
            "repo_id": "r1", "common_dir": "/c", "hook": "pre-rebase",
            "operation": {"kind": "rebase"}, "exception_token": text
        });
        assert!(
            serde_json::from_value::<EvaluateParams>(params).is_err(),
            "reading EvaluateParams.exception_token should refuse {text:?}"
        );
    }
    assert!(
        serde_json::from_value::<ExceptionToken>(json!(SECRET)).is_ok(),
        "reading a valid ExceptionToken"
    );

    // IssuedException: Debug hides it, JSON shows it once, in its own field.
    let issued = IssuedException {
        token,
        git: Untrusted::new("/usr/bin/git"),
        config: vec!["core.x=y".into()],
        ttl_ms: 60_000,
    };
    assert!(
        !format!("{issued:?}").contains(SECRET),
        "the Debug of IssuedException shows the token"
    );
    let value = serde_json::to_value(&issued).unwrap();
    assert_eq!(
        value["token"],
        json!(SECRET),
        "IssuedException writes the token in clear"
    );
    assert_eq!(
        value.to_string().matches(SECRET).count(),
        1,
        "the token appears only in its own field: {value}"
    );

    // EvaluateParams: Debug hides it, JSON shows it once, in its own field.
    let params: EvaluateParams = serde_json::from_value(json!({
        "repo_id": "r1", "common_dir": "/c", "hook": "pre-rebase",
        "operation": {"kind": "rebase"}, "exception_token": SECRET
    }))
    .expect("EvaluateParams should read exception_token");
    assert!(
        !format!("{params:?}").contains(SECRET),
        "the Debug of EvaluateParams shows the token"
    );
    let value = serde_json::to_value(&params).unwrap();
    assert_eq!(value["exception_token"], json!(SECRET));
    assert_eq!(
        value.to_string().matches(SECRET).count(),
        1,
        "only in its own field: {value}"
    );
}

#[test]
fn exception_methods_are_reserved_outside_mcp_and_without_since() {
    for (name, reserved) in [
        (GUARD_EXCEPTION, true),
        (GUARD_EXCEPTION_CHILD, true),
        (GUARD_EXCEPTION_END, false),
    ] {
        let m = spec(name).unwrap_or_else(|| panic!("{name} should be declared in a group"));
        assert_eq!(m.reserved, reserved, "{name}: reserved");
        assert!(!m.mcp, "{name} should not be offered to raptor-mcp");
        assert_eq!(m.writes, RepoWrite::None, "{name} does not write the repo");
        assert_eq!(
            m.since, CAPABILITIES_PROTOCOL,
            "{name} is discovered in hello.methods from the frozen CAPABILITIES_PROTOCOL, like every method added since"
        );
        assert_eq!(
            m.implemented_by,
            Some("US-GRD-006"),
            "{name} is declared ahead of its story until PR-B"
        );
    }
}

#[test]
fn the_exception_contract_is_declared_in_the_guard_group() {
    assert_eq!((MAX_EXCEPTION_ARGV, MAX_EXCEPTION_ARG_BYTES), (64, 4096));
    let guard = GROUPS
        .iter()
        .find(|g| g.module == "guard")
        .expect("the guard group");
    for name in [GUARD_EXCEPTION, GUARD_EXCEPTION_CHILD, GUARD_EXCEPTION_END] {
        let owners: Vec<_> = GROUPS
            .iter()
            .filter(|g| g.methods.iter().any(|m| m.name == name))
            .map(|g| g.module)
            .collect();
        assert_eq!(
            owners,
            ["guard"],
            "{name} should be declared only in the guard group"
        );
        assert!(
            guard.methods.iter().any(|m| m.name == name),
            "{name} in GROUP"
        );
    }
    let owners: Vec<_> = GROUPS
        .iter()
        .filter(|g| {
            g.capabilities
                .iter()
                .any(|c| c.name == CAP_GUARD_EXCEPTION.name)
        })
        .map(|g| g.module)
        .collect();
    assert_eq!(
        owners,
        ["guard"],
        "guard.exception should be declared only in the guard group"
    );
    assert_eq!(
        capability::all()
            .filter(|c| c.name == "guard.exception")
            .count(),
        1,
        "the capability guard.exception is in the contract exactly once"
    );
    assert_eq!(GUARD_EXCEPTION_REFUSED.name, "guard-exception-refused");
    assert_eq!(
        GUARD_EXCEPTION_REFUSED.code,
        GUARD_UNINSTALL_REFUSED.code - 2,
        "the error is BLOCK - 2"
    );
}
