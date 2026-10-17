//! US-GRD-020: the first line of `raptor guard status` says in one line whether the repo is
//! protected, E1 of the story, in English and Spanish (an explicit `Tr`, not the process language).

use gitraptor_api::Untrusted;
use gitraptor_api::guard::{
    GuardStatus, HooksLayer, HooksStatus, LossCause, MinimumSet, MinimumSetStatus, Permission,
    ProtectionState,
};

use super::status_lines;
use crate::i18n::Tr;

const EN: Tr = Tr { spanish: false };
const ES: Tr = Tr { spanish: true };

fn status(base_confirmed: bool, bases: &[&str], hooks: HooksLayer) -> GuardStatus {
    GuardStatus {
        repo_id: "r".into(),
        state: ProtectionState::HooksOnly,
        permission: Permission::Granted,
        offer: false,
        protected_bases: bases.iter().map(|b| Untrusted::new(*b)).collect(),
        base_confirmed,
        not_preventable: Vec::new(),
        last_refusal: Vec::new(),
        misnamed_settings: Vec::new(),
        pending: None,
        hooks: Some(hooks),
        diagnostics: Vec::new(),
        minimum_set: Some(MinimumSet {
            status: MinimumSetStatus::Active,
        }),
    }
}

fn active() -> HooksLayer {
    HooksLayer::of(HooksStatus::Active)
}

/// GRD020-STATE-CONFIRMED
#[test]
fn a_confirmed_base_reads_protected_with_the_base() {
    let s = status(true, &["main"], active());

    let en = status_lines(&s, EN);
    assert_eq!(en[0], "state: protected, base branch main");
    let es = status_lines(&s, ES);
    assert_eq!(es[0], "estado: protegido, rama base main");

    for (lines, forbidden) in [
        (&en, ["hooks only", "protected base branch"]),
        (&es, ["solo hooks", "rama base protegida"]),
    ] {
        for line in lines.iter() {
            for word in forbidden {
                assert!(!line.contains(word), "{word} in {line}");
            }
        }
    }
}

/// GRD020-STATE-UNCONFIRMED
#[test]
fn an_unconfirmed_base_names_the_branches_protected_meanwhile() {
    let s = status(false, &["main", "master"], active());

    assert_eq!(
        status_lines(&s, EN)[0],
        "state: protected, base branch not confirmed (protected meanwhile: main, master)"
    );
    assert_eq!(
        status_lines(&s, ES)[0],
        "estado: protegido, rama base sin confirmar (mientras tanto se protegen: main, master)"
    );
}

/// GRD020-STATE-PARTIAL
#[test]
fn inactive_hooks_read_partly_protected_never_protected() {
    let s = status(
        true,
        &["main"],
        HooksLayer::lost(LossCause::HookspathChanged),
    );

    let en = status_lines(&s, EN);
    assert_eq!(
        en[0],
        "state: partly protected: the hooks are not active (details below)"
    );
    let es = status_lines(&s, ES);
    assert_eq!(
        es[0],
        "estado: protección incompleta: los hooks no están activos (detalle abajo)"
    );
    // Never "protected" as the state, and the lines after still explain the cause.
    assert!(!en[0].starts_with("state: protected"), "{}", en[0]);
    assert!(!es[0].starts_with("estado: protegido"), "{}", es[0]);
    assert!(
        en[1..].iter().any(|l| l.contains("no longer active")),
        "{en:?}"
    );
    assert!(
        es[1..].iter().any(|l| l.contains("ya no está activa")),
        "{es:?}"
    );
}
