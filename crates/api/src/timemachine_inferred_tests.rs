//! Contract test of the inferred hint on a timeline entry: on the wire only
//! when present, and it never changes the actor.

use serde_json::Value;

use crate::Untrusted;
use crate::actor::{Actor, AgentKind};
use crate::messages::{GitEventKind, InferredAgent, TrailerCheck};
use crate::timemachine::{
    Attribution, ChangedFiles, EntryOrigin, Protection, ProtectionLevel, TimelineEntry,
};

fn git_event_entry(inferred: Option<InferredAgent>) -> TimelineEntry {
    TimelineEntry {
        id: "event:42".into(),
        origin: EntryOrigin::GitEvent {
            seq: 42,
            kind: GitEventKind::Commit,
            branch: None,
        },
        occurred_utc_ms: 1,
        utc_offset_s: 0,
        worktrees: vec![Untrusted::new("/r")],
        actor: Actor::Unattributed,
        inferred,
        attribution: Attribution::Current,
        protection: Protection {
            level: ProtectionLevel::None,
            snapshot_id: None,
        },
        files: ChangedFiles::Unavailable,
    }
}

fn hint(trailer: Option<TrailerCheck>) -> InferredAgent {
    InferredAgent {
        kind: AgentKind::ClaudeCode,
        session_id: "20:2000".into(),
        trailer,
    }
}

#[test]
fn a_timeline_entry_carries_the_hint_only_when_present() {
    for (trailer, text) in [
        (TrailerCheck::Confirmed, "confirmed"),
        (TrailerCheck::Unconfirmed, "unconfirmed"),
    ] {
        let entry = git_event_entry(Some(hint(Some(trailer))));
        let v = serde_json::to_value(&entry).unwrap();
        assert_eq!(
            v["actor"]["actor"], "unattributed",
            "the hint never changes the actor"
        );
        assert_eq!(v["inferred"]["kind"], "claude-code");
        assert_eq!(v["inferred"]["session_id"], "20:2000");
        assert_eq!(v["inferred"]["trailer"], text);
        let back: TimelineEntry = serde_json::from_value(v).unwrap();
        assert_eq!(back, entry);
    }

    // Without a hint the key is not on the wire: a client that did not
    // accept the capability reads today's shape (`deny_unknown_fields`).
    let entry = git_event_entry(None);
    let v = serde_json::to_value(&entry).unwrap();
    let Value::Object(map) = &v else {
        panic!("an object: {v}");
    };
    assert!(!map.contains_key("inferred"), "no inferred key: {v}");
    let back: TimelineEntry = serde_json::from_value(v).unwrap();
    assert_eq!(back, entry);
}
