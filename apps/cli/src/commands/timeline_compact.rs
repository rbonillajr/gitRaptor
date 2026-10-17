//! The text of `raptor timeline`: one line per event, grouped by day, columns aligned, the
//! low-value entries folded and the id `raptor restore` takes on the same line. Pure: the
//! language comes in as a [`Tr`] and the clock as a [`LocalDay`], so both languages are testable
//! in one process. Everything that comes from the repo is printed sanitized.

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use gitraptor_api::messages::{GitEventKind, InferredAgent, TrailerCheck};
use gitraptor_api::timemachine::{
    ActedOn, ChangedFiles, EntryOrigin, ProtectionLevel, TimelineChannel, TimelineEntry,
    TimelineOperationKind, TimelineOperationState, TimelineResult, TimelineSource,
};
use gitraptor_api::untrusted::sanitize;
use gitraptor_api::{Actor, AgentKind};

use crate::events::local_time;
use crate::i18n::Tr;
use crate::status::wire;

/// A calendar day in the user's local time, as days since 1970-01-01.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct LocalDay(pub i64);

impl LocalDay {
    /// The day of an instant at a UTC offset.
    pub(super) fn of(utc_ms: i64, offset_s: i32) -> Self {
        Self((utc_ms.div_euclid(1000) + i64::from(offset_s)).div_euclid(86_400))
    }
}

pub(super) struct CompactOpts {
    pub verbose: bool,
    pub today: LocalDay,
    /// The default 24 h window was applied (no `--since`, no `--json`).
    pub window_applied: bool,
}

/// The widest a worktree or actor column grows in the compact view.
const COLUMN_CAP: usize = 12;
/// Characters the id carries when the engine did not say how many tell it apart.
const DEFAULT_ID_LEN: u8 = 8;

/// One printed line of the table, with the lines that detail it under it.
struct Row {
    day: LocalDay,
    date: String,
    time: String,
    worktree: String,
    what: String,
    who: String,
    level: String,
    action: String,
    details: Vec<String>,
    you_or_other: bool,
}

pub(super) fn render_compact(result: &TimelineResult, opts: &CompactOpts, tr: Tr) -> String {
    let degraded = !result.unavailable.is_empty();
    if result.entries.is_empty() && !degraded {
        return format!("{}\n", empty_text(opts.window_applied, false, tr));
    }
    let (rows, summarized) = rows(result, opts, tr);

    let mut out = String::new();
    let _ = writeln!(out, "{}", header(result, opts, summarized, tr));
    let width = |pick: fn(&Row) -> &str| {
        rows.iter().map(|r| pick(r).chars().count()).max().unwrap_or(0)
    };
    let (w_tree, w_what, w_who, w_level) = (
        width(|r| &r.worktree),
        width(|r| &r.what),
        width(|r| &r.who),
        width(|r| &r.level),
    );
    let mut day: Option<LocalDay> = None;
    for row in &rows {
        if day != Some(row.day) {
            day = Some(row.day);
            let key = if row.day == opts.today {
                Some("timeline.day.today")
            } else if row.day.0 == opts.today.0 - 1 {
                Some("timeline.day.yesterday")
            } else {
                None
            };
            let _ = writeln!(out);
            let _ = match key {
                Some(key) => writeln!(out, "{}", tr.t(key, &[("date", &row.date)])),
                None => writeln!(out, "{}", row.date),
            };
        }
        let _ = writeln!(
            out,
            "  {}  {}  {}  {}  {}  {}",
            row.time,
            pad(&row.worktree, w_tree),
            pad(&row.what, w_what),
            pad(&row.who, w_who),
            pad(&row.level, w_level),
            row.action
        );
        for detail in &row.details {
            let _ = writeln!(out, "    {detail}");
        }
    }

    let mut footer = Vec::new();
    if result.truncated {
        footer.push(tr.t("timeline.truncated", &[]));
    }
    if degraded {
        let sources: Vec<String> = result
            .unavailable
            .iter()
            .map(|s| tr.t(source_key(*s), &[]))
            .collect();
        footer.push(tr.t("timeline.incomplete", &[("sources", &sources.join(", "))]));
    }
    // `--verbose` leaves the legend out: its wording says "no agent", which that view never uses.
    if !opts.verbose && rows.iter().any(|r| r.you_or_other) {
        footer.push(tr.t("timeline.legend.you-or-other", &[]));
    }
    if result.entries.iter().any(|e| e.protection.snapshot_id.is_some()) {
        footer.push(tr.t("timeline.hint.restore", &[]));
    }
    if !footer.is_empty() {
        let _ = writeln!(out);
        for line in footer {
            let _ = writeln!(out, "{line}");
        }
    }
    out
}

fn pad(text: &str, width: usize) -> String {
    let missing = width.saturating_sub(text.chars().count());
    format!("{text}{}", " ".repeat(missing))
}

fn header(result: &TimelineResult, opts: &CompactOpts, summarized: usize, tr: Tr) -> String {
    let total = result.entries.len();
    let entries = if total == 1 {
        tr.t("timeline.entries.one", &[])
    } else {
        tr.t("timeline.entries.many", &[("count", &total)])
    };
    let key = match (opts.window_applied, summarized > 0) {
        (true, false) => "timeline.header.window",
        (true, true) => "timeline.header.window-summarized",
        (false, false) => "timeline.header.all",
        (false, true) => "timeline.header.all-summarized",
    };
    tr.t(
        key,
        &[
            ("repo", &sanitize(&result.repo_id)),
            ("entries", &entries),
            ("summarized", &summarized),
        ],
    )
}

/// `id[..len]`, one character more when the cut ends in `-`. Never fewer than 8 characters and
/// never more than the id has.
pub(crate) fn short_id(id: &str, len: u8) -> String {
    let mut n = usize::from(len).max(8).min(id.len());
    if id.as_bytes().get(n.wrapping_sub(1)) == Some(&b'-') {
        n = (n + 1).min(id.len());
    }
    // An id is ASCII; a cut inside a multi-byte character (a hostile id) shows the whole text.
    id.get(..n).unwrap_or(id).to_owned()
}

/// `Some("24h")` only for text output without `--since`.
pub(super) fn default_since(json: bool, since: Option<String>) -> Option<String> {
    match since {
        None if !json => Some("24h".to_owned()),
        other => other,
    }
}

/// What an empty answer says: E4 when the default window hid older activity, otherwise
/// `timeline.empty`.
pub(super) fn empty_text(window_applied: bool, activity_outside_window: bool, tr: Tr) -> String {
    if window_applied && activity_outside_window {
        tr.t("timeline.window-empty", &[])
    } else {
        tr.t("timeline.empty", &[])
    }
}

fn source_key(source: TimelineSource) -> &'static str {
    match source {
        TimelineSource::Operations => "timeline.source.operations",
        TimelineSource::Events => "timeline.source.events",
    }
}

/// The rows to print and how many entries were folded into summary rows.
fn rows(result: &TimelineResult, opts: &CompactOpts, tr: Tr) -> (Vec<Row>, usize) {
    let mut line: Vec<Row> = result
        .entries
        .iter()
        .map(|e| row_of(e, result.detection_available, opts.verbose, tr))
        .collect();
    if opts.verbose {
        return (line, 0);
    }
    let folds = folds(&result.entries, &line);
    let mut absorbed: HashSet<usize> = HashSet::new();
    let mut at: HashMap<usize, Row> = HashMap::new();
    for fold in folds {
        let first = fold.members[0];
        absorbed.extend(fold.members.iter().copied());
        at.insert(first, summary_row(&line, &fold, tr));
    }
    let summarized = absorbed.len();
    let mut out = Vec::with_capacity(line.len());
    for (i, row) in line.drain(..).enumerate() {
        if let Some(summary) = at.remove(&i) {
            out.push(summary);
        } else if !absorbed.contains(&i) {
            out.push(row);
        }
    }
    (out, summarized)
}

/// A group of entries printed as one line.
struct Fold {
    members: Vec<usize>,
    kind: FoldKind,
}

enum FoldKind {
    /// Branches created and deleted; the names of the branches in order.
    Branches(Vec<String>),
    /// Resets that did not move the branch.
    Resets,
}

/// The entries that fold, per day and worktree. Only an entry without a restore point folds:
/// what can be recovered is never hidden.
fn folds(entries: &[TimelineEntry], line: &[Row]) -> Vec<Fold> {
    let mut groups: Vec<((i64, &str), Vec<usize>)> = Vec::new();
    for (i, row) in line.iter().enumerate() {
        let key = (row.day.0, row.worktree.as_str());
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, members)) => members.push(i),
            None => groups.push((key, vec![i])),
        }
    }
    let foldable = |i: usize| entries[i].protection.snapshot_id.is_none();
    let branch_of = |i: usize| match &entries[i].origin {
        EntryOrigin::GitEvent { branch: Some(b), .. } => Some(b.sanitized()),
        _ => None,
    };
    let kind_of = |i: usize| match &entries[i].origin {
        EntryOrigin::GitEvent { kind, .. } => Some(*kind),
        _ => None,
    };
    let mut out = Vec::new();
    for (_, list) in groups {
        let mut taken: HashSet<usize> = HashSet::new();
        // (a) A branch created and deleted with no other entry about it in between.
        let mut pairs: Vec<(usize, usize, String)> = Vec::new();
        for (p, &i) in list.iter().enumerate() {
            let (true, Some(GitEventKind::BranchCreate), Some(name)) =
                (foldable(i), kind_of(i), branch_of(i))
            else {
                continue;
            };
            let next = list[p + 1..]
                .iter()
                .copied()
                .find(|&j| !taken.contains(&j) && branch_of(j).as_deref() == Some(&name));
            if let Some(j) = next
                && foldable(j)
                && kind_of(j) == Some(GitEventKind::BranchDelete)
            {
                taken.insert(i);
                taken.insert(j);
                pairs.push((i, j, name));
            }
        }
        if !pairs.is_empty() {
            let mut members: Vec<usize> = pairs.iter().flat_map(|(i, j, _)| [*i, *j]).collect();
            members.sort_unstable();
            let names = pairs.into_iter().map(|(_, _, n)| n).collect();
            out.push(Fold {
                members,
                kind: FoldKind::Branches(names),
            });
        }
        // (b) Two or more resets in a row that did not move the branch.
        let mut run: Vec<usize> = Vec::new();
        let flush = |run: &mut Vec<usize>, out: &mut Vec<Fold>| {
            if run.len() >= 2 {
                out.push(Fold {
                    members: std::mem::take(run),
                    kind: FoldKind::Resets,
                });
            }
            run.clear();
        };
        for &i in &list {
            let same = run.first().is_none_or(|&f| line[f].what == line[i].what);
            if foldable(i) && kind_of(i) == Some(GitEventKind::Reset) && !taken.contains(&i) {
                if !same {
                    flush(&mut run, &mut out);
                }
                run.push(i);
            } else {
                flush(&mut run, &mut out);
            }
        }
        flush(&mut run, &mut out);
    }
    out
}

fn summary_row(line: &[Row], fold: &Fold, tr: Tr) -> Row {
    let first = &line[fold.members[0]];
    let what = match &fold.kind {
        FoldKind::Branches(names) if names.len() == 1 => {
            tr.t("timeline.summary.branch", &[("first", &names[0])])
        }
        FoldKind::Branches(names) => tr.t(
            "timeline.summary.branches",
            &[
                ("count", &names.len()),
                ("first", &names[0]),
                ("more", &(names.len() - 1)),
            ],
        ),
        FoldKind::Resets => tr.t(
            "timeline.summary.resets",
            &[("what", &first.what), ("count", &fold.members.len())],
        ),
    };
    let same_actor = fold.members.iter().all(|&i| line[i].who == first.who);
    let you_or_other = fold.members.iter().any(|&i| line[i].you_or_other);
    Row {
        day: first.day,
        date: first.date.clone(),
        time: first.time.clone(),
        worktree: first.worktree.clone(),
        what,
        who: if same_actor {
            first.who.clone()
        } else {
            tr.t("timeline.actor.several", &[])
        },
        you_or_other: same_actor && you_or_other,
        level: first.level.clone(),
        action: tr.t("timeline.action.none", &[]),
        details: Vec::new(),
    }
}

fn row_of(entry: &TimelineEntry, detection_available: bool, verbose: bool, tr: Tr) -> Row {
    let (date, time) = date_time(entry.occurred_utc_ms, entry.utc_offset_s);
    let worktrees: Vec<String> = entry
        .worktrees
        .iter()
        .map(|w| worktree_name(&w.sanitized()))
        .collect();
    let (who, you_or_other) = who(entry, detection_available, verbose, tr);
    let action = match &entry.protection.snapshot_id {
        Some(id) => {
            let id = sanitize(id);
            let id = if verbose {
                id
            } else {
                short_id(&id, entry.short_id_len.unwrap_or(DEFAULT_ID_LEN))
            };
            tr.t("timeline.action.restore", &[("id", &id)])
        }
        None => tr.t("timeline.action.none", &[]),
    };
    let level_key = |key: &str| tr.t(key, &[]);
    let level = match (verbose, entry.protection.level) {
        (false, ProtectionLevel::GuaranteedPrior | ProtectionLevel::HookPrior) => {
            level_key("timeline.level.short.prior")
        }
        (false, ProtectionLevel::Observation) => level_key("timeline.level.short.observation"),
        (false, ProtectionLevel::Manual) => level_key("timeline.level.short.manual"),
        (false, ProtectionLevel::None) => level_key("timeline.level.short.none"),
        (true, ProtectionLevel::GuaranteedPrior | ProtectionLevel::HookPrior) => {
            level_key("timeline.level.prior")
        }
        (true, ProtectionLevel::Observation) => level_key("timeline.level.observation"),
        (true, ProtectionLevel::Manual) => level_key("timeline.level.manual"),
        (true, ProtectionLevel::None) => level_key("timeline.level.none"),
    };
    Row {
        day: LocalDay::of(entry.occurred_utc_ms, entry.utc_offset_s),
        date,
        time,
        worktree: cap(&worktrees.join(", ")),
        what: what(&entry.origin, tr),
        who: if verbose { who } else { cap(&who) },
        level,
        action,
        details: if verbose { details(entry, tr) } else { Vec::new() },
        you_or_other,
    }
}

/// `YYYY-MM-DD` and `HH:MM` of an instant at its own offset.
fn date_time(utc_ms: i64, offset_s: i32) -> (String, String) {
    let text = local_time(utc_ms, offset_s);
    let (date, clock) = text.split_once(' ').unwrap_or((&text, ""));
    (date.to_owned(), clock.chars().take(5).collect())
}

/// At most [`COLUMN_CAP`] characters, ending in `…` when cut.
fn cap(text: &str) -> String {
    if text.chars().count() <= COLUMN_CAP {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(COLUMN_CAP - 1).collect();
    cut.push('…');
    cut
}

/// The last component of a worktree root; the whole text when it has none.
pub(crate) fn worktree_name(root: &str) -> String {
    let name = root.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next();
    name.filter(|n| !n.is_empty()).unwrap_or(root).to_owned()
}

/// The lines under an entry in `--verbose`: what an undo took back, and the changed files only
/// when there are some to show.
fn details(entry: &TimelineEntry, tr: Tr) -> Vec<String> {
    let mut out = Vec::new();
    if let EntryOrigin::Operation { acted_on, .. } = &entry.origin
        && !acted_on.is_empty()
    {
        let targets: Vec<String> = acted_on.iter().map(|t| acted_text(t, tr)).collect();
        out.push(tr.t("timeline.acted-on", &[("targets", &targets.join(", "))]));
    }
    if let ChangedFiles::Available {
        paths,
        total,
        first_parent,
    } = &entry.files
    {
        let mut items: Vec<String> = paths.iter().map(|p| p.sanitized()).collect();
        let hidden = (*total as usize).saturating_sub(paths.len());
        if hidden > 0 {
            items.push(tr.t("timeline.files-more", &[("count", &hidden)]));
        }
        if !items.is_empty() {
            let mut text = tr.t(
                "timeline.files",
                &[("total", total), ("paths", &items.join(", "))],
            );
            if *first_parent {
                text.push_str(&format!(" ({})", tr.t("timeline.files-first-parent", &[])));
            }
            out.push(text);
        }
    }
    out
}

fn acted_text(target: &ActedOn, tr: Tr) -> String {
    match target {
        ActedOn::Operation(id) => tr.t("timeline.acted.operation", &[("id", &sanitize(id))]),
        ActedOn::GitEvent(seq) => tr.t("timeline.acted.event", &[("seq", seq)]),
        ActedOn::Snapshot(id) => tr.t("timeline.acted.snapshot", &[("id", &sanitize(id))]),
    }
}

/// What happened, in words. Also the text of a candidate of an ambiguous prefix.
pub(crate) fn what(origin: &EntryOrigin, tr: Tr) -> String {
    match origin {
        EntryOrigin::ManualSnapshot { label, channel, .. } => tr.t(
            "timeline.manual",
            &[
                ("label", &label.sanitized()),
                ("channel", &channel_text(*channel)),
            ],
        ),
        EntryOrigin::Operation {
            kind,
            subtype,
            state,
            ..
        } => {
            let mut text = match (kind, subtype) {
                (TimelineOperationKind::Protected, Some(subtype)) => tr.t(
                    "timeline.op.protected",
                    &[("subtype", &subtype.sanitized())],
                ),
                (TimelineOperationKind::Protected, None) => tr.t("timeline.op.protected-unnamed", &[]),
                (TimelineOperationKind::Undo, _) => tr.t("timeline.op.undo", &[]),
                (TimelineOperationKind::Redo, _) => tr.t("timeline.op.redo", &[]),
                (TimelineOperationKind::Restore, _) => tr.t("timeline.op.restore", &[]),
            };
            match state {
                TimelineOperationState::Finished => {}
                TimelineOperationState::Applying => {
                    text.push_str(&format!(" ({})", tr.t("timeline.state.applying", &[])));
                }
                TimelineOperationState::Interrupted => {
                    text.push_str(&format!(" ({})", tr.t("timeline.state.interrupted", &[])));
                }
            }
            text
        }
        EntryOrigin::GitEvent { kind, branch, .. } => {
            let known = branch.is_some();
            let branch = branch
                .as_ref()
                .map_or_else(|| tr.t("timeline.branch-unknown", &[]), |b| b.sanitized());
            match kind {
                GitEventKind::BranchSwitch => tr.t("event.branch-switch-to", &[("branch", &branch)]),
                GitEventKind::Reset if !known => tr.t("event.reset-detached", &[]),
                kind => tr.t(&format!("event.{}", kind.as_str()), &[("branch", &branch)]),
            }
        }
    }
}

/// The wire name of the surface; it is a fixed set, so it is not translated.
fn channel_text(channel: TimelineChannel) -> &'static str {
    match channel {
        TimelineChannel::Cli => "cli",
        TimelineChannel::Tui => "tui",
        TimelineChannel::Mcp => "mcp",
        TimelineChannel::Hook => "hook",
    }
}

/// Who, and whether it reads "you or other". An unattributed change is never the human: it may
/// be the developer or another tool (BR-TMC-CONS-005). The compact view names the agent alone;
/// `--verbose` adds how it was told, and the hint of an agent inferred without evidence.
fn who(entry: &TimelineEntry, detection_available: bool, verbose: bool, tr: Tr) -> (String, bool) {
    let you_or_other = || (tr.t("timeline.actor.you-or-other", &[]), true);
    match &entry.actor {
        Actor::Unattributed => match &entry.inferred {
            Some(hint) if verbose => (inferred_text(hint, tr), false),
            Some(_) => you_or_other(),
            None if !detection_available && matches!(entry.attribution, gitraptor_api::timemachine::Attribution::Current) => {
                (tr.t("timeline.actor-unavailable", &[]), false)
            }
            None => you_or_other(),
        },
        Actor::Agent { kind, name, origin } => {
            let agent = match name {
                Some(name) => name.sanitized(),
                None => tr.t(&format!("actor.{}", wire(kind)), &[]),
            };
            let text = if verbose {
                tr.t(
                    "actor.with-origin",
                    &[
                        ("origin", &tr.t(&format!("origin.{}", wire(origin)), &[])),
                        ("agent", &agent),
                    ],
                )
            } else {
                agent
            };
            (text, false)
        }
    }
}

/// "no agent; inferred: Claude Code (confirmed by the trailer)", as `raptor events` says it.
fn inferred_text(hint: &InferredAgent, tr: Tr) -> String {
    let kind: &AgentKind = &hint.kind;
    let agent = tr.t(&format!("actor.{}", wire(kind)), &[]);
    let agent = match hint.trailer {
        Some(TrailerCheck::Confirmed) => {
            format!("{agent} ({})", tr.t("events.inferred_confirmed", &[]))
        }
        Some(TrailerCheck::Unconfirmed) => {
            format!("{agent} ({})", tr.t("events.inferred_unconfirmed", &[]))
        }
        None => agent,
    };
    tr.t("events.no_agent_inferred", &[("agent", &agent)])
}
