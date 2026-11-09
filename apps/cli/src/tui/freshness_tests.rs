use gitraptor_api::prediction::{PairLevel, PairReason, PairStateView};

use super::*;
use crate::present::i18n::Lang;
use crate::tui::prediction::Side;

const T0: i64 = 1_000_000;

fn row(
    state: PairStateView,
    reason: Option<PairReason>,
    level: Option<PairLevel>,
    computed: Option<i64>,
) -> PairRow {
    PairRow {
        pair_id: "p".into(),
        rev: 1,
        a: Side::Base,
        b: Side::Base,
        state,
        reason,
        level,
        computed_at_utc_ms: computed,
        overlap_files: 0,
        overlap_partial: false,
        conflict_files: 0,
        conflict_truncated: false,
    }
}

fn current(level: PairLevel) -> PairRow {
    row(PairStateView::Current, None, Some(level), Some(T0))
}

fn text(t: FreshnessText) -> String {
    t.render(Lang::Es)
}

#[test]
fn age_is_never_negative() {
    assert_eq!(age_of(T0 - 5_000, T0), (0, AgeUnit::Seconds));
    let t = row_text(&current(PairLevel::Conflict), T0 - 5_000).unwrap();
    assert_eq!(text(t), "hace 0 s");
}

#[test]
fn age_units_follow_p4() {
    let cases = [
        (0, "hace 0 s", "0 s ago"),
        (59_000, "hace 59 s", "59 s ago"),
        (60_000, "hace 1 min", "1 min ago"),
        (59 * 60_000, "hace 59 min", "59 min ago"),
        (3_600_000, "hace 1 h", "1 h ago"),
        (23 * 3_600_000, "hace 23 h", "23 h ago"),
        (86_400_000, "hace 1 d", "1 d ago"),
    ];
    for (ms, es, en) in cases {
        let (n, unit) = age_of(T0 + ms, T0);
        let t = FreshnessText::Age { n, unit };
        assert_eq!(t.render(Lang::Es), es);
        assert_eq!(t.render(Lang::En), en);
    }
}

#[test]
fn maps_every_state_and_reason_to_prediction() {
    use PairStateView as S;
    let at = Some(T0);
    for level in [None, Some(PairLevel::Clean), Some(PairLevel::Conflict)] {
        let cases = [
            (S::Current, Prediction::Current),
            (S::Recalculating, Prediction::Stale),
            (S::Calculating, Prediction::Calculating),
        ];
        for (state, want) in cases {
            // A `current` result without a level says nothing to show: it counts as calculating.
            let want = if state == S::Current && level.is_none() {
                Prediction::Calculating
            } else {
                want
            };
            assert_eq!(prediction_of(&row(state, None, level, at)), want);
        }
    }
    // A `pending` without a reason is not given one (E4).
    assert_eq!(
        prediction_of(&row(S::Pending, None, None, None)),
        Prediction::Calculating
    );
    // E4: no time, no result.
    for state in [S::Current, S::Recalculating] {
        let r = row(state, None, Some(PairLevel::Clean), None);
        assert_eq!(prediction_of(&r), Prediction::Calculating);
        assert!(!listed(&r));
    }
    for reason in REASON_ORDER {
        let pending = prediction_of(&row(S::Pending, Some(reason), None, None));
        let want = if reason == PairReason::BaseUnconfirmed {
            Prediction::PendingBase
        } else {
            Prediction::Pending
        };
        assert_eq!(pending, want, "{reason:?}");
        assert_eq!(
            prediction_of(&row(S::NotComputable, Some(reason), None, None)),
            Prediction::NotComputable
        );
    }
}

#[test]
fn pairs_without_result_never_say_no_conflicts() {
    use PairStateView as S;
    for state in [
        S::Calculating,
        S::Recalculating,
        S::Pending,
        S::NotComputable,
    ] {
        for reason in REASON_ORDER.map(Some).into_iter().chain([None]) {
            for level in [None, Some(PairLevel::Clean), Some(PairLevel::Conflict)] {
                for computed in [None, Some(T0)] {
                    let p = row(state, reason, level, computed);
                    for lang in [Lang::Es, Lang::En] {
                        let all: Vec<_> = summary(&[current(PairLevel::Clean), p.clone()], T0)
                            .into_iter()
                            .map(|(_, t)| t.render(lang))
                            .collect();
                        assert!(
                            all.iter()
                                .all(|l| !l.contains("Sin conflictos")
                                    && !l.contains("No conflicts")),
                            "{state:?} {reason:?} {level:?}: {all:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn recalculating_without_marks_has_a_row() {
    let r = row(
        PairStateView::Recalculating,
        None,
        Some(PairLevel::Clean),
        Some(T0 - 40_000),
    );
    assert!(listed(&r));
    assert_eq!(
        text(row_text(&r, T0).unwrap()),
        "desactualizado · hace 40 s · recalculando"
    );
    assert!(!listed(&current(PairLevel::Clean)));
    assert!(listed(&current(PairLevel::Overlap)));
    assert!(row_rank(&current(PairLevel::Conflict)) < row_rank(&current(PairLevel::Overlap)));
    assert!(row_rank(&current(PairLevel::Overlap)) < row_rank(&r));
}

#[test]
fn pending_and_not_computable_group_by_reason_with_singular_and_plural() {
    let pend = |r| row(PairStateView::Pending, Some(r), None, None);
    let mut pairs = vec![pend(PairReason::BaseUnconfirmed); 4];
    pairs.push(pend(PairReason::OperationInProgress));
    pairs.push(row(
        PairStateView::NotComputable,
        Some(PairReason::Shallow),
        None,
        None,
    ));
    let lines: Vec<String> = summary(&pairs, T0)
        .into_iter()
        .map(|(_, t)| text(t))
        .collect();
    assert_eq!(
        lines,
        [
            "Pendiente: confirma la rama base (4 pares)",
            "Pendiente: operación en curso (1 par)",
            "No calculable: el clon es superficial (1 par)",
        ]
    );
}

#[test]
fn clear_only_when_every_pair_is_current_without_marks() {
    let mut old = current(PairLevel::Clean);
    old.computed_at_utc_ms = Some(T0 - 120_000);
    let pairs = [current(PairLevel::Clean), old];
    let lines = summary(&pairs, T0 + 180_000);
    assert_eq!(lines.len(), 1);
    assert_eq!(text(lines[0].1), "Sin conflictos previstos · hace 5 min");
    for blocker in [
        current(PairLevel::Overlap),
        current(PairLevel::Conflict),
        row(
            PairStateView::Recalculating,
            None,
            Some(PairLevel::Clean),
            Some(T0),
        ),
        row(PairStateView::Calculating, None, None, None),
    ] {
        let with = [current(PairLevel::Clean), blocker];
        assert!(
            summary(&with, T0)
                .iter()
                .all(|(_, t)| !matches!(t, FreshnessText::Clear { .. }))
        );
    }
}

#[test]
fn no_clear_line_without_pairs() {
    assert!(summary(&[], T0).is_empty());
}

#[test]
fn calculating_line_singular_and_plural() {
    let calc = || row(PairStateView::Calculating, None, None, None);
    let one = summary(&[calc(), current(PairLevel::Conflict)], T0);
    assert_eq!(text(one[0].1), "Calculando 1 de 2 pares");
    let single = summary(&[calc()], T0);
    assert_eq!(text(single[0].1), "Calculando 1 de 1 par");
    let en = summary(&[calc(), calc(), calc()], T0)[0].1.render(Lang::En);
    assert_eq!(en, "Calculating 3 of 3 pairs");
    assert!(has_ages(&[current(PairLevel::Clean)]));
    assert!(!has_ages(&[calc()]));
}
