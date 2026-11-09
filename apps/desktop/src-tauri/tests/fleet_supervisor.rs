//! D4 (DS-US-DSK-004 § 9.4): the repo scope of the engine supervisor end to end against the fake
//! daemon (`support/fake_daemon.rs`, scripts "tienda" and "notas"), through a real Unix socket.
//!
//! Every test collects the `EngineMessage`s of `Sink::Collect` and waits for them with a 5 s
//! deadline, never with a fixed sleep. Each test ends with the common assertion that the app
//! never called `daemon.replace`, `daemon.stop`, `repo.add` or a method with writes. The keys the
//! tests expect are computed here, independently, from the raw root of each worktree (FNV-1a 64).
//! Temporary folders only (NFR-01).
#![cfg(unix)]

#[path = "support/fake_daemon.rs"]
mod fake_daemon;

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use fake_daemon::{
    FakeDaemon, NOTAS_MAIN, REPO_RUN, REPO_SEQ, TIENDA_LOGIN, TIENDA_MAIN, TIENDA_PAGOS,
    assert_read_only, script_tienda_and_notas, session_view, worktree_view,
};
use gitraptor_api::Untrusted;
use gitraptor_api::actor::{Actor, AgentKind, AgentOrigin};
use gitraptor_api::messages::{
    ResyncReason as ApiResync, SessionStateView, SessionView, SessionsListParams,
    WorktreeStateData, WorktreeView,
};
use gitraptor_api::prediction::{
    PairLevel as ApiLevel, PairSideView, PairStateView, PairSummary, PredictionChanged,
};
use gitraptor_api::scope::{ConnectionRequester, Scope, ScopeSubscribeParams};
use gitraptor_desktop::engine::supervisor::{self, HubCmd, Sink, SupervisorHandle};
use gitraptor_desktop::engine::wire::{
    AgentDto, ConnState, DivergenceDto, EngineMessage, HeadDto, PairDto, PairLevel, PairState,
    RepoFleetDto, SessionDto, SessionState, WorktreeDto, WorktreeStatusDto,
};
use serde_json::Value;

const DEADLINE: Duration = Duration::from_secs(5);

/// FNV-1a 64 of the raw bytes, 16 lowercase hex digits: the key of a worktree (DS-US-CKP-001 § 2).
fn fnv(raw: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in raw.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

struct Harness {
    rx: Receiver<EngineMessage>,
    seen: Vec<EngineMessage>,
    daemon: Arc<FakeDaemon>,
    handle: Option<SupervisorHandle>,
}

impl Harness {
    fn spawn(daemon: &Arc<FakeDaemon>) -> Self {
        let (tx, rx) = mpsc::channel();
        let handle = supervisor::spawn(Box::new(daemon.connector()), None, Sink::Collect(tx));
        Self {
            rx,
            seen: Vec::new(),
            daemon: Arc::clone(daemon),
            handle: Some(handle),
        }
    }

    /// A harness that is already live.
    fn live(daemon: &Arc<FakeDaemon>) -> Self {
        let mut h = Self::spawn(daemon);
        h.wait_live();
        h
    }

    /// Consumes messages until `pick` returns a value, for at most 5 s.
    fn wait_for<T>(&mut self, what: &str, mut pick: impl FnMut(&EngineMessage) -> Option<T>) -> T {
        let deadline = Instant::now() + DEADLINE;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(message) => {
                    let found = pick(&message);
                    self.seen.push(message);
                    if let Some(found) = found {
                        return found;
                    }
                }
                Err(err) => {
                    let tail: Vec<&EngineMessage> = self.seen.iter().rev().take(6).collect();
                    panic!(
                        "waiting for {what}: the supervisor sent nothing more ({err}); \
                         last messages (newest first): {tail:?}"
                    );
                }
            }
        }
    }

    fn wait_live(&mut self) {
        self.wait_for("`conn live`", |m| {
            matches!(
                m,
                EngineMessage::Conn {
                    state: ConnState::Live,
                    ..
                }
            )
            .then_some(())
        });
    }

    fn open(&self, repo_id: &str) {
        let handle = self.handle.as_ref().expect("supervisor handle");
        assert!(
            handle.send(HubCmd::Open {
                repo_id: repo_id.to_owned()
            }),
            "the supervisor thread ended before Open"
        );
    }

    fn wait_repo(&mut self, repo_id: &str) -> RepoFleetDto {
        let id = repo_id.to_owned();
        self.wait_for(
            &format!("a `repo` message of {repo_id}"),
            move |m| match m {
                EngineMessage::Repo { fleet } if fleet.repo_id == id => Some(fleet.clone()),
                _ => None,
            },
        )
    }

    fn wait_sessions(&mut self, repo_id: &str) -> (bool, Vec<SessionDto>) {
        let id = repo_id.to_owned();
        self.wait_for(
            &format!("a `sessions` message of {repo_id}"),
            move |m| match m {
                EngineMessage::Sessions {
                    repo_id,
                    detection_available,
                    sessions,
                } if *repo_id == id => Some((*detection_available, sessions.clone())),
                _ => None,
            },
        )
    }

    fn wait_worktrees(&mut self, repo_id: &str) -> (Vec<WorktreeDto>, Option<u64>, u64) {
        let id = repo_id.to_owned();
        self.wait_for(
            &format!("a `worktrees` message of {repo_id}"),
            move |m| match m {
                EngineMessage::Worktrees {
                    repo_id,
                    worktrees,
                    fetched_utc_ms,
                    perf,
                } if *repo_id == id => Some((worktrees.clone(), *fetched_utc_ms, perf.backend_ns)),
                _ => None,
            },
        )
    }

    fn wait_closed(&mut self, repo_id: &str) {
        let id = repo_id.to_owned();
        self.wait_for(&format!("`repoClosed` of {repo_id}"), move |m| {
            matches!(m, EngineMessage::RepoClosed { repo_id } if *repo_id == id).then_some(())
        });
    }

    /// Opens `repo_id` and waits for its `repo` and `sessions`.
    fn open_and_settle(&mut self, repo_id: &str) -> (RepoFleetDto, Vec<SessionDto>) {
        self.open(repo_id);
        let fleet = self.wait_repo(repo_id);
        let (_, sessions) = self.wait_sessions(repo_id);
        (fleet, sessions)
    }
}

impl Drop for Harness {
    /// The common assertion: the app never writes, whatever the test did.
    fn drop(&mut self) {
        drop(self.handle.take());
        if std::thread::panicking() {
            return;
        }
        assert_read_only(&self.daemon.calls());
    }
}

fn daemon_with_repos() -> Arc<FakeDaemon> {
    let daemon = FakeDaemon::start();
    daemon.script(script_tienda_and_notas);
    daemon
}

fn is_repo_scope(params: &Value, repo_id: &str) -> bool {
    params["scope"]["scope"] == "repo" && params["scope"]["repo_id"] == repo_id
}

/// The position in `calls()` of every `scope.snapshot`, `scope.subscribe` or `sessions.list` of
/// the repo.
fn repo_calls(daemon: &FakeDaemon, method: &str, repo_id: &str) -> Vec<usize> {
    daemon
        .calls()
        .iter()
        .enumerate()
        .filter(|(_, (m, p))| {
            m == method
                && match method {
                    "sessions.list" => p["repo_id"] == repo_id,
                    _ => is_repo_scope(p, repo_id),
                }
        })
        .map(|(i, _)| i)
        .collect()
}

fn worktree_event(repo_id: &str, worktrees: Vec<WorktreeView>) -> WorktreeStateData {
    WorktreeStateData {
        repo_id: repo_id.to_owned(),
        worktrees,
        fetched_utc_ms: Some(1_699_990_500_000),
    }
}

/// `tienda` with `feat-pagos` at `ahead` / `behind`.
fn tienda_with_pagos(ahead: u64, behind: u64) -> Vec<WorktreeView> {
    vec![
        worktree_view(TIENDA_MAIN, true, "main", 0, 0, 0),
        worktree_view(TIENDA_PAGOS, false, "feat-pagos", ahead, behind, 2),
        worktree_view(TIENDA_LOGIN, false, "feat-login", 1, 0, 0),
    ]
}

fn ahead_behind(w: &WorktreeDto) -> Option<(u32, u32)> {
    match &w.status {
        WorktreeStatusDto::Ready(ready) => match ready.divergence {
            DivergenceDto::Counted { ahead, behind } => Some((ahead.count, behind.count)),
            _ => None,
        },
        WorktreeStatusDto::Unavailable { .. } => None,
    }
}

fn by_folder<'a>(worktrees: &'a [WorktreeDto], folder: &str) -> &'a WorktreeDto {
    worktrees
        .iter()
        .find(|w| w.folder == folder)
        .unwrap_or_else(|| panic!("no worktree with folder {folder} in {worktrees:?}"))
}

fn pair_summary(id: &str, a: &str, b: Option<&str>, level: ApiLevel) -> PairSummary {
    PairSummary {
        pair_id: id.to_owned(),
        rev: REPO_SEQ + 1,
        a: PairSideView::Worktree {
            root: Untrusted::new(a),
        },
        b: match b {
            Some(root) => PairSideView::Worktree {
                root: Untrusted::new(root),
            },
            None => PairSideView::Base {},
        },
        state: PairStateView::Current,
        reason: None,
        level: Some(level),
        computed_at_utc_ms: Some(1_700_000_300_000),
        overlap_files: 1,
        overlap_partial: false,
        conflict_files: 1,
        conflict_truncated: false,
    }
}

/// No character that acts on a terminal or flips text, and no JSON escape of one.
fn assert_no_raw_escape(json: &str, what: &str) {
    assert!(
        !json.contains('\u{202e}') && !json.contains("\\u001b") && !json.contains("\\u0007"),
        "D4-SAFE: {what} carries a raw escape: {json}"
    );
}

// ---------------------------------------------------------------- D4-OPEN

#[test]
fn opening_a_repo_delivers_its_worktrees_and_sessions_with_matching_keys() {
    let daemon = daemon_with_repos();
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    let fleet = h.wait_repo("tienda");
    let (detection, sessions) = h.wait_sessions("tienda");

    assert_eq!(fleet.name, "tienda", "D4-OPEN: the repo name is its folder");
    assert_eq!(
        fleet.worktrees.len(),
        3,
        "D4-OPEN: one worktree per working copy: {:?}",
        fleet.worktrees
    );
    let folders: Vec<&str> = fleet.worktrees.iter().map(|w| w.folder.as_str()).collect();
    assert_eq!(
        folders,
        ["tienda", "feat-pagos", "feat-login"],
        "D4-OPEN: the main worktree first, folders from the raw path"
    );
    assert!(
        fleet.worktrees[0].main,
        "D4-OPEN: the first is the main one"
    );
    for (folder, root) in [
        ("tienda", TIENDA_MAIN),
        ("feat-pagos", TIENDA_PAGOS),
        ("feat-login", TIENDA_LOGIN),
    ] {
        assert_eq!(
            by_folder(&fleet.worktrees, folder).key,
            fnv(root),
            "D4-OPEN: the key of {folder} is FNV-1a 64 of its raw root"
        );
    }
    assert_eq!(
        ahead_behind(by_folder(&fleet.worktrees, "feat-pagos")),
        Some((3, 1)),
        "D4-OPEN: feat-pagos is 3 ahead and 1 behind"
    );
    assert_eq!(
        ahead_behind(by_folder(&fleet.worktrees, "feat-login")),
        Some((1, 0)),
        "D4-OPEN: feat-login is 1 ahead and 0 behind"
    );
    match &by_folder(&fleet.worktrees, "feat-pagos").status {
        WorktreeStatusDto::Ready(ready) => {
            assert_eq!(
                (
                    ready.counts.staged,
                    ready.counts.unstaged,
                    ready.counts.untracked
                ),
                (0, 2, 0),
                "D4-OPEN: feat-pagos has 2 changed files"
            );
            assert_eq!(
                ready.head,
                HeadDto::Branch {
                    name: "feat-pagos".to_owned()
                },
                "D4-OPEN: the branch of feat-pagos"
            );
        }
        other => panic!("D4-OPEN: feat-pagos must be ready, it is {other:?}"),
    }

    assert!(detection, "D4-OPEN: detection is available in the script");
    let mut ids: Vec<&str> = sessions.iter().map(|s| s.session_id.as_str()).collect();
    ids.sort_unstable();
    assert_eq!(ids, ["claude-1", "claude-2"], "D4-OPEN: the two sessions");
    for (id, root) in [("claude-1", TIENDA_PAGOS), ("claude-2", TIENDA_LOGIN)] {
        let session = sessions.iter().find(|s| s.session_id == id).unwrap();
        assert_eq!(
            session.worktree_key,
            fnv(root),
            "D4-OPEN: the session {id} carries the key of its worktree"
        );
        assert!(
            fleet
                .worktrees
                .iter()
                .any(|w| w.key == session.worktree_key),
            "D4-OPEN: the key of {id} matches a card"
        );
        assert_eq!(session.agent, AgentDto::ClaudeCode, "D4-OPEN: {id} agent");
        assert_eq!(session.state, SessionState::Active, "D4-OPEN: {id} state");
    }

    // The order of the calls and their parameters.
    let snapshot = repo_calls(&daemon, "scope.snapshot", "tienda");
    let subscribe = repo_calls(&daemon, "scope.subscribe", "tienda");
    let list = repo_calls(&daemon, "sessions.list", "tienda");
    assert_eq!(
        (snapshot.len(), subscribe.len(), list.len()),
        (1, 1, 1),
        "D4-OPEN: one snapshot, one subscribe and one sessions.list for the repo"
    );
    assert!(
        snapshot[0] < subscribe[0] && subscribe[0] < list[0],
        "D4-OPEN: snapshot, then subscribe, then sessions.list"
    );
    let calls = daemon.calls();
    let params: ScopeSubscribeParams =
        serde_json::from_value(calls[subscribe[0]].1.clone()).expect("subscribe params");
    assert_eq!(
        params,
        ScopeSubscribeParams {
            scope: Scope::Repo {
                repo_id: "tienda".to_owned()
            },
            from_seq: Some(REPO_SEQ + 1),
            run_id: Some(REPO_RUN.to_owned()),
        },
        "D4-OPEN: subscribe from N + 1 of the repo snapshot"
    );
    let list_params: SessionsListParams =
        serde_json::from_value(calls[list[0]].1.clone()).expect("sessions.list params");
    assert_eq!(
        list_params,
        SessionsListParams {
            repo_id: Some("tienda".to_owned()),
            include_ended: false,
            limit: None,
        },
        "D4-OPEN: sessions.list for the repo, without the ended ones"
    );
    assert!(
        daemon.calls_of("events.unsubscribe").is_empty(),
        "D4-OPEN: nothing to unsubscribe the first time"
    );
}

#[test]
fn a_worktree_state_event_reaches_the_webview_in_order() {
    let daemon = daemon_with_repos();
    let mut h = Harness::live(&daemon);
    h.open_and_settle("tienda");

    let push = |seq: u64, ahead: u64, behind: u64| {
        assert!(
            daemon.push_repo_event(
                "tienda",
                seq,
                "worktree.state",
                worktree_event("tienda", tienda_with_pagos(ahead, behind)),
            ),
            "the fake could not push worktree.state"
        );
    };
    push(REPO_SEQ + 1, 4, 1);
    push(REPO_SEQ + 2, 5, 0);
    // A duplicate of an applied event is dropped, never shown again.
    push(REPO_SEQ + 1, 4, 1);
    push(REPO_SEQ + 3, 6, 0);

    let (first, fetched, _) = h.wait_worktrees("tienda");
    assert_eq!(
        ahead_behind(by_folder(&first, "feat-pagos")),
        Some((4, 1)),
        "D4-OPEN: the event N + 1 reaches the webview as `worktrees` with 4 ahead"
    );
    assert_eq!(
        fetched,
        Some(1_699_990_500_000),
        "D4-OPEN: `worktrees` carries the fetch time of the event"
    );
    assert_eq!(
        first.iter().map(|w| w.key.clone()).collect::<Vec<_>>(),
        [fnv(TIENDA_MAIN), fnv(TIENDA_PAGOS), fnv(TIENDA_LOGIN)],
        "D4-OPEN: `worktrees` carries the keys"
    );
    let _ = h.wait_worktrees("tienda");
    let _ = h.wait_worktrees("tienda");
    let ahead: Vec<u32> = h
        .seen
        .iter()
        .filter_map(|m| match m {
            EngineMessage::Worktrees { worktrees, .. } => {
                ahead_behind(by_folder(worktrees, "feat-pagos")).map(|(a, _)| a)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        ahead,
        [4, 5, 6],
        "D4-OPEN: the events arrive in order and the duplicate is dropped"
    );
}

#[test]
fn switching_repos_unsubscribes_and_drops_the_old_scope() {
    let daemon = daemon_with_repos();
    let mut h = Harness::live(&daemon);
    h.open_and_settle("tienda");
    let old = daemon
        .repo_subscription("tienda")
        .expect("the fake subscribed tienda");

    h.open("notas");
    let fleet = h.wait_repo("notas");
    assert_eq!(fleet.worktrees.len(), 1, "D4-OPEN: notas has its main only");
    h.wait_sessions("notas");

    let unsubscribe = daemon.calls_of("events.unsubscribe");
    assert_eq!(
        unsubscribe,
        [serde_json::json!({ "subscription": old })],
        "D4-OPEN: switching unsubscribes the subscription of the previous repo"
    );
    let calls = daemon.calls();
    let position = calls
        .iter()
        .position(|(m, _)| m == "events.unsubscribe")
        .expect("events.unsubscribe");
    let notas_snapshot = repo_calls(&daemon, "scope.snapshot", "notas");
    assert!(
        !notas_snapshot.is_empty() && position < notas_snapshot[0],
        "D4-OPEN: unsubscribe the old scope before the snapshot of the new one"
    );

    // An event of the old scope that still arrives is dropped; the new one applies.
    assert!(daemon.push_repo_event(
        "tienda",
        REPO_SEQ + 1,
        "worktree.state",
        worktree_event("tienda", tienda_with_pagos(9, 9)),
    ));
    assert!(daemon.push_repo_event(
        "notas",
        REPO_SEQ + 1,
        "worktree.state",
        worktree_event(
            "notas",
            vec![worktree_view(NOTAS_MAIN, true, "main", 2, 0, 1)]
        ),
    ));
    let (worktrees, _, _) = h.wait_worktrees("notas");
    assert_eq!(
        ahead_behind(&worktrees[0]),
        Some((2, 0)),
        "D4-OPEN: the new scope applies"
    );
    assert!(
        !h.seen.iter().any(|m| matches!(
            m,
            EngineMessage::Worktrees { repo_id, .. } if repo_id == "tienda"
        )),
        "D4-OPEN: an event of the old scope never reaches the webview"
    );
}

// -------------------------------------------------------------- D4-EVENTS

#[test]
fn a_repo_session_event_reaches_the_webview_as_repo_session() {
    let daemon = daemon_with_repos();
    let mut h = Harness::live(&daemon);
    h.open_and_settle("tienda");
    let inactive: SessionView = session_view(
        "tienda",
        "claude-1",
        TIENDA_PAGOS,
        SessionStateView::Inactive,
        1_700_000_900_000,
    );
    assert!(daemon.push_repo_event("tienda", REPO_SEQ + 1, "session.state", inactive));
    let (session, _perf) = h.wait_for("a `repoSession` message", |m| match m {
        EngineMessage::RepoSession {
            repo_id,
            session,
            perf,
        } if repo_id == "tienda" => Some((session.clone(), *perf)),
        _ => None,
    });
    assert_eq!(session.session_id, "claude-1", "D4-EVENTS: the session");
    assert_eq!(
        session.state,
        SessionState::Inactive,
        "D4-EVENTS: the new state"
    );
    assert_eq!(
        session.state_since_utc_ms, 1_700_000_900_000,
        "D4-EVENTS: since when"
    );
    assert_eq!(
        session.worktree_key,
        fnv(TIENDA_PAGOS),
        "D4-EVENTS: the key of its worktree"
    );
}

#[test]
fn a_prediction_change_reaches_the_webview_with_worktree_keys() {
    let daemon = daemon_with_repos();
    let mut h = Harness::live(&daemon);
    h.open_and_settle("tienda");
    let change = PredictionChanged {
        repo_id: "tienda".to_owned(),
        pairs: vec![
            pair_summary(
                "pair-1",
                TIENDA_LOGIN,
                Some(TIENDA_PAGOS),
                ApiLevel::Conflict,
            ),
            pair_summary("pair-2", TIENDA_PAGOS, None, ApiLevel::Overlap),
        ],
        removed: vec!["pair-old".to_owned()],
    };
    assert!(daemon.push_repo_event("tienda", REPO_SEQ + 1, "prediction.changed", change));
    let (pairs, removed) = h.wait_for("a `prediction` message", |m| match m {
        EngineMessage::Prediction {
            repo_id,
            pairs,
            removed,
            ..
        } if repo_id == "tienda" => Some((pairs.clone(), removed.clone())),
        _ => None,
    });
    assert_eq!(
        pairs,
        [
            PairDto {
                pair_id: "pair-1".to_owned(),
                a: fnv(TIENDA_LOGIN),
                b: fnv(TIENDA_PAGOS),
                state: PairState::Current,
                level: Some(PairLevel::Conflict),
            },
            PairDto {
                pair_id: "pair-2".to_owned(),
                a: fnv(TIENDA_PAGOS),
                b: "base".to_owned(),
                state: PairState::Current,
                level: Some(PairLevel::Overlap),
            },
        ],
        "D4-EVENTS: each side of a pair is the key of its worktree, or `base`"
    );
    assert_eq!(removed, ["pair-old"], "D4-EVENTS: the removed pair ids");
}

#[test]
fn a_repo_without_sessions_reports_detection_and_no_sessions() {
    let daemon = daemon_with_repos();
    let mut h = Harness::live(&daemon);
    h.open("notas");
    let fleet = h.wait_repo("notas");
    let (detection, sessions) = h.wait_sessions("notas");
    assert_eq!(
        fleet.worktrees.len(),
        1,
        "D4-EVENTS: a repo without agents still has its main worktree"
    );
    assert!(fleet.worktrees[0].main, "D4-EVENTS: the main worktree");
    assert_eq!(
        fleet.worktrees[0].key,
        fnv(NOTAS_MAIN),
        "D4-EVENTS: its key"
    );
    assert!(
        detection,
        "D4-EVENTS: `sessions` says detection is available, so no agent means no agent"
    );
    assert!(sessions.is_empty(), "D4-EVENTS: and it lists no session");
}

#[test]
fn a_new_repo_snapshot_asks_for_the_sessions_again() {
    let daemon = daemon_with_repos();
    let mut h = Harness::live(&daemon);
    h.open_and_settle("tienda");
    assert_eq!(
        repo_calls(&daemon, "sessions.list", "tienda").len(),
        1,
        "one sessions.list so far"
    );
    // A third agent appears while the view is rebuilt from a new snapshot.
    daemon.script(|s| {
        s.sessions.get_mut("tienda").unwrap().1.push(session_view(
            "tienda",
            "claude-3",
            TIENDA_MAIN,
            SessionStateView::Active,
            1_700_000_400_000,
        ));
    });
    assert!(daemon.push_repo_resync("tienda", ApiResync::SlowConsumer));
    let _ = h.wait_repo("tienda");
    let (_, sessions) = h.wait_sessions("tienda");
    assert_eq!(
        sessions.len(),
        3,
        "D4-EVENTS: a new snapshot of the repo asks for the sessions again: {sessions:?}"
    );
    assert_eq!(
        repo_calls(&daemon, "scope.snapshot", "tienda").len(),
        2,
        "D4-EVENTS: a new snapshot"
    );
    assert_eq!(
        repo_calls(&daemon, "sessions.list", "tienda").len(),
        2,
        "D4-EVENTS: and the sessions again"
    );
}

// -------------------------------------------------------------- D4-RESYNC

#[test]
fn a_gap_in_the_repo_scope_takes_a_new_snapshot_and_sessions() {
    let daemon = daemon_with_repos();
    let mut h = Harness::live(&daemon);
    h.open_and_settle("tienda");
    daemon.script(|s| s.repos.get_mut("tienda").unwrap().scope_seq = 40);
    // N + 3 skips N + 1 and N + 2: a gap.
    assert!(daemon.push_repo_event(
        "tienda",
        REPO_SEQ + 3,
        "worktree.state",
        worktree_event("tienda", tienda_with_pagos(9, 9)),
    ));
    let _ = h.wait_repo("tienda");
    let _ = h.wait_sessions("tienda");
    assert_eq!(
        repo_calls(&daemon, "scope.snapshot", "tienda").len(),
        2,
        "D4-RESYNC: a gap takes a new snapshot"
    );
    assert_eq!(
        repo_calls(&daemon, "sessions.list", "tienda").len(),
        2,
        "D4-RESYNC: and the sessions again"
    );
    let subscribe = repo_calls(&daemon, "scope.subscribe", "tienda");
    assert_eq!(subscribe.len(), 2, "D4-RESYNC: and a new subscription");
    let from_seq = daemon.calls()[subscribe[1]].1["from_seq"].clone();
    assert_eq!(from_seq, 41, "D4-RESYNC: from the new snapshot's N + 1");
    // The stream goes on from the new position.
    assert!(daemon.push_repo_event(
        "tienda",
        41,
        "worktree.state",
        worktree_event("tienda", tienda_with_pagos(2, 0)),
    ));
    let (worktrees, _, _) = h.wait_worktrees("tienda");
    assert_eq!(
        ahead_behind(by_folder(&worktrees, "feat-pagos")),
        Some((2, 0)),
        "D4-RESYNC: the event after the new snapshot applies"
    );
    assert!(
        !h.seen.iter().any(|m| matches!(
            m,
            EngineMessage::Worktrees { worktrees, .. }
                if ahead_behind(by_folder(worktrees, "feat-pagos")) == Some((9, 9))
        )),
        "D4-RESYNC: the event after the gap is never applied"
    );
}

#[test]
fn a_closed_repo_is_reported_and_forgotten() {
    let daemon = daemon_with_repos();
    let mut h = Harness::live(&daemon);
    // The daemon rejects the snapshot of a repo it does not observe.
    h.open("ghost");
    h.wait_closed("ghost");
    // A repo whose scope the daemon closes.
    let _ = h.open_and_settle("tienda");
    assert!(daemon.push_repo_resync("tienda", ApiResync::ScopeClosed));
    h.wait_closed("tienda");
    // Forgotten: a reconnection does not open either again.
    daemon.drop_connection();
    h.wait_for("`conn reconnecting`", |m| {
        matches!(
            m,
            EngineMessage::Conn {
                state: ConnState::Reconnecting { .. },
                ..
            }
        )
        .then_some(())
    });
    h.wait_live();
    for repo in ["ghost", "tienda"] {
        assert_eq!(
            repo_calls(&daemon, "scope.snapshot", repo).len(),
            1,
            "D4-RESYNC: {repo} is forgotten, a reconnection does not open it again"
        );
    }
    assert_eq!(
        h.seen
            .iter()
            .filter(|m| matches!(m, EngineMessage::Repo { .. }))
            .count(),
        1,
        "D4-RESYNC: no `repo` message after the repo was closed"
    );
}

// ---------------------------------------------------------------- D4-SAFE

#[test]
fn a_restricted_session_never_opens_a_repo() {
    // Control: the same script on a full session does open the repo.
    let control = daemon_with_repos();
    let mut hc = Harness::live(&control);
    hc.open("tienda");
    let _ = hc.wait_repo("tienda");
    drop(hc);

    let daemon = daemon_with_repos();
    daemon.script(|s| s.requester = Some(ConnectionRequester::Unverified));
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    // A `watch` after `open` is attended after it, and replays the session message.
    let (tx, replay) = mpsc::channel();
    assert!(
        h.handle
            .as_ref()
            .unwrap()
            .send(HubCmd::Watch(Sink::Collect(tx))),
        "supervisor gone"
    );
    replay
        .recv_timeout(DEADLINE)
        .expect("the watch after Open is attended");
    for method in [
        "scope.snapshot",
        "scope.subscribe",
        "sessions.list",
        "events.unsubscribe",
    ] {
        let repo_calls = daemon
            .calls_of(method)
            .into_iter()
            .filter(|p| p["scope"]["scope"] != "global")
            .count();
        assert_eq!(
            repo_calls, 0,
            "D4-SAFE: a restricted session must not call {method} for a repo"
        );
    }
    while let Ok(m) = h.rx.try_recv() {
        h.seen.push(m);
    }
    assert!(
        !h.seen.iter().any(|m| matches!(
            m,
            EngineMessage::Repo { .. }
                | EngineMessage::Worktrees { .. }
                | EngineMessage::Sessions { .. }
                | EngineMessage::RepoSession { .. }
                | EngineMessage::Prediction { .. }
        )),
        "D4-SAFE: no repo data reaches the webview of a restricted session"
    );
}

#[test]
fn untrusted_branch_and_folder_reach_the_webview_escaped() {
    let hostile_root = "/w/wt/fe\u{202e}at";
    let hostile_branch = "b\u{1b}]0;x\u{7}ad";
    let daemon = daemon_with_repos();
    daemon.script(|s| {
        let repo = s.repos.get_mut("tienda").unwrap();
        repo.repo.worktrees[1] = worktree_view(hostile_root, false, hostile_branch, 3, 1, 2);
        let mut other = session_view(
            "tienda",
            "claude-2",
            TIENDA_LOGIN,
            SessionStateView::Active,
            1_700_000_200_000,
        );
        other.actor = Actor::Agent {
            kind: AgentKind::Other,
            name: Some(gitraptor_api::UntrustedName::new("ev\u{1b}]0;il\u{7}")),
            origin: AgentOrigin::Registered,
        };
        let sessions = &mut s.sessions.get_mut("tienda").unwrap().1;
        sessions[0] = session_view(
            "tienda",
            "claude-1",
            hostile_root,
            SessionStateView::Active,
            1_700_000_100_000,
        );
        sessions[1] = other;
    });
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    let fleet = h.wait_repo("tienda");
    let (_, sessions) = h.wait_sessions("tienda");
    // A live event with the same hostile text.
    let mut worktrees = tienda_with_pagos(4, 1);
    worktrees[1] = worktree_view(hostile_root, false, hostile_branch, 4, 1, 2);
    assert!(daemon.push_repo_event(
        "tienda",
        REPO_SEQ + 1,
        "worktree.state",
        worktree_event("tienda", worktrees)
    ));
    let (live, _, _) = h.wait_worktrees("tienda");
    for m in &h.seen {
        assert_no_raw_escape(&serde_json::to_string(m).unwrap(), "a message");
    }
    for list in [&fleet.worktrees, &live] {
        let hostile = &list[1];
        assert!(
            hostile.folder.contains("\\u{202e}"),
            "D4-SAFE: the folder is escaped: {:?}",
            hostile.folder
        );
        assert!(
            hostile.path.contains("\\u{202e}"),
            "D4-SAFE: the path is escaped: {:?}",
            hostile.path
        );
        assert_eq!(
            hostile.key,
            fnv(hostile_root),
            "D4-SAFE: the key comes from the raw path, never from the escaped text"
        );
        match &hostile.status {
            WorktreeStatusDto::Ready(ready) => match &ready.head {
                HeadDto::Branch { name } => assert!(
                    name.contains("\\x1b]0;") && !name.contains('\u{1b}'),
                    "D4-SAFE: the branch is escaped: {name:?}"
                ),
                other => panic!("D4-SAFE: expected a branch, got {other:?}"),
            },
            other => panic!("D4-SAFE: expected a ready worktree, got {other:?}"),
        }
    }
    let first = sessions
        .iter()
        .find(|s| s.session_id == "claude-1")
        .expect("claude-1");
    assert_eq!(
        first.worktree_key,
        fnv(hostile_root),
        "D4-SAFE: the session key comes from the raw root too"
    );
    let other = sessions
        .iter()
        .find(|s| s.session_id == "claude-2")
        .expect("claude-2");
    match &other.agent {
        AgentDto::Other { name } => assert!(
            name.contains("\\x1b]0;") && !name.contains('\u{1b}'),
            "D4-SAFE: the agent name is escaped: {name:?}"
        ),
        other => panic!("D4-SAFE: expected another agent, got {other:?}"),
    }
}

#[test]
fn a_fleet_run_never_sends_replace_stop_or_writes() {
    let daemon = daemon_with_repos();
    let mut h = Harness::live(&daemon);
    h.open_and_settle("tienda");
    assert!(daemon.push_repo_event(
        "tienda",
        REPO_SEQ + 1,
        "worktree.state",
        worktree_event("tienda", tienda_with_pagos(4, 1)),
    ));
    let _ = h.wait_worktrees("tienda");
    h.open_and_settle("notas");
    assert!(daemon.push_repo_resync("notas", ApiResync::SlowConsumer));
    let _ = h.wait_repo("notas");
    let calls = daemon.calls();
    let called: Vec<&str> = calls.iter().map(|(m, _)| m.as_str()).collect();
    for expected in [
        "scope.snapshot",
        "scope.subscribe",
        "sessions.list",
        "events.unsubscribe",
    ] {
        assert!(
            called.contains(&expected),
            "D4-SAFE: the fleet run never called {expected}; calls: {called:?}"
        );
    }
    assert_read_only(&calls);
    drop(h);
}

// --------------------------------------------------------------- D4-FRESH

#[test]
fn fleet_deltas_carry_backend_perf() {
    let daemon = daemon_with_repos();
    let mut h = Harness::live(&daemon);
    h.open_and_settle("tienda");
    assert!(daemon.push_repo_event(
        "tienda",
        REPO_SEQ + 1,
        "worktree.state",
        worktree_event("tienda", tienda_with_pagos(4, 1)),
    ));
    assert!(daemon.push_repo_event(
        "tienda",
        REPO_SEQ + 2,
        "session.state",
        session_view(
            "tienda",
            "claude-2",
            TIENDA_LOGIN,
            SessionStateView::Inactive,
            1_700_000_950_000
        ),
    ));
    assert!(daemon.push_repo_event(
        "tienda",
        REPO_SEQ + 3,
        "prediction.changed",
        PredictionChanged {
            repo_id: "tienda".to_owned(),
            pairs: vec![pair_summary(
                "pair-1",
                TIENDA_LOGIN,
                Some(TIENDA_PAGOS),
                ApiLevel::Conflict
            )],
            removed: Vec::new(),
        },
    ));
    let (_, _, worktrees_ns) = h.wait_worktrees("tienda");
    let session_ns = h.wait_for("a `repoSession` message", |m| match m {
        EngineMessage::RepoSession { perf, .. } => Some(perf.backend_ns),
        _ => None,
    });
    let prediction_ns = h.wait_for("a `prediction` message", |m| match m {
        EngineMessage::Prediction { perf, .. } => Some(perf.backend_ns),
        _ => None,
    });
    for (what, ns) in [
        ("worktrees", worktrees_ns),
        ("repoSession", session_ns),
        ("prediction", prediction_ns),
    ] {
        assert!(
            ns > 0 && ns < 20_000_000,
            "D4-FRESH: perf.backendNs of `{what}` is {ns} ns, it must be above 0 and under the 20 ms of the backend share"
        );
    }
}
