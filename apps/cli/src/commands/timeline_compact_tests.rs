//! Contract tests of the compact `raptor timeline` text (US-TMC-023): the literal E1, E2 and E4
//! examples of the story, rendered in both languages with an explicit `Tr` (the process language
//! is fixed once, so the catalog's global `t` cannot serve both in one process).

use gitraptor_api::messages::GitEventKind;
use gitraptor_api::timemachine::{
    ActedOn, Attribution, ChangedFiles, EntryOrigin, Protection, ProtectionLevel, TimelineEntry,
    TimelineOperationKind, TimelineOperationState, TimelineResult,
};
use gitraptor_api::untrusted::{Untrusted, UntrustedName};
use gitraptor_api::{Actor, AgentKind, AgentOrigin};

use super::compact::{CompactOpts, LocalDay, default_since, empty_text, render_compact, short_id};
use crate::i18n::Tr;

const EN: Tr = Tr { spanish: false };
const ES: Tr = Tr { spanish: true };

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
fn days(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// UTC milliseconds of a local time at offset 0.
fn at(y: i64, m: i64, d: i64, h: i64, mi: i64) -> i64 {
    (days(y, m, d) * 86_400 + h * 3600 + mi * 60) * 1000
}

fn today() -> LocalDay {
    LocalDay(days(2026, 10, 17))
}

fn opts(verbose: bool, window_applied: bool) -> CompactOpts {
    CompactOpts {
        verbose,
        today: today(),
        window_applied,
    }
}

fn claude() -> Actor {
    Actor::Agent {
        kind: AgentKind::ClaudeCode,
        name: None,
        origin: AgentOrigin::Detected,
    }
}

fn point(prefix: &str) -> String {
    format!("{prefix}-0000-4000-8000-000000000001")
}

struct Row {
    seq: i64,
    when: i64,
    worktree: &'static str,
    origin: EntryOrigin,
    actor: Actor,
    level: ProtectionLevel,
    snapshot: Option<String>,
    snapshot_len: Option<u8>,
    files: ChangedFiles,
}

fn git(kind: GitEventKind, branch: &str) -> EntryOrigin {
    EntryOrigin::GitEvent {
        seq: 0,
        kind,
        branch: Some(UntrustedName::new(branch)),
    }
}

fn row(seq: i64, when: i64, worktree: &'static str, origin: EntryOrigin, actor: Actor) -> Row {
    Row {
        seq,
        when,
        worktree,
        origin,
        actor,
        level: ProtectionLevel::None,
        snapshot: None,
        snapshot_len: None,
        files: ChangedFiles::Unavailable,
    }
}

impl Row {
    fn protected(mut self, level: ProtectionLevel, prefix: &str) -> Self {
        self.level = level;
        self.snapshot = Some(point(prefix));
        self.snapshot_len = Some(8);
        self
    }

    fn build(self) -> TimelineEntry {
        TimelineEntry {
            id: format!("event:{}", self.seq),
            origin: self.origin,
            occurred_utc_ms: self.when,
            utc_offset_s: 0,
            worktrees: vec![Untrusted::new(self.worktree)],
            actor: self.actor,
            inferred: None,
            attribution: Attribution::Current,
            protection: Protection {
                level: self.level,
                snapshot_id: self.snapshot,
            },
            files: self.files,
            short_id_len: self.snapshot_len,
        }
    }
}

fn result(rows: Vec<Row>) -> TimelineResult {
    TimelineResult {
        // The header names the repo; the brief gives `CompactOpts` no field for it.
        repo_id: "gitRaptor".into(),
        entries: rows.into_iter().map(Row::build).collect(),
        truncated: false,
        unavailable: Vec::new(),
        detection_available: true,
    }
}

fn undo(seq: i64, when: i64, acted_on: Vec<ActedOn>) -> Row {
    row(
        seq,
        when,
        "/w/main",
        EntryOrigin::Operation {
            operation_id: format!("op-{seq}"),
            kind: TimelineOperationKind::Undo,
            subtype: None,
            state: TimelineOperationState::Finished,
            acted_on,
        },
        Actor::Unattributed,
    )
}

const BRANCH: &str = "feat/ux-timeline-status";

/// The activity of E1 and E2: 11 entries, 7 of them summarized.
fn e1_activity() -> TimelineResult {
    result(vec![
        row(1, at(2026, 10, 16, 22, 41), "/w/main", git(GitEventKind::Commit, "main"), claude())
            .protected(ProtectionLevel::Observation, "3e9d07b1"),
        undo(2, at(2026, 10, 16, 23, 5), Vec::new()).protected(ProtectionLevel::HookPrior, "5c2a8e40"),
        row(3, at(2026, 10, 17, 9, 12), "/w/uxtl", git(GitEventKind::BranchCreate, "rbonillajr/uxtl"), claude()),
        row(4, at(2026, 10, 17, 9, 13), "/w/uxtl", git(GitEventKind::BranchDelete, "rbonillajr/uxtl"), claude()),
        row(5, at(2026, 10, 17, 9, 14), "/w/uxtl", git(GitEventKind::BranchCreate, "rbonillajr/other"), claude()),
        row(6, at(2026, 10, 17, 9, 15), "/w/uxtl", git(GitEventKind::BranchDelete, "rbonillajr/other"), claude()),
        row(7, at(2026, 10, 17, 9, 20), "/w/uxtl", git(GitEventKind::Rebase, BRANCH), claude())
            .protected(ProtectionLevel::HookPrior, "47bfc884"),
        row(8, at(2026, 10, 17, 9, 41), "/w/uxtl", git(GitEventKind::Reset, BRANCH), Actor::Unattributed),
        row(9, at(2026, 10, 17, 9, 42), "/w/uxtl", git(GitEventKind::Reset, BRANCH), Actor::Unattributed),
        row(10, at(2026, 10, 17, 9, 43), "/w/uxtl", git(GitEventKind::Reset, BRANCH), Actor::Unattributed),
        row(11, at(2026, 10, 17, 10, 2), "/w/uxtl", git(GitEventKind::Commit, BRANCH), claude())
            .protected(ProtectionLevel::Observation, "9a1c0d22"),
    ])
}

fn text(lines: &[&str]) -> String {
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// TMC023-FORMAT-EN: E1 of the story, character for character.
#[test]
fn the_timeline_matches_e1_in_english() {
    let shown = render_compact(&e1_activity(), &opts(false, true), EN);

    assert_eq!(
        shown,
        text(&[
            "Last 24 h of gitRaptor: 11 entries, 7 of them summarized (--verbose shows them all)",
            "",
            "2026-10-16 (yesterday)",
            "  22:41  main  commit on main                                           Claude Code   observed  restore 3e9d07b1",
            "  23:05  main  undo                                                     you or other  prior     restore 5c2a8e40",
            "",
            "2026-10-17 (today)",
            "  09:12  uxtl  2 branches created and deleted: rbonillajr/uxtl, +1      Claude Code   none      -",
            "  09:20  uxtl  rebase of feat/ux-timeline-status                        Claude Code   prior     restore 47bfc884",
            "  09:41  uxtl  reset on feat/ux-timeline-status without moving it (x3)  you or other  none      -",
            "  10:02  uxtl  commit on feat/ux-timeline-status                        Claude Code   observed  restore 9a1c0d22",
            "",
            "you or other: no agent was detected; it may be you or another tool",
            "restore a point with raptor restore <id>; a unique prefix of 6 or more characters is enough",
        ])
    );
}

/// TMC023-FORMAT-ES: E2 of the story, character for character.
#[test]
fn the_timeline_matches_e2_in_spanish() {
    let shown = render_compact(&e1_activity(), &opts(false, true), ES);

    assert_eq!(
        shown,
        text(&[
            "Últimas 24 h de gitRaptor: 11 entradas, 7 de ellas resumidas (--verbose las muestra todas)",
            "",
            "2026-10-16 (ayer)",
            "  22:41  main  commit en main                                     Claude Code  observado  restore 3e9d07b1",
            "  23:05  main  undo                                               tú u otro    previo     restore 5c2a8e40",
            "",
            "2026-10-17 (hoy)",
            "  09:12  uxtl  2 ramas creadas y borradas: rbonillajr/uxtl, +1    Claude Code  ninguno    -",
            "  09:20  uxtl  rebase de feat/ux-timeline-status                  Claude Code  previo     restore 47bfc884",
            "  09:41  uxtl  reset en feat/ux-timeline-status sin moverla (x3)  tú u otro    ninguno    -",
            "  10:02  uxtl  commit en feat/ux-timeline-status                  Claude Code  observado  restore 9a1c0d22",
            "",
            "tú u otro: no se detectó ningún agente; puede ser tuyo o de otra herramienta",
            "restaura un punto con raptor restore <id>; basta un prefijo único de 6 caracteres o más",
        ])
    );
}

/// TMC023-SUMMARY: low-value entries fold, a reset that saved a point never does.
#[test]
fn low_value_entries_are_summarized_but_recoverable_ones_never() {
    let wt = "/w/uxtl";
    let rows = vec![
        row(1, at(2026, 10, 17, 9, 0), wt, git(GitEventKind::BranchCreate, "feat/a"), claude()),
        row(2, at(2026, 10, 17, 9, 1), wt, git(GitEventKind::BranchDelete, "feat/a"), claude()),
        row(3, at(2026, 10, 17, 9, 2), wt, git(GitEventKind::BranchCreate, "feat/b"), claude()),
        row(4, at(2026, 10, 17, 9, 3), wt, git(GitEventKind::BranchDelete, "feat/b"), claude()),
        row(5, at(2026, 10, 17, 9, 10), wt, git(GitEventKind::Reset, "feat/x"), Actor::Unattributed),
        row(6, at(2026, 10, 17, 9, 11), wt, git(GitEventKind::Reset, "feat/x"), Actor::Unattributed),
        row(7, at(2026, 10, 17, 9, 12), wt, git(GitEventKind::Reset, "feat/x"), Actor::Unattributed),
        row(8, at(2026, 10, 17, 9, 20), wt, git(GitEventKind::Reset, "feat/x"), Actor::Unattributed)
            .protected(ProtectionLevel::HookPrior, "d00dfeed"),
    ];

    let shown = render_compact(&result(rows), &opts(false, true), EN);

    let lines: Vec<&str> = shown.lines().collect();
    let with = |needle: &str| lines.iter().filter(|l| l.contains(needle)).collect::<Vec<_>>();
    assert_eq!(with("branches created and deleted").len(), 1, "{shown}");
    assert!(with("branches created and deleted")[0].contains("2 branches created and deleted: feat/a, +1"), "{shown}");
    assert!(with("branches created and deleted")[0].trim_start().starts_with("09:00"), "{shown}");
    assert_eq!(with("(x3)").len(), 1, "{shown}");
    assert!(with("(x3)")[0].contains("reset on feat/x without moving it (x3)"), "{shown}");
    // The reset that saved a point keeps its own line with its id.
    let own = with("restore d00dfeed");
    assert_eq!(own.len(), 1, "{shown}");
    assert!(own[0].contains("reset on feat/x without moving it") && !own[0].contains("(x"), "{shown}");
    assert_eq!(with("reset on").len(), 2, "{shown}");
    assert!(lines[0].contains("8 entries, 7 of them summarized"), "{shown}");
}

/// TMC023-EMPTY-WINDOW: an empty default window with older activity says how to widen it; a repo
/// with no activity at all still says "no activity yet"; `--since` shows the activity.
#[test]
fn an_empty_default_window_says_how_to_widen_it() {
    assert_eq!(
        empty_text(true, true, EN).trim_end(),
        "no activity in the last 24 h; widen it with --since, such as --since 7d"
    );
    assert_eq!(
        empty_text(true, true, ES).trim_end(),
        "no hay actividad en las últimas 24 h; amplía la ventana con --since, por ejemplo --since 7d"
    );

    // Nothing at all in the repo: the old answer, never E4.
    assert_eq!(empty_text(true, false, EN).trim_end(), "no activity yet");
    assert_eq!(empty_text(true, false, ES).trim_end(), "aún no hay actividad");

    // Three days ago: visible without the default window, under its own date.
    let old = result(vec![
        row(1, at(2026, 10, 14, 11, 30), "/w/main", git(GitEventKind::Commit, "main"), claude())
            .protected(ProtectionLevel::Observation, "3e9d07b1"),
    ]);
    let shown = render_compact(&old, &opts(false, false), EN);
    assert!(shown.contains("2026-10-14"), "{shown}");
    assert!(shown.contains("commit on main"), "{shown}");
    assert!(!shown.contains("no activity") && !shown.contains("widen"), "{shown}");
}

/// The short id is `id[..len]` and never ends in the dash of the UUID.
#[test]
fn a_short_id_never_ends_in_a_dash() {
    let id = point("47bfc884");
    assert_eq!(short_id(&id, 8), "47bfc884");
    assert_eq!(short_id(&id, 9), "47bfc884-0");
    assert_eq!(short_id(&id, 10), "47bfc884-0");
}

/// TMC023-VERBOSE: nothing summarized, full detail, files only when there are files.
#[test]
fn verbose_shows_every_entry_with_full_detail() {
    let acted = "aaaa1111-0000-4000-8000-00000000000a";
    let mut with_files = row(
        12,
        at(2026, 10, 17, 10, 5),
        "/w/uxtl",
        git(GitEventKind::Commit, BRANCH),
        claude(),
    );
    with_files.files = ChangedFiles::Available {
        paths: vec![Untrusted::new("a.rs"), Untrusted::new("b.rs")],
        total: 2,
        first_parent: false,
    };
    let mut all = e1_activity();
    all.entries.push(with_files.build());
    all.entries[1] = undo(2, at(2026, 10, 16, 23, 5), vec![ActedOn::Snapshot(acted.into())])
        .protected(ProtectionLevel::HookPrior, "5c2a8e40")
        .build();

    let shown = render_compact(&all, &opts(true, true), EN);

    // Every entry on its own, none folded.
    assert!(!shown.contains("(x3)") && !shown.contains("created and deleted"), "{shown}");
    assert_eq!(shown.matches("reset on feat/ux-timeline-status without moving it").count(), 3, "{shown}");
    assert!(shown.contains("branch rbonillajr/uxtl created") && shown.contains("branch rbonillajr/uxtl deleted"), "{shown}");
    // Actor with its origin; an unattributed change is never "no agent".
    assert!(shown.contains("detected"), "{shown}");
    assert!(shown.contains("you or other") && !shown.contains("no agent"), "{shown}");
    // The whole level text and the whole id.
    assert!(shown.contains("prior snapshot") && shown.contains("captured by observation"), "{shown}");
    assert!(shown.contains(&point("3e9d07b1")), "{shown}");
    // What the undo took back.
    assert!(shown.contains(acted), "{shown}");
    // Files: listed where there are, never "not available".
    assert!(shown.contains("a.rs, b.rs"), "{shown}");
    assert!(!shown.contains("not available"), "{shown}");
}

/// TMC023-JSON-UNCHANGED: only text output without `--since` gets the 24 h window.
#[test]
fn json_keeps_no_default_window() {
    assert_eq!(default_since(true, None), None);
    assert_eq!(default_since(false, None), Some("24h".to_owned()));
    assert_eq!(default_since(false, Some("7d".into())), Some("7d".to_owned()));
    assert_eq!(default_since(true, Some("2h".into())), Some("2h".to_owned()));
}
