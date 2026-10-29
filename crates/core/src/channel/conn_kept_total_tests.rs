//! The kept count of a finished undo is the real total: the list is cut at `MAX_KEPT_PATHS`, its
//! total travels as `kept_total` (omitted when the list is whole), and the MCP count comes from
//! the full report, never from the cut list. Child module of `channel::conn`.

use super::*;
use crate::timemachine::apply::ApplyReport;
use crate::timemachine::oplog::{OpRef, Requester};
use gitraptor_api::timemachine::ResolvedVia;

fn done(kept: usize) -> UndoDone {
    UndoDone {
        operation_id: "op-undo".into(),
        prior_snapshot_id: "prior".into(),
        undone: Undone {
            op: OpRef::Oplog("op-1".into()),
            id: "op-1".into(),
            subtype: Some("reset-hard".into()),
            requester: Requester::Unattributed,
            at_ms: Some(1),
        },
        target_snapshot_id: "target".into(),
        report: ApplyReport {
            kept: (0..kept)
                .map(|i| ("main".to_owned(), format!("src/k{i:04}.rs")))
                .collect(),
            ..ApplyReport::default()
        },
        warnings: Vec::new(),
    }
}

fn requester() -> RequesterView {
    RequesterView {
        actor: gitraptor_api::Actor::Unattributed,
        channel: RequestChannel::Cli,
        via: ResolvedVia::Ancestry,
        confirmable: false,
    }
}

#[test]
fn a_cut_list_carries_the_real_total() {
    let all = MAX_KEPT_PATHS + 37;
    let result = undo_result(done(all), requester(), true);
    assert_eq!(result.kept.len(), MAX_KEPT_PATHS, "the list is cut");
    assert_eq!(result.kept_total, all as u64, "the total is the real one");
    assert_eq!(
        result.for_mcp().kept,
        all as u64,
        "the MCP count is the full report's, not the cut list's"
    );
}

#[test]
fn a_whole_list_omits_the_total() {
    let result = undo_result(done(3), requester(), true);
    assert_eq!(result.kept.len(), 3);
    assert_eq!(result.kept_total, 0);
    assert_eq!(result.for_mcp().kept, 3);
    let wire = serde_json::to_value(&result).unwrap();
    assert!(
        wire.get("kept_total").is_none(),
        "0 is omitted on the wire: {wire}"
    );
}

#[test]
fn without_the_capability_nothing_kept_travels() {
    let result = undo_result(done(MAX_KEPT_PATHS + 1), requester(), false);
    assert!(result.kept.is_empty());
    assert_eq!(result.kept_total, 0);
    let wire = serde_json::to_value(&result).unwrap();
    assert!(wire.get("kept").is_none() && wire.get("kept_total").is_none());
}
