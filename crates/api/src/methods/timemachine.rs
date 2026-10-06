//! The Time Machine's commands.

use super::{BASE, Group, MethodSpec, RepoWrite};

pub const TM_SNAPSHOT: &str = "timemachine.snapshot";
pub const TM_UNDO: &str = "timemachine.undo";
pub const TM_REDO: &str = "timemachine.redo";
pub const TM_RESTORE: &str = "timemachine.restore";
pub const TM_TIMELINE: &str = "timemachine.timeline";

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
    ..Group::new("timemachine")
};
