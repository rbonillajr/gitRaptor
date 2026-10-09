//! Contract tests of the inferred hint on the timeline: an entry of a Git
//! event without an agent carries the event's hint, and `without_inferred`
//! removes every hint for a connection without the capability.

use std::io;

use gitraptor_api::messages::{GitEventDetails, InferredAgent, TrailerCheck};

use super::super::oplog::{CompleteInfo, NewOperation, NewSnapshot, OperationTransition, Scope};
use super::*;
use crate::profile::ProfileDirs;

const REPO: &str = "0a1b2c3d-0000-4000-8000-00000000abcd";
const WT: &str = "/repo/feat-login";
const KEY: &str = "wt-feat-login";

struct Refs(Vec<String>);

impl SnapshotRefs for Refs {
    fn snapshot_ids(&self) -> io::Result<Option<Vec<String>>> {
        Ok(Some(self.0.clone()))
    }
    fn delete(&mut self, _: &[String]) -> io::Result<()> {
        Ok(())
    }
}

fn hint(trailer: TrailerCheck) -> InferredAgent {
    InferredAgent {
        kind: AgentKind::ClaudeCode,
        session_id: "20:2000".into(),
        trailer: Some(trailer),
    }
}

fn event(seq: i64, at: i64, actor: Actor, inferred: Option<InferredAgent>) -> GitEventView {
    GitEventView {
        repo_id: REPO.into(),
        seq,
        worktree: Untrusted::new(WT),
        kind: GitEventKind::Commit,
        actor,
        observed_utc_ms: at,
        utc_offset_s: 0,
        details: GitEventDetails::default(),
        gap_id: None,
        inferred,
        authorship: None,
    }
}

fn engine(events: Vec<GitEventView>) -> EngineSide {
    EngineSide {
        raw: HashMap::from([(WT.to_owned(), RawSide::default())]),
        snapshot_keys: HashMap::from([(WT.to_owned(), KEY.to_owned())]),
        history_full: false,
        history_oldest_ms: events.iter().map(|e| e.observed_utc_ms).min(),
        events,
        detection_available: true,
    }
}

fn query() -> TimelineQuery {
    TimelineQuery {
        since_ms: None,
        agent: None,
        only_worktree: None,
        limit: 50,
    }
}

fn detected(kind: AgentKind) -> Actor {
    Actor::Agent {
        kind,
        name: None,
        origin: AgentOrigin::Detected,
    }
}

/// A finished protected operation of nobody, with its prior point.
fn finished_operation(log: &mut Oplog, refs: &mut Refs, at: i64) {
    let new = NewOperation {
        kind: OperationKind::Protected,
        subtype: Some("reset-hard".to_owned()),
        scope: Scope {
            worktrees: vec![WT.into()],
            refs: vec![],
        },
        requester: Requester::Unattributed,
        channel: Channel::Cli,
        confirmed: false,
        target: Target::None,
        warnings: vec![],
        engine_mark: 1,
    };
    let id = log.record_operation(&new, at).unwrap();
    let snap = log
        .begin_snapshot(
            &NewSnapshot {
                level: SnapshotLevel::GuaranteedPrior,
                worktrees: vec![KEY.into()],
                engine_mark: Some(1),
                cause_operation: Some(id.clone()),
                cause_event_seq: None,
            },
            at,
        )
        .unwrap();
    log.complete_snapshot(&snap, &CompleteInfo::default(), at)
        .unwrap();
    refs.0.push(snap.clone());
    for t in [
        OperationTransition::PriorSnapshot { snapshot_id: &snap },
        OperationTransition::Ready,
        OperationTransition::Applying { step: 1 },
        OperationTransition::Finished,
    ] {
        log.advance_operation(&id, t, at).unwrap();
    }
}

#[test]
fn a_git_event_entry_carries_its_inferred_hint() {
    let tmp = tempfile::tempdir().unwrap();
    let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
    let (mut log, _) = Oplog::open(&dirs, REPO, 1_000).unwrap();
    let mut refs = Refs(Vec::new());
    finished_operation(&mut log, &mut refs, 10);

    let confirmed = hint(TrailerCheck::Confirmed);
    let engine = engine(vec![
        event(1, 20, Actor::Unattributed, Some(confirmed.clone())),
        // The same hint on an event an agent made: the agent wins, no hint.
        event(
            2,
            30,
            detected(AgentKind::ClaudeCode),
            Some(confirmed.clone()),
        ),
        event(3, 40, Actor::Unattributed, None),
    ]);
    let t = build_timeline(
        &log,
        &refs,
        &query(),
        Some(&engine),
        &SessionActors::default(),
        (5_000, 0),
    );
    let by_id = |id: &str| {
        t.entries
            .iter()
            .find(|e| e.id == id)
            .unwrap_or_else(|| panic!("{id} in {:?}", t.entries))
    };
    let unattributed = by_id("event:1");
    assert_eq!(unattributed.actor, Actor::Unattributed);
    assert_eq!(unattributed.inferred.as_ref(), Some(&confirmed));
    assert_eq!(by_id("event:2").inferred, None, "an agent's entry");
    assert_eq!(by_id("event:3").inferred, None, "no hint stored");
    let operations: Vec<_> = t
        .entries
        .iter()
        .filter(|e| matches!(e.origin, EntryOrigin::Operation { .. }))
        .collect();
    assert_eq!(operations.len(), 1, "{:?}", t.entries);
    assert_eq!(operations[0].inferred, None, "an operation never has one");
}

#[test]
fn without_inferred_removes_every_hint() {
    let entry = |id: &str, trailer: TrailerCheck| TimelineEntry {
        id: id.into(),
        origin: EntryOrigin::GitEvent {
            seq: 1,
            kind: GitEventKind::Commit,
            branch: None,
        },
        occurred_utc_ms: 1,
        utc_offset_s: 0,
        worktrees: vec![Untrusted::new(WT)],
        actor: Actor::Unattributed,
        inferred: Some(hint(trailer)),
        attribution: Attribution::Current,
        protection: Protection {
            level: ProtectionLevel::None,
            snapshot_id: None,
        },
        files: ChangedFiles::Unavailable,
    };
    let mut result = TimelineResult {
        repo_id: REPO.into(),
        entries: vec![
            entry("event:1", TrailerCheck::Confirmed),
            entry("event:2", TrailerCheck::Unconfirmed),
        ],
        truncated: false,
        unavailable: Vec::new(),
        detection_available: true,
    };
    without_inferred(&mut result);
    assert_eq!(result.entries.len(), 2, "no entry is dropped");
    for e in &result.entries {
        assert_eq!(e.inferred, None, "{}", e.id);
        assert_eq!(e.actor, Actor::Unattributed, "the actor stays");
    }
}
