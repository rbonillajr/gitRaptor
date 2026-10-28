//! The texts of `raptor agent correct` and `uncorrect` (US-GRP-010, T005). Child module of
//! `agent`, attached there with
//! `#[cfg(test)] #[path = "agent_correction_tests.rs"] mod correction_tests;`.

use gitraptor_api::attribution::AttributionRejection;

use crate::i18n::{has_key, text_in};
use crate::status::wire;

/// GRP010-CLI-MSG: every reason the engine can give has its text in English and in Spanish, in
/// the `correction.reason.*` group, and so do the success, the withdrawal, the hint and the
/// reserved refusal; the error code has its name in `contract.txt`.
#[test]
fn every_correction_rejection_has_a_message() {
    // An exhaustive match: a new reason fails to compile here until it gets its text.
    let all = [
        AttributionRejection::NotAWorktree,
        AttributionRejection::RepoNotObserved,
        AttributionRejection::NoDetectedAttribution,
        AttributionRejection::RegisteredSession,
        AttributionRejection::AmbiguousSession,
        AttributionRejection::UnknownSession,
        AttributionRejection::SameAgent,
        AttributionRejection::NotCorrected,
    ];
    for reason in all {
        match reason {
            AttributionRejection::NotAWorktree
            | AttributionRejection::RepoNotObserved
            | AttributionRejection::NoDetectedAttribution
            | AttributionRejection::RegisteredSession
            | AttributionRejection::AmbiguousSession
            | AttributionRejection::UnknownSession
            | AttributionRejection::SameAgent
            | AttributionRejection::NotCorrected => {}
        }
        let key = format!("correction.reason.{}", wire(&reason));
        assert!(has_key(&key), "{key} is missing in en or es");
        for spanish in [false, true] {
            let text = text_in(spanish, &key).unwrap_or_default();
            assert!(!text.trim().is_empty(), "{key} is empty (es: {spanish})");
        }
    }
    for key in [
        "correction.done",
        "correction.withdrawn",
        "correction.hint-uncorrect",
        "correction.reserved",
        "error.attribution-rejected",
    ] {
        assert!(has_key(key), "{key} is missing in en or es");
    }
    assert_eq!(
        text_in(false, "correction.reserved"),
        Some("only the developer can correct an attribution")
    );
    assert_eq!(
        text_in(true, "correction.reserved"),
        Some("solo el desarrollador puede corregir una atribución")
    );
}
