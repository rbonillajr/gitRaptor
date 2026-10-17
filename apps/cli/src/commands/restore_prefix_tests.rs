//! US-TMC-023: the messages of `raptor restore` for a prefix that does not name one restore
//! point: E3 of the story in English and Spanish (rendered with an explicit `Tr`), the cap of 5
//! candidates with "+K", and the text of a prefix under 6 characters.

use gitraptor_api::messages::GitEventKind;
use gitraptor_api::timemachine::{AmbiguousCandidate, AmbiguousData, EntryOrigin};
use gitraptor_api::untrusted::{Untrusted, UntrustedName};

use super::ambiguous_text;
use crate::i18n::Tr;

const EN: Tr = Tr { spanish: false };
const ES: Tr = Tr { spanish: true };

/// UTC milliseconds of 2026-10-17 09:20 and 2026-10-16 22:41 (offset 0).
const OCT_17_0920: i64 = 1_792_228_800_000;
const OCT_16_2241: i64 = 1_792_190_460_000;

fn candidate(id: &str, at: i64, worktree: &str, kind: GitEventKind, branch: &str) -> AmbiguousCandidate {
    AmbiguousCandidate {
        snapshot_id: format!("{id}-0000-4000-8000-000000000001"),
        origin: EntryOrigin::GitEvent {
            seq: 1,
            kind,
            branch: Some(UntrustedName::new(branch)),
        },
        worktrees: vec![Untrusted::new(worktree)],
        occurred_utc_ms: at,
        utc_offset_s: 0,
    }
}

fn two() -> AmbiguousData {
    AmbiguousData {
        candidates: vec![
            candidate("47bfc884", OCT_17_0920, "/w/uxtl", GitEventKind::Rebase, "feat/ux-timeline-status"),
            candidate("47bfc8e1", OCT_16_2241, "/w/main", GitEventKind::Commit, "main"),
        ],
        total: 2,
    }
}

/// TMC023-AMBIGUOUS-TEXT: E3 in both languages.
#[test]
fn the_ambiguous_prefix_message_matches_e3_in_en_and_es() {
    assert_eq!(
        ambiguous_text("47bfc8", &two(), EN),
        "47bfc8 matches 2 restore points: 47bfc884 (2026-10-17 09:20, uxtl, rebase of feat/ux-timeline-status), 47bfc8e1 (2026-10-16 22:41, main, commit on main); type more characters; nothing changed"
    );
    assert_eq!(
        ambiguous_text("47bfc8", &two(), ES),
        "47bfc8 coincide con 2 puntos de restauración: 47bfc884 (2026-10-17 09:20, uxtl, rebase de feat/ux-timeline-status), 47bfc8e1 (2026-10-16 22:41, main, commit en main); escribe más caracteres; nada cambió"
    );
}

/// TMC023-AMBIGUOUS-TEXT: at most 5 candidates are listed and "+K" counts the rest.
#[test]
fn more_than_five_candidates_are_capped_with_the_rest_counted() {
    let many = AmbiguousData {
        candidates: (0..5)
            .map(|i| candidate(&format!("47bfc80{i}"), OCT_17_0920, "/w/uxtl", GitEventKind::Commit, "main"))
            .collect(),
        total: 8,
    };

    let en = ambiguous_text("47bfc8", &many, EN);
    assert!(en.starts_with("47bfc8 matches 8 restore points: "), "{en}");
    assert_eq!(en.matches("47bfc80").count(), 5, "{en}");
    assert!(en.contains("+3"), "{en}");
    let es = ambiguous_text("47bfc8", &many, ES);
    assert!(es.starts_with("47bfc8 coincide con 8 puntos de restauración: "), "{es}");
    assert_eq!(es.matches("47bfc80").count(), 5, "{es}");
    assert!(es.contains("+3"), "{es}");
}

/// TMC023-AMBIGUOUS-TEXT: a prefix under 6 characters has its own text in both languages.
#[test]
fn a_prefix_under_four_characters_has_its_own_text() {
    assert_eq!(
        EN.t("restore.too-short", &[]),
        "type at least 6 characters of the id that raptor timeline shows"
    );
    assert_eq!(
        ES.t("restore.too-short", &[]),
        "escribe al menos 6 caracteres del id que muestra raptor timeline"
    );
}
