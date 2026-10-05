//! TS-GRP-004, amendment N1 to N7 of ADR-CKP-003 § 4 (protocol 6): what the
//! Cockpit needs from the channel, with a real daemon (in-process), the
//! real client library, a temporary profile and testkit repos (NFR-01).
//!
//! No fixed waits: every test waits for the notification or answer that
//! proves the fact, with a deadline only as a ceiling.
#![cfg(target_os = "macos")]

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::catalog::OperationId;
use gitraptor_api::event::{OPERATION_QUEUED, REPO_OBSERVATION, RESERVED_AUDIT};
use gitraptor_api::messages::{ClientKind, HelloResult, ResyncReason, WorktreeStatus};
use gitraptor_api::rpc::{InvalidData, InvalidReason, Notification, code};
use gitraptor_api::scope::{
    AttentionCount, AutostartView, ConnectionRequester, RepoLocateResult, Scope,
    ScopeEventNotification, ScopeResyncNotification, ScopeSnapshot, ScopeSubscribeResult,
    UnavailableCause,
};
use gitraptor_api::{MIN_COMPATIBLE_PROTOCOL, PROTOCOL_VERSION, methods};
use gitraptor_core::channel::{ChannelConfig, ChannelLimits, EventBus};
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause,
};
use gitraptor_testkit::{Exceptions, Fixture, check};
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(10);

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap()
}

struct Running {
    tp: TempProfile,
    bus: Arc<EventBus>,
    handle: ShutdownHandle,
    join: Option<JoinHandle<gitraptor_core::daemon::StopReport>>,
    repo_id: Option<String>,
}

impl Running {
    /// A daemon on a fresh profile, observing `repo` (a worktree root) if
    /// given.
    fn start(repo: Option<&Path>, channel: ChannelConfig) -> Self {
        let tp = TempProfile::new();
        let repo_id = repo.map(|repo| {
            let mut profile = tp.open();
            let (entry, _) = profile
                .add_repo(&canonical(&repo.join(".git")), None, 1)
                .unwrap();
            entry.repo_id
        });
        let env = DaemonEnv::from_vars(std::env::vars_os());
        let config = DaemonConfig {
            dirs: tp.dirs(),
            git: env.git_resolve_config(None),
            env,
            heartbeat: Duration::from_secs(3600),
            log: LogLimits::default(),
            stop_deadline: None,
            channel,
            protected: None,
            operations: None,
        };
        let daemon = Daemon::start(config).unwrap();
        let handle = daemon.shutdown_handle();
        let bus = daemon.events();
        let join = std::thread::spawn(move || daemon.run());
        let r = Self {
            tp,
            bus,
            handle,
            join: Some(join),
            repo_id,
        };
        if r.repo_id.is_some() {
            r.wait_reconciled();
        }
        r
    }

    fn connect(&self, kind: ClientKind, protocol: u32) -> Result<Client, ClientError> {
        let start = Instant::now();
        loop {
            match Client::connect(&self.tp.dirs(), kind, protocol) {
                Err(ClientError::NotRunning) if start.elapsed() < DEADLINE => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                other => return other,
            }
        }
    }

    fn client(&self) -> Client {
        self.connect(ClientKind::Cli, PROTOCOL_VERSION).unwrap()
    }

    fn repo_id(&self) -> String {
        self.repo_id.clone().unwrap()
    }

    fn repo_scope(&self) -> Scope {
        Scope::Repo {
            repo_id: self.repo_id(),
        }
    }

    /// The signal that the start-up reconciliation of the repo is done:
    /// its scope snapshot lists its worktrees.
    fn wait_reconciled(&self) {
        let mut c = self.client();
        let start = Instant::now();
        loop {
            let snap: ScopeSnapshot = c
                .call(methods::SCOPE_SNAPSHOT, json!({"scope": self.repo_scope()}))
                .unwrap();
            if let ScopeSnapshot::Repo(s) = &snap
                && !s.repo.worktrees.is_empty()
            {
                return;
            }
            assert!(start.elapsed() < DEADLINE, "never reconciled: {snap:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Publishes an engine event of `kind` with `data` straight on the bus.
    fn publish(&self, kind: &str, data: Value) -> u64 {
        self.bus.publish(kind, data, None, |_| {})
    }

    fn queued(&self, repo_id: &str) -> u64 {
        self.publish(
            OPERATION_QUEUED,
            json!({"repo_id": repo_id, "operation": OperationId::Commit, "layer": "cockpit", "position": 0}),
        )
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

fn snapshot(c: &mut Client, scope: &Scope) -> ScopeSnapshot {
    c.call(methods::SCOPE_SNAPSHOT, json!({ "scope": scope }))
        .unwrap()
}

fn subscribe(c: &mut Client, scope: &Scope, snap: &ScopeSnapshot) -> ScopeSubscribeResult {
    c.call(
        methods::SCOPE_SUBSCRIBE,
        json!({"scope": scope, "from_seq": snap.scope_seq() + 1, "run_id": snap.run_id()}),
    )
    .unwrap()
}

/// The next notification of `method`, skipping the others.
fn next_of(c: &mut Client, method: &str) -> Notification {
    let start = Instant::now();
    loop {
        let left = DEADLINE.saturating_sub(start.elapsed());
        assert!(!left.is_zero(), "no {method} notification");
        if let Some(n) = c.next_notification(left).unwrap()
            && n.method == method
        {
            return n;
        }
    }
}

fn next_event(c: &mut Client) -> ScopeEventNotification {
    serde_json::from_value(next_of(c, methods::NOTIFY_SCOPE_EVENT).params).unwrap()
}

/// Scoped events until the one published as daemon sequence `last`.
fn events_until(c: &mut Client, last: u64) -> Vec<ScopeEventNotification> {
    let mut out = Vec::new();
    loop {
        let e = next_event(c);
        let done = e.event.seq == last;
        out.push(e);
        if done {
            return out;
        }
    }
}

fn rpc(err: ClientError) -> (i64, Option<Value>) {
    match err {
        ClientError::Rpc(e) => (e.code, e.data),
        other => panic!("expected an RPC error, got {other}"),
    }
}

/// A repo with `lib.rs` committed and a linked worktree "feat" on its own
/// branch.
fn repo() -> (Fixture, PathBuf) {
    let fx = Fixture::with_commit(&git());
    fx.git(&["branch", "feat"]);
    let wt = fx.add_worktree("feat", "feat");
    (fx, canonical(&wt))
}

// ---------------------------------------------------------------- N1

/// N1: a global snapshot at N and a subscription from N + 1 neither miss
/// nor repeat an event, in order.
#[test]
fn global_snapshot_then_subscribe_is_gapless() {
    let r = Running::start(None, ChannelConfig::default());
    let mut c = r.client();
    r.publish(RESERVED_AUDIT, json!({"n": 0}));
    let snap = snapshot(&mut c, &Scope::Global);
    assert!(matches!(snap, ScopeSnapshot::Global(_)));
    let n = snap.scope_seq();
    assert!(n >= 1, "engine.state and the audit are global events");
    // Published between the snapshot and the subscription: replayed.
    r.publish(RESERVED_AUDIT, json!({"n": 1}));
    r.publish(RESERVED_AUDIT, json!({"n": 2}));
    let sub = subscribe(&mut c, &Scope::Global, &snap);
    assert_eq!(sub.from_seq, n + 1);
    let last = r.publish(RESERVED_AUDIT, json!({"n": 3}));
    let got = events_until(&mut c, last);
    let seqs: Vec<u64> = got.iter().map(|e| e.scope_seq).collect();
    assert_eq!(seqs, [n + 1, n + 2, n + 3]);
    let ns: Vec<_> = got.iter().map(|e| e.event.data["n"].clone()).collect();
    assert_eq!(ns, [json!(1), json!(2), json!(3)]);
    assert!(got.iter().all(|e| e.subscription == sub.subscription));
}

/// N1: the same for one repo, with a real change of its worktree observed
/// by the engine.
#[test]
fn repo_snapshot_then_subscribe_sees_a_real_change_without_gaps() {
    let (fx, wt) = repo();
    let r = Running::start(Some(&fx.repo), ChannelConfig::default());
    let scope = r.repo_scope();
    let mut c = r.client();
    let snap = snapshot(&mut c, &scope);
    let ScopeSnapshot::Repo(repo) = &snap else {
        panic!("{snap:?}")
    };
    assert_eq!(repo.repo.repo_id, r.repo_id());
    assert_eq!(repo.repo.worktrees.len(), 2);
    subscribe(&mut c, &scope, &snap);

    std::fs::write(wt.join("a.txt"), "changed\n").unwrap();
    let mut expected = snap.scope_seq() + 1;
    loop {
        let e = next_event(&mut c);
        assert_eq!(e.scope, scope);
        assert_eq!(e.scope_seq, expected, "gap or duplicate in the repo scope");
        expected += 1;
        if e.event.kind != gitraptor_api::event::WORKTREE_STATE {
            continue;
        }
        let shows = e.event.data["worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| {
                let w: gitraptor_api::messages::WorktreeView =
                    serde_json::from_value(w.clone()).unwrap();
                Path::new(w.path.raw()) == wt
                    && matches!(&w.status, WorktreeStatus::Ready { changes, .. }
                    if changes.iter().any(|c| c.path.raw() == "a.txt"))
            });
        if shows {
            break;
        }
    }
}

// ---------------------------------------------------------------- N2

/// N2: each scope has its own contiguous sequence, even with the events of
/// other scopes in between; each subscription only sees its scope's.
#[test]
fn each_scope_has_its_own_contiguous_sequence() {
    let (fx, _) = repo();
    let r = Running::start(Some(&fx.repo), ChannelConfig::default());
    let id = r.repo_id();
    let mut global = r.client();
    let mut repo = r.client();
    let gsnap = snapshot(&mut global, &Scope::Global);
    let rsnap = snapshot(&mut repo, &r.repo_scope());
    subscribe(&mut global, &Scope::Global, &gsnap);
    subscribe(&mut repo, &r.repo_scope(), &rsnap);

    r.publish(RESERVED_AUDIT, json!({"n": "g1"}));
    r.queued(&id);
    r.queued("other-repo");
    r.queued(&id);
    r.publish(RESERVED_AUDIT, json!({"n": "g2"}));
    let last_repo = r.queued(&id);
    let last_global = r.publish(RESERVED_AUDIT, json!({"n": "g3"}));

    let g = events_until(&mut global, last_global);
    let rs = events_until(&mut repo, last_repo);
    let n = gsnap.scope_seq();
    assert_eq!(
        g.iter().map(|e| e.scope_seq).collect::<Vec<_>>(),
        [n + 1, n + 2, n + 3]
    );
    assert!(g.iter().all(|e| e.scope == Scope::Global));
    let m = rsnap.scope_seq();
    assert_eq!(
        rs.iter().map(|e| e.scope_seq).collect::<Vec<_>>(),
        [m + 1, m + 2, m + 3]
    );
    assert!(rs.iter().all(|e| e.event.data["repo_id"] == id.as_str()));
    // The daemon-wide sequence interleaves them.
    let daemon: Vec<u64> = rs.iter().map(|e| e.event.seq).collect();
    assert!(daemon.windows(2).all(|w| w[1] > w[0] + 1), "{daemon:?}");
}

/// N2: a scope that cannot continue gets an explicit `scope.resync` with
/// its cause, and the subscription is refused with `RESYNC_REQUIRED`.
#[test]
fn a_lost_replay_or_another_run_gets_a_scoped_resync() {
    let r = Running::start(
        None,
        ChannelConfig {
            limits: ChannelLimits {
                replay: 4,
                ..ChannelLimits::default()
            },
            ..ChannelConfig::default()
        },
    );
    let mut c = r.client();
    let snap = snapshot(&mut c, &Scope::Global);
    for n in 0..10 {
        r.publish(RESERVED_AUDIT, json!({ "n": n }));
    }
    let err = c
        .call::<_, ScopeSubscribeResult>(
            methods::SCOPE_SUBSCRIBE,
            json!({"scope": Scope::Global, "from_seq": snap.scope_seq() + 1, "run_id": snap.run_id()}),
        )
        .unwrap_err();
    assert_eq!(rpc(err).0, code::RESYNC_REQUIRED);
    let resync: ScopeResyncNotification =
        serde_json::from_value(next_of(&mut c, methods::NOTIFY_SCOPE_RESYNC).params).unwrap();
    assert_eq!(resync.scope, Scope::Global);
    assert_eq!(resync.reason, ResyncReason::ReplayUnavailable);

    let err = c
        .call::<_, ScopeSubscribeResult>(
            methods::SCOPE_SUBSCRIBE,
            json!({"scope": Scope::Global, "from_seq": 1, "run_id": "another-run"}),
        )
        .unwrap_err();
    assert_eq!(rpc(err).0, code::RESYNC_REQUIRED);
    let resync: ScopeResyncNotification =
        serde_json::from_value(next_of(&mut c, methods::NOTIFY_SCOPE_RESYNC).params).unwrap();
    assert_eq!(resync.reason, ResyncReason::DaemonRestarted);

    // From a fresh snapshot it continues.
    let snap = snapshot(&mut c, &Scope::Global);
    subscribe(&mut c, &Scope::Global, &snap);
}

/// N2: when a repo stops being observed, its scoped subscriptions get
/// `scope.resync { scope-closed }`; its sequence is not reset, so the same
/// scope continues where it was when the repo comes back.
#[test]
fn a_retired_repo_closes_its_scope() {
    let (fx, _) = repo();
    let r = Running::start(Some(&fx.repo), ChannelConfig::default());
    let id = r.repo_id();
    let mut c = r.client();
    let snap = snapshot(&mut c, &r.repo_scope());
    subscribe(&mut c, &r.repo_scope(), &snap);
    let before = r.queued(&id);
    assert_eq!(next_event(&mut c).event.seq, before);
    let path = fx.repo.join(".git");
    r.bus.publish(
        REPO_OBSERVATION,
        json!({"repo_id": id, "observed": false, "state": "observed", "path": {"untrusted": path}}),
        None,
        |e| e.repos.retain(|repo| repo.repo_id != id),
    );
    let resync: ScopeResyncNotification =
        serde_json::from_value(next_of(&mut c, methods::NOTIFY_SCOPE_RESYNC).params).unwrap();
    assert_eq!(resync.scope, r.repo_scope());
    assert_eq!(resync.reason, ResyncReason::ScopeClosed);
    // No longer observed: its scope does not exist for a new snapshot.
    let err = c
        .call::<_, ScopeSnapshot>(methods::SCOPE_SNAPSHOT, json!({"scope": r.repo_scope()}))
        .unwrap_err();
    assert_eq!(rpc(err).0, code::NOT_FOUND);
    // An event published for it meanwhile reaches no closed subscription:
    // the next scoped event this client sees is the global sentinel's.
    r.queued(&id);
    let gsnap = snapshot(&mut c, &Scope::Global);
    subscribe(&mut c, &Scope::Global, &gsnap);
    let sentinel = r.publish(RESERVED_AUDIT, json!({}));
    let e = next_event(&mut c);
    assert_eq!((e.scope, e.event.seq), (Scope::Global, sentinel));
}

// ---------------------------------------------------------------- N3

/// N3: the global scope lists the repos with an attention summary that is
/// "not available" until something publishes it (never a zero), and the
/// autostart state.
#[test]
fn global_snapshot_has_attention_and_autostart() {
    let (fx, _) = repo();
    let r = Running::start(Some(&fx.repo), ChannelConfig::default());
    let mut c = r.client();
    let ScopeSnapshot::Global(g) = snapshot(&mut c, &Scope::Global) else {
        panic!("not global")
    };
    assert_eq!(g.daemon.protocol, PROTOCOL_VERSION);
    assert_eq!(g.autostart, AutostartView::Unknown);
    assert_eq!(g.repos.len(), 1);
    let repo = &g.repos[0];
    assert_eq!(repo.repo_id, r.repo_id());
    assert_eq!(Path::new(repo.path.raw()), canonical(&fx.repo.join(".git")));
    let unavailable = AttentionCount::Unavailable {
        reason: UnavailableCause::NotPublished,
    };
    for count in [
        repo.attention.conflicts,
        repo.attention.denials,
        repo.attention.gaps,
    ] {
        assert_eq!(count, unavailable);
    }
}

// ---------------------------------------------------------------- N4

/// N4: the daemon says which observed repo and worktree contain a path;
/// the deepest worktree wins.
#[test]
fn repo_locate_finds_the_observed_worktree() {
    let (fx, wt) = repo();
    std::fs::create_dir_all(wt.join("src/deep")).unwrap();
    let r = Running::start(Some(&fx.repo), ChannelConfig::default());
    let mut c = r.client();
    for (path, root) in [
        (wt.join("src/deep"), wt.clone()),
        (wt.clone(), wt.clone()),
        (fx.repo.join("a.txt"), canonical(&fx.repo)),
    ] {
        let found: RepoLocateResult = c.call(methods::REPO_LOCATE, json!({"path": path})).unwrap();
        assert_eq!(found.repo_id, r.repo_id(), "{}", path.display());
        assert_eq!(Path::new(found.worktree.raw()), root, "{}", path.display());
    }
}

/// N4: outside every observed repo, or missing, is "not found" (the same
/// answer, so it is no oracle); a path that fails the lexical checks is
/// refused before the file system is touched, with a typed reason (N7).
#[test]
fn repo_locate_refuses_what_is_not_observed() {
    let (fx, _) = repo();
    let r = Running::start(Some(&fx.repo), ChannelConfig::default());
    let mut c = r.client();
    for path in [
        fx.other_repo.clone(),
        fx.repo.join("does-not-exist"),
        PathBuf::from("/"),
    ] {
        let err = c
            .call::<_, RepoLocateResult>(methods::REPO_LOCATE, json!({"path": path}))
            .unwrap_err();
        assert_eq!(rpc(err).0, code::NOT_FOUND, "{}", path.display());
    }
    for (path, reason) in [
        ("relative/path", InvalidReason::NotAbsolute),
        ("//server/share/repo", InvalidReason::UncOrDevice),
        ("", InvalidReason::Empty),
        ("/tmp/\u{1b}]52;c;x\u{7}", InvalidReason::ControlCharacter),
    ] {
        let err = c
            .call::<_, RepoLocateResult>(methods::REPO_LOCATE, json!({ "path": path }))
            .unwrap_err();
        let (code, data) = rpc(err);
        assert_eq!(code, code::INVALID_PARAMS, "{path:?}");
        let data: InvalidData = serde_json::from_value(data.unwrap()).unwrap();
        assert_eq!(data.reason, reason, "{path:?}");
    }
}

// ---------------------------------------------------------------- N5

/// N5: a `cli` client of protocol 6 learns in the handshake who the daemon
/// sees and the layer it fixes; it is what `requester.resolve` says.
#[test]
fn hello_says_who_the_caller_is() {
    let r = Running::start(None, ChannelConfig::default());
    let mut c = r.client();
    let requester = c.hello().requester.clone().expect("requester in hello");
    let resolved: Value = c.call(methods::REQUESTER_RESOLVE, json!({})).unwrap();
    match requester {
        ConnectionRequester::Resolved { actor, .. } => {
            assert_eq!(serde_json::to_value(actor).unwrap(), resolved["actor"]);
        }
        ConnectionRequester::Unverified => panic!("the test process is readable"),
    }
    // Not for other clients, nor for protocol 5 connections.
    let other = r.connect(ClientKind::Other, PROTOCOL_VERSION).unwrap();
    assert_eq!(other.hello().requester, None);
    let old = r.connect(ClientKind::Cli, MIN_COMPATIBLE_PROTOCOL).unwrap();
    assert_eq!(old.hello().requester, None);
}

// ------------------------------------------------- Compatibility and MCP

/// E-D1: a protocol 5 client still works with a protocol 6 daemon, in the
/// shapes of protocol 5: no new methods, no requester.
#[test]
fn a_protocol_4_client_still_works_with_a_protocol_5_daemon() {
    let (fx, _) = repo();
    let r = Running::start(Some(&fx.repo), ChannelConfig::default());
    let mut old = r.connect(ClientKind::Cli, MIN_COMPATIBLE_PROTOCOL).unwrap();
    let hello: HelloResult = old.hello().clone();
    assert_eq!(hello.protocol, MIN_COMPATIBLE_PROTOCOL);
    let wire = serde_json::to_value(&hello).unwrap();
    assert!(wire.get("requester").is_none(), "{wire}");
    for name in [
        methods::SCOPE_SNAPSHOT,
        methods::SCOPE_SUBSCRIBE,
        methods::REPO_LOCATE,
    ] {
        assert!(!hello.methods.iter().any(|m| m == name), "{name}");
        let err = old.call::<_, Value>(name, json!({})).unwrap_err();
        assert_eq!(rpc(err).0, code::METHOD_NOT_FOUND, "{name}");
    }
    let snap: gitraptor_api::messages::Snapshot =
        old.call(methods::ENGINE_SNAPSHOT, json!({})).unwrap();
    assert_eq!(snap.repos.len(), 1);
    let _: gitraptor_api::messages::SubscribeResult = old
        .call(
            methods::EVENTS_SUBSCRIBE,
            json!({"from_seq": snap.seq + 1, "run_id": snap.run_id}),
        )
        .unwrap();
    let last = r.queued(&r.repo_id());
    let n = next_of(&mut old, methods::NOTIFY_EVENT);
    assert_eq!(n.params["event"]["seq"], last);
    assert!(n.params.get("scope_seq").is_none());
}

/// E-D1 and SEC-13: an older client in the window cannot replace a newer
/// daemon; the daemon keeps running.
#[test]
fn an_older_client_cannot_replace_a_newer_daemon() {
    let r = Running::start(None, ChannelConfig::default());
    let mut old = r.connect(ClientKind::Cli, MIN_COMPATIBLE_PROTOCOL).unwrap();
    let err = old
        .call::<_, Value>(
            methods::DAEMON_REPLACE,
            json!({ "protocol": MIN_COMPATIBLE_PROTOCOL }),
        )
        .unwrap_err();
    assert_eq!(rpc(err).0, code::INVALID_PARAMS);
    let pong: String = r.client().call(methods::PING, json!({})).unwrap();
    assert_eq!(pong, "pong");
}

/// SEC-12: scopes and `repo.locate` carry paths; `raptor-mcp` gets none.
#[test]
fn mcp_connections_do_not_get_scopes_or_locate() {
    let r = Running::start(None, ChannelConfig::default());
    let mut mcp = r.connect(ClientKind::Mcp, PROTOCOL_VERSION).unwrap();
    assert_eq!(mcp.hello().requester, None);
    for name in [
        methods::SCOPE_SNAPSHOT,
        methods::SCOPE_SUBSCRIBE,
        methods::REPO_LOCATE,
    ] {
        assert!(!mcp.hello().methods.iter().any(|m| m == name), "{name}");
        let err = mcp
            .call::<_, Value>(name, json!({"scope": Scope::Global, "path": "/"}))
            .unwrap_err();
        assert_eq!(rpc(err).0, code::METHOD_NOT_FOUND, "{name}");
    }
}

// ---------------------------------------------------------------- L-06

/// L-06: a client refuses a socket folder that is not private, before it
/// sends anything.
#[test]
fn a_client_refuses_an_open_socket_folder() {
    use std::os::unix::fs::PermissionsExt;
    let r = Running::start(None, ChannelConfig::default());
    drop(r.client());
    let runtime = r.tp.dirs().runtime.unwrap();
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o755)).unwrap();
    let refused = Client::connect(&r.tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION);
    std::fs::set_permissions(&runtime, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        matches!(refused, Err(ClientError::ChannelRejected)),
        "{:?}",
        refused.err()
    );
    // With the folder private again, the same client connects.
    drop(r.client());
}

// ---------------------------------------------------------------- BR-CONS-001

/// The new queries and subscriptions only read: the repo stays byte for
/// byte (testkit fingerprint).
#[test]
fn scopes_and_locate_leave_the_repo_intact() {
    let (fx, wt) = repo();
    let report = check("TS-GRP-004 N1-N7", &fx, &Exceptions::none(), || {
        let r = Running::start(Some(&fx.repo), ChannelConfig::default());
        let mut c = r.client();
        let gsnap = snapshot(&mut c, &Scope::Global);
        let rsnap = snapshot(&mut c, &r.repo_scope());
        subscribe(&mut c, &Scope::Global, &gsnap);
        subscribe(&mut c, &r.repo_scope(), &rsnap);
        let _: RepoLocateResult = c.call(methods::REPO_LOCATE, json!({ "path": wt })).unwrap();
        drop(c);
        drop(r);
    });
    report.assert_intact();
}
