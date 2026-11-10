//! T004 of DS-US-CKP-006: the fleet row with ⚡, the conflict panel, the limits line and the
//! `?` help. The whole screen is rendered (`TestBackend`) after feeding `update` with the
//! snapshot, the capability and the pages, so the names under test are only the DS's.
//!
//! The English texts of the panel are not fixed by the DS (only the Spanish ones are), so
//! English checks stick to names, paths, ranges, symbols and `.gitattributes`.

use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, RepoStateView, RepoView as ApiRepo, SessionStateView,
    SessionsListResult, WorktreeView,
};
use gitraptor_api::prediction::{
    ConflictFileView, ConflictKindView, ConflictView, LimitCode, LineRange, MergeBaseKind,
    PairDetail, PairLevel, PairSideView, PairStateView, PairSummary, PredictionPage,
    PredictionSummary,
};
use gitraptor_api::scope::{RepoSnapshot, ScopeSnapshot};
use gitraptor_api::{Untrusted, UntrustedName};
use gitraptor_theme::{ColorMode, Contrast, SymbolSet, Theme};
use ratatui::crossterm::event::KeyCode;
use unicode_width::UnicodeWidthStr;

use super::tests::{engine, key, lines, model_with, ready, render, session, worktree};
use crate::model::{ConnEvent, EngineMsg, Model, Msg};
use crate::present::i18n::Lang;
use crate::tui::update::update;

const MAIN: &str = "/w/shop";
const A: &str = "/w/shop/feat-a";
const B: &str = "/w/shop/feat-b";
const C: &str = "/w/shop/feat-c";
const ES_LIMITS: &str = "⚠ con lo sin commitear · ⚡ solo commits · sin drivers ni .gitattributes";

fn worktrees() -> Vec<WorktreeView> {
    vec![
        worktree(MAIN, true, ready("main", 0, 0, 0)),
        worktree(A, false, ready("feat-a", 3, 0, 0)),
        worktree(B, false, ready("feat-b", 2, 0, 0)),
        worktree(C, false, ready("feat-c", 1, 0, 0)),
    ]
}

fn side(root: Option<&str>) -> PairSideView {
    match root {
        Some(root) => PairSideView::Worktree {
            root: Untrusted::new(root),
        },
        None => PairSideView::Base {},
    }
}

fn pair(
    id: &str,
    a: Option<&str>,
    b: Option<&str>,
    state: PairStateView,
    level: Option<PairLevel>,
) -> PairSummary {
    PairSummary {
        pair_id: id.into(),
        rev: 2,
        a: side(a),
        b: side(b),
        state,
        reason: None,
        level,
        computed_at_utc_ms: Some(1),
        overlap_files: 1,
        overlap_partial: false,
        conflict_files: u32::from(level == Some(PairLevel::Conflict)),
        conflict_truncated: false,
    }
}

fn conflict(id: &str, a: Option<&str>, b: Option<&str>) -> PairSummary {
    pair(id, a, b, PairStateView::Current, Some(PairLevel::Conflict))
}

fn overlap(id: &str, a: Option<&str>, b: Option<&str>) -> PairSummary {
    pair(id, a, b, PairStateView::Current, Some(PairLevel::Overlap))
}

fn ranges(starts: &[u32]) -> Vec<LineRange> {
    starts
        .iter()
        .map(|&start| LineRange { start, len: 3 })
        .collect()
}

fn file(path: &str, ours: &[u32], theirs: &[u32], rule: bool) -> ConflictFileView {
    ConflictFileView {
        path: Untrusted::new(path),
        kind: ConflictKindView::Content,
        ours: ranges(ours),
        theirs: ranges(theirs),
        hunks_truncated: false,
        has_merge_rule: rule,
    }
}

fn detail(summary: PairSummary, overlap: &[&str], files: Vec<ConflictFileView>) -> PairDetail {
    let conflict = (!files.is_empty()).then_some(ConflictView {
        merge_base: MergeBaseKind::Single,
        files,
        truncated: false,
        rewrites_escalated: false,
    });
    PairDetail {
        summary,
        overlap: overlap.iter().map(|p| Untrusted::new(*p)).collect(),
        conflict,
    }
}

/// The shop with claude-1 Active in feat-a, claude-2 Inactive in feat-b and nobody in feat-c,
/// with the publication and, when `capability`, the capability of `prediction.pairs`.
fn world(lang: Lang, capability: bool, pairs: Vec<PairSummary>, details: Vec<PairDetail>) -> Model {
    let mut model = model_with(lang, worktrees(), None);
    update(
        &mut model,
        engine(EngineMsg::Snapshot(Box::new(ScopeSnapshot::Repo(
            RepoSnapshot {
                run_id: "run".into(),
                scope_seq: 2,
                repo: ApiRepo {
                    fetched_utc_ms: None,
                    repo_id: "r1".into(),
                    state: RepoStateView::Observed,
                    path: Untrusted::new("/w/shop/.git"),
                    base: BaseBranchView {
                        name: Some(UntrustedName::new("main")),
                        status: BaseStatusView::Confirmed,
                    },
                    worktrees: worktrees(),
                    tier: None,
                    checked_utc_ms: None,
                    kept_temps: None,
                    prediction: capability.then_some(PredictionSummary {
                        limits: LimitCode::ALL.to_vec(),
                        pairs,
                        pairs_truncated: false,
                    }),
                },
            },
        )))),
    );
    update(
        &mut model,
        engine(EngineMsg::Sessions {
            repo_id: "r1".into(),
            result: Box::new(SessionsListResult {
                detection_available: true,
                sessions: vec![
                    session("s1", A, "claude-1", SessionStateView::Active),
                    session("s2", B, "claude-2", SessionStateView::Inactive),
                ],
            }),
        }),
    );
    update(&mut model, Msg::Conn(ConnEvent::Prediction(capability)));
    if !details.is_empty() {
        update(
            &mut model,
            engine(EngineMsg::PredictionPage {
                repo_id: "r1".into(),
                page: Box::new(PredictionPage {
                    limits: LimitCode::ALL.to_vec(),
                    pairs: details,
                    next_cursor: None,
                }),
            }),
        );
    }
    model
}

fn themed(model: Model, symbols: SymbolSet) -> Model {
    model.with_theme(Theme::new(ColorMode::NoColor, Contrast::Normal, symbols))
}

fn screen(model: &Model, width: u16, height: u16) -> Vec<String> {
    lines(&render(model, width, height))
}

fn at(screen: &[String], needle: &str) -> Option<usize> {
    screen.iter().position(|l| l.contains(needle))
}

/// The text of a screen line between the borders.
fn inner(line: &str) -> &str {
    line.trim_matches(|c: char| " ┃│┏┓┗┛┌┐└┘━─|".contains(c))
}

/// The fleet row of a worktree: the only line with its change count (`~N`).
fn fleet_row<'s>(screen: &'s [String], changes: &str) -> Option<&'s String> {
    screen.iter().find(|l| l.contains(changes))
}

/// The lines of a pair: both sides and the "versus" glyph.
fn pair_line<'s>(screen: &'s [String], names: &[&str], versus: &str) -> Option<&'s String> {
    screen
        .iter()
        .find(|l| l.contains(versus) && names.iter().all(|n| l.contains(n)))
}

fn demo_pairs() -> (Vec<PairSummary>, Vec<PairDetail>) {
    let c1 = conflict("c1", Some(A), Some(B));
    let c2 = conflict("c2", Some(B), None);
    let w1 = overlap("w1", Some(C), None);
    let mut pairs = vec![w1.clone(), c1.clone(), c2.clone()];
    // Six more ⚠ so that not every pair fits in the panel.
    let mut details = vec![
        detail(w1, &["README.md"], vec![]),
        detail(
            c1,
            &["src/api.rs"],
            vec![
                file(
                    "src/api.rs",
                    &[10, 20, 30, 40, 50],
                    &[11, 21, 31, 41, 51],
                    true,
                ),
                file("src/util.rs", &[5], &[6], false),
            ],
        ),
        detail(c2, &["lib.rs"], vec![file("lib.rs", &[1], &[2], false)]),
    ];
    for i in 0..6 {
        let extra = overlap(&format!("x{i}"), Some(C), Some(B));
        details.push(detail(extra.clone(), &["CHANGELOG.md"], vec![]));
        pairs.push(extra);
    }
    (pairs, details)
}

// ------------------------------------------------------------ CKP006-SIDES

#[test]
fn sides_take_the_row_name_and_the_base_branch() {
    let pairs = vec![
        conflict("p1", Some(A), Some(B)),
        conflict("p2", Some(A), None),
        overlap("p3", Some(C), Some(A)),
    ];
    let details = pairs
        .iter()
        .map(|p| detail(p.clone(), &["x.rs"], vec![file("x.rs", &[1], &[2], false)]))
        .collect();
    let model = world(Lang::En, true, pairs, details);
    let s = screen(&model, 80, 24);
    let text = s.join("\n");
    assert!(
        pair_line(&s, &["claude-1", "claude-2"], "↔").is_some(),
        "the sides carry the agent names of their rows, joined by ↔:\n{text}"
    );
    assert!(
        pair_line(&s, &["claude-1", "main"], "↔").is_some(),
        "the base side carries the base branch name:\n{text}"
    );
    assert!(
        pair_line(&s, &["feat-c", "claude-1"], "↔").is_some(),
        "a row without agent is named as its row is:\n{text}"
    );
}

#[test]
fn an_unmatched_root_falls_back_to_its_folder() {
    let pairs = vec![conflict("p1", Some(A), Some("/w/elsewhere/orphan-tree"))];
    let details = vec![detail(
        pairs[0].clone(),
        &["x.rs"],
        vec![file("x.rs", &[1], &[2], false)],
    )];
    let s = screen(&world(Lang::En, true, pairs, details), 80, 24);
    assert!(
        pair_line(&s, &["claude-1", "orphan-tree"], "↔").is_some(),
        "a root without a row is named by its folder, never empty:\n{}",
        s.join("\n")
    );
}

// ------------------------------------------------------------ CKP006-CAPABILITY

#[test]
fn without_the_capability_there_is_no_panel() {
    let (pairs, details) = demo_pairs();
    // Control: with the capability the same data shows the panel and the ⚡ on the rows.
    let with = screen(
        &world(Lang::En, true, pairs.clone(), details.clone()),
        80,
        24,
    );
    assert!(
        at(&with, ".gitattributes").is_some() && at(&with, "↔").is_some(),
        "with the capability the panel shows pairs and limits:\n{}",
        with.join("\n")
    );
    let without = screen(&world(Lang::En, false, pairs, details), 80, 24);
    let text = without.join("\n");
    assert!(at(&without, "↔").is_none(), "no pair line:\n{text}");
    assert!(
        at(&without, ".gitattributes").is_none(),
        "no limits line without the capability:\n{text}"
    );
    for changes in ["~3", "~2", "~1"] {
        let row = fleet_row(&without, changes).expect("a fleet row");
        assert!(!row.contains('⚡'), "no ⚡ on the rows: {row}");
    }
}

#[test]
fn with_the_capability_and_no_pairs_only_the_limits_line_shows() {
    for lang in [Lang::En, Lang::Es] {
        let s = screen(&world(lang, true, vec![], vec![]), 80, 24);
        let text = s.join("\n");
        let limits = at(&s, ".gitattributes")
            .unwrap_or_else(|| panic!("the limits line shows with 0 pairs ({lang:?}):\n{text}"));
        assert!(at(&s, "↔").is_none(), "no pair lines ({lang:?}):\n{text}");
        // Nothing else of the panel above the limits line besides its frame.
        let above = &s[limits.saturating_sub(1)];
        assert!(
            !above.contains("claude") && !above.contains("src/"),
            "only the limits line: {above}"
        );
        if lang == Lang::Es {
            assert!(text.contains(ES_LIMITS), "{text}");
        }
    }
}

// ------------------------------------------------------------ CKP006-ROW-ATTENTION

#[test]
fn row_with_a_known_conflict_shows_the_bolt_and_rises_to_attention() {
    use PairLevel::{Clean, Conflict, Overlap};
    use PairStateView::{Calculating, Current, Pending, Recalculating};
    // (capability, state, level, known)
    let cases = [
        (true, Current, Some(Conflict), true),
        (true, Recalculating, Some(Conflict), true),
        (true, Calculating, None, false),
        (true, Pending, None, false),
        (true, Current, Some(Overlap), false),
        (true, Current, Some(Clean), false),
        (false, Current, Some(Conflict), false),
    ];
    for (capability, state, level, known) in cases {
        let pairs = vec![pair("p1", Some(B), Some(C), state, level)];
        let s = screen(&world(Lang::En, capability, pairs, vec![]), 80, 24);
        let text = s.join("\n");
        let a = s.iter().position(|l| l.contains("~3")).unwrap();
        let b = s.iter().position(|l| l.contains("~2")).unwrap();
        let c = s.iter().position(|l| l.contains("~1")).unwrap();
        let label = format!("cap={capability} {state:?} {level:?}");
        assert_eq!(
            s[b].contains('⚡'),
            known,
            "{label}: ⚡ on feat-b's row:\n{text}"
        );
        assert_eq!(
            s[c].contains('⚡'),
            known,
            "{label}: ⚡ on feat-c's row:\n{text}"
        );
        assert!(
            !s[a].contains('⚡'),
            "{label}: feat-a is in no pair:\n{text}"
        );
        // Active (feat-a) goes before Inactive (feat-b) unless the conflict lifts feat-b.
        assert_eq!(
            b < a,
            known,
            "{label}: feat-b rises only with a known ⚡:\n{text}"
        );
    }
}

// ------------------------------------------------------------ CKP006-LIMITS

#[test]
fn limits_line_fits_78_columns_in_both_languages_and_ascii() {
    let (pairs, details) = demo_pairs();
    for (lang, symbols) in [
        (Lang::En, SymbolSet::Unicode),
        (Lang::Es, SymbolSet::Unicode),
        (Lang::En, SymbolSet::Ascii),
        (Lang::Es, SymbolSet::Ascii),
    ] {
        let model = themed(world(lang, true, pairs.clone(), details.clone()), symbols);
        let s = screen(&model, 80, 24);
        let label = format!("{lang:?} {symbols:?}");
        let line = s
            .iter()
            .find(|l| l.contains(".gitattributes"))
            .unwrap_or_else(|| panic!("{label}: no limits line:\n{}", s.join("\n")));
        let text = inner(line);
        assert!(
            text.width() <= 78,
            "{label}: the limits line is {} columns: {text}",
            text.width()
        );
        if symbols == SymbolSet::Ascii {
            assert!(text.is_ascii() || lang == Lang::Es, "{label}: {text}");
        } else if lang == Lang::Es {
            assert_eq!(text, ES_LIMITS, "{label}");
        }
    }
}

#[test]
fn help_lists_the_eight_limits() {
    for lang in [Lang::En, Lang::Es] {
        let closed = world(lang, true, vec![], vec![]);
        let mut open = closed.clone();
        update(&mut open, key(KeyCode::Char('?')));
        let mut bare = world(lang, false, vec![], vec![]);
        update(&mut bare, key(KeyCode::Char('?')));
        let help = screen(&open, 100, 45);
        let base = screen(&bare, 100, 45);
        let filled = |s: &[String]| s.iter().filter(|l| !inner(l).is_empty()).count();
        assert!(
            filled(&help) >= filled(&base) + 8,
            "{lang:?}: `?` adds the 8 limits to the help ({} vs {} lines):\n{}",
            filled(&help),
            filled(&base),
            help.join("\n")
        );
        if lang == Lang::Es {
            let title = at(&help, "Límites de la predicción")
                .unwrap_or_else(|| panic!("the group of the limits:\n{}", help.join("\n")));
            let group: Vec<&str> = help[title + 1..]
                .iter()
                // The graph panel under the help shows to the left of the modal's border.
                .map(|l| inner(l).rsplit('┃').next().unwrap_or_default().trim())
                .filter(|l| !l.is_empty())
                .take(8)
                .collect();
            assert_eq!(group.len(), 8, "{group:?}");
            for text in &group {
                assert!(
                    text.width() <= 60,
                    "a limit text is at most 60 columns: {text}"
                );
            }
            assert!(
                group[4].contains("Sin ⚡ el merge aún puede tener conflictos"),
                "`insertions-may-merge` is the fifth, in the fixed order: {group:#?}"
            );
        }
    }
}

// ------------------------------------------------------------ CKP006-SANITIZE

#[test]
fn pair_and_file_names_are_sanitized() {
    let evil_root = "/w/elsewhere/orphan\u{1b}[2J\u{1b}]0;x\u{7}tree";
    let pairs = vec![conflict("p1", Some(A), Some(evil_root))];
    let details = vec![detail(
        pairs[0].clone(),
        &["src/\u{1b}]52;c;eA==\u{7}evil.rs"],
        vec![file("src/\u{1b}]52;c;eA==\u{7}evil.rs", &[1], &[2], false)],
    )];
    let model = world(Lang::En, true, pairs, details);
    let buffer = render(&model, 80, 24);
    assert!(
        !buffer
            .content()
            .iter()
            .any(|c| c.symbol().chars().any(char::is_control)),
        "a control character reached the terminal"
    );
    let text = lines(&buffer).join("\n");
    assert!(
        text.contains("evil.rs"),
        "the path is painted, sanitized:\n{text}"
    );
    assert!(
        text.contains("orphan"),
        "the folder is painted, sanitized:\n{text}"
    );
}

// ------------------------------------------------------------ CKP006-SNAPSHOT

fn panel_checks(s: &[String]) {
    let text = s.join("\n");
    let limits = at(s, ".gitattributes").unwrap_or_else(|| panic!("no limits line:\n{text}"));
    assert!(
        s.iter()
            .enumerate()
            .all(|(i, l)| !l.contains("src/api.rs") || i < limits),
        "the limits line is below every file:\n{text}"
    );
    let last_pair = s.iter().rposition(|l| l.contains("↔") || l.contains("<->"));
    assert!(
        last_pair.is_some_and(|p| p < limits),
        "pairs above the limits:\n{text}"
    );
    assert!(
        text.contains("src/api.rs"),
        "the ⚡ file is listed:\n{text}"
    );
}

#[test]
fn conflicts_80x24_en() {
    let (pairs, details) = demo_pairs();
    let s = screen(&world(Lang::En, true, pairs, details), 80, 24);
    panel_checks(&s);
    let text = s.join("\n");
    let first_bolt = s
        .iter()
        .position(|l| l.contains('⚡') && l.contains('↔') && l.contains("claude"));
    let first_warn = s.iter().position(|l| l.contains('⚠') && l.contains('↔'));
    assert!(
        first_bolt.is_some() && first_bolt < first_warn.or(Some(usize::MAX)),
        "⚡ pairs before ⚠ pairs although the engine listed ⚠ first:\n{text}"
    );
    assert!(text.contains("10–12 ↔ 11–13"), "range per side:\n{text}");
    assert!(
        !text.contains("40–42"),
        "at most 3 ranges per side:\n{text}"
    );
    assert!(text.contains("+2"), "the rest of the ranges as +N:\n{text}");
    assert!(
        s.iter().any(|l| l.contains('+') && l.contains("pairs")),
        "pairs that do not fit end in a `+N … pairs` row:\n{text}"
    );
}

#[test]
fn conflicts_80x24_es() {
    let (pairs, details) = demo_pairs();
    let s = screen(&world(Lang::Es, true, pairs, details), 80, 24);
    panel_checks(&s);
    let text = s.join("\n");
    assert!(text.contains(ES_LIMITS), "{text}");
    assert!(text.contains("10–12 ↔ 11–13"), "range per side:\n{text}");
    assert!(
        !text.contains("40–42"),
        "at most 3 ranges per side:\n{text}"
    );
    assert_eq!(
        text.matches("regla de merge").count(),
        1,
        "the merge-rule mark is only on the ⚡ file that has one:\n{text}"
    );
    assert!(
        text.lines()
            .any(|l| l.contains("+") && l.contains(" pares más")),
        "`+N pares más` when they do not fit:\n{text}"
    );
}

#[test]
fn conflicts_80x24_ascii() {
    let (pairs, details) = demo_pairs();
    let model = themed(world(Lang::En, true, pairs, details), SymbolSet::Ascii);
    let s = screen(&model, 80, 24);
    panel_checks(&s);
    let text = s.join("\n");
    assert!(
        text.contains("<->"),
        "`<->` joins the sides in ASCII:\n{text}"
    );
    assert!(
        !text.contains('↔') && !text.contains('⚡') && !text.contains('⚠'),
        "no Unicode symbol in the ASCII set:\n{text}"
    );
}
