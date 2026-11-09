//! US-CKP-008: the alert of a new predicted conflict, driven only through the public model
//! (snapshot, `prediction.changed`, `prediction.list` pages, `Msg::Tick`). No daemon, no Git.
//! The strings are the contract of `ConflictAlertText` (es/en) and of the toast widget.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use gitraptor_api::Untrusted;
use gitraptor_api::event::Event;
use gitraptor_api::messages::{
    BaseBranchView, BaseStatusView, RepoStateView, RepoView as ApiRepo, ResyncReason,
    UnavailableReason, WorktreeStatus, WorktreeView,
};
use gitraptor_api::methods::PREDICTION_CHANGED;
use gitraptor_api::prediction::{
    ConflictFileView, ConflictKindView, ConflictView, LimitCode, LineRange, MergeBaseKind,
    PairDetail, PairLevel, PairSideView, PairStateView, PairSummary, PredictionChanged,
    PredictionPage, PredictionSummary,
};
use gitraptor_api::scope::{RepoSnapshot, Scope, ScopeSnapshot};
use gitraptor_cli::model::{ConnState, EngineMsg, Model, Msg, Size, Stamped};
use gitraptor_cli::present::i18n::Lang;
use gitraptor_cli::tui::gallery::buffer_text;
use gitraptor_cli::tui::update::update;
use gitraptor_cli::tui::view::view;
use gitraptor_theme::{ColorMode, Contrast, SymbolSet, Theme};
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::layout::Rect;
use ratatui::{Terminal, TerminalOptions, Viewport};

const REPO: &str = "r1";
const T0: i64 = 1_000_000_000;
const BEL_WT: usize = 9;
const TOAST_ES: &str = "⚡ Conflicto previsto: claude-1 ↔ claude-2 en src/api.rs";
const TOAST_EN: &str = "⚡ Predicted conflict: claude-1 ↔ claude-2 in src/api.rs";

fn engine(msg: EngineMsg) -> Msg {
    Msg::Engine(Stamped {
        recv_ns: 0,
        decoded_ns: 0,
        msg,
    })
}

fn root(i: usize) -> String {
    match i {
        0 => "/w/shop".into(),
        BEL_WT => "/w/shop/bel\u{7}9".into(),
        _ => format!("/w/shop/claude-{i}"),
    }
}

fn wt(i: usize) -> WorktreeView {
    WorktreeView {
        last_activity_utc_ms: None,
        last_activity_in_gap: false,
        detached_at: None,
        operation: None,
        unmerged_paths: 0,
        changes_actor: None,
        path: Untrusted::new(root(i)),
        main: i == 0,
        admin_name: None,
        status: WorktreeStatus::Unavailable {
            reason: UnavailableReason::Missing,
        },
    }
}

fn pair(
    id: &str,
    rev: u64,
    a: usize,
    b: Option<usize>,
    state: PairStateView,
    level: Option<PairLevel>,
    files: u32,
) -> PairSummary {
    PairSummary {
        pair_id: id.into(),
        rev,
        a: PairSideView::Worktree {
            root: Untrusted::new(root(a)),
        },
        b: match b {
            Some(i) => PairSideView::Worktree {
                root: Untrusted::new(root(i)),
            },
            None => PairSideView::Base {},
        },
        state,
        reason: None,
        level,
        computed_at_utc_ms: level.map(|_| T0),
        overlap_files: 0,
        overlap_partial: false,
        conflict_files: if level == Some(PairLevel::Conflict) {
            files
        } else {
            0
        },
        conflict_truncated: false,
    }
}

/// `current` with level conflict and `files` conflicting files.
fn bolt_n(id: &str, rev: u64, a: usize, b: Option<usize>, files: u32) -> PairSummary {
    pair(
        id,
        rev,
        a,
        b,
        PairStateView::Current,
        Some(PairLevel::Conflict),
        files,
    )
}

fn bolt(id: &str, rev: u64, a: usize, b: usize) -> PairSummary {
    bolt_n(id, rev, a, Some(b), 1)
}

fn clean(id: &str, rev: u64, a: usize, b: usize) -> PairSummary {
    pair(
        id,
        rev,
        a,
        Some(b),
        PairStateView::Current,
        Some(PairLevel::Clean),
        0,
    )
}

fn recalculating(id: &str, rev: u64, a: usize, b: usize) -> PairSummary {
    pair(
        id,
        rev,
        a,
        Some(b),
        PairStateView::Recalculating,
        Some(PairLevel::Conflict),
        1,
    )
}

fn calculating(id: &str, rev: u64, a: usize, b: usize) -> PairSummary {
    pair(id, rev, a, Some(b), PairStateView::Calculating, None, 0)
}

fn other_state(id: &str, rev: u64, a: usize, b: usize, state: PairStateView) -> PairSummary {
    pair(id, rev, a, Some(b), state, None, 0)
}

/// The detail of a conflicting pair: its files, first one `src/api.rs`.
fn detail(summary: PairSummary, files: &[&str]) -> PairDetail {
    PairDetail {
        summary,
        overlap: vec![],
        conflict: Some(ConflictView {
            merge_base: MergeBaseKind::Single,
            files: files
                .iter()
                .map(|p| ConflictFileView {
                    path: Untrusted::new(*p),
                    kind: ConflictKindView::Content,
                    ours: vec![LineRange { start: 12, len: 7 }],
                    theirs: vec![LineRange { start: 14, len: 7 }],
                    hunks_truncated: false,
                    has_merge_rule: false,
                })
                .collect(),
            truncated: false,
            rewrites_escalated: false,
        }),
    }
}

fn page(details: Vec<PairDetail>) -> Msg {
    engine(EngineMsg::PredictionPage {
        repo_id: REPO.into(),
        page: Box::new(PredictionPage {
            limits: LimitCode::ALL.to_vec(),
            pairs: details,
            next_cursor: None,
        }),
    })
}

fn snapshot_with(pairs: Vec<PairSummary>, truncated: bool) -> Msg {
    engine(EngineMsg::Snapshot(Box::new(ScopeSnapshot::Repo(
        RepoSnapshot {
            run_id: "run".into(),
            scope_seq: 1,
            repo: ApiRepo {
                fetched_utc_ms: None,
                repo_id: REPO.into(),
                state: RepoStateView::Observed,
                path: Untrusted::new("/w/shop/.git"),
                base: BaseBranchView {
                    name: Some(gitraptor_api::UntrustedName::new("main")),
                    status: BaseStatusView::Confirmed,
                },
                worktrees: (0..10).map(wt).collect(),
                tier: None,
                checked_utc_ms: None,
                kept_temps: None,
                prediction: Some(PredictionSummary {
                    limits: LimitCode::ALL.to_vec(),
                    pairs,
                    pairs_truncated: truncated,
                }),
            },
        },
    ))))
}

fn snapshot(pairs: Vec<PairSummary>) -> Msg {
    snapshot_with(pairs, false)
}

fn changed(seq: u64, pairs: Vec<PairSummary>, removed: &[&str]) -> Msg {
    engine(EngineMsg::Event {
        scope: Scope::Repo {
            repo_id: REPO.into(),
        },
        scope_seq: seq,
        event: Box::new(Event {
            seq,
            kind: PREDICTION_CHANGED.into(),
            version: 1,
            wall_ms: 0,
            timings: None,
            data: serde_json::to_value(PredictionChanged {
                repo_id: REPO.into(),
                pairs,
                removed: removed.iter().map(|s| (*s).to_owned()).collect(),
            })
            .unwrap(),
        }),
    })
}

fn resync(reason: ResyncReason) -> Msg {
    engine(EngineMsg::Resync {
        scope: Scope::Repo {
            repo_id: REPO.into(),
        },
        reason,
    })
}

/// An opened TUI that seeded `initial` (no alert) and ticked at `T0`.
fn opened(lang: Lang, w: u16, h: u16, initial: Vec<PairSummary>) -> Model {
    let mut m = Model::new(
        lang,
        Size {
            width: w,
            height: h,
        },
    );
    m.conn = ConnState::Live;
    update(&mut m, snapshot(initial));
    update(&mut m, Msg::Tick { now_ms: T0 });
    m
}

fn tick(m: &mut Model, now_ms: i64) {
    update(m, Msg::Tick { now_ms });
}

fn no_color(m: &mut Model, symbols: SymbolSet) {
    m.ui.theme = Theme::new(ColorMode::NoColor, Contrast::Normal, symbols);
}

fn lines(m: &Model) -> Vec<String> {
    let (w, h) = (m.ui.size.width, m.ui.size.height);
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| view(m, f)).unwrap();
    buffer_text(terminal.backend().buffer())
}

fn screen(m: &Model) -> String {
    lines(m).join("\n")
}

/// A new conflict claude-1 <-> claude-2 with its detail page, as the daemon delivers it.
fn alert_new(m: &mut Model, seq: u64, files: &[&str]) {
    let n = files.len() as u32;
    let p = bolt_n("p12", 2, 1, Some(2), n);
    update(m, changed(seq, vec![p.clone()], &[]));
    update(m, page(vec![detail(p, files)]));
}

/// The toast line (the one carrying the alert text), if any.
fn toast_count(m: &Model, needle: &str) -> usize {
    screen(m).matches(needle).count()
}

#[test]
fn s1_a_new_conflict_records_an_alert_a_toast_and_raises_both_rows() {
    let mut m = opened(Lang::Es, 120, 50, vec![]);
    alert_new(&mut m, 2, &["src/api.rs"]);
    let s = screen(&m);
    assert!(
        s.contains(TOAST_ES),
        "no toast '{TOAST_ES}' for a new conflict:\n{s}"
    );
    assert!(
        s.lines().any(|l| l.contains("nuevo")),
        "the conflict panel entry has no 'nuevo' mark:\n{s}"
    );
    // A pair of the last worktrees: both rows go above a calm worktree (attention zone).
    let mut m2 = opened(Lang::Es, 120, 50, vec![]);
    let p = bolt("p87", 2, 8, 7);
    update(&mut m2, changed(2, vec![p.clone()], &[]));
    update(&mut m2, page(vec![detail(p, &["src/api.rs"])]));
    let l2 = lines(&m2);
    let row = |n: &str| {
        l2.iter()
            .position(|x| x.contains(n) && !x.contains("↔") && !x.contains("Conflicto"))
    };
    let (r8, r7, r3) = (row("claude-8"), row("claude-7"), row("claude-3"));
    assert!(
        r8.is_some() && r7.is_some() && r3.is_some() && r8 < r3 && r7 < r3,
        "the rows of the conflicting worktrees are not in the attention zone ({r8:?} {r7:?} vs {r3:?}):\n{}",
        l2.join("\n")
    );
}

#[test]
fn s2_the_toast_text_in_en_es_and_ascii_with_files_and_without_detail() {
    // es / en, one file.
    let mut es = opened(Lang::Es, 120, 50, vec![]);
    alert_new(&mut es, 2, &["src/api.rs"]);
    assert!(screen(&es).contains(TOAST_ES), "es text missing");
    let mut en = opened(Lang::En, 120, 50, vec![]);
    alert_new(&mut en, 2, &["src/api.rs"]);
    assert!(screen(&en).contains(TOAST_EN), "en text missing");
    // more files.
    let mut es3 = opened(Lang::Es, 120, 50, vec![]);
    alert_new(&mut es3, 2, &["src/api.rs", "b.rs", "c.rs"]);
    assert!(
        screen(&es3).contains(&format!("{TOAST_ES} y 2 más")),
        "es ' y N más' missing"
    );
    let mut en2 = opened(Lang::En, 120, 50, vec![]);
    alert_new(&mut en2, 2, &["src/api.rs", "b.rs"]);
    assert!(
        screen(&en2).contains(&format!("{TOAST_EN} and 1 more")),
        "en ' and N more' missing"
    );
    // against the base: named with the base branch.
    let mut base = opened(Lang::Es, 120, 50, vec![]);
    let p = bolt_n("pb", 2, 1, None, 1);
    update(&mut base, changed(2, vec![p.clone()], &[]));
    update(&mut base, page(vec![detail(p, &["src/api.rs"])]));
    assert!(
        screen(&base).contains("⚡ Conflicto previsto: claude-1 ↔ main en src/api.rs"),
        "the base side is not named with the base branch:\n{}",
        screen(&base)
    );
    // no detail: after two ticks it goes out without file.
    let mut nodetail = opened(Lang::Es, 120, 50, vec![]);
    update(&mut nodetail, changed(2, vec![bolt("p12", 2, 1, 2)], &[]));
    tick(&mut nodetail, T0 + 1_000);
    tick(&mut nodetail, T0 + 2_000);
    let s = screen(&nodetail);
    assert!(
        s.contains("⚡ Conflicto previsto: claude-1 ↔ claude-2"),
        "no file-less toast after two ticks:\n{s}"
    );
    assert!(
        !s.contains("claude-2 en src/api.rs"),
        "a file is named without a detail:\n{s}"
    );
    // ASCII, no color, 80x24, both languages.
    for (lang, text) in [
        (
            Lang::Es,
            "Conflicto previsto: claude-1 <-> claude-2 en src/api.rs",
        ),
        (
            Lang::En,
            "Predicted conflict: claude-1 <-> claude-2 in src/api.rs",
        ),
    ] {
        let mut m = opened(lang, 80, 24, vec![]);
        no_color(&mut m, SymbolSet::Ascii);
        alert_new(&mut m, 2, &["src/api.rs"]);
        let s = screen(&m);
        let line = s
            .lines()
            .find(|l| l.contains(text))
            .unwrap_or_else(|| panic!("no ASCII toast '{text}' at 80x24:\n{s}"));
        assert!(!line.contains('↔'), "'↔' not folded to '<->': {line}");
        assert!(!s.contains('⚡'), "the bolt is not folded in ASCII:\n{s}");
        let from = line.find(text).unwrap();
        assert!(line[from..from + text.len()].is_ascii(), "{line}");
    }
}

#[test]
fn s3_a_known_conflict_never_alerts_again_and_a_returning_one_does() {
    const MARK: &str = "Conflicto previsto:";
    let mut m = opened(Lang::Es, 120, 50, vec![]);
    alert_new(&mut m, 2, &["src/api.rs"]);
    assert_eq!(
        toast_count(&m, "⚡ Conflicto previsto: claude-1"),
        1,
        "the first alert is missing"
    );
    let mut t = T0 + 60_000;
    tick(&mut m, t);
    assert_eq!(
        toast_count(&m, "⚡ Conflicto previsto: claude-1"),
        0,
        "the toast stays after 5 s"
    );
    let mut seq = 3;
    let steps: Vec<Msg> = vec![
        changed(0, vec![recalculating("p12", 3, 1, 2)], &[]),
        changed(0, vec![bolt("p12", 4, 1, 2)], &[]),
        changed(0, vec![calculating("p12", 5, 1, 2)], &[]),
        changed(0, vec![bolt("p12", 6, 1, 2)], &[]),
        changed(
            0,
            vec![other_state("p12", 7, 1, 2, PairStateView::Pending)],
            &[],
        ),
        changed(0, vec![bolt("p12", 8, 1, 2)], &[]),
        changed(
            0,
            vec![other_state("p12", 9, 1, 2, PairStateView::NotComputable)],
            &[],
        ),
        changed(0, vec![bolt("p12", 10, 1, 2)], &[]),
    ];
    let _ = MARK;
    for mut step in steps {
        if let Msg::Engine(s) = &mut step
            && let EngineMsg::Event {
                scope_seq, event, ..
            } = &mut s.msg
        {
            *scope_seq = seq;
            event.seq = seq;
        }
        seq += 1;
        update(&mut m, step);
        for _ in 0..2 {
            t += 1_000;
            tick(&mut m, t);
        }
        assert_eq!(
            toast_count(&m, "⚡ Conflicto previsto: claude-1"),
            0,
            "a known conflict alerted again (step {seq})"
        );
    }
    // resync with the same pair, and daemon-restarted with another run: no alert.
    update(&mut m, resync(ResyncReason::ReplayUnavailable));
    update(&mut m, snapshot(vec![bolt("p12", 11, 1, 2)]));
    update(&mut m, resync(ResyncReason::DaemonRestarted));
    update(&mut m, snapshot(vec![calculating("p12", 12, 1, 2)]));
    update(&mut m, changed(seq, vec![bolt("p12", 13, 1, 2)], &[]));
    for _ in 0..2 {
        t += 1_000;
        tick(&mut m, t);
    }
    assert_eq!(
        toast_count(&m, "⚡ Conflicto previsto: claude-1"),
        0,
        "a resync or a daemon restart repeated the alert"
    );
    // it disappears (clean) and returns: alerts; same through removed.
    update(&mut m, changed(seq + 1, vec![clean("p12", 14, 1, 2)], &[]));
    update(&mut m, changed(seq + 2, vec![bolt("p12", 15, 1, 2)], &[]));
    for _ in 0..2 {
        t += 1_000;
        tick(&mut m, t);
    }
    assert_eq!(
        toast_count(&m, "⚡ Conflicto previsto: claude-1"),
        1,
        "a conflict that went clean and came back does not alert"
    );
    t += 60_000;
    tick(&mut m, t);
    update(&mut m, changed(seq + 3, vec![], &["p12"]));
    update(&mut m, changed(seq + 4, vec![bolt("p12", 16, 1, 2)], &[]));
    for _ in 0..2 {
        t += 1_000;
        tick(&mut m, t);
    }
    assert_eq!(
        toast_count(&m, "⚡ Conflicto previsto: claude-1"),
        1,
        "a conflict that was removed and came back does not alert"
    );
}

#[test]
fn s4_opening_seeds_without_alerting_and_truncated_or_closed_scopes_are_honest() {
    let marks = |m: &Model| toast_count(m, "⚡ Conflicto previsto:");
    // Existing conflicts are seeded: no toast, no mark, even after the ticks.
    let mut m = opened(Lang::Es, 120, 50, vec![bolt("p12", 1, 1, 2)]);
    tick(&mut m, T0 + 1_000);
    tick(&mut m, T0 + 2_000);
    assert_eq!(marks(&m), 0, "an existing conflict alerted when opening");
    assert!(
        !screen(&m).contains("nuevo"),
        "an existing conflict is marked new"
    );
    // A page of prediction.list for the seeded pair does not alert either.
    update(
        &mut m,
        page(vec![detail(bolt("p12", 1, 1, 2), &["src/api.rs"])]),
    );
    tick(&mut m, T0 + 3_000);
    assert_eq!(marks(&m), 0, "a prediction.list page alerted a seeded pair");

    // A truncated snapshot without the pair keeps it known.
    let mut t = opened(Lang::Es, 120, 50, vec![bolt("p12", 1, 1, 2)]);
    update(&mut t, snapshot_with(vec![], true));
    update(&mut t, changed(2, vec![bolt("p12", 2, 1, 2)], &[]));
    tick(&mut t, T0 + 1_000);
    tick(&mut t, T0 + 2_000);
    assert_eq!(marks(&t), 0, "a truncated snapshot dropped a known pair");
    // A complete snapshot without it drops it: its return alerts.
    let mut c = opened(Lang::Es, 120, 50, vec![bolt("p12", 1, 1, 2)]);
    update(&mut c, snapshot_with(vec![], false));
    update(&mut c, changed(2, vec![bolt("p12", 2, 1, 2)], &[]));
    tick(&mut c, T0 + 1_000);
    tick(&mut c, T0 + 2_000);
    assert_eq!(
        marks(&c),
        1,
        "a complete snapshot did not drop the absent pair"
    );

    // ScopeClosed forgets the repo: the pair is unknown again afterwards.
    let mut z = opened(Lang::Es, 120, 50, vec![bolt("p12", 1, 1, 2)]);
    update(&mut z, resync(ResyncReason::ScopeClosed));
    update(&mut z, snapshot(vec![]));
    update(&mut z, changed(2, vec![bolt("p12", 2, 1, 2)], &[]));
    tick(&mut z, T0 + 1_000);
    tick(&mut z, T0 + 2_000);
    assert_eq!(marks(&z), 1, "ScopeClosed did not forget the repo");
}

#[test]
fn s5_the_toast_expires_at_5_s_but_the_mark_stays_and_the_stack_is_bounded() {
    let toast = "⚡ Conflicto previsto: claude-1 ↔ claude-2";
    let mut m = opened(Lang::Es, 120, 50, vec![]);
    alert_new(&mut m, 2, &["src/api.rs"]);
    tick(&mut m, T0 + 4_999);
    assert!(screen(&m).contains(toast), "the toast is gone at +4.999 ms");
    tick(&mut m, T0 + 5_000);
    let s = screen(&m);
    assert!(!s.contains(toast), "the toast stays at +5.000 ms:\n{s}");
    assert!(
        s.lines().any(|l| l.contains("nuevo")),
        "the 'nuevo' mark went with the toast:\n{s}"
    );

    // The toast of a pair that disappears goes at the next tick.
    let mut g = opened(Lang::Es, 120, 50, vec![]);
    alert_new(&mut g, 2, &["src/api.rs"]);
    update(&mut g, changed(3, vec![], &["p12"]));
    tick(&mut g, T0 + 100);
    assert!(
        !screen(&g).contains(toast),
        "a toast of a vanished pair is still shown"
    );

    // Stack of 3, the oldest leaves first.
    let mut st = opened(Lang::Es, 120, 50, vec![]);
    for (i, (a, b)) in [(1, 2), (3, 4), (5, 6), (7, 8)].into_iter().enumerate() {
        let id = format!("p{a}{b}");
        let p = bolt(&id, 2, a, b);
        update(&mut st, changed(2 + i as u64, vec![p.clone()], &[]));
        update(&mut st, page(vec![detail(p, &["src/api.rs"])]));
    }
    let s = screen(&st);
    assert_eq!(
        s.matches("⚡ Conflicto previsto:").count(),
        3,
        "the stack is not bounded to 3 toasts:\n{s}"
    );
    assert!(
        !s.contains(toast) && s.contains("⚡ Conflicto previsto: claude-3 ↔ claude-4"),
        "the oldest toast does not leave first:\n{s}"
    );
}

#[derive(Clone, Default)]
struct Rec(Arc<Mutex<Vec<u8>>>);

impl Write for Rec {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n != "snapshots") {
                sources(&p, out);
            }
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Production code of a file: no comments, nothing from `#[cfg(test)]` on.
fn code(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap();
    let head = text.split("#[cfg(test)]").next().unwrap_or("");
    head.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn s6_no_bell_and_no_os_notification() {
    // The terminal output with the alert, a path carrying a BEL: no 0x07.
    let rec = Rec::default();
    let mut terminal = Terminal::with_options(
        CrosstermBackend::new(rec.clone()),
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 120, 50)),
        },
    )
    .unwrap();
    let mut m = opened(Lang::Es, 120, 50, vec![]);
    let p = bolt("p92", 2, BEL_WT, 2);
    update(&mut m, changed(2, vec![p.clone()], &[]));
    update(&mut m, page(vec![detail(p, &["src/a\u{7}pi.rs"])]));
    terminal.draw(|f| view(&m, f)).unwrap();
    let bytes = rec.0.lock().unwrap().clone();
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("Conflicto previsto"),
        "the alert never reached the terminal output"
    );
    assert!(!bytes.contains(&0x07), "the terminal output carries a BEL");

    // Sources: no BEL writes, no OS notifier, no notification crate.
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    sources(&base.join("tui"), &mut files);
    sources(&base.join("client"), &mut files);
    sources(&base.join("present"), &mut files);
    files.extend(["model.rs", "queue.rs", "paths.rs"].map(|f| base.join(f)));
    for f in files.iter().filter(|f| {
        let n = f.file_name().unwrap().to_string_lossy();
        n != "sanitize.rs" && !n.ends_with("_tests.rs") && n != "tests.rs" && f.exists()
    }) {
        let c = code(f);
        for bell in ["\\x07", "\\u{7}", "\\u{07}", "0x07", "char::from(7)"] {
            assert!(!c.contains(bell), "{} writes a BEL ({bell})", f.display());
        }
        for n in [
            "osascript",
            "notify-send",
            "terminal-notifier",
            "UNUserNotificationCenter",
            "NSUserNotification",
            "ToastNotificationManager",
        ] {
            assert!(!c.contains(n), "{} names an OS notifier ({n})", f.display());
        }
    }
    let manifest =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    for krate in [
        "notify-rust",
        "mac-notification-sys",
        "winrt-notification",
        "tauri-winrt-notification",
        "user-notify",
        "objc2-user-notifications",
        "zbus",
        "dbus",
    ] {
        assert!(
            !manifest.contains(krate),
            "apps/cli depends on a notification crate: {krate}"
        );
    }
}

#[test]
fn s7_the_alert_is_not_intrusive_and_legible_without_color_and_in_ascii() {
    for (lang, symbols, text, limits) in [
        (
            Lang::En,
            SymbolSet::Unicode,
            "Predicted conflict: claude-1 ↔ claude-2",
            "commits only",
        ),
        (
            Lang::Es,
            SymbolSet::Unicode,
            "Conflicto previsto: claude-1 ↔ claude-2",
            "solo commits",
        ),
        (
            Lang::En,
            SymbolSet::Ascii,
            "Predicted conflict: claude-1 <-> claude-2",
            "commits only",
        ),
        (
            Lang::Es,
            SymbolSet::Ascii,
            "Conflicto previsto: claude-1 <-> claude-2",
            "solo commits",
        ),
    ] {
        let mut m = opened(lang, 80, 24, vec![]);
        no_color(&mut m, symbols);
        alert_new(&mut m, 2, &["src/api.rs"]);
        let all = lines(&m);
        let at = all
            .iter()
            .position(|l| l.contains(text))
            .unwrap_or_else(|| panic!("no toast '{text}' at 80x24:\n{}", all.join("\n")));
        let lim = all
            .iter()
            .position(|l| l.contains(limits))
            .unwrap_or_else(|| panic!("the limits line is covered:\n{}", all.join("\n")));
        assert!(
            at < lim,
            "the toast is not anchored in the list above the alerts region:\n{}",
            all.join("\n")
        );
        if symbols == SymbolSet::Ascii {
            let line = &all[at];
            let from = line.find(text).unwrap();
            assert!(line[from..from + text.len()].is_ascii(), "{line}");
            assert!(!all.join("\n").contains('⚡'), "unfolded bolt in ASCII");
        }
    }
}
