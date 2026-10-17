//! US-GRD-020: a denial for a missing recovery point names the operation that was denied, E2 and
//! E3 of the story in English and Spanish, and asks to repeat that same operation.

use gitraptor_api::guard::{Cause, Level, Reason, Rule};

use super::snapshot_required::{DeniedOp, reason_text};
use crate::i18n::Tr;

const EN: Tr = Tr { spanish: false };
const ES: Tr = Tr { spanish: true };

/// The service is not running: the cause the hooks report without the daemon.
fn unavailable() -> Reason {
    Reason {
        rule: Rule::SnapshotFailed,
        level: Level::System,
        cause: Some(Cause::SnapshotUnavailable),
        params: Vec::new(),
    }
}

fn branch(name: &str) -> Option<String> {
    Some(name.to_owned())
}

/// GRD020-DENY-DELETE
#[test]
fn a_branch_delete_denied_without_the_service_matches_e2() {
    let denied = || DeniedOp::BranchDelete {
        branch: branch("feat-x"),
    };

    let en = reason_text(&unavailable(), denied(), EN);
    assert_eq!(
        en.trim_end(),
        [
            "GitRaptor: branch feat-x was not deleted: a recovery point could not be saved first (GitRaptor is not running or no longer observes this repository).",
            "To go on: start GitRaptor (`raptor daemon status` starts it) and delete the branch again; if it is already running, observe the repo again with raptor repo add.",
        ]
        .join("\n")
    );
    let es = reason_text(&unavailable(), denied(), ES);
    assert_eq!(
        es.trim_end(),
        [
            "GitRaptor: no se borró la rama feat-x: no se pudo guardar antes un punto de recuperación (GitRaptor no está en marcha o ya no observa este repositorio).",
            "Para seguir: arranca GitRaptor (`raptor daemon status` lo arranca) y vuelve a borrar la rama; si ya está en marcha, vuelve a observar el repo con raptor repo add.",
        ]
        .join("\n")
    );
    // Nothing about the push, the remote or the minimum set.
    for (text, words) in [
        (&en, ["push", "remote", "minimum"]),
        (&es, ["push", "remoto", "mínimo"]),
    ] {
        let lower = text.to_lowercase();
        for word in words {
            assert!(!lower.contains(word), "{word} in {text}");
        }
    }
}

/// GRD020-DENY-REBASE
#[test]
fn a_rebase_denied_without_the_service_matches_e3() {
    let named = || DeniedOp::Rebase {
        branch: branch("feat-x"),
    };

    assert_eq!(
        reason_text(&unavailable(), named(), EN).trim_end(),
        [
            "GitRaptor: the rebase of feat-x did not run: a recovery point could not be saved first (GitRaptor is not running or no longer observes this repository).",
            "To go on: start GitRaptor (`raptor daemon status` starts it) and run the rebase again; if it is already running, observe the repo again with raptor repo add.",
        ]
        .join("\n")
    );
    assert_eq!(
        reason_text(&unavailable(), named(), ES).trim_end(),
        [
            "GitRaptor: no se ejecutó el rebase de feat-x: no se pudo guardar antes un punto de recuperación (GitRaptor no está en marcha o ya no observa este repositorio).",
            "Para seguir: arranca GitRaptor (`raptor daemon status` lo arranca) y vuelve a lanzar el rebase; si ya está en marcha, vuelve a observar el repo con raptor repo add.",
        ]
        .join("\n")
    );

    // Without a known branch it says "the rebase".
    let unnamed = || DeniedOp::Rebase { branch: None };
    let en = reason_text(&unavailable(), unnamed(), EN);
    assert!(en.starts_with("GitRaptor: the rebase did not run:"), "{en}");
    let es = reason_text(&unavailable(), unnamed(), ES);
    assert!(
        es.starts_with("GitRaptor: no se ejecutó el rebase:"),
        "{es}"
    );
}

/// GRD020-DENY-PUSH
#[test]
fn a_push_denied_without_the_service_still_asks_to_push_again() {
    let en = reason_text(&unavailable(), DeniedOp::Push, EN);
    assert!(en.contains("and push again"), "{en}");
    assert!(
        !en.contains("delete the branch") && !en.contains("rebase"),
        "{en}"
    );

    let es = reason_text(&unavailable(), DeniedOp::Push, ES);
    assert!(es.contains("vuelve a hacer el push"), "{es}");
    assert!(
        !es.contains("borrar la rama") && !es.contains("rebase"),
        "{es}"
    );
}
