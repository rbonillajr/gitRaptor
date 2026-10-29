//! TS-CKP-001 T010 (CKPB-INTACT): the security gate of the conflict predictor. A real daemon
//! (in a thread, temporary profile) with the worker process of `raptor` observes repos built
//! in temporary roots (NFR-01); the pairs are read through `prediction.list`.
//!
//! - NFR-01 / M-02: 10 worktrees and 55 pairs, recalculated by commit and by base move, and a
//!   concurrent user `git gc`, leave the objects of the repo as the user's own writes left them:
//!   no unreachable object, no change of any pack, loose object or `objects/` directory other
//!   than the user's.
//! - SEC-09 / L-04: a canary with a merge driver, `merge=<x>` in every place Git reads attributes
//!   from, `refs/replace/*` and `info/grafts` never fires.
//! - The only child process of the daemon is the worker, and the worker has none.
//! - M-02: a partial clone with a local promisor remote gives `missing-object` and never connects.
//! - M-06: three hostile repos give `not-computable` with the daemon still answering.
#![cfg(unix)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::RwLock;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use common::TempProfile;
use gitraptor_api::messages::ClientKind;
use gitraptor_api::prediction::{
    ListPredictionParams, PairDetail, PairLevel, PairReason, PairSideView, PairStateView,
    PredictionPage,
};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::channel::ChannelConfig;
use gitraptor_core::client::Client;
use gitraptor_core::daemon::{
    Daemon, DaemonConfig, DaemonEnv, LogLimits, ShutdownHandle, StopCause, StopReport,
};
use gitraptor_core::predict::worker::{WorkerConfig, WorkerLimits};
use gitraptor_core::predict::{PredictConfig, PredictMerger};
use gitraptor_git::RefName;
use gitraptor_policy::team::{Confirmed, ConfirmedFloor};
use gitraptor_testkit::canary::{self, Canary};
use gitraptor_testkit::fingerprint::{self, Snapshot};
use gitraptor_testkit::fixture::git_from_path;
use gitraptor_testkit::{Fixture, sibling_bin};
use serde_json::json;

/// How long a pair may take to show what the test waits for.
const WAIT: Duration = Duration::from_secs(20);

/// The tests that read the process table of this test binary (children of the daemon) run
/// alone; the others run together.
static CENSUS: RwLock<()> = RwLock::new(());

// ----- The daemon --------------------------------------------------------------------------

/// `raptor`, the worker's program: built next to this test binary if it is not there.
fn raptor() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let profile_dir = exe
        .parent()
        .and_then(Path::parent)
        .expect("target/<profile>");
    sibling_bin(&profile_dir.join("raptor"), "gitraptor-cli", "raptor")
}

struct Rig {
    tp: TempProfile,
    handle: ShutdownHandle,
    join: Option<JoinHandle<StopReport>>,
    repo_id: String,
}

impl Rig {
    /// A daemon observing `repo` (a worktree root), with the worker of `raptor` under `limits`.
    fn start(repo: &Path, limits: WorkerLimits) -> Self {
        let tp = TempProfile::new();
        let repo_id = {
            let mut profile = tp.open();
            let common_dir = repo.join(".git").canonicalize().unwrap();
            let (entry, _) = profile.add_repo(&common_dir, None, 1).unwrap();
            // `main` confirmed as the base: an unconfirmed base leaves the pairs against it
            // pending(base-unconfirmed) (DS-TS-CKP-001 § Forma del error).
            let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
            store
                .set_confirmed_team_baseline(&Confirmed {
                    base_branch: RefName::new("main").unwrap(),
                    floor: ConfirmedFloor::Absent,
                })
                .unwrap();
            entry.repo_id
        };
        // The real Git, as the daemon resolves it: without it the base is never reconciled and
        // stays unconfirmed.
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
            tiers: Default::default(),
            discovery: Default::default(),
            power: Default::default(),
            predict: PredictConfig {
                merger: PredictMerger::Worker(WorkerConfig {
                    program: raptor(),
                    limits,
                }),
            },
            tm_capture: Default::default(),
        };
        let daemon = Daemon::start(config).unwrap();
        let handle = daemon.shutdown_handle();
        let join = std::thread::spawn(move || daemon.run());
        Self {
            tp,
            handle,
            join: Some(join),
            repo_id,
        }
    }

    fn client(&self) -> Result<Client, String> {
        let start = Instant::now();
        loop {
            match Client::connect(&self.tp.dirs(), ClientKind::Cli, PROTOCOL_VERSION) {
                Ok(c) => return Ok(c),
                Err(_) if start.elapsed() < Duration::from_secs(5) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => return Err(format!("connect: {e}")),
            }
        }
    }

    /// Every pair of the repo, as `prediction.list` pages them.
    fn pairs(&self) -> Result<Vec<PairDetail>, String> {
        let mut client = self.client()?;
        let mut out = Vec::new();
        let mut cursor = None;
        loop {
            let page: PredictionPage = client
                .call(
                    methods::PREDICTION_LIST,
                    ListPredictionParams {
                        repo_id: self.repo_id.clone(),
                        cursor: cursor.take(),
                        ..Default::default()
                    },
                )
                .map_err(|e| format!("prediction.list: {e}"))?;
            out.extend(page.pairs);
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => return Ok(out),
            }
        }
    }

    /// The pairs once `ready` holds, or a panic that names `what` is missing.
    fn wait(&self, what: &str, ready: impl Fn(&[PairDetail]) -> bool) -> Vec<PairDetail> {
        let start = Instant::now();
        loop {
            let seen = self.pairs();
            if let Ok(pairs) = &seen
                && ready(pairs)
            {
                return seen.unwrap();
            }
            if start.elapsed() > WAIT {
                panic!(
                    "the predictor never published {what} within {WAIT:?} (T006, T009): {}",
                    match &seen {
                        Ok(pairs) => describe(pairs),
                        Err(e) => e.clone(),
                    }
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The daemon still answers.
    fn assert_answers(&self, after: &str) {
        let pong: String = self
            .client()
            .unwrap()
            .call(methods::PING, json!({}))
            .unwrap_or_else(|e| panic!("the daemon does not answer after {after}: {e}"));
        assert_eq!(pong, "pong");
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        if let Some(join) = self.join.take() {
            self.handle.request(StopCause::Signal("TERM"));
            let _ = join.join();
        }
    }
}

fn describe(pairs: &[PairDetail]) -> String {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for p in pairs {
        let key = format!("{:?}/{:?}", p.summary.state, p.summary.reason);
        *counts.entry(key).or_default() += 1;
    }
    format!("{} pairs {counts:?}", pairs.len())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

// ----- Reading pairs -----------------------------------------------------------------------

fn root_of(side: &PairSideView) -> Option<&str> {
    match side {
        PairSideView::Worktree { root } => Some(root.raw()),
        PairSideView::Base {} => None,
    }
}

fn involves(p: &PairDetail, root: &Path) -> bool {
    let root = root.to_str().unwrap();
    root_of(&p.summary.a) == Some(root) || root_of(&p.summary.b) == Some(root)
}

fn is_base_pair(p: &PairDetail) -> bool {
    root_of(&p.summary.b).is_none()
}

fn between<'a>(pairs: &'a [PairDetail], x: &Path, y: &Path) -> Option<&'a PairDetail> {
    pairs
        .iter()
        .find(|p| involves(p, x) && involves(p, y) && !is_base_pair(p))
}

fn all_current(pairs: &[PairDetail], n: usize) -> bool {
    pairs.len() == n
        && pairs
            .iter()
            .all(|p| p.summary.state == PairStateView::Current)
}

/// Every pair of `selected` is `current` and was computed at or after `since_ms`.
fn recomputed(pairs: &[PairDetail], since_ms: i64, selected: impl Fn(&PairDetail) -> bool) -> bool {
    let chosen: Vec<_> = pairs.iter().filter(|p| selected(p)).collect();
    !chosen.is_empty()
        && chosen.iter().all(|p| {
            p.summary.state == PairStateView::Current
                && p.summary.computed_at_utc_ms.is_some_and(|t| t >= since_ms)
        })
}

// ----- Building repos ----------------------------------------------------------------------

fn fixture() -> Fixture {
    Fixture::with_commit(&git_from_path())
}

/// A repo with 10 worktrees (the main one on `w0`, nine linked) and `main` checked out
/// nowhere: 10 pairs against the base and 45 between worktrees, 55 in all. `w1` and `w2` both
/// change the same line of `shared.dat`.
fn ten_worktrees(f: &Fixture) -> Vec<PathBuf> {
    f.write("shared.dat", "one\ntwo\nthree\n");
    f.git(&["add", "shared.dat"]);
    f.git(&["commit", "-q", "-m", "shared"]);
    f.git(&["repack", "-a", "-d", "-q"]);
    f.git(&["switch", "-q", "-c", "w0"]);
    let mut roots = vec![f.repo.clone()];
    for i in 1..10 {
        let root = f.root.join(format!("wt-w{i}"));
        f.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            &format!("w{i}"),
            root.to_str().unwrap(),
            "main",
        ]);
        roots.push(root);
    }
    for (i, root) in roots.iter().enumerate() {
        let (name, body) = match i {
            1 => ("shared.dat".to_owned(), "ONE\ntwo\nthree\n".to_owned()),
            2 => ("shared.dat".to_owned(), "uno\ntwo\nthree\n".to_owned()),
            _ => (format!("own-{i}.dat"), format!("work of {i}\n")),
        };
        std::fs::write(root.join(&name), body).unwrap();
        f.git_in(root, &["add", &name]);
        f.git_in(root, &["commit", "-q", "-m", &format!("w{i}")]);
    }
    roots
}

/// A user commit in `root` with content nobody had.
fn user_commit(f: &Fixture, root: &Path, n: u32) {
    let name = format!("user-{n}.dat");
    std::fs::write(
        root.join(&name),
        format!("the user's change {n} {}\n", now_ms()),
    )
    .unwrap();
    f.git_in(root, &["add", &name]);
    f.git_in(root, &["commit", "-q", "-m", &format!("user {n}")]);
}

/// The base branch moves: a new commit on `main`, made as the user would, in no worktree.
fn move_base(f: &Fixture, n: u32) {
    let tree = f.git(&["rev-parse", "main^{tree}"]);
    let commit = f.git(&[
        "commit-tree",
        tree.trim(),
        "-p",
        "main",
        "-m",
        &format!("base moves {n} {}", now_ms()),
    ]);
    f.git(&["update-ref", "refs/heads/main", commit.trim()]);
}

/// Streams `data` into `git fast-import` in `dir`.
fn fast_import(f: &Fixture, dir: &Path, data: &[u8]) {
    let mut child = f
        .git_command(dir, &["fast-import", "--quiet"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(data).unwrap();
    assert!(child.wait().unwrap().success(), "git fast-import failed");
}

/// A fast-import commit of `branch` over `main` that adds (or rewrites) `files`.
fn import_branch(branch: &str, files: &[(String, Vec<u8>)]) -> Vec<u8> {
    let msg = format!("import {branch}");
    let mut out = format!(
        "commit refs/heads/{branch}\ncommitter T <t@example.com> 1790000000 +0000\ndata {}\n{msg}\nfrom refs/heads/main\n",
        msg.len()
    )
    .into_bytes();
    for (path, body) in files {
        out.extend(format!("M 100644 inline {path}\ndata {}\n", body.len()).into_bytes());
        out.extend(body);
        out.push(b'\n');
    }
    out
}

/// Two linked worktrees `wx` and `wy` on the branches of the same names; the main worktree
/// keeps `main`.
fn two_worktrees(f: &Fixture) -> (PathBuf, PathBuf) {
    let mut roots = Vec::new();
    for b in ["wx", "wy"] {
        let root = f.root.join(format!("wt-{b}"));
        f.git(&["worktree", "add", "-q", root.to_str().unwrap(), b]);
        roots.push(root);
    }
    (roots.remove(0), roots.remove(0))
}

// ----- The footprint -----------------------------------------------------------------------

fn objects_dir(f: &Fixture) -> PathBuf {
    f.repo.join(".git/objects")
}

fn object_ids(f: &Fixture) -> BTreeSet<String> {
    f.git(&[
        "cat-file",
        "--batch-all-objects",
        "--batch-check=%(objectname)",
    ])
    .lines()
    .map(str::to_owned)
    .collect()
}

fn reachable_ids(f: &Fixture) -> BTreeSet<String> {
    f.git(&["rev-list", "--objects", "--all"])
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .map(str::to_owned)
        .collect()
}

/// What the user's own writes left in `objects/` since `before`; the predictor left nothing:
/// every new object is reachable, and the only paths that changed (mtime included) are those
/// of the new objects, their `xx/` directory and `objects/` itself. A pack, a loose object or
/// a directory of anything else that changed is the predictor's.
struct ObjectsFootprint {
    snapshot: Snapshot,
    ids: BTreeSet<String>,
}

impl ObjectsFootprint {
    fn take(f: &Fixture) -> Self {
        fingerprint::wait_for_timestamp_tick();
        Self {
            snapshot: Snapshot::of_dir(&objects_dir(f)),
            ids: object_ids(f),
        }
    }

    fn assert_untouched_since(&self, f: &Fixture, what: &str) {
        let now_ids = object_ids(f);
        let reachable = reachable_ids(f);
        let new: BTreeSet<String> = now_ids.difference(&self.ids).cloned().collect();
        let unreachable: Vec<_> = new.iter().filter(|id| !reachable.contains(*id)).collect();
        assert!(
            unreachable.is_empty(),
            "{what}: the predictor wrote objects the user never made: {unreachable:?}"
        );
        let removed: Vec<_> = self.ids.difference(&now_ids).collect();
        assert!(
            removed.is_empty(),
            "{what}: objects disappeared: {removed:?}"
        );
        let shards: BTreeSet<String> = new.iter().map(|id| id[..2].to_owned()).collect();
        let loose: BTreeSet<String> = new
            .iter()
            .map(|id| format!("{}/{}", &id[..2], &id[2..]))
            .collect();
        let after = Snapshot::of_dir(&objects_dir(f));
        let foreign: Vec<String> = fingerprint::diff(&self.snapshot, &after)
            .into_iter()
            .filter(|c| {
                let path = c.path.to_string_lossy();
                let own = path.is_empty()
                    || shards.contains(path.as_ref())
                    || loose.contains(path.as_ref());
                !own
            })
            .map(|c| c.to_string())
            .collect();
        assert!(
            foreign.is_empty(),
            "{what}: the footprint of objects/ changed beyond the user's own writes: {foreign:?}"
        );
    }
}

/// Nothing in `.git` changes while the predictor is idle, and it leaves no lock or temporary.
fn assert_quiet(f: &Fixture, what: &str) {
    let git_dir = f.repo.join(".git");
    fingerprint::wait_for_timestamp_tick();
    let before = Snapshot::of_dir(&git_dir);
    std::thread::sleep(Duration::from_millis(1200));
    let changes: Vec<String> = fingerprint::diff(&before, &Snapshot::of_dir(&git_dir))
        .iter()
        .map(|c| c.to_string())
        .collect();
    assert!(
        changes.is_empty(),
        "{what}: .git changes by itself: {changes:?}"
    );
    let leftovers: Vec<String> = before
        .entries
        .keys()
        .map(|(_, p)| p.to_string_lossy().into_owned())
        .filter(|p| {
            let name = p.rsplit('/').next().unwrap_or(p);
            name.ends_with(".lock") || name.starts_with("tmp_") || name.contains("raptor")
        })
        .collect();
    assert!(
        leftovers.is_empty(),
        "{what}: lock or temporary files left: {leftovers:?}"
    );
}

// ----- The process table -------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Proc {
    pid: u32,
    ppid: u32,
    name: String,
}

fn process_table() -> Vec<Proc> {
    let out = std::process::Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,comm="])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let pid = it.next()?.parse().ok()?;
            let ppid = it.next()?.parse().ok()?;
            let comm = it.collect::<Vec<_>>().join(" ");
            let name = comm.rsplit('/').next()?.to_owned();
            Some(Proc { pid, ppid, name })
        })
        .collect()
}

/// The children of this process (but `ps` itself) and their own children.
fn census() -> (Vec<Proc>, Vec<Proc>) {
    let me = std::process::id();
    let table = process_table();
    let children: Vec<Proc> = table
        .iter()
        .filter(|p| p.ppid == me && p.name != "ps")
        .cloned()
        .collect();
    let grandchildren = table
        .iter()
        .filter(|p| p.name != "ps" && children.iter().any(|c| c.pid == p.ppid))
        .cloned()
        .collect();
    (children, grandchildren)
}

/// Samples the process table until `stop`; what it saw of the worker and of anything else.
struct Sampler {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pause: std::sync::Arc<std::sync::atomic::AtomicBool>,
    join: JoinHandle<(Vec<Proc>, Vec<Proc>)>,
}

impl Sampler {
    fn start() -> Self {
        use std::sync::atomic::{AtomicBool, Ordering};
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let pause = std::sync::Arc::new(AtomicBool::new(false));
        let paused = pause.clone();
        let join = std::thread::spawn(move || {
            let (mut kids, mut grand) = (Vec::new(), Vec::new());
            while !flag.load(Ordering::Relaxed) {
                if paused.load(Ordering::SeqCst) {
                    std::thread::sleep(Duration::from_millis(2));
                    continue;
                }
                let (c, g) = census();
                // A sample that overlaps one of the test's own `git` is not the daemon's.
                if paused.load(Ordering::SeqCst) {
                    continue;
                }
                for p in c {
                    if !kids.iter().any(|k: &Proc| k.pid == p.pid) {
                        kids.push(p);
                    }
                }
                for p in g {
                    if !grand.iter().any(|k: &Proc| k.pid == p.pid) {
                        grand.push(p);
                    }
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            (kids, grand)
        });
        Self { stop, pause, join }
    }

    /// Runs `own` (the test's own processes) out of the samples.
    fn without_sampling<T>(&self, own: impl FnOnce() -> T) -> T {
        use std::sync::atomic::Ordering;
        self.pause.store(true, Ordering::SeqCst);
        let out = own();
        self.pause.store(false, Ordering::SeqCst);
        out
    }

    fn finish(self) -> (Vec<Proc>, Vec<Proc>) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        self.join.join().unwrap()
    }
}

// ----- The tests ---------------------------------------------------------------------------

mod repo_intact {
    use super::*;

    /// T010 (NFR-01): 10 worktrees, 55 pairs; a recalculation by commit and another by base
    /// move leave `objects/` exactly as the user's own writes left it.
    #[test]
    fn ckpb_intact_fifty_five_pairs_recalculated_by_commit_and_base_move_leave_objects_untouched() {
        let _shared = CENSUS.read().unwrap_or_else(|e| e.into_inner());
        let f = fixture();
        let roots = ten_worktrees(&f);
        let rig = Rig::start(&f.repo, WorkerLimits::default());
        let pairs = rig.wait("the 55 pairs of 10 worktrees as current", |p| {
            all_current(p, 55)
        });
        assert_eq!(pairs.iter().filter(|p| is_base_pair(p)).count(), 10);
        assert_eq!(
            between(&pairs, &roots[1], &roots[2]).and_then(|p| p.summary.level),
            Some(PairLevel::Conflict),
            "w1 and w2 change the same line: the predictor must see a conflict"
        );
        assert_quiet(&f, "after the first calculation");

        // Recalculation by commit.
        let before = ObjectsFootprint::take(&f);
        let since = now_ms();
        user_commit(&f, &roots[3], 1);
        rig.wait("the 10 pairs of the committed worktree recomputed", |p| {
            recomputed(p, since, |p| involves(p, &roots[3]))
        });
        before.assert_untouched_since(&f, "recalculation by commit");

        // Recalculation by base move.
        let before = ObjectsFootprint::take(&f);
        let since = now_ms();
        move_base(&f, 1);
        let pairs = rig.wait("the 10 pairs against the moved base recomputed", |p| {
            recomputed(p, since, is_base_pair)
        });
        assert_eq!(pairs.len(), 55);
        before.assert_untouched_since(&f, "recalculation by base move");
        assert_quiet(&f, "after the recalculations");
        rig.assert_answers("the recalculations");
    }

    /// T010 (NFR-01, M-02): a `git gc` of the user runs while the predictor recalculates; the
    /// repo stays whole, the pairs settle, and the predictor leaves no footprint in `objects/`.
    #[test]
    fn ckpb_intact_concurrent_user_gc_leaves_repo_whole_and_pairs_settled() {
        let _shared = CENSUS.read().unwrap_or_else(|e| e.into_inner());
        let f = fixture();
        let roots = ten_worktrees(&f);
        let rig = Rig::start(&f.repo, WorkerLimits::default());
        rig.wait("the 55 pairs as current", |p| all_current(p, 55));

        let since = now_ms();
        let mut gc = f
            .git_command(&f.repo, &["gc", "--prune=now", "-q"])
            .spawn()
            .unwrap();
        for (n, root) in roots.iter().enumerate().take(4) {
            user_commit(&f, root, 100 + n as u32);
        }
        move_base(&f, 2);
        assert!(gc.wait().unwrap().success(), "the user's gc failed");
        rig.wait(
            "all 55 pairs current again after commits, a base move and gc",
            |p| all_current(p, 55) && recomputed(p, since, is_base_pair),
        );
        f.git(&["fsck", "--no-dangling", "--strict"]);

        // After the gc, the predictor still writes nothing: packs, loose objects, directories.
        let before = ObjectsFootprint::take(&f);
        let since = now_ms();
        user_commit(&f, &roots[5], 200);
        move_base(&f, 3);
        rig.wait("the pairs recomputed after the gc", |p| {
            all_current(p, 55)
                && recomputed(p, since, |p| involves(p, &roots[5]) || is_base_pair(p))
        });
        before.assert_untouched_since(&f, "recalculation after the gc");
        assert_quiet(&f, "after the gc");
    }

    /// T010 (SEC-09, L-04): a merge driver, `merge=<x>` in the tree, the index, the working tree,
    /// `info/attributes` and `core.attributesFile`, `refs/replace/*` and `info/grafts`: no
    /// program of the canary ever runs, and every pair still settles.
    #[test]
    fn ckpb_intact_extended_canary_never_fires_while_every_pair_settles() {
        let _shared = CENSUS.read().unwrap_or_else(|e| e.into_inner());
        let f = Fixture::busy(&git_from_path());
        f.write("shared.dat", "base\n");
        f.git(&["add", "shared.dat"]);
        f.git(&["commit", "-q", "-m", "shared", "--", "shared.dat"]);
        let mut roots = vec![f.repo.clone()];
        for (i, side) in ["alpha", "beta"].iter().enumerate() {
            let root = f.root.join(format!("wt-c{i}"));
            f.git(&[
                "worktree",
                "add",
                "-q",
                "-b",
                side,
                root.to_str().unwrap(),
                "main",
            ]);
            std::fs::write(root.join("shared.dat"), format!("{side}\n")).unwrap();
            std::fs::write(root.join("x.cfg"), format!("{side}\n")).unwrap();
            f.git_in(&root, &["add", "shared.dat", "x.cfg"]);
            f.git_in(&root, &["commit", "-q", "-m", side]);
            roots.push(root);
        }
        f.write("shared.dat", "main\n");
        f.git(&["commit", "-q", "-m", "main side", "--", "shared.dat"]);
        let canary = Canary::arm_extended(f);
        assert!(
            canary.fired().is_empty(),
            "arming fired: {:?}",
            canary.fired()
        );
        for program in canary::PROGRAMS.iter().chain(canary::MERGE_PROGRAMS) {
            assert!(
                !canary.marker(program).exists(),
                "{program} fired while arming"
            );
        }

        let rig = Rig::start(&canary.f.repo, WorkerLimits::default());
        // `alpha` and `beta` against each other and against the base. The main worktree has the
        // base checked out and no work of its own, so it forms no pairs (T002).
        let pairs = rig.wait("the 3 pairs of the canary repo as current", |p| {
            all_current(p, 3)
        });
        assert_eq!(
            between(&pairs, &roots[1], &roots[2]).and_then(|p| p.summary.level),
            Some(PairLevel::Conflict)
        );
        assert!(
            canary.fired().is_empty(),
            "the predictor ran a program of the repo: {:?}",
            canary.fired()
        );
    }

    /// T010 (SEC-09): the only child process of the daemon is the worker, by the `raptor`
    /// binary, and the worker launches none, however many recalculations run.
    #[test]
    fn ckpb_intact_only_child_process_is_the_worker_and_it_launches_none() {
        let _alone = CENSUS.write().unwrap_or_else(|e| e.into_inner());
        let f = fixture();
        let roots = ten_worktrees(&f);
        // Brought up to date before sampling: its `cargo build` is the test's, not the daemon's.
        raptor();
        let sampler = Sampler::start();
        let rig = Rig::start(&f.repo, WorkerLimits::default());
        rig.wait("the 55 pairs as current", |p| all_current(p, 55));
        let since = now_ms();
        for (n, root) in roots.iter().enumerate().take(3) {
            sampler.without_sampling(|| user_commit(&f, root, 300 + n as u32));
        }
        sampler.without_sampling(|| move_base(&f, 4));
        rig.wait("the recalculation by commits and base move", |p| {
            recomputed(p, since, |p| {
                roots[..3].iter().any(|r| involves(p, r)) || is_base_pair(p)
            })
        });
        let (kids, grandchildren) = sampler.finish();
        // The daemon's own Git CLI (ADR-GRP-009: reconciling the base) is not the predictor's:
        // `predict` imports no `invoke` (predict_boundary), so its only child is the worker.
        let kids: Vec<_> = kids.into_iter().filter(|p| p.name != "git").collect();
        let names: BTreeSet<_> = kids.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(
            names,
            BTreeSet::from(["raptor"]),
            "the daemon's only child must be the worker (`raptor`): {kids:?}"
        );
        assert!(
            kids.len() == 1,
            "the worker was launched {} times with no job over its limits: {kids:?}",
            kids.len()
        );
        assert!(
            grandchildren.is_empty(),
            "the worker launched a process: {grandchildren:?}"
        );
    }

    /// T010 (M-02): a partial clone with a local promisor remote gives `missing-object`, and
    /// the remote is never contacted: its upload-pack is a marker script.
    #[test]
    fn ckpb_intact_partial_clone_gives_missing_object_and_never_connects() {
        let _shared = CENSUS.read().unwrap_or_else(|e| e.into_inner());
        let f = fixture();
        let upstream = f.other_repo.clone();
        f.git_in(&upstream, &["commit", "-q", "--allow-empty", "-m", "root"]);
        std::fs::write(upstream.join("shared.txt"), "one\ntwo\nthree\n").unwrap();
        f.git_in(&upstream, &["add", "shared.txt"]);
        f.git_in(&upstream, &["commit", "-q", "-m", "shared"]);
        f.git_in(&upstream, &["config", "uploadpack.allowFilter", "true"]);
        f.git_in(
            &upstream,
            &["config", "uploadpack.allowAnySHA1InWant", "true"],
        );
        for (branch, body) in [("b1", "ONE\ntwo\nthree\n"), ("b2", "uno\ntwo\nthree\n")] {
            f.git_in(&upstream, &["switch", "-q", "-c", branch, "main"]);
            std::fs::write(upstream.join("shared.txt"), body).unwrap();
            f.git_in(&upstream, &["commit", "-q", "-am", branch]);
        }
        f.git_in(&upstream, &["switch", "-q", "main"]);

        let clone = f.root.join("clone");
        let url = format!("file://{}", upstream.display());
        f.git_in(
            &f.root,
            &[
                "clone",
                "-q",
                "--filter=blob:none",
                "--no-checkout",
                &url,
                clone.to_str().unwrap(),
            ],
        );
        let mut roots = Vec::new();
        for b in ["b1", "b2"] {
            f.git_in(&clone, &["branch", b, &format!("origin/{b}")]);
            let root = f.root.join(format!("wt-{b}"));
            f.git_in(
                &clone,
                &[
                    "worktree",
                    "add",
                    "-q",
                    "--no-checkout",
                    root.to_str().unwrap(),
                    b,
                ],
            );
            roots.push(root);
        }
        // From here any fetch of the promisor remote runs this script: the marker is a connection.
        let bin = f.root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let marker = f.root.join("connected");
        let script = bin.join("upload-pack");
        canary::script(&script, &format!(": > '{}'\nexit 1", marker.display()));
        f.git_in(
            &clone,
            &[
                "config",
                "remote.origin.uploadpack",
                script.to_str().unwrap(),
            ],
        );
        assert!(!marker.exists());

        let rig = Rig::start(&clone, WorkerLimits::default());
        let pairs = rig.wait(
            "the pair of the two partial worktrees as not-computable",
            |p| {
                between(p, &roots[0], &roots[1])
                    .is_some_and(|p| p.summary.state == PairStateView::NotComputable)
            },
        );
        let pair = between(&pairs, &roots[0], &roots[1]).unwrap();
        assert_eq!(
            pair.summary.reason,
            Some(PairReason::MissingObject),
            "a blob that is not in the partial clone must be `missing-object`"
        );
        std::thread::sleep(Duration::from_millis(500));
        assert!(
            !marker.exists(),
            "the predictor connected to the promisor remote (M-02)"
        );
        rig.assert_answers("the partial clone");
    }

    /// T010 (M-06): a 100 MB blob on both sides is cut by its header with the real limits; the
    /// worker is not killed, so it is never relaunched, and the pair settles.
    #[test]
    fn ckpb_intact_hostile_blob_of_100_mb_is_cut_by_header_without_relaunching_the_worker() {
        let _alone = CENSUS.write().unwrap_or_else(|e| e.into_inner());
        let f = fixture();
        f.write("big.bin", "small\n");
        f.git(&["add", "big.bin"]);
        f.git(&["commit", "-q", "-m", "big"]);
        let mut roots = Vec::new();
        for (tag, b) in [(3u8, "wx"), (5u8, "wy")] {
            let root = f.root.join(format!("wt-{b}"));
            f.git(&[
                "worktree",
                "add",
                "-q",
                "-b",
                b,
                root.to_str().unwrap(),
                "main",
            ]);
            let block: Vec<u8> = (0..1 << 20)
                .map(|i| ((i / 4096) as u8).wrapping_mul(tag))
                .collect();
            let mut file = std::fs::File::create(root.join("big.bin")).unwrap();
            for _ in 0..100 {
                file.write_all(&block).unwrap();
            }
            drop(file);
            f.git_in(&root, &["add", "big.bin"]);
            f.git_in(&root, &["commit", "-q", "-m", b]);
            roots.push(root);
        }
        let sampler = Sampler::start();
        let rig = Rig::start(&f.repo, WorkerLimits::default());
        let pairs = rig.wait(
            "the hostile pair settled (current or not-computable)",
            |p| {
                between(p, &roots[0], &roots[1]).is_some_and(|p| {
                    matches!(
                        p.summary.state,
                        PairStateView::Current | PairStateView::NotComputable
                    )
                })
            },
        );
        let hostile = between(&pairs, &roots[0], &roots[1]).unwrap();
        assert!(
            hostile.summary.state == PairStateView::Current
                || hostile.summary.reason == Some(PairReason::LimitExceeded),
            "an oversized blob must not end as {:?}/{:?}",
            hostile.summary.state,
            hostile.summary.reason
        );
        rig.assert_answers("the 100 MB blob");
        let (kids, _) = sampler.finish();
        let pids: BTreeSet<u32> = kids
            .iter()
            .filter(|p| p.name == "raptor")
            .map(|p| p.pid)
            .collect();
        assert_eq!(
            pids.len(),
            1,
            "the worker was launched {} times: a cut by header must not kill it: {pids:?}",
            pids.len()
        );
    }

    /// T010 (M-06): 10,000 conflicted files push the worker past a reduced memory cap: it is
    /// killed, the pair is `not-computable (limit-exceeded)` and the daemon keeps answering.
    #[test]
    fn ckpb_intact_hostile_ten_thousand_conflicted_files_kill_the_worker_by_memory() {
        let _shared = CENSUS.read().unwrap_or_else(|e| e.into_inner());
        let f = fixture();
        for b in ["wx", "wy"] {
            let files: Vec<(String, Vec<u8>)> = (0..10_000)
                .map(|i| {
                    (
                        format!("d{}/f{i}.dat", i % 100),
                        format!("{b} {i}\n").repeat(300).into_bytes(),
                    )
                })
                .collect();
            fast_import(&f, &f.repo, &import_branch(b, &files));
        }
        let (x, y) = two_worktrees(&f);
        let limits = WorkerLimits {
            memory_cap: 64 * 1024 * 1024,
            deadline: Duration::from_secs(120),
            escalated_deadline: Duration::from_secs(120),
            ..WorkerLimits::default()
        };
        let rig = Rig::start(&f.repo, limits);
        let pairs = rig.wait("the 10,000-file pair as not-computable", |p| {
            between(p, &x, &y).is_some_and(|p| p.summary.state == PairStateView::NotComputable)
        });
        assert_eq!(
            between(&pairs, &x, &y).unwrap().summary.reason,
            Some(PairReason::LimitExceeded),
            "a worker killed by memory must give `limit-exceeded`"
        );
        rig.assert_answers("the worker killed by memory");
    }

    /// T010 (M-06): a dense 2 MB blob edited on both sides takes longer than a reduced deadline:
    /// the worker is killed by the deadline, the pair is `not-computable (limit-exceeded)` and
    /// the daemon keeps answering.
    #[test]
    fn ckpb_intact_hostile_dense_blob_of_2_mb_is_killed_by_the_deadline() {
        let _shared = CENSUS.read().unwrap_or_else(|e| e.into_inner());
        let f = fixture();
        let line = |i: usize, tag: &str| {
            if i.is_multiple_of(3) && tag == "x" {
                "xxxxxxxxx\n"
            } else if i % 3 == 1 && tag == "y" {
                "yyyyyyyyy\n"
            } else {
                "ooooooooo\n"
            }
        };
        let body = |tag: &str| (0..200_000).map(|i| line(i, tag)).collect::<String>();
        fast_import(
            &f,
            &f.repo,
            &import_branch("wx", &[("dense.txt".into(), body("x").into_bytes())]),
        );
        fast_import(
            &f,
            &f.repo,
            &import_branch("wy", &[("dense.txt".into(), body("y").into_bytes())]),
        );
        let (x, y) = two_worktrees(&f);
        let limits = WorkerLimits {
            deadline: Duration::from_millis(1),
            escalated_deadline: Duration::from_millis(1),
            ..WorkerLimits::default()
        };
        let rig = Rig::start(&f.repo, limits);
        let pairs = rig.wait("the dense pair as not-computable", |p| {
            between(p, &x, &y).is_some_and(|p| p.summary.state == PairStateView::NotComputable)
        });
        assert_eq!(
            between(&pairs, &x, &y).unwrap().summary.reason,
            Some(PairReason::LimitExceeded),
            "a worker killed by its deadline must give `limit-exceeded`"
        );
        rig.assert_answers("the worker killed by the deadline");
    }
}
