//! The Time Machine's commands.

use super::{BASE, Group, MethodSpec, RepoWrite};
use crate::capability::Capability;

pub const TM_SNAPSHOT: &str = "timemachine.snapshot";
pub const TM_UNDO: &str = "timemachine.undo";
pub const TM_REDO: &str = "timemachine.redo";
pub const TM_RESTORE: &str = "timemachine.restore";
pub const TM_TIMELINE: &str = "timemachine.timeline";

/// `RepoView.kept_temps` in snapshots: temporary entries the sweep after a crash kept
/// (DS-TS-TMC-003, Enmienda T2).
pub const CAP_TM_KEPT_TEMPS: Capability = Capability::new("timemachine.kept-temps");

/// `timemachine.timeline` serves entries `manual-snapshot` and protection level `manual`.
/// A connection without it never receives either (they are filtered out).
pub const CAP_TM_TIMELINE_MANUAL: Capability = Capability::new("timemachine.timeline-manual");

/// `timemachine.undo` and `timemachine.restore` accept `confirmation` and answer a
/// `confirmation-required` rejection with `TmConfirmData`: the one-use challenge or why one
/// cannot be given, and whose work it is; a bad token is `challenge-invalid`. Never over MCP.
pub const CAP_TM_CONFIRMATION: Capability = Capability::new("timemachine.confirmation");
/// `timemachine.timeline` serves `inferred` on the entries of Git events
/// without an agent. A connection without it never receives the field.
pub const CAP_TM_TIMELINE_INFERRED: Capability = Capability::new("timemachine.timeline-inferred");

/// A Time Machine command: not reserved (an agent may undo its own work,
/// ADR-TMC-005 § 2), declared with its parameters and validated, and
/// implemented by its story.
const fn time_machine(
    name: &'static str,
    mcp: bool,
    writes: RepoWrite,
    story: &'static str,
) -> MethodSpec {
    MethodSpec {
        name,
        reserved: false,
        mcp,
        implemented_by: Some(story),
        writes,
        since: BASE,
    }
}

pub(super) const GROUP: Group = Group {
    // Redo, restore and the full timeline are not offered over MCP
    // (Q-MCP-11); the hook snapshot is a CLI command (US-TMC-005).
    methods: &[
        time_machine(TM_SNAPSHOT, false, RepoWrite::None, "US-TMC-005"),
        time_machine(TM_UNDO, true, RepoWrite::TimeMachine, "US-TMC-002"),
        time_machine(TM_REDO, false, RepoWrite::TimeMachine, "US-TMC-003"),
        time_machine(TM_RESTORE, false, RepoWrite::TimeMachine, "US-TMC-009"),
        time_machine(TM_TIMELINE, false, RepoWrite::None, "US-TMC-006"),
    ],
    capabilities: &[
        CAP_TM_KEPT_TEMPS,
        CAP_TM_TIMELINE_MANUAL,
        CAP_TM_CONFIRMATION,
    ],
    ..Group::new("timemachine")
};
