//! Measurements of the refs barrier of the engine's calm (fix/tm-restore-calm-mark). Both are
//! `#[ignore]`: they measure, print the numbers and gate only what the Brief fixes.
//!
//! - `settle_holds_under_another_worktrees_ref_churn`: the barrier is per repo, so another
//!   worktree's commits delay a worktree's calm. A real daemon (in-process) over a testkit fixture
//!   and a temporary profile; worktree B commits every X ms (pacing, not waiting) while A asks for
//!   undos, which settle first. Prints the smallest X with no `repo-busy`.
//! - `settle_is_cheap_when_idle`: p95 of `settle` at rest, with a few branches and with ~5000.
//!   The 5 ms gate holds in release only.
//!
//! Never this repo or the real profile (NFR-01). Run with
//! `cargo test -p gitraptor-core --release --test tm_calm_refs_load -- --ignored --nocapture`.
#![cfg(any(target_os = "macos", target_os = "linux"))]

mod common;

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::TempProfile;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport, TierConfig,
    TmCapture,
};
use gitraptor_core::timemachine::continuous::CaptureConfig;
use gitraptor_core::timemachine::engine::{RepoMarks, git_dir_of};
use gitraptor_testkit::Fixture;
use serde_json::json;

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn canonical(p: &Path) -> PathBuf {
    p.canonicalize().unwrap()
}

fn branches(f: &Fixture) -> BTreeMap<String, String> {
    f.git(&[
        "for-each-ref",
        "--format=%(refname:short) %(objectname)",
        "refs/heads",
    ])
    .lines()
    .filter_map(|l| l.split_once(' '))
    .map(|(n, c)| (n.to_owned(), c.to_owned()))
    .collect()
}

struct Running {
    tp: TempProfile,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

fn connect(tp: &TempProfile) -> Client {
    let start = Instant::now();
    loop {
        match Client::connect(&tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION) {
            Ok(c) => return c,
            Err(_) if start.elapsed() < Duration::from_secs(5) => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("{e}"),
        }
    }
}

/// A daemon observing `fx`'s repo, always active, with the Time Machine on.
fn start(fx: &Fixture) -> Running {
    let tp = TempProfile::new();
    let mut profile = tp.open();
    profile
        .add_repo(&canonical(&fx.repo.join(".git")), None, 1)
        .unwrap();
    drop(profile);
    let env = DaemonEnv::from_vars(std::env::vars_os());
    let config = DaemonConfig {
        dirs: tp.dirs(),
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: ChannelConfig::default(),
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tiers: TierConfig::default(),
        discovery: Default::default(),
        tm_capture: TmCapture {
            config: CaptureConfig {
                enabled: true,
                quiet: Duration::from_millis(150),
                max_interval: Duration::from_millis(600),
            },
            layer: None,
            no_free_space_floor: true,
        },
    };
    let daemon = Daemon::start(config).unwrap();
    let handle = daemon.shutdown_handle();
    let join = std::thread::spawn(move || daemon.run());
    let r = Running {
        tp,
        handle,
        join: Some(join),
    };
    drop(connect(&r.tp));
    r
}

/// Whether an undo of `worktree` was refused as `repo-busy` (its calm did not come in time).
fn undo_busy(tp: &TempProfile, worktree: &Path) -> bool {
    match connect(tp).call::<_, serde_json::Value>(
        methods::TM_UNDO,
        json!({ "worktree": worktree.to_str().unwrap(), "surface": "cli" }),
    ) {
        Ok(_) => false,
        Err(ClientError::Rpc(e)) => format!("{:?}", e.data).contains("repo-busy"),
        Err(e) => panic!("{e:?}"),
    }
}

#[test]
#[ignore = "measurement: run with --ignored --nocapture"]
fn settle_holds_under_another_worktrees_ref_churn() {
    const UNDOS: usize = 3;
    let fx = Arc::new(Fixture::with_commit(&git()));
    fx.git(&["branch", "b"]);
    let a = canonical(&fx.repo);
    let b = canonical(&fx.add_worktree("b", "b"));
    let r = start(&fx);
    let mut min_ok = None;
    for every_ms in [500u64, 200, 100, 50, 20, 10] {
        let stop = Arc::new(AtomicBool::new(false));
        let churn = {
            let (fx, b, stop) = (Arc::clone(&fx), b.clone(), Arc::clone(&stop));
            std::thread::spawn(move || {
                while !stop.load(Ordering::Acquire) {
                    fx.git_in(&b, &["commit", "-q", "--allow-empty", "-m", "churn"]);
                    // Pacing of the churn, not a wait for a state.
                    std::thread::sleep(Duration::from_millis(every_ms));
                }
            })
        };
        let busy = (0..UNDOS).filter(|_| undo_busy(&r.tp, &a)).count();
        stop.store(true, Ordering::Release);
        churn.join().unwrap();
        println!("churn every {every_ms} ms: {busy}/{UNDOS} undos repo-busy");
        if every_ms == 500 {
            assert_eq!(busy, 0, "A settles under B's churn every 500 ms");
        }
        if busy == 0 {
            min_ok = Some(every_ms);
        }
    }
    println!("X_min (no repo-busy): {min_ok:?} ms");
}

/// p95 of `settle(ZERO)` over a calm repo, in microseconds.
fn p95_us(marks: &RepoMarks, wt: &Path) -> u128 {
    let mut samples: Vec<u128> = (0..200)
        .map(|_| {
            let t = Instant::now();
            assert!(
                marks
                    .settle("r", std::slice::from_ref(&wt.to_path_buf()), Duration::ZERO)
                    .is_some(),
                "the idle repo is calm"
            );
            t.elapsed().as_micros()
        })
        .collect();
    samples.sort_unstable();
    samples[samples.len() * 95 / 100]
}

fn publish(marks: &RepoMarks, f: &Fixture) {
    let common = gitraptor_core::observe::locate(&f.repo).unwrap();
    marks.set_persisted("r", 1, &common, branches(f));
    let git_dir = git_dir_of(&f.repo).unwrap();
    let len = std::fs::metadata(git_dir.join("logs/HEAD")).map_or(0, |m| m.len());
    marks.set_head_logs("r", &[(f.repo.clone(), len)]);
    marks.set_heads(
        "r",
        &[(f.repo.clone(), std::fs::read(git_dir.join("HEAD")).unwrap())],
    );
}

#[test]
#[ignore = "measurement: run with --release --ignored --nocapture"]
fn settle_is_cheap_when_idle() {
    let f = Fixture::with_commit(&git());
    let marks = RepoMarks::default();
    publish(&marks, &f);
    let few = p95_us(&marks, &f.repo);
    println!(
        "settle p95 at rest, {} branches: {few} us",
        branches(&f).len()
    );
    if !cfg!(debug_assertions) {
        assert!(few < 5_000, "p95 {few} us is over 5 ms");
    }
    // About 5000 branches, in one `update-ref --stdin`.
    let head = f.git(&["rev-parse", "HEAD"]).trim().to_owned();
    let mut child = f
        .git_command(&f.repo, &["update-ref", "--stdin"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for i in 0..5000 {
        writeln!(stdin, "create refs/heads/b{i:04} {head}").unwrap();
    }
    drop(stdin);
    assert!(child.wait().unwrap().success());
    publish(&marks, &f);
    let many = p95_us(&marks, &f.repo);
    println!(
        "settle p95 at rest, {} branches: {many} us",
        branches(&f).len()
    );
}
