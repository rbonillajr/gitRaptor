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
        authorship: None,
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
        tiers: Default::default(),
        discovery: Default::default(),
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

/// US-GRD-018 on the daemon's evaluation, without a channel: a confirmed floor relaxes to
/// `flexible`, an unconfirmed one does not (D2); without `guard.authorship` no authorship rule
/// applies and no notice travels, and the old wire shapes are unchanged (D11).
mod commit_authorship {
    use super::*;
    use gitraptor_api::AgentKind;
    use gitraptor_api::guard::{AuthorshipFacts, CommitStage};
    use gitraptor_core::guardrails::evaluate::{Caller, serve_as};
    use gitraptor_core::guardrails::{GuardEntry, GuardRegistry};
    use gitraptor_policy::team::{Confirmed, ConfirmedFloor};

    fn repo_with(settings: &str) -> (tempfile::TempDir, std::path::PathBuf, String) {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "-q", "-b", "main"]);
        std::fs::create_dir_all(repo.path().join(".gitraptor")).unwrap();
        std::fs::write(repo.path().join(".gitraptor/settings.json"), settings).unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "a"]);
        let blob = std::process::Command::new("git")
            .args(["rev-parse", "main:.gitraptor/settings.json"])
            .current_dir(repo.path())
            .output()
            .unwrap();
        let blob = String::from_utf8(blob.stdout).unwrap().trim().to_owned();
        let common = repo.path().join(".git").canonicalize().unwrap();
        (repo, common, blob)
    }

    fn commit_params(
        common: &std::path::Path,
        coauthors: Vec<Option<AgentKind>>,
    ) -> EvaluateParams {
        EvaluateParams {
            repo_id: "0000-ffff".into(),
            common_dir: common.to_string_lossy().into_owned(),
            hook: Hook::CommitMsg,
            operation: Operation::Commit {
                stage: CommitStage::CommitMsg,
            },
            authorship: Some(AuthorshipFacts {
                coauthors,
                trailer_table: 1,
                unreadable: false,
            }),
        }
    }

    fn registry(common: &std::path::Path, confirmed: Option<Confirmed>) -> GuardRegistry {
        let r = GuardRegistry::default();
        r.set(
            "0000-ffff",
            GuardEntry {
                common_dir: common.to_string_lossy().into_owned(),
                bases: vec!["main".into()],
                confirmed,
            },
        );
        r
    }

    fn agent(cwd: &std::path::Path) -> Caller {
        Caller {
            actor: Some(AgentKind::ClaudeCode),
            cwd: Some(cwd.to_path_buf()),
            authorship: true,
            ..Caller::default()
        }
    }

    /// DS-US-GRD-018 § 11 (S5, S6): the second line evaluates like `commit-msg`, unless the
    /// channel found the same `git` already decided or certainly a rebase (`second_line_skip`).
    #[test]
    fn the_second_line_evaluates_unless_skipped() {
        let (repo, common, _) =
            repo_with(r#"{"policies":{"commitAuthorship":{"mode":"human-author"}}}"#);
        let mut params = commit_params(&common, vec![Some(AgentKind::ClaudeCode)]);
        params.hook = Hook::ReferenceTransaction;
        params.operation = Operation::Commit {
            stage: CommitStage::SecondLine,
        };
        let d = serve_as(&registry(&common, None), &params, &agent(repo.path()));
        assert_eq!(d.applied_effect, Effect::Deny, "{d:?}");
        assert_eq!(d.reasons[0].rule, Rule::AuthorshipHumanAuthor);

        let skipped = Caller {
            second_line_skip: true,
            ..agent(repo.path())
        };
        let d = serve_as(&registry(&common, None), &params, &skipped);
        assert_eq!(d.applied_effect, Effect::Allow, "{d:?}");
        assert!(d.reasons.is_empty() && d.notices.is_empty(), "{d:?}");
    }

    #[test]
    fn a_confirmed_floor_relaxes_to_flexible() {
        let (repo, common, blob) =
            repo_with(r#"{"policies":{"commitAuthorship":{"mode":"flexible"}}}"#);
        let confirmed = Confirmed {
            base_branch: gitraptor_git::RefName::new("main").unwrap(),
            floor: ConfirmedFloor::Blob(blob),
        };
        let d = serve_as(
            &registry(&common, Some(confirmed.clone())),
            &commit_params(&common, vec![]),
            &agent(repo.path()),
        );
        assert_eq!(d.applied_effect, Effect::Allow, "{d:?}");

        // US-GRD-005: the agent's `flexible` commit is still recorded, as a notice.
        let params = commit_params(&common, vec![]);
        let (d, policy) = gitraptor_core::guardrails::evaluate::serve_logged(
            &registry(&common, Some(confirmed)),
            &params,
            &agent(repo.path()),
        );
        assert_eq!(policy, Some("flexible"));
        let ctx = gitraptor_core::guardrails::log::LogContext {
            actor: Some(AgentKind::ClaudeCode),
            authorship_policy: policy.map(str::to_owned),
            ..Default::default()
        };
        let e = gitraptor_core::guardrails::log::entry(&params, &d, &ctx).expect("recorded");
        assert_eq!(e.kind, gitraptor_api::guard::LogKind::Notice);
        assert_eq!(e.authorship.unwrap().policy.as_deref(), Some("flexible"));

        // Unconfirmed: `agents-commit` rules.
        let d = serve_as(
            &registry(&common, None),
            &commit_params(&common, vec![]),
            &agent(repo.path()),
        );
        assert_eq!(d.applied_effect, Effect::Deny);
        assert_eq!(d.reasons[0].rule, Rule::AuthorshipTrailerRequired);
    }

    #[test]
    fn without_the_capability_nothing_changes() {
        let (repo, common, _) = repo_with(
            r#"{"policies":{"commitAuthorship":{"mode":"human-author","onAgentCommit":"warn"}}}"#,
        );
        let params = commit_params(&common, vec![Some(AgentKind::ClaudeCode)]);
        let d = serve_as(&registry(&common, None), &params, &agent(repo.path()));
        assert_eq!(d.applied_effect, Effect::Allow);
        assert_eq!(d.notices.len(), 1, "{d:?}");
        assert_eq!(d.notices[0].rule, Rule::AuthorshipHumanAuthor);

        let old = Caller {
            authorship: false,
            ..agent(repo.path())
        };
        let d = serve_as(
            &registry(&common, None),
            &commit_params(&common, vec![]),
            &old,
        );
        assert_eq!(d.applied_effect, Effect::Allow);
        assert!(d.notices.is_empty());
        // The wire shape of a decision without notices is the old one.
        let wire = serde_json::to_value(&d).unwrap();
        assert!(wire.get("notices").is_none(), "{wire}");
        // A request without `authorship` parses as before.
        let legacy = serde_json::json!({
            "repo_id": "0000-ffff",
            "common_dir": common.to_string_lossy(),
            "hook": "reference-transaction",
            "operation": {"kind": "ref-transaction", "updates": []}
        });
        let p: EvaluateParams = serde_json::from_value(legacy).unwrap();
        assert!(p.authorship.is_none());
    }
}

/// US-GRD-008 on the daemon's evaluation, without a channel: the protected branches and
/// forbidden paths apply to a connection that asked for `guard.policies` and to the actor it
/// resolved (D3, D9), and whatever cannot be verified within the bounds is denied, never allowed
/// (D5).
mod protected_branches_and_forbidden_paths {
    use super::*;
    use gitraptor_api::AgentKind;
    use gitraptor_core::guardrails::evaluate::{Caller, serve_as};
    use gitraptor_core::guardrails::{GuardEntry, GuardRegistry};

    const SETTINGS: &str = r#"{"policies":{
        "protectedBranches":{"patterns":["main"]},
        "forbiddenPaths":{"patterns":["secrets/"]}}}"#;

    fn rev(repo: &std::path::Path, what: &str) -> String {
        let out = std::process::Command::new("git")
            .args(["rev-parse", what])
            .current_dir(repo)
            .output()
            .unwrap();
        String::from_utf8(out.stdout).unwrap().trim().to_owned()
    }

    fn repo() -> (tempfile::TempDir, std::path::PathBuf) {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "-q", "-b", "main"]);
        std::fs::create_dir_all(repo.path().join(".gitraptor")).unwrap();
        std::fs::write(repo.path().join(".gitraptor/settings.json"), SETTINGS).unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "a"]);
        let common = repo.path().join(".git").canonicalize().unwrap();
        (repo, common)
    }

    fn registry(common: &std::path::Path) -> GuardRegistry {
        let r = GuardRegistry::default();
        r.set(
            "0000-ffff",
            GuardEntry {
                common_dir: common.to_string_lossy().into_owned(),
                bases: vec!["main".into()],
                confirmed: None,
            },
        );
        r
    }

    fn update(common: &std::path::Path, refname: &str, old: &str, new: &str) -> EvaluateParams {
        let value = |v: &str| RefValue::Oid(v.to_owned());
        EvaluateParams {
            repo_id: "0000-ffff".into(),
            common_dir: common.to_string_lossy().into_owned(),
            hook: Hook::ReferenceTransaction,
            operation: Operation::RefTransaction {
                updates: vec![RefUpdate {
                    refname: refname.into(),
                    old: value(old),
                    new: value(new),
                }],
                orphan_head: None,
            },
            authorship: None,
        }
    }

    fn caller(repo: &std::path::Path, agent: bool, policies: bool) -> Caller {
        Caller {
            actor: agent.then_some(AgentKind::ClaudeCode),
            cwd: Some(repo.to_path_buf()),
            policies,
            ..Caller::default()
        }
    }

    #[test]
    fn they_apply_to_the_agent_of_a_connection_that_asked_for_them() {
        let (repo, common) = repo();
        let head = rev(repo.path(), "HEAD");
        git(repo.path(), &["commit", "-q", "--allow-empty", "-m", "b"]);
        let next = rev(repo.path(), "HEAD");
        let params = update(&common, "refs/heads/main", &head, &next);

        let d = serve_as(
            &registry(&common),
            &params,
            &caller(repo.path(), true, true),
        );
        assert_eq!(d.applied_effect, Effect::Deny, "{d:?}");
        assert_eq!(d.reasons[0].rule, Rule::ProtectedBranch);
        // The person, and a connection that did not ask for them (an older hook): as before.
        for (agent, policies) in [(false, true), (true, false), (false, false)] {
            let d = serve_as(
                &registry(&common),
                &params,
                &caller(repo.path(), agent, policies),
            );
            assert_eq!(d.applied_effect, Effect::Allow, "{agent} {policies} {d:?}");
        }
    }

    #[test]
    fn what_cannot_be_verified_is_denied_never_allowed() {
        let (repo, common) = repo();
        let base = rev(repo.path(), "HEAD");
        let denied = |new: &str| {
            let params = update(&common, "refs/heads/feat-x", &base, new);
            serve_as(
                &registry(&common),
                &params,
                &caller(repo.path(), true, true),
            )
        };
        let check = |d: gitraptor_api::guard::Decision, why: &str| {
            assert_eq!(d.applied_effect, Effect::Deny, "{why}: {d:?}");
            assert_eq!(d.reasons[0].rule, Rule::ForbiddenPath, "{why}");
            assert_eq!(d.reasons[0].cause, Some(Cause::Unverifiable), "{why}");
        };
        // An object the repo does not have.
        check(denied(&"e".repeat(40)), "missing object");
        // More new commits than the bound (256): a chain written with plumbing, nothing moved.
        let tree = rev(repo.path(), "HEAD^{tree}");
        let mut tip = base.clone();
        for n in 0..300 {
            let out = std::process::Command::new("git")
                .args(["commit-tree", &tree, "-p", &tip, "-m", &format!("c{n}")])
                .current_dir(repo.path())
                .output()
                .unwrap();
            tip = String::from_utf8(out.stdout).unwrap().trim().to_owned();
        }
        check(denied(&tip), "more commits than the bound");
        // The same chain by the person: nothing to verify, nothing denied.
        let params = update(&common, "refs/heads/feat-x", &base, &tip);
        let d = serve_as(
            &registry(&common),
            &params,
            &caller(repo.path(), false, true),
        );
        assert_eq!(d.applied_effect, Effect::Allow, "{d:?}");
    }
}

/// Measurement of DS-US-GRD-008 D12, not a gate (a performance gate never runs in a debug test):
/// the evaluation of an agent's branch movement with 2 000 branches, for one new commit and for
/// a range of 256. Run it in release: `cargo test --release -p gitraptor-core --test
/// guard_evaluate policies_cost -- --ignored --nocapture`.
mod policies_cost {
    use super::*;
    use gitraptor_api::AgentKind;
    use gitraptor_core::guardrails::evaluate::{Caller, serve_as};
    use gitraptor_core::guardrails::{GuardEntry, GuardRegistry};

    #[test]
    #[ignore = "measurement, run in release"]
    fn measure_an_evaluation_with_2000_branches() {
        let repo = tempfile::tempdir().unwrap();
        let path = repo.path();
        git(path, &["init", "-q", "-b", "main"]);
        std::fs::create_dir_all(path.join(".gitraptor")).unwrap();
        std::fs::write(
            path.join(".gitraptor/settings.json"),
            r#"{"policies":{"protectedBranches":{"patterns":["main","release/*"]},
                "forbiddenPaths":{"patterns":["secrets/","*.pem"]}}}"#,
        )
        .unwrap();
        git(path, &["add", "."]);
        git(path, &["commit", "-q", "-m", "a"]);
        let rev = |what: &str| {
            let out = std::process::Command::new("git")
                .args(["rev-parse", what])
                .current_dir(path)
                .output()
                .unwrap();
            String::from_utf8(out.stdout).unwrap().trim().to_owned()
        };
        let base = rev("HEAD");
        let tree = rev("HEAD^{tree}");
        let refs: String = (0..1999)
            .map(|n| format!("create refs/heads/b/{n} {base}\n"))
            .collect();
        let mut child = std::process::Command::new("git")
            .args(["update-ref", "--stdin"])
            .current_dir(path)
            .stdin(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        std::io::Write::write_all(&mut child.stdin.take().unwrap(), refs.as_bytes()).unwrap();
        assert!(child.wait().unwrap().success());
        let mut tip = base.clone();
        let mut one = String::new();
        for n in 0..256 {
            let out = std::process::Command::new("git")
                .args(["commit-tree", &tree, "-p", &tip, "-m", &format!("c{n}")])
                .current_dir(path)
                .output()
                .unwrap();
            tip = String::from_utf8(out.stdout).unwrap().trim().to_owned();
            if n == 0 {
                one = tip.clone();
            }
        }
        let common = path.join(".git").canonicalize().unwrap();
        let registry = GuardRegistry::default();
        registry.set(
            "0000-ffff",
            GuardEntry {
                common_dir: common.to_string_lossy().into_owned(),
                bases: vec!["main".into()],
                confirmed: None,
            },
        );
        let caller = Caller {
            actor: Some(AgentKind::ClaudeCode),
            cwd: Some(path.to_path_buf()),
            policies: true,
            ..Caller::default()
        };
        let evaluation = |new: &str| EvaluateParams {
            repo_id: "0000-ffff".into(),
            common_dir: common.to_string_lossy().into_owned(),
            hook: Hook::ReferenceTransaction,
            operation: Operation::RefTransaction {
                updates: vec![RefUpdate {
                    refname: "refs/heads/feat-x".into(),
                    old: RefValue::Oid(base.clone()),
                    new: RefValue::Oid(new.to_owned()),
                }],
                orphan_head: None,
            },
            authorship: None,
        };
        let person = Caller {
            actor: None,
            ..caller.clone()
        };
        let d = serve_as(&registry, &evaluation(&one), &person);
        assert_eq!(d.applied_effect, Effect::Allow, "{d:?}");
        let mut times: Vec<f64> = (0..21)
            .map(|_| {
                let start = Instant::now();
                serve_as(&registry, &evaluation(&one), &person);
                start.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "the person (nothing to read): p50 {:.1} ms, p95 {:.1} ms",
            times[10], times[19]
        );
        for (label, new) in [("1 new commit", &one), ("256 new commits", &tip)] {
            let mut times: Vec<f64> = (0..21)
                .map(|_| {
                    let start = Instant::now();
                    let d = serve_as(&registry, &evaluation(new), &caller);
                    assert_eq!(d.applied_effect, Effect::Allow, "{d:?}");
                    start.elapsed().as_secs_f64() * 1000.0
                })
                .collect();
            times.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!(
                "{label}: p50 {:.1} ms, p95 {:.1} ms (2000 branches)",
                times[10], times[19]
            );
        }
    }
}
