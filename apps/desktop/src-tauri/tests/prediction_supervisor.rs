//! D8 (DS-US-DSK-008 § 9.4): the supervisor asks the daemon for the detail of the predicted pairs
//! by itself (`prediction.list`, no IPC command), fills a truncated snapshot with the cursor, and
//! the webview gets `pairDetails`, the freshness of each pair and the confirm command.
//!
//! Same arrange as `fleet_supervisor.rs` (the fake daemon of `support/fake_daemon.rs`, a real
//! Unix socket, `Sink::Collect`, a 5 s deadline, never a fixed sleep) with the scripts "tienda"
//! plus predictions and "tienda-pendiente". Every test ends with the read-only assertion.
//! The expected values come from the story and the plan: pair ids and revs from the script,
//! the ranges of `pagos.rs` (ours 40+19, theirs 44+18), the 50-id cap, the 2 MiB walk cap.
//! Temporary folders only (NFR-01).
#![cfg(unix)]

#[path = "support/fake_daemon.rs"]
mod fake_daemon;

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use fake_daemon::{
    FakeDaemon, PAIR_COMPUTED, PAIR_REV, REPO_SEQ, TIENDA_LOGIN, TIENDA_PAGOS, assert_read_only,
    conflict_detail, overlap_detail, pagos_base_overlap, pagos_login_conflict, pair_id_of,
    script_prediction, script_tienda_and_notas, script_tienda_pending,
    script_tienda_with_predictions, summary_of,
};
use gitraptor_api::display::{TEXT_MAX_CHARS, visible};
use gitraptor_api::messages::HelloResult;
use gitraptor_api::prediction::{
    self, PairDetail, PairLevel as ApiLevel, PairStateView, PredictionChanged,
};
use gitraptor_api::rpc::Notification;
use gitraptor_desktop::engine::link::{ConnectFailure, Connector, Link, LinkError};
use gitraptor_desktop::engine::supervisor::{self, HubCmd, Sink, SupervisorHandle};
use gitraptor_desktop::engine::wire::{
    BaseStatus, ConfirmCommandDto, ConflictFileDto, ConflictKind, ConnState, EngineMessage,
    LineRangeDto, PairDetailDto, PairDto, PairReason, PairState, RepoFleetDto,
};
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(5);
const LIST: &str = "prediction.list";

/// A link that tells the supervisor which executable serves the daemon (the real link learns it
/// from the `ready` report of `raptor daemon ensure`).
struct ExeLink {
    inner: Box<dyn Link>,
    exe: PathBuf,
}

impl Link for ExeLink {
    fn hello(&self) -> &HelloResult {
        self.inner.hello()
    }
    fn call(&mut self, method: &str, params: Value) -> Result<Value, LinkError> {
        self.inner.call(method, params)
    }
    fn next(&mut self, timeout: Duration) -> Result<Option<(u64, Notification)>, LinkError> {
        self.inner.next(timeout)
    }
    fn daemon_exe(&self) -> Option<&std::path::Path> {
        Some(&self.exe)
    }
}

struct WithDaemonExe {
    inner: Box<dyn Connector>,
    exe: PathBuf,
}

impl Connector for WithDaemonExe {
    fn connect(&mut self, start: bool) -> Result<Box<dyn Link>, ConnectFailure> {
        let inner = self.inner.connect(start)?;
        Ok(Box::new(ExeLink {
            inner,
            exe: self.exe.clone(),
        }))
    }
}

struct Harness {
    rx: Receiver<EngineMessage>,
    seen: Vec<EngineMessage>,
    daemon: Arc<FakeDaemon>,
    handle: Option<SupervisorHandle>,
}

impl Harness {
    fn spawn(daemon: &Arc<FakeDaemon>, connector: Box<dyn Connector>) -> Self {
        let (tx, rx) = mpsc::channel();
        let handle = supervisor::spawn(connector, None, Sink::Collect(tx));
        let mut h = Self {
            rx,
            seen: Vec::new(),
            daemon: Arc::clone(daemon),
            handle: Some(handle),
        };
        h.wait_for("`conn live`", |m| {
            matches!(
                m,
                EngineMessage::Conn {
                    state: ConnState::Live,
                    ..
                }
            )
            .then_some(())
        });
        h
    }

    fn live(daemon: &Arc<FakeDaemon>) -> Self {
        Self::spawn(daemon, Box::new(daemon.connector()))
    }

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
                    let tail: Vec<&EngineMessage> = self.seen.iter().rev().take(4).collect();
                    panic!(
                        "waiting for {what}: the supervisor sent nothing more ({err}); \
                         last messages (newest first): {tail:?}"
                    );
                }
            }
        }
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

    /// Consumes `pairDetails` messages until `want` distinct pairs came with their detail.
    fn wait_details(&mut self, want: usize) -> Vec<PairDetailDto> {
        let mut got: Vec<PairDetailDto> = Vec::new();
        self.wait_for(&format!("`pairDetails` for {want} pairs"), |m| {
            if let EngineMessage::PairDetails { details, .. } = m {
                for d in details {
                    got.retain(|g| g.pair.pair_id != d.pair.pair_id);
                    got.push(d.clone());
                }
            }
            (got.len() >= want).then_some(())
        });
        got
    }

    fn wait_prediction(&mut self) -> Vec<PairDto> {
        self.wait_for("a `prediction` message", |m| match m {
            EngineMessage::Prediction { pairs, .. } => Some(pairs.clone()),
            _ => None,
        })
    }

    fn list_calls(&self) -> Vec<Value> {
        self.daemon.calls_of(LIST)
    }

    fn filled_count(&self) -> usize {
        self.seen
            .iter()
            .filter(|m| matches!(m, EngineMessage::PairDetails { filled: true, .. }))
            .count()
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

fn daemon_with(script: impl FnOnce(&mut fake_daemon::Script)) -> Arc<FakeDaemon> {
    let daemon = FakeDaemon::start();
    daemon.script(script);
    daemon
}

fn changed(pairs: Vec<prediction::PairSummary>) -> PredictionChanged {
    PredictionChanged {
        repo_id: "tienda".to_owned(),
        pairs,
        removed: Vec::new(),
    }
}

fn ids_of(call: &Value) -> Vec<String> {
    call["pair_ids"]
        .as_array()
        .map(|ids| {
            ids.iter()
                .map(|i| i.as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// `n` clean pairs of "tienda", none against a worktree the repo knows.
fn clean_details(n: usize) -> Vec<PairDetail> {
    (0..n)
        .map(|i| {
            let root = format!("/w/wt/w{i:03}");
            overlap_detail(
                summary_of("tienda", &root, None, PAIR_REV, ApiLevel::Clean),
                &[],
            )
        })
        .collect()
}

/// The cursors a walk of `details` sends: none first, then each page's `next_cursor`.
fn walk_cursors(details: &[PairDetail]) -> Vec<Option<String>> {
    let mut cursors = vec![None];
    let mut page = prediction::page(details.to_vec(), None);
    while let Some(next) = page.next_cursor.clone() {
        page = prediction::page(details.to_vec(), Some(&next));
        cursors.push(Some(next));
    }
    cursors
}

fn cursor_of(call: &Value) -> Option<String> {
    call["cursor"].as_str().map(str::to_owned)
}

// ---------------------------------------------------------------- D8-DETAIL

#[test]
fn a_conflict_pair_gets_its_files_and_ranges_without_any_command() {
    let daemon = daemon_with(script_tienda_with_predictions);
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    let details = h.wait_details(2);
    let conflict_id = pair_id_of("tienda", TIENDA_PAGOS, Some(TIENDA_LOGIN));
    let overlap_id = pair_id_of("tienda", TIENDA_PAGOS, None);
    let conflict = details
        .iter()
        .find(|d| d.pair.pair_id == conflict_id)
        .expect("D8-DETAIL: the conflict pair has its detail");
    assert_eq!(
        conflict.pair.rev, PAIR_REV,
        "D8-DETAIL: the detail carries the rev of its summary"
    );
    assert_eq!(
        conflict.conflict.as_ref().map(|c| c.files.clone()),
        Some(vec![ConflictFileDto {
            path: "pagos.rs".to_owned(),
            kind: ConflictKind::Content,
            ours: vec![LineRangeDto { start: 40, len: 19 }],
            theirs: vec![LineRangeDto { start: 44, len: 18 }],
            hunks_truncated: false,
            has_merge_rule: false,
        }]),
        "D8-DETAIL: pagos.rs with the ranges of each side (ours 40+19, theirs 44+18)"
    );
    let overlap = details
        .iter()
        .find(|d| d.pair.pair_id == overlap_id)
        .expect("D8-DETAIL: the overlap pair has its detail");
    assert_eq!(
        overlap.overlap,
        ["README.md"],
        "D8-DETAIL: the overlap files of the pair against the base"
    );
    assert_eq!(
        h.list_calls(),
        [json!({"repo_id": "tienda", "pair_ids": [conflict_id, overlap_id]})],
        "D8-DETAIL: one prediction.list by ids, the conflict first, made by the supervisor itself"
    );
}

#[test]
fn a_new_conflict_event_asks_for_its_detail() {
    let daemon = daemon_with(script_tienda_and_notas);
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    let _ = h.wait_repo("tienda");
    let rev = REPO_SEQ + 1;
    daemon.script(|s| script_prediction(s, "tienda", vec![pagos_login_conflict(rev)], 0));
    assert!(daemon.push_repo_event(
        "tienda",
        rev,
        "prediction.changed",
        changed(vec![pagos_login_conflict(rev).summary]),
    ));
    let details = h.wait_details(1);
    let id = pair_id_of("tienda", TIENDA_PAGOS, Some(TIENDA_LOGIN));
    assert_eq!(
        (details[0].pair.pair_id.as_str(), details[0].pair.rev),
        (id.as_str(), rev),
        "D8-DETAIL: the new conflict gets the detail of its rev"
    );
    assert_eq!(
        h.list_calls(),
        [json!({"repo_id": "tienda", "pair_ids": [id]})],
        "D8-DETAIL: exactly one prediction.list, for the pair of the event"
    );
}

#[test]
fn a_detail_is_asked_again_when_the_rev_moves() {
    let daemon = daemon_with(script_tienda_with_predictions);
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    let _ = h.wait_details(2);
    let rev = REPO_SEQ + 1;
    let moved = {
        let mut d = pagos_login_conflict(rev);
        d.summary.computed_at_utc_ms = Some(PAIR_COMPUTED + 60_000);
        d
    };
    daemon.script(|s| {
        s.prediction.insert(
            "tienda".to_owned(),
            vec![moved.clone(), pagos_base_overlap(PAIR_REV)],
        );
    });
    assert!(daemon.push_repo_event(
        "tienda",
        rev,
        "prediction.changed",
        changed(vec![moved.summary.clone()]),
    ));
    let id = pair_id_of("tienda", TIENDA_PAGOS, Some(TIENDA_LOGIN));
    let again = h.wait_for("the detail of the new rev", |m| match m {
        EngineMessage::PairDetails { details, .. } => details
            .iter()
            .find(|d| d.pair.pair_id == id && d.pair.rev == rev)
            .cloned(),
        _ => None,
    });
    assert_eq!(
        again.pair.computed_at_utc_ms,
        Some(PAIR_COMPUTED as u64 + 60_000),
        "D8-DETAIL: the detail of the moved rev is the new one"
    );
    let calls = h.list_calls();
    assert_eq!(
        calls.len(),
        2,
        "D8-DETAIL: a second prediction.list when the rev moves"
    );
    assert_eq!(
        ids_of(&calls[1]),
        [id],
        "D8-DETAIL: it asks only for the pair whose rev moved"
    );
}

#[test]
fn a_burst_of_changes_asks_for_details_once_with_at_most_50_ids() {
    let daemon = daemon_with(script_tienda_and_notas);
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    let _ = h.wait_repo("tienda");
    let burst = |from: u64, count: u64| -> Vec<String> {
        let mut details = Vec::new();
        for seq in from..from + count {
            let root = format!("/w/wt/burst-{seq}");
            details.push(conflict_detail(
                summary_of("tienda", &root, None, seq, ApiLevel::Conflict),
                &format!("a{seq}.rs"),
            ));
        }
        daemon.script(|s| {
            let all = s.prediction.entry("tienda".to_owned()).or_default();
            all.extend(details.clone());
        });
        let ids = details.iter().map(|d| d.summary.pair_id.clone()).collect();
        for d in &details {
            assert!(daemon.push_repo_event(
                "tienda",
                d.summary.rev,
                "prediction.changed",
                changed(vec![d.summary.clone()]),
            ));
        }
        ids
    };
    let first = burst(REPO_SEQ + 1, 30);
    let _ = h.wait_details(30);
    let calls = h.list_calls();
    assert_eq!(
        calls.len(),
        1,
        "D8-DETAIL: 30 changes in a burst are one prediction.list, not one per event"
    );
    assert_eq!(
        ids_of(&calls[0]).into_iter().collect::<BTreeSet<_>>(),
        first.into_iter().collect::<BTreeSet<_>>(),
        "D8-DETAIL: that call asks for the 30 ids of the burst"
    );
    let second = burst(REPO_SEQ + 31, 60);
    let _ = h.wait_details(90);
    let later: Vec<Vec<String>> = h.list_calls().iter().skip(1).map(ids_of).collect();
    assert!(
        later.iter().all(|ids| ids.len() <= 50),
        "D8-DETAIL: no call asks for more than 50 ids (MAX_LIST_PAIR_IDS): {:?}",
        later.iter().map(Vec::len).collect::<Vec<_>>()
    );
    let asked: Vec<String> = later.into_iter().flatten().collect();
    assert_eq!(
        asked.iter().collect::<BTreeSet<_>>(),
        second.iter().collect::<BTreeSet<_>>(),
        "D8-DETAIL: the 60 new ids are all asked for"
    );
    assert_eq!(asked.len(), 60, "D8-DETAIL: and none is asked for twice");
}

#[test]
fn a_refused_list_leaves_pairs_without_detail_and_keeps_streaming() {
    let daemon = daemon_with(script_tienda_with_predictions);
    daemon.script(|s| s.refuse_list = true);
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    let fleet = h.wait_repo("tienda");
    assert!(
        daemon.wait_for_calls(LIST, 1, DEADLINE),
        "D8-DETAIL: the supervisor asks the daemon for the detail of the ⚡ and ⚠ pairs"
    );
    assert_eq!(
        fleet.pairs.len(),
        2,
        "the pairs of the snapshot still reach the webview"
    );
    let same_rev = pagos_login_conflict(PAIR_REV).summary;
    for seq in [REPO_SEQ + 1, REPO_SEQ + 2] {
        assert!(daemon.push_repo_event(
            "tienda",
            seq,
            "prediction.changed",
            changed(vec![same_rev.clone()]),
        ));
        assert_eq!(
            h.wait_prediction().len(),
            1,
            "D8-DETAIL: the stream goes on after a refused prediction.list"
        );
    }
    assert_eq!(
        h.list_calls().len(),
        1,
        "D8-DETAIL: a refused list is not retried until the rev changes"
    );
    assert!(
        !h.seen
            .iter()
            .any(|m| matches!(m, EngineMessage::PairDetails { .. })),
        "D8-DETAIL: no pairDetails message after a refused list"
    );
}

// ---------------------------------------------------------------- D8-FILL

#[test]
fn a_truncated_snapshot_walks_the_cursor_and_ends_filled() {
    let details = clean_details(120);
    let expected = walk_cursors(&details);
    let daemon = daemon_with(|s| {
        script_tienda_and_notas(s);
        script_prediction(s, "tienda", details.clone(), 3);
    });
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    let mut seen_ids = BTreeSet::new();
    h.wait_for("the last page with `filled`", |m| match m {
        EngineMessage::PairDetails {
            details, filled, ..
        } => {
            seen_ids.extend(details.iter().map(|d| d.pair.pair_id.clone()));
            filled.then_some(())
        }
        _ => None,
    });
    let calls = h.list_calls();
    assert_eq!(
        calls.iter().map(cursor_of).collect::<Vec<_>>(),
        expected,
        "D8-FILL: the walk follows the cursor of each page (50 + 50 + 20 pairs)"
    );
    assert!(
        calls.iter().all(|c| c.get("pair_ids").is_none()),
        "D8-FILL: a walk asks by cursor, not by ids"
    );
    assert_eq!(
        seen_ids.len(),
        120,
        "D8-FILL: every pair of the walk reaches the webview"
    );
    assert_eq!(
        h.filled_count(),
        1,
        "D8-FILL: only the last page carries filled: true"
    );
}

#[test]
fn a_resync_during_a_walk_never_runs_two_walks() {
    let details = clean_details(120);
    let walk = walk_cursors(&details);
    let daemon = daemon_with(|s| {
        script_tienda_and_notas(s);
        script_prediction(s, "tienda", details.clone(), 3);
        s.resync_during_list = Some(2);
    });
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    h.wait_for("the last page of the new walk with `filled`", |m| {
        matches!(m, EngineMessage::PairDetails { filled: true, .. }).then_some(())
    });
    let cursors: Vec<Option<String>> = h.list_calls().iter().map(cursor_of).collect();
    let mut expected = vec![walk[0].clone(), walk[1].clone()];
    expected.extend(walk.iter().cloned());
    assert_eq!(
        cursors, expected,
        "D8-FILL: the walk in course stops at its page, the new one starts after it from the new \
         snapshot, and the two never interleave"
    );
    let snapshots = daemon
        .calls_of("scope.snapshot")
        .iter()
        .filter(|p| p["scope"]["repo_id"] == "tienda")
        .count();
    assert_eq!(
        snapshots, 2,
        "D8-FILL: the resync took a new snapshot of the repo"
    );
    assert_eq!(
        h.filled_count(),
        1,
        "D8-FILL: only the walk that finished says filled"
    );
}

#[test]
fn a_walk_stops_at_2_mib_without_filled() {
    let heavy: Vec<PairDetail> = (0..30)
        .map(|i| {
            let root = format!("/w/wt/h{i:03}");
            let files: Vec<String> = (0..270)
                .map(|j| format!("{}/{i:03}-{j:03}.rs", "d".repeat(980)))
                .collect();
            let refs: Vec<&str> = files.iter().map(String::as_str).collect();
            overlap_detail(
                summary_of("tienda", &root, None, PAIR_REV, ApiLevel::Clean),
                &refs,
            )
        })
        .collect();
    let daemon = daemon_with(|s| {
        script_tienda_and_notas(s);
        script_prediction(s, "tienda", heavy.clone(), 1);
    });
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    assert!(
        daemon.wait_for_calls(LIST, 7, DEADLINE),
        "D8-FILL: the walk of a truncated snapshot goes on page by page"
    );
    // The loop is sequential: a message pushed now is read only after the walk ended.
    let rev = REPO_SEQ + 1;
    assert!(daemon.push_repo_event(
        "tienda",
        rev,
        "prediction.changed",
        changed(vec![summary_of(
            "tienda",
            TIENDA_PAGOS,
            None,
            rev,
            ApiLevel::Clean
        )]),
    ));
    let _ = h.wait_prediction();
    let pages = h.list_calls().len();
    assert!(
        (7..=9).contains(&pages),
        "D8-FILL: a page is ~268 KB, so 2 MiB of answers are 8 pages; the walk made {pages} (not 20 nor 30)"
    );
    assert_eq!(
        h.filled_count(),
        0,
        "D8-FILL: a walk cut at 2 MiB never says filled"
    );
}

// ---------------------------------------------------------------- D8-BASE

#[test]
fn a_base_move_reaches_the_webview_as_calculating_pairs() {
    let daemon = daemon_with(script_tienda_with_predictions);
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    let _ = h.wait_details(2);
    let overlap_id = pair_id_of("tienda", TIENDA_PAGOS, None);
    let rev = REPO_SEQ + 1;
    let moving = {
        let mut s = pagos_base_overlap(rev).summary;
        s.state = PairStateView::Calculating;
        s.level = None;
        s.computed_at_utc_ms = None;
        s.overlap_files = 0;
        s
    };
    assert!(daemon.push_repo_event("tienda", rev, "prediction.changed", changed(vec![moving]),));
    let pairs = h.wait_prediction();
    assert_eq!(
        pairs
            .iter()
            .map(|p| (
                p.pair_id.as_str(),
                p.b.as_str(),
                p.state,
                p.level,
                p.computed_at_utc_ms,
                p.rev
            ))
            .collect::<Vec<_>>(),
        [(
            overlap_id.as_str(),
            "base",
            PairState::Calculating,
            None,
            None,
            rev
        )],
        "D8-BASE: the pair against the base reaches the webview calculating, with no level and no result time"
    );
    // The base settles: the pair has its result again, a new rev, and its detail is asked again.
    let settled_rev = rev + 1;
    let settled = overlap_detail(
        summary_of("tienda", TIENDA_PAGOS, None, settled_rev, ApiLevel::Overlap),
        &["README.md", "docs/guia.md"],
    );
    daemon.script(|s| {
        s.prediction
            .insert("tienda".to_owned(), vec![settled.clone()]);
    });
    assert!(daemon.push_repo_event(
        "tienda",
        settled_rev,
        "prediction.changed",
        changed(vec![settled.summary.clone()]),
    ));
    let again = h.wait_for("the detail after the base settled", |m| match m {
        EngineMessage::PairDetails { details, .. } => details
            .iter()
            .find(|d| d.pair.pair_id == overlap_id && d.pair.rev == settled_rev)
            .cloned(),
        _ => None,
    });
    assert_eq!(again.overlap, ["README.md", "docs/guia.md"]);
    let calls = h.list_calls();
    assert_eq!(
        calls.iter().map(ids_of).collect::<Vec<_>>(),
        [
            vec![
                pair_id_of("tienda", TIENDA_PAGOS, Some(TIENDA_LOGIN)),
                overlap_id.clone()
            ],
            vec![overlap_id]
        ],
        "D8-BASE: no detail is asked for a calculating pair, only when its result comes back"
    );
}

#[test]
fn a_pending_base_pair_reaches_the_webview_with_its_reason() {
    let daemon = daemon_with(script_tienda_pending);
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    let fleet = h.wait_repo("tienda");
    assert_eq!(
        fleet.base.status,
        BaseStatus::Unconfirmed,
        "D8-BASE: the base of the repo is unconfirmed"
    );
    let pending_id = pair_id_of("tienda", TIENDA_PAGOS, None);
    let pending = fleet
        .pairs
        .iter()
        .find(|p| p.pair_id == pending_id)
        .expect("D8-BASE: the pair against the base is in the snapshot");
    assert_eq!(
        (
            pending.state,
            pending.reason,
            pending.level,
            pending.computed_at_utc_ms
        ),
        (
            PairState::Pending,
            Some(PairReason::BaseUnconfirmed),
            None,
            None
        ),
        "D8-BASE: pending, with the reason base-unconfirmed and no result"
    );
    let details = h.wait_details(1);
    let conflict_id = pair_id_of("tienda", TIENDA_PAGOS, Some(TIENDA_LOGIN));
    assert_eq!(
        details[0].pair.pair_id, conflict_id,
        "D8-BASE: the conflict between the worktrees stays in view while the base waits"
    );
    assert_eq!(
        h.list_calls(),
        [json!({"repo_id": "tienda", "pair_ids": [conflict_id]})],
        "D8-BASE: the pending pair has no result, so its detail is not asked for"
    );
}

#[test]
fn the_repo_message_carries_the_confirm_command_with_the_daemon_raptor_and_the_repo_path() {
    let dir = tempfile::Builder::new()
        .prefix("daemon-")
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .expect("temporary folder");
    let exe = dir.path().join("raptor");
    std::fs::write(&exe, "#!/bin/sh\n").expect("write raptor");
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let exe = exe.canonicalize().expect("canonical raptor");
    let daemon = daemon_with(script_tienda_pending);
    let connector = WithDaemonExe {
        inner: Box::new(daemon.connector()),
        exe: exe.clone(),
    };
    let mut h = Harness::spawn(&daemon, Box::new(connector));
    h.open("tienda");
    let fleet = h.wait_repo("tienda");
    assert_eq!(
        fleet.confirm_command,
        ConfirmCommandDto::Offered {
            command: format!("'{}' 'guard' 'confirm' '/w/tienda/.git'", exe.display()),
            with_path: true,
        },
        "D8-BASE: the repo message carries raptor guard confirm with the daemon's raptor and the raw repo path"
    );
}

// ---------------------------------------------------------------- D8-SAFE

#[test]
fn a_prediction_run_never_sends_reserved_or_writing_methods() {
    let details = clean_details(60);
    let daemon = daemon_with(|s| {
        script_tienda_with_predictions(s);
        let mut all = vec![pagos_login_conflict(PAIR_REV), pagos_base_overlap(PAIR_REV)];
        all.extend(details.clone());
        script_prediction(s, "tienda", all, 2);
    });
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    h.wait_for("the end of the walk", |m| {
        matches!(m, EngineMessage::PairDetails { filled: true, .. }).then_some(())
    });
    assert!(
        daemon.wait_for_calls(LIST, 2, DEADLINE),
        "D8-SAFE: the run asked for the detail (ids) and walked the cursor"
    );
    assert_read_only(&daemon.calls());
}

#[test]
fn untrusted_conflict_paths_reach_the_webview_escaped() {
    let hostile = "src/\u{1b}]52;c;QUFB\u{7}pagos\u{202e}gnp.rs";
    let long = format!("{}.rs", "d/".repeat(TEXT_MAX_CHARS));
    let daemon = daemon_with(|s| {
        script_tienda_and_notas(s);
        let summary = summary_of(
            "tienda",
            TIENDA_PAGOS,
            Some(TIENDA_LOGIN),
            PAIR_REV,
            ApiLevel::Conflict,
        );
        let mut detail = conflict_detail(summary, hostile);
        detail.overlap = vec![
            gitraptor_api::Untrusted::new(hostile),
            gitraptor_api::Untrusted::new(long.as_str()),
        ];
        script_prediction(s, "tienda", vec![detail], 1);
    });
    let mut h = Harness::live(&daemon);
    h.open("tienda");
    let _ = h.wait_details(1);
    let message = h
        .seen
        .iter()
        .find(|m| matches!(m, EngineMessage::PairDetails { .. }))
        .expect("D8-SAFE: a pairDetails message");
    let json = serde_json::to_string(message).expect("a message serializes");
    assert!(
        !json.contains('\u{202e}') && !json.contains("\\u001b") && !json.contains("\\u0007"),
        "D8-SAFE: pairDetails carries a raw escape or bidi control: {json}"
    );
    let EngineMessage::PairDetails { details, .. } = message else {
        unreachable!()
    };
    assert_eq!(
        details[0]
            .conflict
            .as_ref()
            .map(|c| c.files[0].path.clone()),
        Some(visible(hostile, TEXT_MAX_CHARS)),
        "D8-SAFE: the conflict path is the display::visible text"
    );
    assert_eq!(
        details[0].overlap,
        [
            visible(hostile, TEXT_MAX_CHARS),
            visible(&long, TEXT_MAX_CHARS)
        ],
        "D8-SAFE: the overlap paths are display::visible text, cut at 1024 characters"
    );
}
