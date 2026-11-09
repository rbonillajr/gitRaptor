//! D7-READ, D7-RATE and D7-ONLYREAD (DS-US-DSK-007 § 9.4, T001/T006): the supervisor reads
//! `guard.status` and `guard.log` of the open repo against the fake daemon (`support/fake_daemon.rs`,
//! script "tienda" with a guard log) through a real Unix socket, and sends `guardBlocks` or
//! `guardBlocksUnavailable`.
//!
//! The arrange (harness, connectors, `WithDaemonExe`, the `raptor` file under `CARGO_TARGET_TMPDIR`)
//! follows `prediction_supervisor.rs`, which passes today and drives the same fake. Every wait is on
//! a signal with a deadline: never a fixed sleep. The only waits that run to their end are the
//! bounded "nothing more arrives" windows of the rate tests. Each test ends with `assert_read_only`
//! (the harness drop) and the tests that run a whole read also check that no `guard.exception*`,
//! `guard.cancel`, `guard.install` or `guard.uninstall` was sent and that no command has `--yes`.
//! Temporary folders only (NFR-01).
#![cfg(unix)]

#[path = "support/fake_daemon.rs"]
mod fake_daemon;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use fake_daemon::{
    FakeDaemon, GUARD_PUSH_AT, REPO_SEQ, assert_read_only, forced_main_push, guard_status,
    log_entry, pending_exception, repo_snapshot, script_tienda_guard,
    script_tienda_guard_log_unavailable, worktree_view,
};
use gitraptor_api::Untrusted;
use gitraptor_api::actor::{Actor, AgentKind, AgentOrigin};
use gitraptor_api::catalog::Layer;
use gitraptor_api::display::{NAME_MAX_CHARS, TEXT_MAX_CHARS, visible};
use gitraptor_api::guard::{
    GuardLogResult, LoggedOperation, LoggedRef, PendingKind, ProtectionState, RefChange, Rule,
};
use gitraptor_api::messages::HelloResult;
use gitraptor_api::rpc::Notification;
use gitraptor_api::scope::ConnectionRequester;
use gitraptor_desktop::engine::link::{ConnectFailure, Connector, Link, LinkError};
use gitraptor_desktop::engine::supervisor::{self, HubCmd, Sink, SupervisorHandle};
use gitraptor_desktop::engine::wire::{ConnState, EngineMessage, RepoFleetDto};
use serde_json::{Value, json};

const DEADLINE: Duration = Duration::from_secs(5);
/// How long a "nothing more arrives" check waits: below the 2 s gap of D3, above its 500 ms join.
const QUIET: Duration = Duration::from_millis(1500);
const STATUS: &str = "guard.status";
const LOG: &str = "guard.log";
const FORBIDDEN: [&str; 6] = [
    "guard.exception",
    "guard.exception-child",
    "guard.exception-end",
    "guard.cancel",
    "guard.install",
    "guard.uninstall",
];

// ---------------------------------------------------------------- harness

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

/// A connector whose links name `exe` as the executable that serves the daemon (what the `ready`
/// report of `raptor daemon ensure` gives the real link).
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

/// A `raptor` file (0755) in a fresh folder of the target dir (not a system temp folder, which
/// `is_installed_raptor` rejects); returns the folder guard and the canonical path of the file.
fn installed_raptor(prefix: &str, sub: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(env!("CARGO_TARGET_TMPDIR"))
        .expect("temporary folder");
    let folder = dir.path().join(sub);
    std::fs::create_dir_all(&folder).expect("folder of raptor");
    let file = folder.join("raptor");
    std::fs::write(&file, "#!/bin/sh\n").expect("write raptor");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let canonical = file.canonicalize().expect("canonical raptor");
    (dir, canonical)
}

fn daemon_with(script: impl FnOnce(&mut fake_daemon::Script)) -> Arc<FakeDaemon> {
    let daemon = FakeDaemon::start();
    daemon.script(script);
    daemon
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
        Self {
            rx,
            seen: Vec::new(),
            daemon: Arc::clone(daemon),
            handle: Some(handle),
        }
    }

    /// A harness whose daemon is served by the installed `exe`, with "tienda" open.
    fn open_with(daemon: &Arc<FakeDaemon>, exe: &Path) -> Self {
        let connector = WithDaemonExe {
            inner: Box::new(daemon.connector()),
            exe: exe.to_path_buf(),
        };
        let mut h = Self::spawn(daemon, Box::new(connector));
        h.open_tienda();
        h
    }

    /// A harness with no known `raptor`, with "tienda" open.
    fn open_plain(daemon: &Arc<FakeDaemon>) -> Self {
        let mut h = Self::spawn(daemon, Box::new(daemon.connector()));
        h.open_tienda();
        h
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
                    let tail: Vec<&EngineMessage> = self.seen.iter().rev().take(6).collect();
                    panic!(
                        "waiting for {what}: the supervisor sent nothing more ({err}); \
                         last messages (newest first): {tail:?}"
                    );
                }
            }
        }
    }

    fn send(&self, cmd: HubCmd) {
        let handle = self.handle.as_ref().expect("supervisor handle");
        assert!(handle.send(cmd), "the supervisor thread ended");
    }

    fn open_tienda(&mut self) {
        self.send(HubCmd::Open {
            repo_id: "tienda".to_owned(),
        });
        let _: RepoFleetDto = self.wait_for("the `repo` message of tienda", |m| match m {
            EngineMessage::Repo { fleet } if fleet.repo_id == "tienda" => Some(fleet.clone()),
            _ => None,
        });
    }

    fn ask(&self, repo_id: &str) {
        self.send(HubCmd::GuardBlocks {
            repo_id: repo_id.to_owned(),
        });
    }

    /// The next `guardBlocks` message, as the JSON the webview gets (its `blocks`).
    fn wait_blocks(&mut self) -> Value {
        self.wait_for("a `guardBlocks` message", |m| match m {
            EngineMessage::GuardBlocks { blocks } => {
                Some(serde_json::to_value(blocks).expect("blocks serialize"))
            }
            _ => None,
        })
    }

    /// The next `guardBlocksUnavailable` message, as the JSON the webview gets.
    fn wait_unavailable(&mut self) -> Value {
        self.wait_for("a `guardBlocksUnavailable` message", |m| match m {
            EngineMessage::GuardBlocksUnavailable { .. } => {
                Some(serde_json::to_value(m).expect("message serializes"))
            }
            _ => None,
        })
    }

    /// Asks for the blocks of "tienda" and waits for them.
    fn read(&mut self) -> Value {
        self.ask("tienda");
        self.wait_blocks()
    }

    fn guard_messages(&self) -> usize {
        self.seen
            .iter()
            .filter(|m| matches!(m, EngineMessage::GuardBlocks { .. }))
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

fn guard_calls(daemon: &FakeDaemon) -> Vec<(String, Value)> {
    daemon
        .calls()
        .into_iter()
        .filter(|(method, _)| method.starts_with("guard."))
        .collect()
}

/// The common assertion of T006: nothing but `guard.status` and `guard.log` of the guard family,
/// and no command with `--yes`.
fn assert_only_reads(daemon: &FakeDaemon, blocks: &[&Value]) {
    for (method, _) in guard_calls(daemon) {
        assert!(
            !FORBIDDEN.contains(&method.as_str()),
            "D7-ONLYREAD: the app called {method}"
        );
        assert!(
            method == STATUS || method == LOG,
            "D7-ONLYREAD: the app called {method}, not guard.status or guard.log"
        );
    }
    for value in blocks {
        for entry in value["entries"].as_array().into_iter().flatten() {
            let command = entry["exception"]["command"].as_str().unwrap_or_default();
            assert!(
                !command.contains("--yes"),
                "D7-ONLYREAD: a copyable command carries --yes: {command}"
            );
        }
    }
}

fn audit(operation: &str, repo_id: &str) -> Value {
    json!({
        "id": 1, "at_ms": GUARD_PUSH_AT, "operation": operation, "repo_id": repo_id,
        "outcome": "accepted", "reason": null,
        "client": {"pid": 1, "start_us": 1, "exe": null, "agent_ancestor": false,
                   "daemon_descendant": false, "controlling_terminal": true,
                   "chain_truncated": false}
    })
}

fn quoted(exe: &Path, rest: &str) -> String {
    format!("'{}' {rest}", exe.display())
}

fn ids(blocks: &Value) -> Vec<&str> {
    blocks["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .map(|e| e["decisionId"].as_str().expect("decisionId"))
        .collect()
}

fn entry<'a>(blocks: &'a Value, id: &str) -> &'a Value {
    blocks["entries"]
        .as_array()
        .expect("entries")
        .iter()
        .find(|e| e["decisionId"] == id)
        .unwrap_or_else(|| panic!("the blocks have no entry {id}: {:?}", ids(blocks)))
}

// ---------------------------------------------------------------- D7-READ

#[test]
fn a_denied_force_push_reaches_the_webview_with_agent_operation_rule_and_time() {
    let (_dir, exe) = installed_raptor("daemon-", "bin");
    let daemon = daemon_with(script_tienda_guard);
    let mut h = Harness::open_with(&daemon, &exe);
    let blocks = h.read();
    assert_eq!(
        blocks["repoId"], "tienda",
        "D7-READ: guardBlocks names the repo that was read"
    );
    assert_eq!(
        blocks["protection"],
        json!({"state": "full"}),
        "D7-READ: the protection state comes from guard.status"
    );
    assert_eq!(
        blocks["summary"],
        json!({"blocked": 3, "exceptions": 1, "exceptionsRejected": 0}),
        "D7-READ: the summary counts come from guard.log"
    );
    assert_eq!(
        entry(&blocks, "d-push"),
        &json!({
            "decisionId": "d-push", "kind": "denial",
            "atMs": GUARD_PUSH_AT, "lastMs": GUARD_PUSH_AT, "count": 1,
            "actor": {"kind": "claude-code"},
            "worktree": {"folder": "feat-pagos", "path": "/w/wt/feat-pagos"},
            "branch": "feat-pagos",
            "operation": {"kind": "push", "remote": "origin",
                          "refs": [{"name": "main", "change": "force"}]},
            "rules": [
                {"rule": "minimum.force-push", "level": "minimum", "cause": "not-fast-forward"},
                {"rule": "policy.protected-branch", "level": "floor", "cause": null}
            ],
            "layer": "hooks", "detail": "full",
            "exception": {"state": "offered", "command": quoted(&exe,
                "'guard' 'exec' '--' 'git' '-C' '/w/wt/feat-pagos' 'push' \
                 '--force-with-lease' '--force-if-includes' 'origin' 'feat-pagos:main'")}
        }),
        "D7-READ: the denied force-push carries agent, operation, rules, time and command"
    );
}

#[test]
fn each_block_carries_its_command_or_the_reason_there_is_none() {
    let (_dir, exe) = installed_raptor("daemon-", "bin");
    let daemon = daemon_with(script_tienda_guard);
    let mut h = Harness::open_with(&daemon, &exe);
    let blocks = h.read();
    assert_eq!(
        entry(&blocks, "d-commit")["exception"],
        json!({"state": "not-offered", "reason": "commit"}),
        "D7-READ: the commit denial says why it has no exception"
    );
    assert_eq!(
        entry(&blocks, "d-rebase")["exception"],
        json!({"state": "not-offered", "reason": "rebase"}),
        "D7-READ: the rebase denial says why it has no command"
    );
    for id in ["d-exception", "d-cancelled"] {
        assert_eq!(
            entry(&blocks, id)["exception"],
            Value::Null,
            "D7-READ: {id} is not a denial, so it carries no command"
        );
    }
    assert_only_reads(&daemon, &[&blocks]);
}

#[test]
fn a_cancelled_exception_reaches_the_webview_with_who_cancelled() {
    let daemon = daemon_with(script_tienda_guard);
    let mut h = Harness::open_plain(&daemon);
    let blocks = h.read();
    let cancelled = entry(&blocks, "d-cancelled");
    assert_eq!(
        (&cancelled["kind"], &cancelled["actor"]),
        (&json!("exception-cancelled"), &json!({"kind": "other"})),
        "D7-READ: the cancelled exception is listed with the actor the engine logged"
    );
    let used = entry(&blocks, "d-exception");
    assert_eq!(
        (&used["kind"], &used["operation"]["kind"]),
        (&json!("exception"), &json!("push")),
        "D7-READ: the used exception is listed with its operation"
    );
    assert_eq!(
        ids(&blocks),
        [
            "d-cancelled",
            "d-exception",
            "d-rebase",
            "d-commit",
            "d-push"
        ],
        "D7-READ: the entries keep the order of the log (newest first)"
    );
}

#[test]
fn notice_and_protection_state_entries_are_not_listed() {
    let daemon = daemon_with(script_tienda_guard);
    let mut h = Harness::open_plain(&daemon);
    let blocks = h.read();
    let listed = ids(&blocks);
    assert!(
        !listed.contains(&"d-notice") && !listed.contains(&"d-protection"),
        "D7-READ: a notice and a protection-state are not blocks, listed: {listed:?}"
    );
    assert_eq!(
        listed.len(),
        5,
        "D7-READ: the log has seven entries and five are blocks (D4): {listed:?}"
    );
}

#[test]
fn the_status_is_read_before_the_log_with_the_raw_path_and_a_limit_of_50() {
    let daemon = daemon_with(script_tienda_guard);
    let mut h = Harness::open_plain(&daemon);
    h.read();
    assert_eq!(
        guard_calls(&daemon),
        [
            (STATUS.to_owned(), json!({"path": "/w/tienda/.git"})),
            (
                LOG.to_owned(),
                json!({"path": "/w/tienda/.git", "limit": 50})
            ),
        ],
        "D7-READ: guard.status then guard.log, with the raw path of the open repo and limit 50"
    );
}

#[test]
fn an_announced_exception_reaches_the_webview_as_pending() {
    let (_dir, exe) = installed_raptor("daemon-", "bin");
    let daemon = daemon_with(|s| {
        script_tienda_guard(s);
        s.guard_status = Some(guard_status(
            ProtectionState::Full,
            Some(pending_exception(
                forced_main_push(),
                vec![Rule::MinimumForcePush, Rule::ProtectedBranch],
            )),
        ));
    });
    let mut h = Harness::open_with(&daemon, &exe);
    let blocks = h.read();
    assert_eq!(
        blocks["pending"],
        json!({
            "appliesAtMs": GUARD_PUSH_AT + 70_000,
            "windowMs": 10_000,
            "operation": {"kind": "push", "remote": "origin",
                          "refs": [{"name": "main", "change": "force"}]},
            "rules": ["minimum.force-push", "policy.protected-branch"],
            "cancelCommand": quoted(&exe, "'guard' 'cancel' '/w/wt/feat-pagos'")
        }),
        "D7-READ: the announced exception is pending with its window, operation, rules and the \
         cancel command of the worktree of the denial of that operation"
    );
}

#[test]
fn an_announced_exception_carries_its_operation_and_rules_to_compare() {
    let (_dir, exe) = installed_raptor("daemon-", "bin");
    let pending = |operation: LoggedOperation, rules: Vec<Rule>| {
        move |s: &mut fake_daemon::Script| {
            script_tienda_guard(s);
            s.guard_status = Some(guard_status(
                ProtectionState::Full,
                Some(pending_exception(operation, rules)),
            ));
        }
    };
    let daemon = daemon_with(pending(
        LoggedOperation::Rebase {
            upstream: None,
            branch: None,
        },
        vec![Rule::PermissionRebase],
    ));
    let blocks = Harness::open_with(&daemon, &exe).read();
    assert_eq!(
        (&blocks["pending"]["operation"], &blocks["pending"]["rules"]),
        (&json!({"kind": "rebase"}), &json!(["permission.rebase"])),
        "D7-READ: the pending exception carries its own operation and rules"
    );
    assert_eq!(
        blocks["pending"]["cancelCommand"],
        json!(quoted(&exe, "'guard' 'cancel' '/w/wt/feat-login'")),
        "D7-READ: the cancel command names the worktree of the latest denial of the same operation"
    );
    // No denial of this operation: the repo path.
    let daemon = daemon_with(pending(
        LoggedOperation::RefTransaction {
            refs: vec![LoggedRef {
                name: Untrusted::new("refs/heads/old"),
                change: RefChange::Delete,
            }],
        },
        vec![Rule::MinimumBaseBranchDelete],
    ));
    let blocks = Harness::open_with(&daemon, &exe).read();
    assert_eq!(
        blocks["pending"]["cancelCommand"],
        json!(quoted(&exe, "'guard' 'cancel' '/w/tienda/.git'")),
        "D7-READ: with no denial of that operation the cancel command names the repo path"
    );
}

#[test]
fn only_an_announced_exception_is_reported_as_pending() {
    let daemon = daemon_with(|s| {
        script_tienda_guard(s);
        let mut uninstall = pending_exception(forced_main_push(), vec![Rule::MinimumForcePush]);
        uninstall.action = PendingKind::Uninstall;
        uninstall.exception = None;
        s.guard_status = Some(guard_status(ProtectionState::Full, Some(uninstall)));
    });
    let blocks = Harness::open_plain(&daemon).read();
    assert_eq!(
        blocks["pending"],
        Value::Null,
        "D7-READ: a pending removal of the protection is not an exception"
    );
}

#[test]
fn an_unreadable_log_reports_unavailable_with_the_protection_state() {
    let daemon = daemon_with(script_tienda_guard_log_unavailable);
    let mut h = Harness::open_plain(&daemon);
    h.ask("tienda");
    let message = h.wait_unavailable();
    assert_eq!(
        message,
        json!({"type": "guardBlocksUnavailable", "repoId": "tienda",
               "cause": "log-unavailable", "protection": {"state": "hooks-only"}}),
        "D7-READ: guard.log failing with INTERNAL reports log-unavailable with the protection \
         state of guard.status"
    );
    assert_eq!(
        h.guard_messages(),
        0,
        "D7-READ: an unreadable log never sends a list (nor 'nothing blocked')"
    );
}

#[test]
fn an_unreadable_log_and_status_report_unknown_protection() {
    let daemon = daemon_with(|s| {
        script_tienda_guard_log_unavailable(s);
        s.guard_status = None;
        s.guard_status_fails = true;
    });
    let mut h = Harness::open_plain(&daemon);
    h.ask("tienda");
    assert_eq!(
        h.wait_unavailable(),
        json!({"type": "guardBlocksUnavailable", "repoId": "tienda",
               "cause": "log-unavailable", "protection": null}),
        "D7-READ: with guard.status also failing the protection state is unknown (null)"
    );
}

#[test]
fn another_repo_is_not_open_and_calls_nothing() {
    let daemon = daemon_with(script_tienda_guard);
    let mut h = Harness::open_plain(&daemon);
    h.ask("notas");
    assert_eq!(
        h.wait_unavailable(),
        json!({"type": "guardBlocksUnavailable", "repoId": "notas",
               "cause": "not-open", "protection": null}),
        "D7-READ: a repo that is not the open one answers not-open"
    );
    assert!(
        guard_calls(&daemon).is_empty(),
        "D7-READ: not-open must not call the engine: {:?}",
        guard_calls(&daemon)
    );
}

#[test]
fn without_an_open_repo_the_answer_is_not_open() {
    let daemon = daemon_with(script_tienda_guard);
    let mut h = Harness::spawn(&daemon, Box::new(daemon.connector()));
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
    h.ask("tienda");
    assert_eq!(
        h.wait_unavailable()["cause"],
        "not-open",
        "D7-READ: with no repo open nothing is read"
    );
    assert!(
        guard_calls(&daemon).is_empty(),
        "D7-READ: not-open must not call the engine"
    );
}

#[test]
fn untrusted_branch_worktree_and_remote_reach_the_webview_escaped() {
    let (_dir, exe) = installed_raptor("daemon-", "bin");
    let branch = "x\u{1b}]52;c;AAAA\u{7}";
    let worktree = "/w/wt/fe\u{202e}at";
    let remote = "or\u{2028}igin";
    let reference = "ma\u{e0041}in";
    let daemon = daemon_with(|s| {
        script_tienda_guard(s);
        s.guard_log = Some(GuardLogResult {
            entries: vec![log_entry(json!({
                "worktree": {"untrusted": worktree},
                "branch": {"untrusted": branch},
                "operation": {"kind": "push", "remote": {"untrusted": remote},
                              "refs": [{"name": {"untrusted": reference}, "change": "force"}]}
            }))],
            ..fake_daemon::tienda_guard_log()
        });
    });
    let mut h = Harness::open_with(&daemon, &exe);
    let blocks = h.read();
    let e = entry(&blocks, "d-push");
    assert_eq!(
        e["branch"],
        json!(visible(branch, NAME_MAX_CHARS)),
        "D7-READ: the branch arrives escaped by display::visible"
    );
    assert_eq!(
        e["worktree"],
        json!({"folder": visible("fe\u{202e}at", NAME_MAX_CHARS),
               "path": visible(worktree, TEXT_MAX_CHARS)}),
        "D7-READ: the worktree arrives as an escaped folder and path"
    );
    assert_eq!(
        e["operation"],
        json!({"kind": "push", "remote": visible(remote, NAME_MAX_CHARS),
               "refs": [{"name": visible(reference, NAME_MAX_CHARS), "change": "force"}]}),
        "D7-READ: the remote and the ref names arrive escaped"
    );
    assert_eq!(
        e["exception"],
        json!({"state": "not-offered", "reason": "unsafe-text"}),
        "D7-READ: hostile text never gives a command"
    );
    let wire = serde_json::to_string(&blocks).expect("json");
    for raw in ['\u{1b}', '\u{7}', '\u{202e}', '\u{2028}', '\u{e0041}'] {
        assert!(
            !wire.contains(raw),
            "D7-READ: the raw character U+{:04X} reached the webview",
            u32::from(raw)
        );
    }
}

#[test]
fn a_restricted_session_never_reads_the_log() {
    // Control: an unrestricted session does read the log, so the check below is not vacuous.
    let daemon = daemon_with(script_tienda_guard);
    let mut open = Harness::open_plain(&daemon);
    open.read();
    assert_eq!(
        daemon.calls_of(LOG).len(),
        1,
        "D7-READ: the unrestricted control must have read guard.log once"
    );
    drop(open);

    let restricted = daemon_with(|s| {
        script_tienda_guard(s);
        s.requester = Some(ConnectionRequester::Resolved {
            actor: Actor::Agent {
                kind: AgentKind::ClaudeCode,
                name: None,
                origin: AgentOrigin::Detected,
            },
            layer: Layer::Mcp,
            confirmable: false,
        });
    });
    let mut h = Harness::spawn(&restricted, Box::new(restricted.connector()));
    h.wait_for("a restricted `session`", |m| match m {
        EngineMessage::Session { restricted, .. } => restricted.then_some(()),
        _ => None,
    });
    h.send(HubCmd::Open {
        repo_id: "tienda".to_owned(),
    });
    h.ask("tienda");
    let end = Instant::now() + Duration::from_millis(500);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        if let Ok(message) = h.rx.recv_timeout(left) {
            assert!(
                !matches!(
                    message,
                    EngineMessage::GuardBlocks { .. }
                        | EngineMessage::GuardBlocksUnavailable { .. }
                ),
                "D7-READ: a restricted view got guard data: {message:?}"
            );
            h.seen.push(message);
        }
    }
    assert!(
        guard_calls(&restricted).is_empty(),
        "D7-READ: a restricted view never calls guard.status nor guard.log: {:?}",
        guard_calls(&restricted)
    );
}

// ---------------------------------------------------------------- D7-RATE

#[test]
fn two_reads_within_two_seconds_call_the_log_once() {
    let daemon = daemon_with(script_tienda_guard);
    let mut h = Harness::open_plain(&daemon);
    h.ask("tienda");
    h.ask("tienda");
    // A probe that answers after the two requests were handled.
    h.ask("notas");
    h.wait_unavailable();
    assert_eq!(
        daemon.calls_of(LOG).len(),
        1,
        "D7-RATE: the second read within 2 s is dropped, so guard.log is called once"
    );
    assert_eq!(
        h.guard_messages(),
        1,
        "D7-RATE: one read sends one guardBlocks"
    );
}

#[test]
fn a_reserved_audit_of_an_exception_reads_the_log_again() {
    let daemon = daemon_with(script_tienda_guard);
    let mut h = Harness::open_plain(&daemon);
    h.read();
    // Not an exception of this repo: no read. Then the one that is.
    assert!(daemon.push_global_event(6, "reserved.audit", audit("guard.install", "tienda")));
    assert!(daemon.push_global_event(7, "reserved.audit", audit("guard.exception", "notas")));
    assert!(daemon.push_global_event(8, "reserved.audit", audit("guard.exception", "tienda")));
    assert!(
        daemon.wait_for_calls(LOG, 2, Duration::from_millis(1500)),
        "D7-RATE: a reserved.audit of guard.exception in the open repo must read guard.log again \
         without waiting 2 s; calls: {:?}",
        guard_calls(&daemon)
    );
    h.wait_blocks();
    assert!(
        !daemon.wait_for_calls(LOG, 3, QUIET),
        "D7-RATE: only the exception of the open repo reads again; the install and the other \
         repo do not"
    );
    assert_only_reads(&daemon, &[]);
}

#[test]
fn a_burst_of_reserved_audit_reads_the_log_at_most_twice() {
    let daemon = daemon_with(script_tienda_guard);
    let mut h = Harness::open_plain(&daemon);
    h.read();
    for seq in 6..26 {
        assert!(daemon.push_global_event(
            seq,
            "reserved.audit",
            audit("guard.exception", "tienda")
        ));
    }
    assert!(
        daemon.wait_for_calls(LOG, 2, DEADLINE),
        "D7-RATE: a burst of reserved.audit must read guard.log again; calls: {:?}",
        guard_calls(&daemon)
    );
    assert!(
        !daemon.wait_for_calls(LOG, 4, QUIET),
        "D7-RATE: twenty events must join into at most two reads, got {}",
        daemon.calls_of(LOG).len()
    );
}

#[test]
fn the_command_uses_the_daemon_raptor_of_the_ready_report() {
    let (_dir, exe) = installed_raptor("daemon-", "bin");
    let daemon = daemon_with(script_tienda_guard);
    let mut h = Harness::open_with(&daemon, &exe);
    let blocks = h.read();
    let command = entry(&blocks, "d-push")["exception"]["command"]
        .as_str()
        .expect("D7-RATE: the force-push offers a command")
        .to_owned();
    assert!(
        command.starts_with(&quoted(&exe, "'guard' 'exec' '--' 'git'")),
        "D7-RATE: the command names the raptor of the daemon: {command}"
    );
}

#[test]
fn a_daemon_raptor_inside_a_watched_worktree_falls_back_to_the_bundle() {
    // The repo is the folder that holds the daemon's `raptor`: it must not be named. The test
    // process has no `raptor` beside it, so the bundle is unknown too and nothing is offered.
    let (dir, exe) = installed_raptor("watched-", "bin");
    let root = dir.path().canonicalize().expect("canonical root");
    let git = format!("{}/.git", root.display());
    let daemon = daemon_with(|s| {
        script_tienda_guard(s);
        s.global.repos[0].path = Untrusted::new(git.clone());
        s.repos.insert(
            "tienda".to_owned(),
            repo_snapshot(
                "tienda",
                &git,
                REPO_SEQ,
                vec![worktree_view(
                    &root.display().to_string(),
                    true,
                    "main",
                    0,
                    0,
                    0,
                )],
            ),
        );
    });
    let mut h = Harness::open_with(&daemon, &exe);
    let blocks = h.read();
    assert_eq!(
        daemon.calls_of(LOG),
        [json!({"path": git, "limit": 50})],
        "D7-RATE: the log of the open repo is read with its raw path"
    );
    assert_eq!(
        entry(&blocks, "d-push")["exception"],
        json!({"state": "not-offered", "reason": "raptor-unknown"}),
        "D7-RATE: a daemon raptor inside a watched repo is not named; with no bundle there is none"
    );
}

// ---------------------------------------------------------------- D7-ONLYREAD

#[test]
fn a_guard_run_never_calls_exception_cancel_or_writes() {
    let (_dir, exe) = installed_raptor("daemon-", "bin");
    let daemon = daemon_with(|s| {
        script_tienda_guard(s);
        s.guard_status = Some(guard_status(
            ProtectionState::Full,
            Some(pending_exception(
                forced_main_push(),
                vec![Rule::MinimumForcePush],
            )),
        ));
    });
    let mut h = Harness::open_with(&daemon, &exe);
    let first = h.read();
    assert!(daemon.push_global_event(6, "reserved.audit", audit("guard.exception", "tienda")));
    assert!(
        daemon.wait_for_calls(LOG, 2, DEADLINE),
        "D7-ONLYREAD: the run must reach a second read (reserved.audit); calls: {:?}",
        guard_calls(&daemon)
    );
    let second = h.wait_blocks();
    let methods: Vec<String> = guard_calls(&daemon).into_iter().map(|(m, _)| m).collect();
    assert!(
        methods.iter().any(|m| m == STATUS) && methods.iter().any(|m| m == LOG),
        "D7-ONLYREAD: the run read guard.status and guard.log: {methods:?}"
    );
    assert_only_reads(&daemon, &[&first, &second]);
    assert_read_only(&daemon.calls());
}
