//! US-GRP-020 and US-GRP-022: discovery roots and discovered repos with a
//! real daemon (in-process) over a temporary profile and temporary folders
//! (NFR-01). Declaring a root for real needs a developer in a terminal: that
//! path is in `apps/cli/tests/discovery_roots.rs`; here the roots are seeded
//! in the profile, as `discovery.root.add` leaves them.
#![cfg(target_os = "macos")]

mod common;

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::*;
use gitraptor_api::discovery::{CandidateView, CandidatesResult, RepoDiscoveredData, RootsResult};
use gitraptor_api::event::REPO_DISCOVERED;
use gitraptor_api::messages::{
    AuditListResult, AuditOutcome, ClientKind, RefusalReason, RefusedData, SubscribeResult,
};
use gitraptor_api::rpc::code;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::{AgentMatcher, ChannelConfig};
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, DiscoveryConfig, LogLimits, ShutdownHandle, StopCause,
    StopReport,
};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_git::resolve::ResolveConfig;
use serde_json::json;

const DEADLINE: Duration = Duration::from_secs(20);

fn no_git() -> ResolveConfig {
    ResolveConfig {
        configured_path: None,
        path_env: None,
        known_locations: Vec::new(),
        shim_paths: Vec::new(),
        toolchain_gits: Vec::new(),
    }
}

/// This test binary as the simulated agent: every reserved command it sends
/// is refused by ancestry.
fn self_as_agent() -> AgentMatcher {
    let exe = std::env::current_exe().unwrap();
    AgentMatcher::only(vec![
        exe.file_name().unwrap().to_string_lossy().into_owned(),
    ])
}

struct Running {
    dirs: ProfileDirs,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

impl Running {
    fn start(dirs: ProfileDirs, channel: ChannelConfig, discovery: DiscoveryConfig) -> Self {
        let config = DaemonConfig {
            dirs: dirs.clone(),
            env: DaemonEnv::from_vars(Vec::new()),
            git: no_git(),
            heartbeat: Duration::from_secs(3600),
            log: LogLimits::default(),
            stop_deadline: None,
            channel,
            protected: None,
            operations: None,
            tm_prior_layer: None,
            tiers: Default::default(),
            discovery,
            tm_capture: Default::default(),
        };
        let daemon = Daemon::start(config).unwrap();
        let handle = daemon.shutdown_handle();
        let join = std::thread::spawn(move || daemon.run());
        Self {
            dirs,
            handle,
            join: Some(join),
        }
    }

    fn client(&self, kind: ClientKind) -> Client {
        let start = Instant::now();
        loop {
            match Client::connect(&self.dirs, kind, PROTOCOL_VERSION) {
                Ok(client) => return client,
                Err(_) if start.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(err) => panic!("connect: {err}"),
            }
        }
    }

    fn stop(mut self) -> StopReport {
        self.handle.request(StopCause::Signal("TERM"));
        self.join.take().unwrap().join().unwrap()
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

/// Every root listed every 20 ms, a temporary home.
fn fast(home: &Path) -> DiscoveryConfig {
    DiscoveryConfig {
        home: Some(home.to_path_buf()),
        ..DiscoveryConfig::default()
    }
    .every(Duration::from_millis(20))
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap()
}

fn text(path: &Path) -> String {
    canonical(path).to_str().unwrap().to_owned()
}

fn candidates(client: &mut Client) -> Vec<CandidateView> {
    let result: CandidatesResult = client
        .call(methods::DISCOVERY_CANDIDATES, json!({}))
        .unwrap();
    result.candidates
}

/// The candidates' names once they are exactly `want`, or a panic at the
/// deadline: no fixed waits.
fn wait_for(client: &mut Client, want: &[&str]) -> Vec<CandidateView> {
    let start = Instant::now();
    loop {
        let found = candidates(client);
        let mut names: Vec<&str> = found.iter().map(|c| c.name.as_str()).collect();
        names.sort_unstable();
        if names == want {
            return found;
        }
        assert!(
            start.elapsed() < DEADLINE,
            "candidates {names:?}, want {want:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn refusal(err: ClientError) -> RefusalReason {
    match err {
        ClientError::Rpc(e) => {
            assert_eq!(e.code, code::RESERVED_REFUSED, "{e}");
            serde_json::from_value::<RefusedData>(e.data.unwrap())
                .unwrap()
                .reason
        }
        other => panic!("expected a refusal, got {other}"),
    }
}

/// Every file under `dir` with its length and modification time.
fn fingerprint(dir: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let mut out: Vec<_> = files_under(dir)
        .into_iter()
        .map(|p| {
            let meta = std::fs::symlink_metadata(&p).unwrap();
            (p, meta.len(), meta.modified().unwrap())
        })
        .collect();
    out.sort();
    out
}

/// A declared root proposes the repos of its first level, keeps them
/// across a restart and never observes them; a repo cloned later is
/// announced with `repo.discovered`; a subfolder's repo and a worktree of
/// an observed repo are not proposed; nothing is written in the root.
#[test]
fn discovery_a_root_proposes_its_first_level_repos_without_observing_them() {
    let tp = TempProfile::new();
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let code = home.join("code");
    std::fs::create_dir_all(&code).unwrap();
    let shop = init_repo(&code, "shop", true);
    init_repo(&code, "api", true);
    init_repo(&code.join("clientes"), "acme", false);
    git(&shop, &["worktree", "add", "-q", "../shop-feat"]);
    init_repo(&code, ".dotfiles", false);
    {
        let mut profile = tp.open();
        profile
            .add_repo(&common_dir(&shop), None, 1)
            .expect("shop observed");
        profile.add_discovery_root(&text(&code), false, 1).unwrap();
    }
    let before = fingerprint(&code);

    let r = Running::start(tp.dirs(), ChannelConfig::default(), fast(&home));
    let mut client = r.client(ClientKind::Cli);
    let mut watcher = r.client(ClientKind::Cli);
    let _: SubscribeResult = watcher.call(methods::EVENTS_SUBSCRIBE, json!({})).unwrap();
    let roots: RootsResult = client.call(methods::DISCOVERY_ROOTS, json!({})).unwrap();
    assert_eq!(roots.roots.len(), 1);
    assert_eq!(roots.roots[0].path, text(&code));
    assert!(!roots.roots[0].broad);

    let found = wait_for(&mut client, &["api"]);
    assert_eq!(found[0].path, text(&code.join("api")));
    assert_eq!(found[0].root, text(&code));

    // A repo cloned later, with any tool, is announced.
    init_repo(&code, "billing", false);
    wait_for(&mut client, &["api", "billing"]);
    let start = Instant::now();
    let announced = loop {
        let n = watcher
            .next_notification(Duration::from_millis(200))
            .unwrap();
        if let Some(n) = n
            && n.params["event"]["kind"] == REPO_DISCOVERED
        {
            let data: RepoDiscoveredData =
                serde_json::from_value(n.params["event"]["data"].clone()).unwrap();
            if data.candidates.iter().any(|c| c.name == "billing") {
                break data;
            }
        }
        assert!(start.elapsed() < DEADLINE, "no repo.discovered for billing");
    };
    assert_eq!(announced.root, text(&code));
    assert_eq!(announced.count, 1);

    // A deleted repo stops being proposed.
    std::fs::remove_dir_all(code.join("billing")).unwrap();
    wait_for(&mut client, &["api"]);

    drop((client, watcher));
    r.stop();
    // Nothing observed but shop; discovery wrote nothing in the root.
    let profile = tp.open();
    let observed: Vec<PathBuf> = profile
        .repos()
        .unwrap()
        .into_iter()
        .map(|e| e.canonical_path)
        .collect();
    assert_eq!(observed, [canonical(&common_dir(&shop))]);
    assert_eq!(
        profile
            .discovery_candidates()
            .unwrap()
            .iter()
            .map(|c| c.path.clone())
            .collect::<Vec<_>>(),
        [text(&code.join("api"))],
        "candidates survive a restart"
    );
    let after = fingerprint(&code);
    let changed: Vec<_> = after
        .iter()
        .filter(|f| !before.contains(f))
        .filter(|(p, _, _)| !p.starts_with(code.join("billing")))
        .collect();
    assert!(changed.is_empty(), "discovery wrote {changed:?}");
}

/// The home folder as a broad root: hidden folders and the macOS exclusions
/// are never proposed.
#[test]
fn discovery_the_home_root_skips_hidden_and_excluded_folders() {
    let tp = TempProfile::new();
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(home.join("Library")).unwrap();
    init_repo(&home, "dotlab", false);
    init_repo(&home, ".oh-my-zsh", false);
    init_repo(&home.join("Library"), "x", false);
    init_repo(&home, "Documents", false);
    tp.open().add_discovery_root(&text(&home), true, 1).unwrap();

    let r = Running::start(tp.dirs(), ChannelConfig::default(), fast(&home));
    let mut client = r.client(ClientKind::Cli);
    wait_for(&mut client, &["dotlab"]);
}

/// Dismissing is for good, by path: it survives a restart, and adding the
/// repo by hand observes it and forgets the dismissal. Removing a root
/// takes its pending candidates and leaves observed repos and dismissals.
#[test]
fn discovery_dismissals_last_and_removing_a_root_keeps_what_is_observed() {
    let tp = TempProfile::new();
    let tmp = tempfile::tempdir().unwrap();
    let code = tmp.path().join("code");
    let legacy = init_repo(&code, "legacy-tools", false);
    let billing = init_repo(&code, "billing", false);
    let shop = init_repo(&code, "shop", false);
    let root = text(&code);
    let pair = |repo: &Path| {
        (
            text(repo),
            gitraptor_core::profile::normalize_common_dir(&common_dir(repo))
                .unwrap()
                .key_path,
        )
    };
    let found = [pair(&billing), pair(&legacy)];
    {
        let mut profile = tp.open();
        profile.add_discovery_root(&root, false, 1).unwrap();
        let new = profile.sync_discovery_candidates(&root, &found, 2).unwrap();
        assert_eq!(new.len(), 2);
        assert!(
            profile
                .dismiss_discovery_candidate(&text(&legacy), 3)
                .unwrap()
        );
        assert!(
            !profile
                .dismiss_discovery_candidate(&text(&shop), 3)
                .unwrap()
        );
    }
    {
        // After a restart: a new listing does not propose it again.
        let mut profile = tp.open();
        let new = profile.sync_discovery_candidates(&root, &found, 4).unwrap();
        assert!(new.is_empty());
        let paths: Vec<String> = profile
            .discovery_candidates()
            .unwrap()
            .into_iter()
            .map(|c| c.path)
            .collect();
        assert_eq!(paths, [text(&billing)]);
        // Adding it by hand observes it and forgets the dismissal.
        profile.add_repo(&common_dir(&legacy), None, 5).unwrap();
        profile.forget_discovered_key(&pair(&legacy).1).unwrap();
        // An observed repo is never proposed.
        let new = profile
            .sync_discovery_candidates(&root, &[pair(&legacy), pair(&billing)], 6)
            .unwrap();
        assert!(new.is_empty());
        // Removing the root takes billing and leaves legacy observed.
        assert_eq!(profile.remove_discovery_root(&root).unwrap(), Some(1));
        assert!(profile.discovery_candidates().unwrap().is_empty());
        assert_eq!(profile.repos().unwrap().len(), 1);
        assert_eq!(profile.remove_discovery_root(&root).unwrap(), None);
        // A listing of a removed root changes nothing.
        assert!(
            profile
                .sync_discovery_candidates(&root, &found, 7)
                .unwrap()
                .is_empty()
        );
    }
}

/// An agent can neither declare nor remove a root nor dismiss a candidate;
/// each attempt is audited and the roots do not change.
#[test]
fn discovery_an_agent_cannot_declare_roots_nor_dismiss() {
    let tp = TempProfile::new();
    let tmp = tempfile::tempdir().unwrap();
    let code = tmp.path().join("code");
    init_repo(&code, "billing", false);
    let r = Running::start(
        tp.dirs(),
        ChannelConfig {
            agents: self_as_agent(),
            ..ChannelConfig::default()
        },
        fast(tmp.path()),
    );
    let mut client = r.client(ClientKind::Cli);
    let path = code.to_str().unwrap();
    let attempts = [
        (methods::DISCOVERY_ROOT_ADD, json!({"path": path})),
        (
            methods::DISCOVERY_ROOT_ADD,
            json!({"path": path, "confirm_broad": true}),
        ),
        (methods::DISCOVERY_ROOT_REMOVE, json!({"path": path})),
        (
            methods::DISCOVERY_DISMISS,
            json!({"path": code.join("billing").to_str().unwrap()}),
        ),
    ];
    for (method, params) in &attempts {
        let err = client
            .call::<_, serde_json::Value>(method, params)
            .unwrap_err();
        assert_eq!(refusal(err), RefusalReason::AgentAncestry, "{method}");
    }
    let audit: AuditListResult = client.call(methods::AUDIT_LIST, json!({})).unwrap();
    assert_eq!(audit.entries.len(), attempts.len());
    for (entry, (method, _)) in audit.entries.iter().zip(&attempts) {
        assert_eq!(entry.operation, *method);
        assert_eq!(entry.outcome, AuditOutcome::Rejected);
    }
    let roots: RootsResult = client.call(methods::DISCOVERY_ROOTS, json!({})).unwrap();
    assert!(roots.roots.is_empty());
    assert!(candidates(&mut client).is_empty());
}

/// `raptor-mcp` sees nothing of discovery (SEC-MCP-01): no method in its
/// handshake, the reads do not exist and the reserved ones are refused.
#[test]
fn discovery_nothing_reaches_the_mcp() {
    let tp = TempProfile::new();
    let tmp = tempfile::tempdir().unwrap();
    let r = Running::start(tp.dirs(), ChannelConfig::default(), fast(tmp.path()));
    let mut mcp = r.client(ClientKind::Mcp);
    assert!(
        !mcp.hello()
            .methods
            .iter()
            .any(|m| m.starts_with("discovery.")),
        "{:?}",
        mcp.hello().methods
    );
    for method in [methods::DISCOVERY_ROOTS, methods::DISCOVERY_CANDIDATES] {
        let err = mcp
            .call::<_, serde_json::Value>(method, json!({}))
            .unwrap_err();
        match err {
            ClientError::Rpc(e) => assert_eq!(e.code, code::METHOD_NOT_FOUND, "{method}"),
            other => panic!("{other}"),
        }
    }
    let err = mcp
        .call::<_, serde_json::Value>(
            methods::DISCOVERY_ROOT_ADD,
            json!({"path": tmp.path().to_str().unwrap()}),
        )
        .unwrap_err();
    assert_eq!(refusal(err), RefusalReason::NotAvailableToMcp);
    let full = r.client(ClientKind::Cli);
    assert!(
        full.hello()
            .methods
            .iter()
            .any(|m| m == methods::DISCOVERY_ROOT_ADD)
    );
}
