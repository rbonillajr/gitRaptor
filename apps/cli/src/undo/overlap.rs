//! What `raptor undo` prints for an `overlap` / `overlap-unverified` rejection and for the paths
//! an undo kept. Every text from the repo is printed sanitized; no line offers a way to go on.

use gitraptor_api::timemachine::{
    KeptPath, OverlapConflict, OverlapSubject, TmOverlap, TmRejectReason,
};
use gitraptor_api::untrusted::sanitize;
use gitraptor_api::{Actor, AgentOrigin};

use crate::events::local_time;
use crate::i18n::Tr;
use crate::status::wire;

#[cfg(test)]
#[path = "overlap_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "overlap_kept_total_tests.rs"]
mod kept_total_tests;

/// Most conflicts or kept paths listed on screen; the rest is "+K more" from the real total.
const SHOWN: usize = 20;

/// The lines of an overlap rejection: the reason, the undone change and one line per conflict
/// (the first 20 and "+K more" from the real total). `overlap-unverified`, or an `overlap`
/// without its detail, is the reason alone.
pub(super) fn rejection_lines(
    tr: Tr,
    reason: TmRejectReason,
    overlap: Option<&TmOverlap>,
) -> Vec<String> {
    let key = match reason {
        TmRejectReason::OverlapUnverified => "undo.reason.overlap-unverified",
        _ => "undo.reason.overlap",
    };
    let mut lines = vec![tr.t(key, &[])];
    let Some(detail) = overlap.filter(|_| reason == TmRejectReason::Overlap) else {
        return lines;
    };
    let offset = gitraptor_core::timemachine::overlap::local_utc_offset_s();
    let undone = &detail.undone;
    let operation = undone.subtype.as_ref().map_or_else(
        || tr.t("undo.operation-unnamed", &[]),
        gitraptor_api::Untrusted::sanitized,
    );
    let id = sanitize(&undone.id);
    let who = actor_text(tr, &undone.actor);
    lines.push(match undone.at_utc_ms {
        Some(ms) => tr.t(
            "undo.overlap.undone",
            &[
                ("operation", &operation),
                ("id", &id),
                ("actor", &who),
                ("time", &clock(ms, offset)),
            ],
        ),
        None => tr.t(
            "undo.overlap.undone-untimed",
            &[("operation", &operation), ("id", &id), ("actor", &who)],
        ),
    });
    for conflict in detail.conflicts.iter().take(SHOWN) {
        lines.push(conflict_line(tr, conflict, offset));
    }
    let shown = detail.conflicts.len().min(SHOWN);
    let total = usize::try_from(detail.total)
        .unwrap_or(usize::MAX)
        .max(detail.conflicts.len());
    if total > shown {
        lines.push(more(tr, total - shown));
    }
    lines
}

/// The lines announcing the paths an undo left as they were (the first 20 and "+K more"); none
/// when nothing was kept. `print_done` uses [`kept_lines_of`] with the daemon's real total.
#[cfg(test)]
pub(super) fn kept_lines(tr: Tr, kept: &[KeptPath]) -> Vec<String> {
    kept_lines_of(tr, kept, kept.len())
}

/// [`kept_lines`] with the real number of kept paths, `total`, when `kept` was cut.
pub(super) fn kept_lines_of(tr: Tr, kept: &[KeptPath], total: usize) -> Vec<String> {
    if kept.is_empty() {
        return Vec::new();
    }
    let total = total.max(kept.len());
    let mut lines = vec![tr.t("undo.overlap.kept", &[("count", &total)])];
    for k in kept.iter().take(SHOWN) {
        lines.push(tr.t(
            "undo.overlap.kept-path",
            &[
                ("path", &k.path.sanitized()),
                ("worktree", &k.worktree.sanitized()),
            ],
        ));
    }
    let shown = kept.len().min(SHOWN);
    if total > shown {
        lines.push(more(tr, total - shown));
    }
    lines
}

fn conflict_line(tr: Tr, conflict: &OverlapConflict, offset: i32) -> String {
    let actor = actor_text(tr, &conflict.actor);
    let time = clock(conflict.seen_utc_ms, offset);
    match &conflict.subject {
        OverlapSubject::Path { worktree, path } => tr.t(
            "undo.overlap.conflict-path",
            &[
                ("path", &path.sanitized()),
                ("worktree", &worktree.sanitized()),
                ("actor", &actor),
                ("time", &time),
            ],
        ),
        OverlapSubject::Ref { name } => tr.t(
            "undo.overlap.conflict-ref",
            &[
                ("name", &name.sanitized()),
                ("actor", &actor),
                ("time", &time),
            ],
        ),
    }
}

fn more(tr: Tr, count: usize) -> String {
    tr.t("undo.overlap.more", &[("count", &count)])
}

/// `HH:MM:SS` in local time.
fn clock(utc_ms: i64, offset_s: i32) -> String {
    let full = local_time(utc_ms, offset_s);
    full.rsplit(' ').next().unwrap_or(&full).to_owned()
}

/// The actor in `tr`'s language: "no agent" for `unattributed` (D1 of DS-US-TMC-006).
fn actor_text(tr: Tr, actor: &Actor) -> String {
    match actor {
        Actor::Unattributed => tr.t("events.no_agent", &[]),
        Actor::Agent { kind, name, origin } => {
            let agent = match name {
                Some(name) => tr.t("actor.other-agent", &[("name", &name.sanitized())]),
                None => tr.t(&format!("actor.{}", wire(kind)), &[]),
            };
            let origin = match origin {
                AgentOrigin::Detected => tr.t("origin.detected", &[]),
                AgentOrigin::Registered => tr.t("origin.registered", &[]),
            };
            tr.t(
                "actor.with-origin",
                &[("agent", &agent), ("origin", &origin)],
            )
        }
    }
}
