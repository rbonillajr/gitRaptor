//! TMC012-CLI-TEXT / TMC012-SEC-SANITIZE (T006 of DS-US-TMC-012): the lines `raptor undo` prints
//! for an `overlap` / `overlap-unverified` rejection and for the paths an undo kept, in English
//! and Spanish. Child module of `undo::overlap` (`use super::*`), against `rejection_lines` and
//! `kept_lines`.

use super::*;
use crate::i18n::{Tr, has_key};
use gitraptor_api::Untrusted;
use gitraptor_api::actor::{Actor, AgentKind, AgentOrigin};
use gitraptor_api::timemachine::{
    KeptPath, OverlapConflict, OverlapSubject, OverlapUndone, TmOverlap, TmRejectReason,
};

const SEEN_MS: i64 = 1_700_000_300_000;
const EN: Tr = Tr { spanish: false };
const ES: Tr = Tr { spanish: true };

fn claude() -> Actor {
    Actor::Agent {
        kind: AgentKind::ClaudeCode,
        name: None,
        origin: AgentOrigin::Detected,
    }
}

fn path_conflict(path: &str, actor: Actor) -> OverlapConflict {
    OverlapConflict {
        subject: OverlapSubject::Path {
            worktree: Untrusted::new("feat-login"),
            path: Untrusted::new(path),
        },
        actor,
        seen_utc_ms: SEEN_MS,
    }
}

fn detail(conflicts: Vec<OverlapConflict>, total: u32) -> TmOverlap {
    TmOverlap {
        undone: OverlapUndone {
            id: "op-1".into(),
            subtype: Some(Untrusted::new("reset-hard")),
            actor: claude(),
            at_utc_ms: Some(SEEN_MS - 300_000),
        },
        conflicts,
        total,
    }
}

fn kept(path: &str) -> KeptPath {
    KeptPath {
        worktree: Untrusted::new("feat-login"),
        path: Untrusted::new(path),
    }
}

fn has_clock_time(text: &str) -> bool {
    text.as_bytes().windows(8).any(|w| {
        w.iter().enumerate().all(|(i, c)| match i {
            2 | 5 => *c == b':',
            _ => c.is_ascii_digit(),
        })
    })
}

fn lines_with(lines: &[String], needle: &str) -> usize {
    lines.iter().filter(|l| l.contains(needle)).count()
}

fn nothing_changed(tr: Tr) -> &'static str {
    if tr.spanish {
        "nada cambió"
    } else {
        "nothing changed"
    }
}

#[test]
fn the_new_keys_exist_in_both_catalogs() {
    for key in ["undo.reason.overlap", "undo.reason.overlap-unverified"] {
        assert!(has_key(key), "{key}");
    }
}

#[test]
fn an_overlap_shows_the_reason_the_undone_change_and_one_line_per_conflict() {
    for tr in [EN, ES] {
        let lines = rejection_lines(
            tr,
            TmRejectReason::Overlap,
            Some(&detail(
                vec![
                    path_conflict("src/api.rs", Actor::Unattributed),
                    OverlapConflict {
                        subject: OverlapSubject::Ref {
                            name: Untrusted::new("feat-pagos"),
                        },
                        actor: claude(),
                        seen_utc_ms: SEEN_MS,
                    },
                ],
                2,
            )),
        );
        let all = lines.join("\n");
        let spanish = tr.spanish;

        assert!(
            all.to_lowercase().contains(nothing_changed(tr)),
            "{spanish}: reason and 'nothing changed': {all}"
        );
        assert!(
            all.contains("reset-hard"),
            "{spanish}: the undone change: {all}"
        );
        assert_eq!(lines_with(&lines, "src/api.rs"), 1, "{spanish}: {all}");
        assert_eq!(lines_with(&lines, "feat-pagos"), 1, "{spanish}: {all}");
        assert!(all.contains("feat-login"), "{spanish}: the worktree: {all}");
        let api_line = lines.iter().find(|l| l.contains("src/api.rs")).unwrap();
        assert!(
            api_line.contains(&tr.t("events.no_agent", &[])),
            "{spanish}: unattributed reads 'no agent'/'sin agente': {api_line}"
        );
        assert!(has_clock_time(&all), "{spanish}: a local HH:MM:SS: {all}");
    }
}

#[test]
fn only_twenty_conflicts_are_shown_with_the_real_total() {
    for tr in [EN, ES] {
        let conflicts: Vec<_> = (0..50)
            .map(|i| path_conflict(&format!("src/f{i:02}.rs"), Actor::Unattributed))
            .collect();
        let lines = rejection_lines(tr, TmRejectReason::Overlap, Some(&detail(conflicts, 80)));
        let all = lines.join("\n");

        assert_eq!(lines_with(&lines, "src/f"), 20, "{all}");
        assert!(
            all.contains("src/f19.rs") && !all.contains("src/f20.rs"),
            "{all}"
        );
        assert!(
            all.contains("+60"),
            "'+K' from the real total (80 - 20): {all}"
        );
        let more = if tr.spanish { "más" } else { "more" };
        assert!(all.contains(more), "{all}");
    }
}

#[test]
fn an_unverified_overlap_has_no_list() {
    for tr in [EN, ES] {
        let lines = rejection_lines(tr, TmRejectReason::OverlapUnverified, None);
        let all = lines.join("\n");

        assert_eq!(lines.len(), 1, "{all}");
        assert!(all.to_lowercase().contains(nothing_changed(tr)), "{all}");
        assert!(!all.contains(".rs") && !has_clock_time(&all), "{all}");
    }
}

#[test]
fn kept_paths_are_announced_with_their_number_and_a_list_of_twenty() {
    for tr in [EN, ES] {
        let few = kept_lines(tr, &[kept("b.rs"), kept("c.rs")]);
        let all = few.join("\n");
        assert!(all.contains('2'), "the number of paths: {all}");
        assert_eq!(lines_with(&few, "b.rs"), 1, "{all}");
        assert_eq!(lines_with(&few, "c.rs"), 1, "{all}");

        let many: Vec<_> = (0..35).map(|i| kept(&format!("src/k{i:02}.rs"))).collect();
        let lines = kept_lines(tr, &many);
        let all = lines.join("\n");
        assert_eq!(lines_with(&lines, "src/k"), 20, "{all}");
        assert!(all.contains("35"), "the real number: {all}");
        assert!(all.contains("+15"), "{all}");

        assert!(kept_lines(tr, &[]).is_empty(), "nothing kept, nothing said");
    }
}

#[test]
fn no_line_after_an_overlap_suggests_continuing_or_forcing() {
    for tr in [EN, ES] {
        let mut lines = rejection_lines(
            tr,
            TmRejectReason::Overlap,
            Some(&detail(
                vec![path_conflict("src/api.rs", Actor::Unattributed)],
                1,
            )),
        );
        lines.extend(rejection_lines(tr, TmRejectReason::OverlapUnverified, None));
        let lower = lines.join("\n").to_lowercase();
        for word in [
            "force",
            "--yes",
            "continue",
            "anyway",
            "override",
            "forzar",
            "continuar",
            "de todos modos",
            "sobrescribir",
        ] {
            assert!(!lower.contains(word), "{word:?} in: {lower}");
        }
    }
}

#[test]
fn paths_with_control_characters_are_printed_sanitized() {
    let hostile = "src/\u{1b}[31mred\u{7}\nfake line.rs";
    for tr in [EN, ES] {
        let mut lines = rejection_lines(
            tr,
            TmRejectReason::Overlap,
            Some(&detail(
                vec![
                    path_conflict(hostile, Actor::Unattributed),
                    OverlapConflict {
                        subject: OverlapSubject::Ref {
                            name: Untrusted::new("feat\u{1b}]0;title\u{7}-x\nforged"),
                        },
                        actor: Actor::Unattributed,
                        seen_utc_ms: SEEN_MS,
                    },
                ],
                2,
            )),
        );
        let kept_out = kept_lines(tr, &[kept(hostile)]);
        for (what, out) in [("rejection_lines", &lines), ("kept_lines", &kept_out)] {
            let visible = out.iter().any(|l| {
                l.contains("src/") && l.contains("red") && !l.chars().any(char::is_control)
            });
            assert!(
                visible,
                "{what}: the conflict line with the sanitized path is missing: {out:?}"
            );
        }
        lines.extend(kept_out);

        for line in &lines {
            assert!(
                !line.chars().any(char::is_control),
                "a control character reaches the terminal: {line:?}"
            );
            assert!(
                !line.trim_start().starts_with("fake line")
                    && !line.trim_start().starts_with("forged"),
                "a newline in a name forged a line: {line:?}"
            );
        }
    }
}
