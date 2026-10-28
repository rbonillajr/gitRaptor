//! TS-CKP-001 T009 (and T007/D5 for `power`): what the predictor publishes, with a real daemon
//! (in-process, `PredictMerger::InProcess`), the wire of a raw connection, a temporary profile
//! and temporary repos with linked worktrees (NFR-01). Nothing here depends on an agent session
//! (S15).
//!
//! Layout of every repo: `main` (checked out in the main worktree, confirmed as base) and two
//! linked worktrees `wt-a` (branch `a`) and `wt-b` (branch `b`) with own work. The main
//! worktree has the base checked out and no own work, so the pairs are exactly three: `a` with
//! `b`, `a` with the base and `b` with the base.
//!
//! No fixed waits: every test waits for the notification that proves the fact, with a deadline
//! only as a ceiling.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::PROTOCOL_VERSION;
use gitraptor_api::catalog::OperationId;
use gitraptor_api::event::{OPERATION_QUEUED, REPO_ATTENTION, RESERVED_AUDIT};
use gitraptor_api::methods;
use gitraptor_api::prediction::{
    ListPredictionParams, PairLevel, PairReason, PairSideView, PairStateView, PairSummary,
    PredictionChanged, PredictionPage,
};
use gitraptor_api::scope::{
    AttentionCount, AttentionView, RepoAttentionData, Scope, ScopeEventNotification, ScopeSnapshot,
};
use gitraptor_core::channel::EventBus;
use gitraptor_core::client::socket_path;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, PowerConfig, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::power::{FakePowerSource, PowerReading};
use gitraptor_core::predict::{PredictConfig, PredictMerger};
use gitraptor_git::RefName;
use gitraptor_policy::team::{Confirmed, ConfirmedFloor};
use gitraptor_testkit::Fixture;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(20);
const PAIRS: usize = 3;

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap()
}

// ------------------------------------------------------------------------------ the daemon

struct Running {
    bus: Arc<EventBus>,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

impl Running {
    fn start(tp: &TempProfile, fake: Option<&FakePowerSource>) -> Self {
        let env = DaemonEnv::from_vars(std::env::vars_os());
        let power = match fake {
            Some(fake) => PowerConfig {
                source: Arc::new(fake.clone()),
                poll: Duration::from_millis(50),
            },
            None => PowerConfig::default(),
        };
        let config = DaemonConfig {
            dirs: tp.dirs(),
            git: env.git_resolve_config(None),
            env,
            heartbeat: Duration::from_secs(3600),
            log: LogLimits::default(),
            stop_deadline: None,
            channel: Default::default(),
            protected: None,
            operations: None,
            tm_prior_layer: None,
            tiers: Default::default(),
            discovery: Default::default(),
            power,
            predict: PredictConfig {
                merger: PredictMerger::InProcess,
            },
            tm_capture: Default::default(),
        };
        let daemon = Daemon::start(config).unwrap();
        let handle = daemon.shutdown_handle();
        let bus = daemon.events();
        let join = std::thread::spawn(move || daemon.run());
        Self {
            bus,
            handle,
            join: Some(join),
        }
    }

    fn marker(&self, kind: &str, data: Value) -> u64 {
        self.bus.publish(kind, data, None, |_| {})
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

// ------------------------------------------------------------------------------ the repo

#[derive(Clone, Copy)]
enum Work {
    /// `a` and `b` change the same line of `a.txt`: the pair conflicts.
    Conflicting,
    /// `a` changes `a.txt`, `b` changes `b.txt`: no pair conflicts.
    Disjoint,
}

struct Env {
    tp: TempProfile,
    fx: Fixture,
    wt_a: PathBuf,
    wt_b: PathBuf,
    repo_id: String,
}

fn commit_in(fx: &Fixture, wt: &Path, file: &str, content: &str) {
    std::fs::write(wt.join(file), content).unwrap();
    fx.git_in(wt, &["add", "."]);
    fx.git_in(wt, &["commit", "-q", "-m", "work"]);
}

/// The repo, its worktrees and the profile that observes it with `main` confirmed as the base,
/// before any daemon starts.
fn env(work: Work, text: (&str, &str)) -> Env {
    let fx = Fixture::with_commit(&git());
    fx.git(&["branch", "a"]);
    fx.git(&["branch", "b"]);
    let wt_a = canonical(&fx.add_worktree("a", "a"));
    let wt_b = canonical(&fx.add_worktree("b", "b"));
    commit_in(&fx, &wt_a, "a.txt", text.0);
    match work {
        Work::Conflicting => commit_in(&fx, &wt_b, "a.txt", text.1),
        Work::Disjoint => commit_in(&fx, &wt_b, "b.txt", text.1),
    }
    let tp = TempProfile::new();
    let repo_id = {
        let mut profile = tp.open();
        let (entry, _) = profile
            .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
            .unwrap();
        let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
        store
            .set_confirmed_team_baseline(&Confirmed {
                base_branch: RefName::new("main").unwrap(),
                floor: ConfirmedFloor::Absent,
            })
            .unwrap();
        entry.repo_id
    };
    Env {
        tp,
        fx,
        wt_a,
        wt_b,
        repo_id,
    }
}

fn conflicting() -> Env {
    env(Work::Conflicting, ("alpha-a\n", "alpha-b\n"))
}

// ------------------------------------------------------------------------------ the wire

/// A raw connection: a line out, a line in. Notifications that arrive before an answer wait in
/// `notes`; every line received is kept in `log`.
struct Raw {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    line: String,
    notes: VecDeque<Value>,
    next_id: u64,
    log: Vec<String>,
    hello: Value,
}

impl Raw {
    /// Connects, says hello and accepts `accept` (nothing if empty).
    fn open(tp: &TempProfile, accept: &[&str]) -> Self {
        let path = socket_path(&tp.dirs()).unwrap();
        let start = Instant::now();
        let stream = loop {
            match UnixStream::connect(&path) {
                Ok(stream) => break stream,
                Err(err) => {
                    assert!(
                        start.elapsed() < DEADLINE,
                        "the daemon never listened: {err}"
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        };
        let reader = BufReader::new(stream.try_clone().unwrap());
        let mut raw = Self {
            stream,
            reader,
            line: String::new(),
            notes: VecDeque::new(),
            next_id: 1,
            log: Vec::new(),
            hello: Value::Null,
        };
        raw.hello = raw.result(
            methods::HELLO,
            json!({"protocol": PROTOCOL_VERSION, "client": "cli", "client_version": "t"}),
        );
        if !accept.is_empty() {
            let granted: Value =
                raw.result(methods::CONNECTION_ACCEPT, json!({"capabilities": accept}));
            for name in accept {
                assert!(
                    granted["capabilities"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|c| c == name),
                    "the daemon does not serve the capability {name}: {granted}"
                );
            }
        }
        raw
    }

    fn capable(tp: &TempProfile) -> Self {
        Self::open(tp, &[methods::CAP_PREDICTION_PAIRS.name])
    }

    /// One line, or `None` when `timeout` ran out first.
    fn read_value(&mut self, timeout: Duration) -> Option<Value> {
        let start = Instant::now();
        loop {
            let left = timeout.saturating_sub(start.elapsed());
            if left.is_zero() {
                return None;
            }
            self.stream.set_read_timeout(Some(left)).unwrap();
            match self.reader.read_line(&mut self.line) {
                Ok(0) => panic!("the daemon closed the connection"),
                Ok(_) if self.line.ends_with('\n') => {
                    let text = std::mem::take(&mut self.line);
                    let value = serde_json::from_str(&text).unwrap();
                    self.log.push(text);
                    return Some(value);
                }
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(e) => panic!("read: {e}"),
            }
        }
    }

    /// The whole answer (result or error) to `method`.
    fn call(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.stream.write_all(line.to_string().as_bytes()).unwrap();
        self.stream.write_all(b"\n").unwrap();
        loop {
            let value = self
                .read_value(DEADLINE)
                .unwrap_or_else(|| panic!("no answer to {method}"));
            if value["id"] == id {
                return value;
            }
            self.notes.push_back(value);
        }
    }

    fn result<T: DeserializeOwned>(&mut self, method: &str, params: Value) -> T {
        let answer = self.call(method, params);
        assert!(answer.get("error").is_none(), "{method}: {answer}");
        serde_json::from_value(answer["result"].clone()).unwrap()
    }

    /// The next `scope.event`, or `None` when `timeout` ran out.
    fn next_event(&mut self, timeout: Duration) -> Option<ScopeEventNotification> {
        let start = Instant::now();
        loop {
            let note = match self.notes.pop_front() {
                Some(note) => note,
                None => self.read_value(timeout.saturating_sub(start.elapsed()))?,
            };
            if note["method"] == methods::NOTIFY_SCOPE_EVENT {
                return Some(serde_json::from_value(note["params"].clone()).unwrap());
            }
        }
    }

    fn snapshot(&mut self, scope: &Scope) -> ScopeSnapshot {
        self.result(methods::SCOPE_SNAPSHOT, json!({"scope": scope}))
    }

    fn subscribe(&mut self, scope: &Scope, snap: &ScopeSnapshot) {
        let _: Value = self.result(
            methods::SCOPE_SUBSCRIBE,
            json!({"scope": scope, "from_seq": snap.scope_seq() + 1, "run_id": snap.run_id()}),
        );
    }

    fn list(&mut self, params: &ListPredictionParams) -> PredictionPage {
        self.result(
            methods::PREDICTION_LIST,
            serde_json::to_value(params).unwrap(),
        )
    }
}

// ------------------------------------------------------------------------------ pairs

fn side_is(side: &PairSideView, want: Option<&Path>) -> bool {
    match (side, want) {
        (PairSideView::Worktree { root }, Some(p)) => Path::new(root.raw()) == p,
        (PairSideView::Base {}, None) => true,
        _ => false,
    }
}

fn is_pair(p: &PairSummary, a: &Path, b: Option<&Path>) -> bool {
    side_is(&p.a, Some(a)) && side_is(&p.b, b)
}

fn find<'a>(
    pairs: &'a BTreeMap<String, PairSummary>,
    a: &Path,
    b: Option<&Path>,
) -> Option<&'a PairSummary> {
    pairs.values().find(|p| is_pair(p, a, b))
}

fn brief(pairs: &BTreeMap<String, PairSummary>) -> String {
    pairs
        .values()
        .map(|p| format!("{:?}/{:?}/{:?}", p.state, p.reason, p.level))
        .collect::<Vec<_>>()
        .join(", ")
}

fn settled(pairs: &BTreeMap<String, PairSummary>) -> bool {
    pairs.len() == PAIRS && pairs.values().all(|p| p.state == PairStateView::Current)
}

/// A pair as one `prediction.changed` batch carried it, with the `scope_seq` of its event.
struct Seen {
    seq: u64,
    pair: PairSummary,
}

/// A connection that negotiated `prediction.pairs`, subscribed to the repo scope, with the pairs
/// it knows from its snapshot and the events since.
struct Watch {
    raw: Raw,
    scope: Scope,
    pairs: BTreeMap<String, PairSummary>,
    seen: Vec<Seen>,
    kinds: Vec<String>,
}

impl Watch {
    fn start(tp: &TempProfile, repo_id: &str) -> Self {
        let mut raw = Raw::capable(tp);
        let scope = Scope::Repo {
            repo_id: repo_id.to_owned(),
        };
        let snap = raw.snapshot(&scope);
        let ScopeSnapshot::Repo(repo) = &snap else {
            panic!("{snap:?}")
        };
        let prediction = repo.repo.prediction.clone().expect(
            "RepoView.prediction is missing for a connection that negotiated prediction.pairs",
        );
        let pairs = prediction
            .pairs
            .into_iter()
            .map(|p| (p.pair_id.clone(), p))
            .collect();
        raw.subscribe(&scope, &snap);
        Self {
            raw,
            scope,
            pairs,
            seen: Vec::new(),
            kinds: Vec::new(),
        }
    }

    fn apply(&mut self, e: ScopeEventNotification) {
        assert_eq!(e.scope, self.scope);
        self.kinds.push(e.event.kind.clone());
        if e.event.kind != methods::PREDICTION_CHANGED {
            return;
        }
        let changed: PredictionChanged = serde_json::from_value(e.event.data).unwrap();
        for id in &changed.removed {
            self.pairs.remove(id);
        }
        for pair in changed.pairs {
            assert_eq!(
                pair.rev, e.scope_seq,
                "the rev of a pair of a batch must be the scope_seq of its prediction.changed"
            );
            self.pairs.insert(pair.pair_id.clone(), pair.clone());
            self.seen.push(Seen {
                seq: e.scope_seq,
                pair,
            });
        }
    }

    /// Reads events until `done`, within `deadline`.
    fn pump_within(&mut self, deadline: Duration, what: &str, done: impl Fn(&Self) -> bool) {
        let start = Instant::now();
        while !done(self) {
            let left = deadline.saturating_sub(start.elapsed());
            assert!(
                !left.is_zero(),
                "no prediction.changed brought {what}; pairs now: [{}]",
                brief(&self.pairs)
            );
            if let Some(e) = self.raw.next_event(left) {
                self.apply(e);
            }
        }
    }

    fn pump(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        self.pump_within(DEADLINE, what, done);
    }

    fn settle(&mut self) {
        self.pump("every pair current", |w| settled(&w.pairs));
    }

    fn mark(&self) -> usize {
        self.seen.len()
    }

    /// What the batches since `mark` said about one pair, in order.
    fn since(&self, mark: usize, a: &Path, b: Option<&Path>) -> Vec<&PairSummary> {
        self.seen[mark..]
            .iter()
            .map(|s| &s.pair)
            .filter(|p| is_pair(p, a, b))
            .collect()
    }

    fn pair(&self, a: &Path, b: Option<&Path>) -> PairSummary {
        find(&self.pairs, a, b)
            .unwrap_or_else(|| panic!("no such pair; pairs: [{}]", brief(&self.pairs)))
            .clone()
    }
}

// ------------------------------------------------------------------------------ attention

/// The global scope, for `repo.attention`.
struct Global {
    raw: Raw,
    repo_id: String,
    initial: AttentionView,
    emitted: Vec<AttentionView>,
    last_seq: u64,
}

impl Global {
    fn start(tp: &TempProfile, repo_id: &str) -> Self {
        let mut raw = Raw::open(tp, &[]);
        let snap = raw.snapshot(&Scope::Global);
        let ScopeSnapshot::Global(g) = &snap else {
            panic!("{snap:?}")
        };
        let initial = g
            .repos
            .iter()
            .find(|r| r.repo_id == repo_id)
            .expect("the repo is in the global snapshot")
            .attention
            .clone();
        raw.subscribe(&Scope::Global, &snap);
        Self {
            raw,
            repo_id: repo_id.to_owned(),
            initial,
            emitted: Vec::new(),
            last_seq: 0,
        }
    }

    fn conflicts(&self) -> AttentionCount {
        self.emitted.last().unwrap_or(&self.initial).conflicts
    }

    fn pump(&mut self, what: &str, done: impl Fn(&Self) -> bool) {
        let start = Instant::now();
        while !done(self) {
            let left = DEADLINE.saturating_sub(start.elapsed());
            assert!(
                !left.is_zero(),
                "no repo.attention brought {what}; attention.conflicts now {:?}",
                self.conflicts()
            );
            if let Some(e) = self.raw.next_event(left) {
                self.last_seq = e.event.seq;
                if e.event.kind == REPO_ATTENTION {
                    let data: RepoAttentionData = serde_json::from_value(e.event.data).unwrap();
                    if data.repo_id == self.repo_id {
                        self.emitted.push(data.attention);
                    }
                }
            }
        }
    }

    fn pump_to(&mut self, seq: u64) {
        self.pump("the marker", |g| g.last_seq >= seq);
    }
}

fn commit_a(env: &Env, text: &str) {
    commit_in(&env.fx, &env.wt_a, "a.txt", text);
}

// ================================================================================== CKPB-PUB-REV

mod rev {
    use super::*;

    #[test]
    fn snapshot_and_event_carry_same_pair_and_rev() {
        let e = conflicting();
        let _d = Running::start(&e.tp, None);
        let mut w = Watch::start(&e.tp, &e.repo_id);
        w.settle();
        assert!(w.seen.iter().all(|s| s.pair.rev == s.seq));

        // A new connection's snapshot carries the same pairs, with the same rev and time.
        let mut other = Raw::capable(&e.tp);
        let scope = w.scope.clone();
        let ScopeSnapshot::Repo(snap) = other.snapshot(&scope) else {
            panic!("repo scope")
        };
        let summary = snap
            .repo
            .prediction
            .expect("the snapshot of a connection with prediction.pairs lacks prediction");
        assert_eq!(summary.limits.len(), 8, "the eight declared limits");
        assert!(!summary.pairs_truncated);
        let from_snapshot: BTreeMap<_, _> = summary
            .pairs
            .iter()
            .map(|p| (p.pair_id.clone(), p.clone()))
            .collect();
        assert_eq!(
            from_snapshot, w.pairs,
            "the snapshot and prediction.changed must carry the same pair, rev and time"
        );
        assert!(summary.pairs.iter().all(|p| p.rev <= snap.scope_seq));

        // `prediction.list` returns the detail with the rev of the summary.
        let ids: Vec<String> = w.pairs.keys().cloned().collect();
        let page = other.list(&ListPredictionParams {
            repo_id: e.repo_id.clone(),
            pair_ids: Some(ids.clone()),
            ..Default::default()
        });
        assert_eq!(page.pairs.len(), ids.len());
        for detail in &page.pairs {
            assert_eq!(
                &detail.summary, &w.pairs[&detail.summary.pair_id],
                "the detail must carry the summary's rev and time"
            );
        }
        let ab = page
            .pairs
            .iter()
            .find(|d| is_pair(&d.summary, &e.wt_a, Some(&e.wt_b)))
            .unwrap();
        assert_eq!(ab.summary.level, Some(PairLevel::Conflict));
        assert_eq!(ab.conflict.as_ref().unwrap().files.len(), 1);
        assert_eq!(ab.conflict.as_ref().unwrap().files[0].path.raw(), "a.txt");
    }

    #[test]
    fn pair_id_survives_daemon_restart() {
        let e = conflicting();
        let first: Vec<String> = {
            let _d = Running::start(&e.tp, None);
            let mut w = Watch::start(&e.tp, &e.repo_id);
            w.settle();
            w.pairs.keys().cloned().collect()
        };
        assert_eq!(first.len(), PAIRS);
        for id in &first {
            assert!(
                id.len() == 16 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
                "a pair id is 16 lowercase hex digits: {id}"
            );
        }
        let _d = Running::start(&e.tp, None);
        let mut w = Watch::start(&e.tp, &e.repo_id);
        w.settle();
        let second: Vec<String> = w.pairs.keys().cloned().collect();
        assert_eq!(first, second, "pair ids must be stable across a restart");
    }
}

// ================================================================================== CKPB-PUB-BASE

mod base {
    use super::*;

    #[test]
    fn base_move_turns_base_pair_calculating_and_drops_level() {
        let e = conflicting();
        let _d = Running::start(&e.tp, None);
        let mut w = Watch::start(&e.tp, &e.repo_id);
        w.settle();
        let before = w.pair(&e.wt_a, None);
        assert!(before.computed_at_utc_ms.is_some());

        let mark = w.mark();
        e.fx.write("a.txt", "alpha-main\n");
        e.fx.git(&["add", "."]);
        e.fx.git(&["commit", "-q", "-m", "base moves"]);
        w.pump("the base pair computed again", |w| {
            let p = w.pair(&e.wt_a, None);
            p.state == PairStateView::Current && p.computed_at_utc_ms != before.computed_at_utc_ms
        });

        let batches = w.since(mark, &e.wt_a, None);
        let calc = batches
            .iter()
            .find(|p| p.state == PairStateView::Calculating)
            .expect("a moved base must publish its pair as calculating, not recalculating");
        assert_eq!(calc.level, None, "calculating carries no level");
        assert_eq!(
            calc.computed_at_utc_ms, None,
            "no earlier result is published"
        );
        assert_eq!(calc.conflict_files, 0);
        assert!(
            batches
                .iter()
                .all(|p| p.state != PairStateView::Recalculating),
            "a base move never leaves the base pair recalculating"
        );
        let after = w.pair(&e.wt_a, None);
        assert_eq!(after.level, Some(PairLevel::Conflict));
        assert_eq!(after.conflict_files, 1);
    }

    #[test]
    fn worktree_tip_move_keeps_recalculating_with_last_level() {
        let e = conflicting();
        let _d = Running::start(&e.tp, None);
        let mut w = Watch::start(&e.tp, &e.repo_id);
        w.settle();
        let before = w.pair(&e.wt_a, Some(&e.wt_b));
        assert_eq!(before.level, Some(PairLevel::Conflict));

        let mark = w.mark();
        commit_a(&e, "alpha-a2\n");
        w.pump("the pair of the two worktrees computed again", |w| {
            let p = w.pair(&e.wt_a, Some(&e.wt_b));
            p.state == PairStateView::Current && p.computed_at_utc_ms > before.computed_at_utc_ms
        });

        let batches = w.since(mark, &e.wt_a, Some(&e.wt_b));
        let recalc = batches
            .iter()
            .find(|p| p.state == PairStateView::Recalculating)
            .expect("a worktree tip move must publish its pair as recalculating");
        assert_eq!(
            recalc.level,
            Some(PairLevel::Conflict),
            "recalculating keeps the last result's level"
        );
        assert_eq!(recalc.computed_at_utc_ms, before.computed_at_utc_ms);
        assert!(
            batches
                .iter()
                .all(|p| p.state != PairStateView::Calculating),
            "a pair with a result is never calculating again after a tip move"
        );
    }

    #[test]
    fn base_move_back_reuses_cached_triple() {
        let e = conflicting();
        let _d = Running::start(&e.tp, None);
        let mut w = Watch::start(&e.tp, &e.repo_id);
        w.settle();
        let original = w.pair(&e.wt_a, None);
        let original_tip = e.fx.git(&["rev-parse", "main"]).trim().to_owned();

        e.fx.write("a.txt", "alpha-main\n");
        e.fx.git(&["add", "."]);
        e.fx.git(&["commit", "-q", "-m", "base moves"]);
        w.pump("the moved base computed", |w| {
            let p = w.pair(&e.wt_a, None);
            p.state == PairStateView::Current && p.level == Some(PairLevel::Conflict)
        });

        let mark = w.mark();
        e.fx.git(&["reset", "-q", "--hard", &original_tip]);
        w.pump("the base pair back on its first result", |w| {
            let p = w.pair(&e.wt_a, None);
            p.state == PairStateView::Current && p.computed_at_utc_ms == original.computed_at_utc_ms
        });

        let batches = w.since(mark, &e.wt_a, None);
        assert!(
            batches.iter().all(|p| !matches!(
                p.state,
                PairStateView::Calculating | PairStateView::Recalculating
            )),
            "going back to a known triple must reuse the cache, not recalculate"
        );
        let back = w.pair(&e.wt_a, None);
        assert_eq!(back.level, original.level);
        assert_eq!(back.computed_at_utc_ms, original.computed_at_utc_ms);
    }
}

// ================================================================================== CKPB-PUB-ATTENTION

mod attention {
    use super::*;

    #[test]
    fn attention_counts_known_conflicts_and_is_unavailable_while_calculating() {
        // One conflicting pair: counted 1, and never a counted 0 on the way (pairs calculating).
        let e = conflicting();
        let _d = Running::start(&e.tp, None);
        let mut g = Global::start(&e.tp, &e.repo_id);
        let mut w = Watch::start(&e.tp, &e.repo_id);
        w.settle();
        g.pump("one known conflict", |g| {
            g.conflicts() == AttentionCount::Counted { count: 1 }
        });
        assert!(
            !g.emitted
                .iter()
                .any(|a| a.conflicts == AttentionCount::Counted { count: 0 }),
            "counted 0 while pairs were calculating: {:?}",
            g.emitted
        );
        let known = w
            .pairs
            .values()
            .filter(|p| p.level == Some(PairLevel::Conflict))
            .count();
        assert_eq!(known, 1, "the setup has exactly one conflicting pair");

        // No conflicting pair: unavailable until nothing calculates, then counted 0.
        let e = env(Work::Disjoint, ("alpha-a\n", "beta-b\n"));
        let _d = Running::start(&e.tp, None);
        let mut g = Global::start(&e.tp, &e.repo_id);
        let mut w = Watch::start(&e.tp, &e.repo_id);
        w.settle();
        g.pump("counted 0 once nothing calculates", |g| {
            g.conflicts() == AttentionCount::Counted { count: 0 }
        });
    }

    #[test]
    fn repo_attention_emitted_only_on_change() {
        let e = conflicting();
        let d = Running::start(&e.tp, None);
        let mut g = Global::start(&e.tp, &e.repo_id);
        let mut w = Watch::start(&e.tp, &e.repo_id);
        w.settle();
        g.pump("one known conflict", |g| {
            g.conflicts() == AttentionCount::Counted { count: 1 }
        });
        let emitted = g.emitted.len();
        assert!(emitted >= 1, "the change of the count was never published");

        // The pair is recalculated and still conflicts: the attention does not change.
        let before = w.pair(&e.wt_a, Some(&e.wt_b));
        commit_a(&e, "alpha-a2\n");
        w.pump("the pair computed again", |w| {
            let p = w.pair(&e.wt_a, Some(&e.wt_b));
            p.state == PairStateView::Current && p.computed_at_utc_ms > before.computed_at_utc_ms
        });
        let marker = d.marker(RESERVED_AUDIT, json!({"n": 1}));
        g.pump_to(marker);
        assert_eq!(
            g.emitted.len(),
            emitted,
            "repo.attention was emitted again without a change: {:?}",
            g.emitted
        );
        let mut all = vec![g.initial.clone()];
        all.extend(g.emitted.clone());
        assert!(
            all.windows(2).all(|p| p[0] != p[1]),
            "two equal consecutive repo.attention: {all:?}"
        );
    }
}

// ================================================================================== CKPB-PUB-NOCONTENT

mod content {
    use super::*;

    const MARK: &str = "SECRET-MARKER-9c1e4f";

    #[test]
    fn no_file_content_in_any_publication() {
        let a = format!("{MARK}-a\n");
        let b = format!("{MARK}-b\n");
        let e = env(Work::Conflicting, (&a, &b));
        std::fs::write(e.wt_a.join("wip.txt"), format!("{MARK}-wip\n")).unwrap();
        let _d = Running::start(&e.tp, None);
        let mut w = Watch::start(&e.tp, &e.repo_id);
        w.settle();
        let page = w.raw.list(&ListPredictionParams {
            repo_id: e.repo_id.clone(),
            ..Default::default()
        });
        let ab = page
            .pairs
            .iter()
            .find(|d| is_pair(&d.summary, &e.wt_a, Some(&e.wt_b)))
            .expect("prediction.list returns the pair of the two worktrees");
        assert_eq!(
            ab.conflict.as_ref().expect("the conflict detail").files[0]
                .path
                .raw(),
            "a.txt",
            "the publication names the file, never its content"
        );
        let mut other = Raw::capable(&e.tp);
        let scope = w.scope.clone();
        let _ = other.snapshot(&scope);
        for log in [&w.raw.log, &other.log] {
            for line in log {
                assert!(!line.contains(MARK), "file content on the wire: {line}");
            }
        }
    }
}

// ================================================================================== CKPB-PUB-CAPOFF

mod capability {
    use super::*;

    #[test]
    fn without_prediction_pairs_nothing_new_is_published_and_list_answers() {
        let e = conflicting();
        let d = Running::start(&e.tp, None);
        let mut oracle = Watch::start(&e.tp, &e.repo_id);
        oracle.settle();

        let mut plain = Raw::open(&e.tp, &[]);
        assert!(
            plain.hello["methods"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m == methods::PREDICTION_LIST),
            "prediction.list must be found in hello.methods"
        );
        let page = plain.list(&ListPredictionParams {
            repo_id: e.repo_id.clone(),
            ..Default::default()
        });
        assert_eq!(page.limits.len(), 8, "prediction.list answers the limits");

        let scope = oracle.scope.clone();
        let snap = plain.snapshot(&scope);
        let ScopeSnapshot::Repo(repo) = &snap else {
            panic!("repo scope")
        };
        assert!(repo.repo.prediction.is_none());
        plain.subscribe(&scope, &snap);
        let raw_snap = plain.call(methods::SCOPE_SNAPSHOT, json!({"scope": scope}));
        assert!(
            raw_snap["result"]["repo"].get("prediction").is_none(),
            "the wire of a connection without prediction.pairs must not change"
        );

        // The capable connection sees the predictor work; the plain one sees none of it.
        let before = oracle.pair(&e.wt_a, Some(&e.wt_b));
        commit_a(&e, "alpha-a2\n");
        oracle.pump("the pair computed again", |w| {
            let p = w.pair(&e.wt_a, Some(&e.wt_b));
            p.state == PairStateView::Current && p.computed_at_utc_ms > before.computed_at_utc_ms
        });
        let marker = d.marker(
            OPERATION_QUEUED,
            json!({"repo_id": e.repo_id, "operation": OperationId::Commit, "layer": "cockpit", "position": 0}),
        );
        let start = Instant::now();
        loop {
            let left = DEADLINE.saturating_sub(start.elapsed());
            assert!(!left.is_zero(), "the marker never arrived");
            let Some(ev) = plain.next_event(left) else {
                continue;
            };
            assert_ne!(
                ev.event.kind,
                methods::PREDICTION_CHANGED,
                "prediction.changed reached a connection without prediction.pairs"
            );
            if ev.event.seq == marker {
                break;
            }
        }
    }
}

// ================================================================================== CKPB-PUB-POWER

mod power {
    use super::*;

    const CEILING: Duration = Duration::from_secs(30);

    #[test]
    fn power_saving_publishes_pending_and_ac_recalculates_only_changed_triples() {
        let e = conflicting();
        let fake = FakePowerSource::new(PowerReading::Ac);
        let _d = Running::start(&e.tp, Some(&fake));
        let mut w = Watch::start(&e.tp, &e.repo_id);
        w.settle();
        let ab = w.pair(&e.wt_a, Some(&e.wt_b));
        let b_base = w.pair(&e.wt_b, None);
        let known = w.pairs.clone();

        // On battery every pair waits, with its last result and time.
        fake.set(PowerReading::Battery);
        w.pump_within(CEILING, "every pair pending (power-saving)", |w| {
            w.pairs.len() == PAIRS && w.pairs.values().all(|p| p.state == PairStateView::Pending)
        });
        for p in w.pairs.values() {
            assert_eq!(p.reason, Some(PairReason::PowerSaving));
            assert_eq!(p.level, None, "pending carries no level");
            let was = &known[&p.pair_id];
            assert_eq!(p.computed_at_utc_ms, was.computed_at_utc_ms);
            assert_eq!(p.conflict_files, was.conflict_files);
        }

        // A tip moves while saving: no merge runs, the pair keeps its last result.
        let mark = w.mark();
        commit_a(&e, "alpha-a2\n");
        let mid = w.since(mark, &e.wt_a, Some(&e.wt_b));
        assert!(mid.iter().all(|p| p.state == PairStateView::Pending));

        // Back on AC only the pairs whose triple changed are computed again.
        let mark = w.mark();
        fake.set(PowerReading::Ac);
        w.pump_within(
            CEILING,
            "the pairs of the moved worktree computed again",
            |w| {
                let p = w.pair(&e.wt_a, Some(&e.wt_b));
                p.state == PairStateView::Current && p.computed_at_utc_ms > ab.computed_at_utc_ms
            },
        );
        w.pump_within(CEILING, "every pair current", |w| settled(&w.pairs));
        let untouched = w.since(mark, &e.wt_b, None);
        assert!(
            untouched.iter().all(|p| p.state == PairStateView::Current
                && p.computed_at_utc_ms == b_base.computed_at_utc_ms),
            "a pair whose triple did not change must come back with its result, not recalculate"
        );
        assert_eq!(
            w.pair(&e.wt_b, None).computed_at_utc_ms,
            b_base.computed_at_utc_ms
        );
        assert_eq!(
            w.pair(&e.wt_a, Some(&e.wt_b)).level,
            Some(PairLevel::Conflict)
        );
    }
}
