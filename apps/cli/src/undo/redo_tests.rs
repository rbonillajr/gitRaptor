//! TMC003-CLI-TEXT (T006 of DS-US-TMC-003): the texts of `raptor redo`, in English and Spanish,
//! with their markers. Child module of `undo::redo`, mounted there as `mod tests`.
//!
//! Needs: the `redo.` group of `apps/cli/i18n/{en,es}/timemachine.txt` (not written yet).

use gitraptor_api::timemachine::TmRejectReason;

use crate::i18n::{has_key, text_in};

fn placeholders(text: &str) -> Vec<&str> {
    let mut out: Vec<&str> = text
        .split('{')
        .skip(1)
        .filter_map(|rest| rest.split_once('}').map(|(name, _)| name))
        .collect();
    out.sort_unstable();
    out
}

/// Every value of `TmRejectReason`. The `match` has no wildcard: a value added later does not
/// compile here until it is listed, so it cannot go without its redo text.
fn every_reason() -> Vec<TmRejectReason> {
    use TmRejectReason::*;
    let all = vec![
        NothingToUndo,
        RawGitNotCovered,
        TargetUnavailable,
        OtherActor,
        ConfirmationRequired,
        GitOperationInProgress,
        GitBusy,
        RepoBusy,
        RepoUntrusted,
        WorktreeUnavailable,
        InvalidSnapshot,
        HostileTree,
        RefMoved,
        RefInUse,
        Unsupported,
        GitUnavailable,
        ChallengeInvalid,
        ConfirmationUnavailable,
        Overlap,
        OverlapUnverified,
        NotSelectable,
        SelectiveUndoBelow,
        AttributionUnavailable,
        NothingToRedo,
        RedoInvalidated,
    ];
    for reason in &all {
        match reason {
            NothingToUndo
            | RawGitNotCovered
            | TargetUnavailable
            | OtherActor
            | ConfirmationRequired
            | GitOperationInProgress
            | GitBusy
            | RepoBusy
            | RepoUntrusted
            | WorktreeUnavailable
            | InvalidSnapshot
            | HostileTree
            | RefMoved
            | RefInUse
            | Unsupported
            | GitUnavailable
            | ChallengeInvalid
            | ConfirmationUnavailable
            | Overlap
            | OverlapUnverified
            | NotSelectable
            | SelectiveUndoBelow
            | AttributionUnavailable
            | NothingToRedo
            | RedoInvalidated => {}
        }
    }
    all
}

/// Every rejection reason has its redo message in English and Spanish, under its wire code.
#[test]
fn every_redo_rejection_has_a_message() {
    for reason in every_reason() {
        let wire = serde_json::to_value(reason).unwrap();
        let key = format!("redo.reason.{}", wire.as_str().unwrap());
        assert!(
            has_key(&key),
            "TMC003-CLI-TEXT: {key} must exist in en and es"
        );
        for spanish in [false, true] {
            let text = text_in(spanish, &key).expect(&key);
            assert!(
                !text.trim().is_empty(),
                "TMC003-CLI-TEXT: {key} (es={spanish}) is empty"
            );
        }
    }
}

#[test]
fn redo_done_and_prior_carry_their_markers() {
    let expected: [(&str, &[&str]); 2] = [
        ("redo.done", &["id", "worktree"]),
        ("redo.prior", &["snapshot"]),
    ];
    for (key, markers) in expected {
        assert!(
            has_key(key),
            "TMC003-CLI-TEXT: {key} must exist in en and es"
        );
        for spanish in [false, true] {
            let text = text_in(spanish, key).expect(key);
            assert_eq!(
                placeholders(text),
                markers,
                "TMC003-CLI-TEXT: {key} (es={spanish}) must carry exactly its markers"
            );
        }
    }
}

#[test]
fn nothing_to_redo_and_redo_invalidated_say_nothing_changed() {
    for key in [
        "redo.reason.nothing-to-redo",
        "redo.reason.redo-invalidated",
    ] {
        for (spanish, phrase) in [(false, "nothing changed"), (true, "nada cambió")] {
            let text = text_in(spanish, key)
                .unwrap_or_else(|| panic!("TMC003-CLI-TEXT: {key} must exist (es={spanish})"));
            assert!(
                text.contains(phrase),
                "TMC003-CLI-TEXT: {key} (es={spanish}) must say `{phrase}`: {text}"
            );
        }
    }
}
