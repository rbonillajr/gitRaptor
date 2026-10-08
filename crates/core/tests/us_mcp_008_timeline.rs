//! The manual points in the timeline (DS-US-MCP-008 T004): the entry with its label and channel,
//! the filters, the connection without the capability and the point that never serves another
//! worktree.
//!
//! Every test runs on a testkit fixture: a temporary repo, home and profile, never this repo or
//! the real profile (NFR-01).

mod tm_common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gitraptor_api::messages::{GitEventDetails, GitEventKind, GitEventView};
use gitraptor_api::timemachine::{EntryOrigin, ProtectionLevel, TimelineChannel, TimelineResult};
use gitraptor_api::{Actor, Untrusted};
use gitraptor_core::timemachine::manual::{self, DAY_MS, ManualAsk, ManualCaptured, ManualError};
use gitraptor_core::timemachine::oplog::{Channel, Requester, RequesterOrigin};
use gitraptor_core::timemachine::timeline::{
    AgentFilter, EngineSide, SessionActors, TimelineQuery, build_timeline, without_manual,
};
use gitraptor_core::timemachine::undo::RawSide;
use gitraptor_testkit::Fixture;
use tm_common::{Env, REPO_ID, git};

const T0: i64 = 1_728_000_000_000;

fn agent(session: &str) -> Requester {
    Requester::Agent {
        name: "claude".into(),
        origin: RequesterOrigin::Detected,
        session_id: session.into(),
    }
}

fn ask(worktree: &Path, session: &str) -> ManualAsk {
    ManualAsk {
        repo_id: REPO_ID.into(),
        worktree: worktree.to_path_buf(),
        label: "before the migration".into(),
        requester: agent(session),
        channel: Channel::Mcp,
    }
}

fn take_marked(
    env: &Env,
    ask: &ManualAsk,
    mark: Option<i64>,
    now_ms: i64,
) -> Result<ManualCaptured, ManualError> {
    manual::capture_in_store(
        &env.store,
        &env.oplog,
        ask,
        mark,
        false,
        None,
        now_ms,
        Instant::now() + Duration::from_secs(25),
    )
}

fn env() -> Env {
    Env::new(Fixture::with_commit(&git()))
}

fn worktree(env: &Env, name: &str) -> PathBuf {
    env.f.git(&["branch", name]);
    env.f.add_worktree(name, name)
}

// ------------------------------------------------------------------------------------ timeline

fn query() -> TimelineQuery {
    TimelineQuery {
        since_ms: None,
        agent: None,
        only_worktree: None,
        limit: 100,
    }
}

fn timeline(env: &Env, query: &TimelineQuery, engine: Option<&EngineSide>) -> TimelineResult {
    let log = env.oplog.lock().unwrap();
    build_timeline(
        &log,
        &env.store,
        query,
        engine,
        &SessionActors::default(),
        (T0 + DAY_MS, 0),
    )
}

fn event(seq: i64, root: &str, at: i64) -> GitEventView {
    GitEventView {
        repo_id: REPO_ID.into(),
        seq,
        worktree: Untrusted::new(root),
        kind: GitEventKind::Commit,
        actor: Actor::Unattributed,
        observed_utc_ms: at,
        utc_offset_s: 0,
        details: GitEventDetails::default(),
        gap_id: None,
        inferred: None,
        authorship: None,
    }
}

fn engine_of(roots: &[(&str, &str)], events: Vec<GitEventView>) -> EngineSide {
    EngineSide {
        raw: roots
            .iter()
            .map(|(root, _)| ((*root).to_owned(), RawSide::default()))
            .collect(),
        snapshot_keys: roots
            .iter()
            .map(|(root, key)| ((*root).to_owned(), (*key).to_owned()))
            .collect(),
        history_full: false,
        history_oldest_ms: events.iter().map(|e| e.observed_utc_ms).min(),
        events,
        detection_available: true,
    }
}

fn root_of(path: &Path) -> String {
    path.canonicalize().unwrap().to_string_lossy().into_owned()
}

#[test]
fn timeline_shows_a_manual_snapshot_with_its_label_and_channel() {
    let env = env();
    let point = take_marked(&env, &ask(&env.f.repo, "s1"), Some(3), T0).unwrap();
    let result = timeline(&env, &query(), None);

    let entry = result
        .entries
        .iter()
        .find(|e| matches!(e.origin, EntryOrigin::ManualSnapshot { .. }))
        .expect("the manual point is an entry");
    let EntryOrigin::ManualSnapshot {
        snapshot_id,
        label,
        channel,
    } = &entry.origin
    else {
        unreachable!()
    };
    assert_eq!(snapshot_id, &point.snapshot_id);
    assert_eq!(label.raw(), "before the migration");
    assert_eq!(*channel, TimelineChannel::Mcp);
    assert_eq!(entry.occurred_utc_ms, T0);
    assert_eq!(entry.protection.level, ProtectionLevel::Manual);
    assert_eq!(
        entry.protection.snapshot_id.as_deref(),
        Some(snapshot_id.as_str())
    );
    match &entry.actor {
        Actor::Agent { name, .. } => assert_eq!(name.as_ref().map(|n| n.raw()), Some("claude")),
        other => panic!("the recorded requester is the actor, got {other:?}"),
    }
    assert_eq!(
        entry.worktrees.iter().map(|w| w.raw()).collect::<Vec<_>>(),
        [root_of(&env.f.repo)]
    );

    // The filters apply as to an operation.
    assert!(
        timeline(
            &env,
            &TimelineQuery {
                since_ms: Some(T0 + 1),
                ..query()
            },
            None
        )
        .entries
        .is_empty()
    );
    let other = root_of(&worktree(&env, "elsewhere"));
    assert!(
        timeline(
            &env,
            &TimelineQuery {
                only_worktree: Some(other),
                ..query()
            },
            None
        )
        .entries
        .is_empty()
    );
    let named = TimelineQuery {
        agent: Some(AgentFilter::parse("claude")),
        only_worktree: Some(root_of(&env.f.repo)),
        ..query()
    };
    assert_eq!(timeline(&env, &named, None).entries.len(), 1);
    let nobody = TimelineQuery {
        agent: Some(AgentFilter::Unattributed),
        ..query()
    };
    assert!(timeline(&env, &nobody, None).entries.is_empty());
}

#[test]
fn without_the_capability_the_timeline_is_unchanged() {
    let env = env();
    let root = root_of(&env.f.repo);
    let engine = engine_of(&[(root.as_str(), "main")], vec![event(10, &root, T0 + 5)]);
    let baseline = timeline(&env, &query(), Some(&engine));

    take_marked(&env, &ask(&env.f.repo, "s1"), Some(5), T0).unwrap();
    let mut with_point = timeline(&env, &query(), Some(&engine));
    assert!(
        with_point
            .entries
            .iter()
            .any(|e| matches!(e.origin, EntryOrigin::ManualSnapshot { .. }))
    );
    // The event is protected by the manual point, as long as the connection knows the level.
    let protected = with_point
        .entries
        .iter()
        .find(|e| e.id == "event:10")
        .unwrap();
    assert_eq!(protected.protection.level, ProtectionLevel::Manual);

    let point_id = protected.protection.snapshot_id.clone();

    without_manual(&mut with_point);
    // The entries of before, and the event protected by the point as an observation, as an old
    // client has always seen a point it does not know.
    assert_eq!(
        with_point.entries.len(),
        baseline.entries.len(),
        "the same entries as without the manual point"
    );
    let seen = with_point
        .entries
        .iter()
        .find(|e| e.id == "event:10")
        .unwrap();
    assert_eq!(seen.protection.level, ProtectionLevel::Observation);
    assert_eq!(seen.protection.snapshot_id, point_id);
    assert!(
        !with_point
            .entries
            .iter()
            .any(|e| matches!(e.origin, EntryOrigin::ManualSnapshot { .. })),
        "no manual entry reaches a connection without the capability"
    );
    assert!(
        !serde_json::to_string(&with_point)
            .unwrap()
            .contains("manual"),
        "the wire is the one of before: no manual level, no manual entry"
    );
}

#[test]
fn a_manual_point_of_one_worktree_does_not_serve_another() {
    let env = env();
    let side = worktree(&env, "side");
    let main_root = root_of(&env.f.repo);
    let side_root = root_of(&side);
    // A manual point of the main worktree only, before both events.
    take_marked(&env, &ask(&env.f.repo, "s1"), Some(5), T0).unwrap();
    let side_key = {
        // The key the store gives the linked worktree: take a point of it to learn it.
        let probe = take_marked(&env, &ask(&side, "s9"), Some(1), T0 + 1).unwrap();
        env.store.meta(&probe.snapshot_id).unwrap().scope[0].clone()
    };
    let engine = engine_of(
        &[
            (main_root.as_str(), "main"),
            (side_root.as_str(), side_key.as_str()),
        ],
        vec![
            event(10, &main_root, T0 + 10),
            event(11, &side_root, T0 + 11),
        ],
    );
    let result = timeline(&env, &query(), Some(&engine));
    let level = |id: &str| {
        result
            .entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| (e.protection.level, e.protection.snapshot_id.clone()))
            .unwrap()
    };
    assert_eq!(level("event:10").0, ProtectionLevel::Manual);
    // The linked worktree's only point has mark 1: it is before the event 11 and its own.
    let (side_level, side_point) = level("event:11");
    assert_eq!(side_level, ProtectionLevel::Manual);
    let main_point = level("event:10").1;
    assert_ne!(
        side_point, main_point,
        "the main worktree's point never protects the other worktree's event"
    );

    // And a worktree that has no point of its own has none: no borrowing from the main one.
    let lonely = worktree(&env, "lonely");
    let lonely_root = root_of(&lonely);
    let engine = engine_of(
        &[
            (main_root.as_str(), "main"),
            (lonely_root.as_str(), "wt-lonely"),
        ],
        vec![event(12, &lonely_root, T0 + 12)],
    );
    let result = timeline(&env, &query(), Some(&engine));
    let entry = result.entries.iter().find(|e| e.id == "event:12").unwrap();
    assert_eq!(entry.protection.level, ProtectionLevel::None);
}
