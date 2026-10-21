//! The plan of `raptor guard confirm` names each branch that stops being protected, sanitized
//! (US-GRD-014, D4 amended).

use gitraptor_api::UntrustedName;
use gitraptor_api::guard::{BaseChangeView, ConfirmPlanView};

use super::confirm::plan_lines;

#[test]
fn the_plan_names_each_branch_that_stops_being_protected() {
    let plan = ConfirmPlanView {
        source: None,
        base: BaseChangeView {
            from: Some(UntrustedName::new("main")),
            to: UntrustedName::new("main"),
            unguarded: vec![UntrustedName::new("mas\u{1b}]0;x\u{7}ter")],
        },
        relaxations: Vec::new(),
        relaxes: true,
    };
    let lines = plan_lines("demo", &plan);
    let shown: Vec<&String> = lines.iter().filter(|l| l.contains("mas")).collect();
    assert_eq!(shown.len(), 1, "{lines:#?}");
    assert!(
        !shown[0].contains('\u{1b}') && !shown[0].contains('\u{7}'),
        "{shown:?}"
    );

    let none = ConfirmPlanView {
        base: BaseChangeView {
            unguarded: Vec::new(),
            ..plan.base.clone()
        },
        ..plan
    };
    assert_eq!(plan_lines("demo", &none).len(), lines.len() - 1);
}
