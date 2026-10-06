//! `guard.evaluate` in the daemon (US-GRD-001, ADR-GRD-003 § 3 and § 4): served on the
//! connection thread, so it never waits for the daemon loop nor for a repo's write lock (no
//! deadlock with the executor's own `git`, ADR-GRD-003 Enmienda Cockpit), and fail-closed on
//! input it cannot validate. Temporary profile and repos only (NFR-01).
#![cfg(unix)]

mod common;

use std::time::{Duration, Instant};

use common::{TempProfile, git};
use gitraptor_api::guard::{
    Cause, Decision, Effect, EvaluateParams, Hook, Operation, RefUpdate, RefValue, Rule,
};
use gitraptor_api::messages::ClientKind;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::Client;
use gitraptor_core::daemon::{Daemon, DaemonConfig, DaemonEnv, LogLimits, StopCause};
use gitraptor_git::resolve::ResolveConfig;

fn no_git() -> ResolveConfig {
    ResolveConfig {
        configured_path: None,
        path_env: None,
        known_locations: Vec::new(),
        shim_paths: Vec::new(),
        toolchain_gits: Vec::new(),
    }
}

fn params(common_dir: &std::path::Path, refname: &str) -> EvaluateParams {
    EvaluateParams {
        repo_id: "0000-ffff".into(),
        common_dir: common_dir.to_string_lossy().into_owned(),
        hook: Hook::ReferenceTransaction,
        operation: Operation::RefTransaction {
            updates: vec![RefUpdate {
                refname: refname.into(),
                old: RefValue::Zero,
                new: RefValue::Zero,
            }],
            orphan_head: None,
        },
    }
}

#[test]
fn repo_intact_evaluation_never_waits_and_fails_closed() {
    let profile = TempProfile::new();
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-q", "-b", "main"]);
    git(repo.path(), &["commit", "-q", "--allow-empty", "-m", "a"]);
    let common_dir = repo.path().join(".git").canonicalize().unwrap();

    let config = DaemonConfig {
        dirs: profile.dirs(),
        env: DaemonEnv::from_vars(Vec::new()),
        git: no_git(),
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tm_capture: Default::default(),
    };
    let daemon = Daemon::start(config).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    let mut client = Client::connect(&profile.dirs(), ClientKind::Other, PROTOCOL_VERSION).unwrap();

    // The repo's write lock is held, as by an operation of the executor whose own `git` runs
    // the hook: the evaluation answers anyway.
    let lock_key = common_dir.to_string_lossy().into_owned();
    let _held = gitraptor_core::repo_lock::try_lock(&lock_key).unwrap();
    let start = Instant::now();
    let decision: Decision = client
        .call(
            methods::GUARD_EVALUATE,
            params(&common_dir, "refs/heads/main"),
        )
        .unwrap();
    assert!(start.elapsed() < Duration::from_secs(5));
    // An unknown repo is evaluated with {main, main branch}: never less (ADR-GRD-004 § 3.5).
    assert_eq!(decision.applied_effect, Effect::Deny);
    assert_eq!(decision.reasons[0].rule, Rule::MinimumBaseBranchDelete);
    assert_eq!(decision.reasons[0].cause, Some(Cause::Delete));
    assert!(!decision.decision_id.is_empty());

    // A feature branch deletion is allowed.
    let decision: Decision = client
        .call(
            methods::GUARD_EVALUATE,
            params(&common_dir, "refs/heads/feat"),
        )
        .unwrap();
    assert_eq!(decision.applied_effect, Effect::Allow);

    // Input it cannot validate is denied, never allowed (SEC-GRD-07).
    let decision: Decision = client
        .call(
            methods::GUARD_EVALUATE,
            params(&common_dir, "refs/heads/a..b"),
        )
        .unwrap();
    assert_eq!(decision.applied_effect, Effect::Deny);
    assert_eq!(decision.reasons[0].rule, Rule::InputRejected);
    let mut relative = params(&common_dir, "refs/heads/main");
    relative.common_dir = "relative/.git".into();
    let decision: Decision = client.call(methods::GUARD_EVALUATE, relative).unwrap();
    assert_eq!(decision.reasons[0].rule, Rule::InputRejected);

    drop(client);

    // Protocol 7: a client of version 5 or 6 neither sees nor reaches `guard.*`, reserved
    // ones included (nothing to audit), and keeps its own shapes.
    for older in [5, 6] {
        let mut old = Client::connect(&profile.dirs(), ClientKind::Cli, older).unwrap();
        assert!(
            !old.hello().methods.iter().any(|m| m.starts_with("guard.")),
            "protocol {older}"
        );
        for method in [methods::GUARD_INSTALL, methods::GUARD_EVALUATE] {
            let err = old
                .call::<_, serde_json::Value>(method, serde_json::json!({"path": "/x"}))
                .unwrap_err();
            assert!(
                matches!(&err, gitraptor_core::client::ClientError::Rpc(e) if e.code == gitraptor_api::rpc::code::METHOD_NOT_FOUND),
                "protocol {older} {method}: {err:?}"
            );
        }
    }
    let current = Client::connect(&profile.dirs(), ClientKind::Cli, PROTOCOL_VERSION).unwrap();
    assert!(
        current
            .hello()
            .methods
            .iter()
            .any(|m| m == methods::GUARD_EVALUATE)
    );
    drop(current);

    handle.request(StopCause::Signal("TERM"));
    join.join().unwrap();
}
