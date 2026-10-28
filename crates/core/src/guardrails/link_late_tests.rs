//! D6 for invalid input: a direct child of a governed plan that sends parameters `guard.evaluate`
//! rejects is denied, and the plan closes with that denial (it is kept in `late`).

use super::*;
use crate::channel::requester::Who;
use crate::profile::ProfileDirs;
use gitraptor_api::catalog::Layer;
use gitraptor_api::guard::{ExceptionState, Hook, RefUpdate, RefValue};
use std::sync::{Arc, Mutex};

fn allowed() -> Decision {
    Decision {
        decision_id: "plan".into(),
        effect: Effect::Allow,
        applied_effect: Effect::Allow,
        reasons: vec![],
        exception: ExceptionState::None,
        config_status: vec![],
        config_ref: vec![],
        notices: vec![],
        prior_snapshot: None,
    }
}

#[test]
fn an_invalid_request_of_a_direct_child_is_kept_in_late() {
    let registered: Arc<[Expected]> = Arc::from(Vec::new());
    let binding = Arc::new(PlanBinding {
        plan_id: "plan-1".into(),
        decision: allowed(),
        layer: Layer::Cockpit,
        who: Who::unattributed(),
        registered: Arc::clone(&registered),
        late: Mutex::new(None),
    });
    let child = ChildLink {
        operation_id: "op-1".into(),
        who: Who::unattributed(),
        binding: Some(Arc::clone(&binding)),
        transitions: registered,
    };
    // A relative common directory and an empty repo id: rejected before any read.
    let params = EvaluateParams {
        exception_token: None,
        repo_id: String::new(),
        common_dir: "relative/.git".into(),
        hook: Hook::ReferenceTransaction,
        operation: Operation::RefTransaction {
            updates: vec![RefUpdate {
                refname: "refs/heads/feat".into(),
                old: RefValue::Zero,
                new: RefValue::Zero,
            }],
            orphan_head: None,
        },
        authorship: None,
    };
    let tmp = tempfile::tempdir().unwrap();
    let registry = GuardRegistry::for_profile(ProfileDirs::under_root(tmp.path().join("p")));
    let served = serve_child(&registry, &params, &child, None);
    assert_eq!(served.decision.applied_effect, Effect::Deny);
    assert_eq!(served.decision.reasons[0].rule, Rule::InputRejected);
    let late = binding.late.lock().unwrap();
    let kept = late.as_ref().expect("the denial is kept for the plan");
    assert_eq!(kept.applied_effect, Effect::Deny);
    assert_eq!(kept.reasons[0].rule, Rule::InputRejected);
}
