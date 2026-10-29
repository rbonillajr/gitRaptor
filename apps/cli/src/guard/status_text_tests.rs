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
        confirm: None,
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

/// The four states in both languages: `mcp-only` and `full` name themselves; a `full` repo keeps
/// the base line of its hooks half.
#[test]
fn the_four_states_read_in_both_languages() {
    let with = |state| GuardStatus {
        state,
        ..status(true, &["main"], active())
    };
    for (state, en, es) in [
        (
            ProtectionState::Unprotected,
            "state: unprotected",
            "estado: sin protección",
        ),
        (
            ProtectionState::HooksOnly,
            "state: protected, base branch main",
            "estado: protegido, rama base main",
        ),
        (
            ProtectionState::McpOnly,
            "state: MCP only",
            "estado: solo MCP",
        ),
        (ProtectionState::Full, "state: full", "estado: completa"),
    ] {
        assert_eq!(status_lines(&with(state), EN)[0], en, "{state:?}");
        assert_eq!(status_lines(&with(state), ES)[0], es, "{state:?}");
    }
    assert_eq!(
        status_lines(&with(ProtectionState::Full), EN)[1],
        "state: protected, base branch main"
    );
}

/// A repo in the MCP allowlist whose hooks were replaced reads `mcp-only` (the hooks half is the
/// active one, BR-WF-002), with the diagnostic right after the state and the recovery action.
#[test]
fn mcp_only_with_inactive_hooks_shows_both() {
    let s = GuardStatus {
        state: ProtectionState::McpOnly,
        ..status(
            true,
            &["main"],
            HooksLayer::lost(LossCause::DispatcherMissing),
        )
    };
    for (tr, state, inactive) in [
        (EN, "state: MCP only", "hooks: inactive ("),
        (ES, "estado: solo MCP", "hooks: inactivos ("),
    ] {
        let lines = status_lines(&s, tr);
        assert_eq!(lines[0], state, "{lines:#?}");
        assert!(lines[1].starts_with(inactive), "{lines:#?}");
        assert!(
            lines.iter().any(|l| l.contains("raptor guard install")),
            "the recovery action: {lines:#?}"
        );
    }
}

/// An `mcp-only` repo is covered by the MCP: it never reads as unprotected, and the permission
/// line is the one of a repo without hooks.
#[test]
fn an_mcp_only_repo_is_not_offered_as_unprotected() {
    let s = GuardStatus {
        state: ProtectionState::McpOnly,
        hooks: None,
        ..status(true, &["main"], active())
    };
    for tr in [EN, ES] {
        let lines = status_lines(&s, tr);
        let unprotected = tr.t("guard.state.unprotected", &[]);
        assert!(lines.iter().all(|l| *l != unprotected), "{lines:#?}");
        assert!(
            lines
                .iter()
                .all(|l| *l != tr.t("guard.permission.granted", &[])),
            "{lines:#?}"
        );
    }
}

/// Without a known cause the diagnostic has no empty parentheses.
#[test]
fn an_inactive_hooks_line_without_a_cause_has_no_parentheses() {
    let mut hooks = HooksLayer::lost(LossCause::DispatcherMissing);
    hooks.cause = None;
    let s = GuardStatus {
        state: ProtectionState::McpOnly,
        ..status(true, &["main"], hooks)
    };
    assert_eq!(status_lines(&s, EN)[1], "hooks: inactive");
    assert_eq!(status_lines(&s, ES)[1], "hooks: inactivos");
}
