//! The diff view on screen (DS-US-CKP-012, T007): both sections with their counts, the notices of
//! each kind of file, every state, no agent name, and the three snapshots at 80×24.

use gitraptor_api::Untrusted;
use gitraptor_api::UntrustedName;
use gitraptor_api::diff::{
    CommittedDiff, CommittedGap, DiffCut, DiffLineView, DiffSection, DiffUnavailable as Reason,
    FileBody, FileChange, FileDiff, Hunk, LineKind, WorktreeDiff,
};

use super::*;
use crate::model::DiffFailure;
use crate::present::diff::rows;
use crate::tui::diff::DiffState;

fn file(path: &str, change: FileChange, added: u32, removed: u32, body: FileBody) -> FileDiff {
    FileDiff {
        path: Untrusted::new(path),
        change,
        added,
        removed,
        body,
    }
}

fn hunk(lines: &[(LineKind, &str)]) -> Vec<Hunk> {
    vec![Hunk {
        old_start: 10,
        old_len: 2,
        new_start: 10,
        new_len: 3,
        lines: lines
            .iter()
            .map(|(kind, text)| DiffLineView {
                kind: *kind,
                text: Untrusted::new(*text),
            })
            .collect(),
    }]
}

/// Three kinds of change in the branch, a cut section and the notices of every special file.
fn sample() -> WorktreeDiff {
    WorktreeDiff {
        computed_at_utc_ms: 1_793_091_792_000,
        committed: CommittedDiff::Ready {
            base: UntrustedName::new("main"),
            section: DiffSection {
                files_total: 6,
                cut: Some(DiffCut::Files),
                files: vec![
                    file(
                        "src/pay.rs",
                        FileChange::Modified,
                        2,
                        1,
                        FileBody::Text {
                            truncated: false,
                            hunks: hunk(&[
                                (LineKind::Context, "fn pay() {"),
                                (LineKind::Removed, "\told();"),
                                (LineKind::Added, "\tcharge();\r"),
                                (LineKind::Added, "\tlog();"),
                            ]),
                        },
                    ),
                    file(
                        "big.txt",
                        FileChange::Modified,
                        2000,
                        0,
                        FileBody::Text {
                            truncated: true,
                            hunks: hunk(&[(LineKind::Added, "new line 1")]),
                        },
                    ),
                    file("logo.png", FileChange::Modified, 0, 0, FileBody::Binary {}),
                    file("data.bin", FileChange::Added, 0, 0, FileBody::TooLarge {}),
                    file("link", FileChange::TypeChanged, 0, 0, FileBody::Symlink {}),
                    file(
                        "vendor/lib",
                        FileChange::Added,
                        0,
                        0,
                        FileBody::Submodule {},
                    ),
                ],
            },
        },
        uncommitted: DiffSection {
            files_total: 1,
            cut: None,
            files: vec![file(
                "README.md",
                FileChange::Modified,
                1,
                1,
                FileBody::Text {
                    truncated: false,
                    hunks: hunk(&[
                        (LineKind::Removed, "shop"),
                        (LineKind::Added, "shop, edited"),
                    ]),
                },
            )],
        },
    }
}

fn open(lang: Lang, diff: &WorktreeDiff) -> Model {
    let mut model = shop_model(lang);
    model.ui.diff = Some(DiffState::Ready {
        worktree: SafeText::name("feat-pagos"),
        at_utc_ms: diff.computed_at_utc_ms,
        lines: rows(diff),
        offset: 0,
    });
    model
}

fn ascii(model: Model) -> Model {
    let theme = Theme::new(ColorMode::NoColor, Contrast::Normal, SymbolSet::Ascii);
    model.with_theme(theme)
}

#[test]
fn diff_in_english_spanish_and_ascii() {
    let mut settings = insta::Settings::clone_current();
    settings.set_prepend_module_to_snapshot(false);
    settings.set_snapshot_path("../../snapshots");
    settings.bind(|| {
        for (name, model) in [
            ("en", open(Lang::En, &sample())),
            ("es", open(Lang::Es, &sample())),
            ("ascii", ascii(open(Lang::En, &sample()))),
        ] {
            let screen = screen(&model, 80, 24);
            // No agent is named in the view, though `claude-1` is in the fleet behind it.
            assert!(!screen.contains("claude-1"), "{screen}");
            if name == "ascii" {
                assert!(screen.is_ascii(), "{screen}");
            }
            insta::assert_snapshot!(format!("diff_80x24_{name}"), screen);
        }
    });
}

#[test]
fn diff_sections_notices_and_title_read_in_both_languages() {
    for (lang, want) in [
        (
            Lang::En,
            [
                "feat-pagos · diff at 09:03:12 UTC",
                "would enter the merge (6 files)",
                "truncated: more than 1,000 files",
                "uncommitted (1 file)",
                "binary",
                "symbolic link",
                "submodule",
                "truncated: more than 2,000 lines",
                "truncated: more than 8 MiB",
            ],
        ),
        (
            Lang::Es,
            [
                "feat-pagos · diff a las 09:03:12 UTC",
                "entraría al merge (6 archivos)",
                "truncado: más de 1.000 archivos",
                "sin commitear (1 archivo)",
                "binario",
                "enlace simbólico",
                "submódulo",
                "truncado: más de 2.000 líneas",
                "truncado: más de 8 MiB",
            ],
        ),
    ] {
        let mut model = open(lang, &sample());
        // Scrolled to the end, so every row has been on screen at some point.
        let mut seen = String::new();
        for offset in 0..30 {
            if let Some(DiffState::Ready { offset: o, .. }) = &mut model.ui.diff {
                *o = offset;
            }
            seen.push_str(&screen(&model, 100, 24));
        }
        for text in want {
            assert!(seen.contains(text), "{lang:?} lacks \"{text}\"");
        }
    }
}

#[test]
fn diff_states_say_why_there_is_no_diff() {
    let worktree = SafeText::name("feat-pagos");
    let cases = [
        (
            DiffState::Loading {
                ticket: 1,
                worktree: worktree.clone(),
            },
            "calculando el diff…",
        ),
        (
            DiffState::Failed {
                worktree: worktree.clone(),
                failure: DiffFailure::Busy,
            },
            "otra consulta en curso; inténtalo en un momento",
        ),
        (
            DiffState::Failed {
                worktree: worktree.clone(),
                failure: DiffFailure::Unavailable(Reason::WorktreeUnavailable),
            },
            "el worktree no está disponible",
        ),
        (
            DiffState::Failed {
                worktree,
                failure: DiffFailure::Lost,
            },
            "se perdió la conexión; vuelve a pedirlo",
        ),
    ];
    for (state, text) in cases {
        let mut model = shop_model(Lang::Es);
        model.ui.diff = Some(state);
        let screen = screen(&model, 80, 24);
        assert!(screen.contains(text), "{screen}");
        assert!(screen.contains("feat-pagos · diff"), "{screen}");
    }
}

#[test]
fn diff_branch_gap_is_a_line_and_the_uncommitted_section_stays() {
    let mut diff = sample();
    diff.committed = CommittedDiff::NotComputable {
        reason: CommittedGap::BaseUnconfirmed,
    };
    let screen = screen(&open(Lang::Es, &diff), 80, 24);
    assert!(
        screen.contains("no calculable: base sin confirmar"),
        "{screen}"
    );
    assert!(screen.contains("sin commitear (1 archivo)"), "{screen}");
    assert!(!screen.contains("entraría al merge"), "{screen}");
}
