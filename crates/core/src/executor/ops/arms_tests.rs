//! T010 (D13, D14): `production()` wires the real engine and no real governed arm; the debug test
//! arms stay inert unless `GITRAPTOR_TEST_EXECUTOR_ARMS=1` and `GITRAPTOR_PROFILE_DIR` are both
//! set. Child module of `executor::ops`, attached there with
//! `#[cfg(test)] #[path = "arms_tests.rs"] mod arms_tests;`.

use super::*;

#[cfg(debug_assertions)]
const TEST_ARMS: [OperationId; 5] = [
    OperationId::Commit,
    OperationId::MergeIntoBase,
    OperationId::CreateWorktree,
    OperationId::DiscardWorktree,
    OperationId::RebaseOntoBase,
];

fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |name| {
        vars.iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| (*v).to_owned())
    }
}

fn armed(vars: &[(&str, &str)]) -> Vec<OperationId> {
    OperationsWiring::production_from(&env(vars))
        .catalog_armed()
        .to_vec()
}

#[test]
fn production_wires_the_engine_and_no_real_governed_arm() {
    for op in ARMS {
        assert!(entry(*op).governed.is_none(), "{op:?} is governed");
    }
    let wiring = OperationsWiring::production_from(&env(&[]));
    // The real engine decides; `NoGuardrails` does not.
    assert!(wiring.gate.decides());
    assert!(!NoGuardrails.decides());
    assert_eq!(wiring.catalog_armed(), [OperationId::Snapshot]);
}

#[test]
fn test_arms_are_inert_without_the_variable() {
    let profile = ("GITRAPTOR_PROFILE_DIR", "/tmp/profile");
    let arms = ("GITRAPTOR_TEST_EXECUTOR_ARMS", "1");
    for vars in [
        vec![],
        vec![arms],
        vec![profile],
        vec![arms, ("GITRAPTOR_PROFILE_DIR", "")],
        vec![("GITRAPTOR_TEST_EXECUTOR_ARMS", "0"), profile],
    ] {
        assert_eq!(armed(&vars), [OperationId::Snapshot], "{vars:?}");
    }
}

/// With both variables, and only in a build with `debug_assertions`, the five test arms are armed.
#[cfg(debug_assertions)]
#[test]
fn test_arms_are_armed_with_both_variables_in_a_debug_build() {
    let both = [
        ("GITRAPTOR_TEST_EXECUTOR_ARMS", "1"),
        ("GITRAPTOR_PROFILE_DIR", "/tmp/profile"),
    ];
    let armed = armed(&both);
    assert!(armed.contains(&OperationId::Snapshot));
    for op in TEST_ARMS {
        assert!(armed.contains(&op), "{op:?} not armed: {armed:?}");
    }
}

/// CKP003-RELEASE: without `debug_assertions` the variables never request the test arms; with it,
/// both variables do (so an always-false answer cannot pass).
#[test]
fn the_variable_is_ignored_without_debug_assertions() {
    let both = [
        ("GITRAPTOR_TEST_EXECUTOR_ARMS", "1"),
        ("GITRAPTOR_PROFILE_DIR", "/tmp/profile"),
    ];
    assert!(!OperationsWiring::test_arms_requested(false, &env(&both)));
    assert!(!OperationsWiring::test_arms_requested(false, &env(&[])));
    assert!(OperationsWiring::test_arms_requested(true, &env(&both)));
}

/// CKP003-RELEASE: a build without `debug_assertions` never arms the test arms, whatever the env.
#[cfg(not(debug_assertions))]
#[test]
fn a_release_build_never_arms_the_test_arms() {
    let both = [
        ("GITRAPTOR_TEST_EXECUTOR_ARMS", "1"),
        ("GITRAPTOR_PROFILE_DIR", "/tmp/x"),
    ];
    let wiring = OperationsWiring::production_from(&env(&both));
    let armed = wiring.catalog_armed();
    for op in armed {
        assert!(entry(*op).governed.is_none(), "{op:?} is governed");
    }
    assert_eq!(armed, [OperationId::Snapshot], "test arms armed in release");
}
