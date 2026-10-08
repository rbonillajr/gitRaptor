//! The chaos harness of the Time Machine (INF-TMC-001, DS-INF-TMC-001): the
//! real `raptor` binary as daemon and as client, over a temporary repo,
//! home and profile; never this repo nor the real profile (NFR-01).
//!
//! `crash_at_*`: the daemon dies with `SIGKILL` at each crash point of an
//! undo (`gitraptor_core::timemachine::chaos`, `chaos` feature of the test
//! build); at the next start the recovery closes the states (ADR-TMC-003
//! § 6) and `raptor undo` gets everything back. `hostile_*`: hostile Git
//! (US-TMC-018, US-TMC-019) between the loss and the undo; nothing is lost
//! and `raptor undo` brings the work back.
//!
//! Each scenario: uncommitted work captured by observation (S0), a raw
//! `git reset --hard` that throws it away (S1), then the undo. "State" is
//! what the user sees: every file of the worktree with its bytes, the index
//! (`ls-files -s`, whose ids are the staged bytes), `HEAD`, the branches and
//! `git status`. No fixed waits: each state is awaited with a deadline.
//!
//! macOS and Linux, like `us_tmc_002`. Windows: the CI does not run tests
//! with Git there yet (TS-GRP-002) and has no `SIGKILL`. Pendiente: etapa de
//! validación multiplataforma (XP-33).
#![cfg(any(target_os = "macos", target_os = "linux"))]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_api::messages::{ClientKind, EventsHistoryResult, GitEventKind, Snapshot};
use gitraptor_api::timemachine::UndoResult;
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::client::Client;
use gitraptor_core::profile::{Profile, ProfileDirs};
use gitraptor_core::timemachine::chaos::{self, CRASH_AT_ENV, POINTS};
use gitraptor_core::timemachine::oplog::{
    NoticeKind, OperationKind, OperationState, Oplog, SnapshotState,
};
use gitraptor_core::timemachine::store::{SnapshotStore, snapshot_refs};
use gitraptor_git::tm_write::store::TreeEntryKind;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fixture::git_from_path;
use serde_json::json;

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
/// The only agent executable the debug daemon knows: the Claude Code session
/// that may run these tests is not taken for an agent.
const FAKE_AGENT: &str = "raptor-fake-agent";
const DEADLINE: Duration = Duration::from_secs(30);
/// Key of the worktree `<root>/wt-feat-login` in the snapshots.
const KEY: &str = "wt-wt-feat-login";

const BASE: &[u8] = b"user\n";
const IGNORE: &[u8] = b"*.log\n";
const IGNORE_WORK: &[u8] = b"*.log\n*.tmp\n";
const GIT_BUSY: &str = "Git is busy";
const REPO_BUSY: &str = "another operation is running on this repo";
const WORK: &[u8] = b"user\npassword\n";
const UTIL: &[u8] = b"fn util() {}\n";

/// What the user sees of a worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
struct State {
    files: BTreeMap<String, Vec<u8>>,
    index: String,
    head: String,
    branches: String,
    status: String,
}

struct Machine {
    f: Fixture,
    worktree: PathBuf,
    daemon: Option<Child>,
}

impl Drop for Machine {
    fn drop(&mut self) {
        if let Some(mut child) = self.daemon.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Machine {
    /// "demo": `main` with `login.txt`, branch `feat-login` and its worktree
    /// (`<root>/wt-feat-login`), observed by a fresh profile; the daemon
    /// serving.
    fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let f = Fixture::new(&git_from_path());
        f.write("login.txt", "user\n");
        f.write(".gitignore", std::str::from_utf8(IGNORE).unwrap());
        f.git(&["add", "login.txt", ".gitignore"]);
        f.git(&["commit", "-q", "-m", "login"]);
        f.git(&["branch", "feat-login"]);
        let worktree = f
            .add_worktree("feat-login", "feat-login")
            .canonicalize()
            .unwrap();
        for dir in ["", "data", "config", "state"] {
            std::fs::set_permissions(f.profile.join(dir), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let common = f.repo.join(".git").canonicalize().unwrap();
        let (mut profile, _) = Profile::open(ProfileDirs::under_root(&f.profile)).unwrap();
        profile.add_repo(&common, None, 1).unwrap();
        drop(profile);
        let mut m = Self {
            f,
            worktree,
            daemon: None,
        };
        m.start(None);
        m
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(&self.f.profile)
    }

    fn env(&self) -> Vec<(&'static str, OsString)> {
        let git_dir = self.f.git.parent().unwrap().to_owned();
        let path = std::env::join_paths([git_dir, "/usr/bin".into(), "/bin".into()]).unwrap();
        vec![
            (
                "GITRAPTOR_PROFILE_DIR",
                self.f.profile.clone().into_os_string(),
            ),
            ("GITRAPTOR_AGENT_EXECUTABLES", FAKE_AGENT.into()),
            ("GITRAPTOR_TEST_TM_NO_FREE_SPACE_FLOOR", "1".into()),
            (
                "GITRAPTOR_TEST_DISCOVERY_HOME",
                self.f.home.clone().into_os_string(),
            ),
            ("HOME", self.f.home.clone().into_os_string()),
            ("PATH", path),
            ("LANG", "en_US.UTF-8".into()),
        ]
    }

    /// Starts the daemon, dying at `crash` if given, and waits until it
    /// answers.
    fn start(&mut self, crash: Option<&str>) {
        assert!(self.daemon.is_none());
        let mut cmd = Command::new(RAPTOR);
        cmd.arg("daemon")
            .env_clear()
            .envs(self.env())
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(point) = crash {
            cmd.env(CRASH_AT_ENV, point);
        }
        self.daemon = Some(cmd.spawn().unwrap());
        let start = Instant::now();
        while Client::connect(&self.dirs(), ClientKind::Cli, PROTOCOL_VERSION).is_err() {
            assert!(start.elapsed() < DEADLINE, "the daemon never answered");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// An orderly stop (`SIGTERM`).
    fn stop(&mut self) {
        let mut child = self.daemon.take().unwrap();
        let status = Command::new("/bin/kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap();
        assert!(status.success());
        assert!(wait_exit(&mut child).success());
    }

    fn client(&self) -> Client {
        Client::connect(&self.dirs(), ClientKind::Cli, PROTOCOL_VERSION).unwrap()
    }

    fn repo_id(&self) -> String {
        let snapshot: Snapshot = self
            .client()
            .call(methods::ENGINE_SNAPSHOT, json!({}))
            .unwrap();
        snapshot.repos[0].repo_id.clone()
    }

    /// `raptor <args>` from the worktree, as the developer.
    fn raptor(&self, args: &[&str]) -> Output {
        Command::new(RAPTOR)
            .args(args)
            .env_clear()
            .envs(self.env())
            .current_dir(&self.worktree)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn undo(&self) -> UndoResult {
        let out = self.raptor(&["undo", "--json"]);
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    /// `raptor undo` that is rejected, saying so in one of `reasons`.
    fn undo_rejected(&self, reasons: &[&str]) {
        let out = self.raptor(&["undo"]);
        assert!(!out.status.success(), "{}", text(&out));
        let said = text(&out);
        assert!(said.contains("nothing changed"), "{said}");
        assert!(reasons.iter().any(|r| said.contains(r)), "{said}");
    }

    /// The fixture's Git in the worktree, as an agent would run it.
    fn git(&self, args: &[&str]) -> String {
        self.f.git_in(&self.worktree, args)
    }

    fn write(&self, rela: &str, content: &[u8]) {
        let path = self.worktree.join(rela);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn snapshot_ids(&self) -> BTreeSet<String> {
        SnapshotStore::open_existing(&self.dirs(), &self.repo_id())
            .ok()
            .flatten()
            .map(|store| {
                snapshot_refs(&store)
                    .unwrap_or_default()
                    .into_keys()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Waits for a snapshot not in `known` whose worktree holds exactly the
    /// files of `state`.
    fn captured(&self, state: &State, known: &BTreeSet<String>) -> String {
        let repo_id = self.repo_id();
        let start = Instant::now();
        loop {
            if let Ok(Some(store)) = SnapshotStore::open_existing(&self.dirs(), &repo_id) {
                for id in snapshot_refs(&store).unwrap_or_default().keys() {
                    if known.contains(id) {
                        continue;
                    }
                    let files: BTreeMap<String, Vec<u8>> = store
                        .files(id, KEY)
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|(_, k, _)| *k != TreeEntryKind::Gitlink)
                        .map(|(p, _, oid)| (p, store.read_blob(oid).unwrap()))
                        .collect();
                    if files == state.files {
                        return id.clone();
                    }
                }
            }
            assert!(
                start.elapsed() < DEADLINE,
                "never captured: {:?}",
                state.files.keys()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Waits until the history has `n` events of `kind` in the worktree.
    fn events(&self, kind: GitEventKind, n: usize) {
        let repo_id = self.repo_id();
        let start = Instant::now();
        loop {
            let page: EventsHistoryResult = self
                .client()
                .call(methods::EVENTS_HISTORY, json!({ "repo_id": repo_id }))
                .unwrap();
            let seen = page
                .events
                .iter()
                .filter(|e| e.kind == kind && Path::new(e.worktree.raw()) == self.worktree)
                .count();
            if seen >= n {
                return;
            }
            assert!(
                start.elapsed() < DEADLINE,
                "{seen} of {n} {kind:?}: {:#?}",
                page.events
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn state(&self) -> State {
        fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                if rel == ".git" {
                    continue;
                }
                if path.symlink_metadata().unwrap().is_dir() {
                    walk(root, &path, out);
                } else {
                    out.insert(rel, std::fs::read(&path).unwrap());
                }
            }
        }
        let mut files = BTreeMap::new();
        walk(&self.worktree, &self.worktree, &mut files);
        State {
            files,
            index: self.git(&["ls-files", "-s"]),
            head: self.git(&["rev-parse", "--symbolic-full-name", "HEAD"])
                + &self.git(&["rev-parse", "HEAD"]),
            branches: self.git(&[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads",
            ]),
            status: self.git(&["status", "--porcelain=v1", "--untracked-files=all"]),
        }
    }

    /// The work of the scenarios: `login.txt` changed and `util.rs` new,
    /// captured by observation (S0); then a raw `git reset --hard` throws it
    /// away (S1). Returns both states and the id of the capture.
    fn lose_work(&self) -> (State, State, String) {
        // Two tracked files change, so the undo writes more than one file.
        self.write(".gitignore", IGNORE_WORK);
        self.write("login.txt", WORK);
        self.write("src/util.rs", UTIL);
        let s0 = self.state();
        let captured = self.captured(&s0, &BTreeSet::new());
        self.git(&["reset", "-q", "--hard"]);
        self.events(GitEventKind::Reset, 1);
        let s1 = self.state();
        assert_eq!(s1.files["login.txt"], BASE);
        assert_eq!(s1.files[".gitignore"], IGNORE);
        assert_eq!(s1.files["src/util.rs"], UTIL, "reset keeps untracked files");
        (s0, s1, captured)
    }

    /// Stops the daemon and opens the repo's oplog; the daemon starts again.
    fn oplog(&mut self) -> Oplog {
        let repo_id = self.repo_id();
        self.stop();
        let oplog = Oplog::open(&self.dirs(), &repo_id, 1).unwrap().0;
        self.start(None);
        oplog
    }

    /// Every `*.lock` file under the repo's Git directory.
    fn git_locks(&self) -> Vec<PathBuf> {
        fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.symlink_metadata().unwrap().is_dir() {
                    walk(&path, out);
                } else if path.extension().is_some_and(|e| e == "lock") {
                    out.push(path);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.f.repo.join(".git"), &mut out);
        out
    }
}

fn text(out: &Output) -> String {
    [
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    ]
    .concat()
}

fn wait_exit(child: &mut Child) -> ExitStatus {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(start.elapsed() < DEADLINE, "the daemon did not exit");
        std::thread::sleep(Duration::from_millis(20));
    }
}

// ----- Death of the daemon -----------------------------------------------------------

/// Whether the undo had started writing when it died at `point`.
fn applying(point: &str) -> bool {
    point.starts_with("apply:") || point == chaos::OPERATION_APPLIED
}

/// The daemon dies at `point` in the middle of `raptor undo`; at the next
/// start the recovery closes the undo and `raptor undo` gets everything back
/// (US-TMC-019).
fn crash_scenario(point: &str) {
    let mut m = Machine::new();
    let (s0, s1, _) = m.lose_work();

    // The undo kills the daemon at `point`: the client fails, the daemon
    // dies by `SIGKILL` (the proof that the point ran).
    m.stop();
    m.start(Some(point));
    let out = m.raptor(&["undo", "--json"]);
    assert!(!out.status.success(), "{point}: {}", text(&out));
    let mut dead = m.daemon.take().unwrap();
    assert_eq!(wait_exit(&mut dead).signal(), Some(9), "{point}");

    m.start(None);
    let oplog = m.oplog();
    let undos: Vec<_> = oplog
        .operations(&Default::default())
        .unwrap()
        .into_iter()
        .filter(|op| op.record.kind == OperationKind::Undo)
        .collect();
    assert_eq!(undos.len(), 1, "{point}: {undos:#?}");
    let crashed = &undos[0];
    // No snapshot is left half-written, and none without its ref is offered.
    let refs = m.snapshot_ids();
    for snap in oplog.snapshots(&Default::default()).unwrap() {
        assert_ne!(snap.state, SnapshotState::Pending, "{point}: {snap:#?}");
        if snap.state == SnapshotState::Discarded {
            assert!(!refs.contains(&snap.record.snapshot_id), "{point}");
        }
    }
    let notices: Vec<_> = oplog
        .notices()
        .unwrap()
        .into_iter()
        .filter(|n| n.kind == NoticeKind::Interruption)
        .collect();

    if !applying(point) {
        // The repo was never touched: aborted, S1 byte for byte, no notice.
        assert_eq!(crashed.state, OperationState::Aborted, "{point}");
        assert!(notices.is_empty(), "{point}: {notices:#?}");
        assert_eq!(m.state(), s1, "{point}");
        assert!(m.git_locks().is_empty(), "{point}: {:?}", m.git_locks());
        // The next undo takes back the reset.
        m.undo();
        assert_eq!(m.state(), s0, "{point}");
        return;
    }

    // Interrupted while writing: one notice for the worktree, and the
    // repo's own locks are released (on Linux this needs the birth time of
    // the temporary folder's file system: ADR-TMC-003 § 6.4).
    assert_eq!(crashed.state, OperationState::Interrupted, "{point}");
    assert_eq!(notices.len(), 1, "{point}: {notices:#?}");
    assert_eq!(
        notices[0].operation_id.as_deref(),
        Some(crashed.record.operation_id.as_str())
    );
    assert_eq!(
        notices[0].worktree.as_deref(),
        Some(m.worktree.to_str().unwrap()),
        "{point}"
    );
    assert!(
        m.git_locks().is_empty(),
        "{point}: own locks left after the recovery (birth time missing?): {:?}",
        m.git_locks()
    );

    // `raptor undo` takes the interrupted undo back to its prior (S1
    // exactly), not a raw event of its half-written files.
    let back = m.undo();
    assert_eq!(
        back.undone_operation_id, crashed.record.operation_id,
        "{point}"
    );
    assert_eq!(m.state(), s1, "{point}");
    // And the next one takes back the reset: nothing was lost.
    m.undo();
    assert_eq!(m.state(), s0, "{point}");
}

macro_rules! crash_scenarios {
    ($($name:ident => $point:expr,)*) => {
        $(
            #[test]
            fn $name() {
                crash_scenario($point);
            }
        )*

        /// The coverage of the harness: every crash point has its scenario.
        #[test]
        fn every_crash_point_has_a_scenario() {
            let covered: BTreeSet<&str> = [$($point),*].into_iter().collect();
            let all: BTreeSet<&str> = POINTS.iter().copied().collect();
            assert_eq!(covered, all);
        }
    };
}

crash_scenarios! {
    crash_at_operation_intent => chaos::OPERATION_INTENT,
    crash_at_prior_pending => chaos::PRIOR_PENDING,
    crash_at_prior_ref => chaos::PRIOR_REF,
    crash_at_operation_prior => chaos::OPERATION_PRIOR,
    crash_at_operation_ready => chaos::OPERATION_READY,
    crash_at_apply_step_3 => "apply:step-3",
    crash_at_apply_step_4 => "apply:step-4",
    crash_at_apply_step_5 => "apply:step-5",
    crash_at_apply_step_6 => "apply:step-6",
    crash_at_apply_mid_files => chaos::APPLY_MID_FILES,
    crash_at_apply_step_7 => "apply:step-7",
    crash_at_operation_applied => chaos::OPERATION_APPLIED,
}

// ----- Hostile Git -------------------------------------------------------------------

/// An agent sends every ref to a remote: no snapshot ref or object leaves
/// the profile, and the undo still brings the work back.
#[test]
fn hostile_mirror_of_every_ref_leaks_no_snapshot() {
    let m = Machine::new();
    let (s0, _, captured) = m.lose_work();
    let remote = m.f.root.join("remote.git");
    m.f.git_in(
        &m.f.root,
        &["init", "-q", "--bare", remote.to_str().unwrap()],
    );
    m.git(&["push", "-q", "--mirror", remote.to_str().unwrap()]);

    let refs =
        m.f.git_in(&remote, &["for-each-ref", "--format=%(refname)"]);
    assert!(!refs.contains(&captured), "{refs}");
    assert!(refs.lines().all(|r| r.starts_with("refs/heads/")), "{refs}");
    // The blob of the untracked file only lives in the snapshot.
    let blob = m.git(&["hash-object", "src/util.rs"]);
    let has =
        m.f.git_command(&remote, &["cat-file", "-e", blob.trim()])
            .status()
            .unwrap();
    assert!(!has.success(), "the snapshot's blob reached the remote");

    m.undo();
    assert_eq!(m.state(), s0);
}

/// An agent throws a commit away and runs aggressive maintenance: the
/// commit and the uncommitted work come back from the store.
#[test]
fn hostile_aggressive_maintenance_after_a_destructive_reset() {
    let m = Machine::new();
    m.write("feature.rs", b"fn feature() {}\n");
    m.git(&["add", "feature.rs"]);
    m.git(&["commit", "-q", "-m", "feature"]);
    let tip = m.git(&["rev-parse", "HEAD"]);
    m.write("login.txt", WORK);
    m.write("src/util.rs", UTIL);
    m.git(&["add", "src/util.rs"]);
    let s0 = m.state();
    m.captured(&s0, &BTreeSet::new());

    m.git(&["reset", "-q", "--hard", "HEAD~1"]);
    m.events(GitEventKind::BranchUpdate, 1);
    m.git(&["reflog", "expire", "--expire=now", "--all"]);
    m.git(&["gc", "-q", "--prune=now", "--aggressive"]);
    let lost =
        m.f.git_command(&m.worktree, &["cat-file", "-e", tip.trim()])
            .status()
            .unwrap();
    assert!(!lost.success(), "gc should have pruned the commit");

    m.undo();
    assert_eq!(m.state(), s0);
    assert_eq!(m.git(&["rev-parse", "feat-login"]), tip);
}

/// An agent cleans and resets: tracked changes and untracked files come
/// back. Ignored files are not captured (US-TMC-001) and are not promised.
#[test]
fn hostile_agent_cleans_and_resets() {
    let m = Machine::new();
    m.write("login.txt", WORK);
    m.write("src/util.rs", UTIL);
    m.write("notes/todo.md", b"- todo\n");
    let s0 = m.state();
    m.captured(&s0, &BTreeSet::new());

    m.git(&["clean", "-q", "-fd"]);
    m.git(&["reset", "-q", "--hard"]);
    m.events(GitEventKind::Reset, 1);
    assert!(!m.worktree.join("src/util.rs").exists());

    m.undo();
    assert_eq!(m.state(), s0);
}

/// Locks of another Git: the undo is rejected without changing anything
/// and the locks stay; once they are gone, the undo brings the work back.
#[test]
fn hostile_foreign_git_locks_reject_and_stay() {
    let m = Machine::new();
    let (s0, s1, _) = m.lose_work();
    let wt_git = PathBuf::from(m.git(&["rev-parse", "--absolute-git-dir"]).trim());
    let common = m.f.repo.join(".git");
    for lock in [
        wt_git.join("index.lock"),
        common.join("refs/heads/feat-login.lock"),
    ] {
        std::fs::write(&lock, b"another git\n").unwrap();
        // A lock of the index also keeps the engine from settling: the
        // repo is busy before the applier sees the lock.
        m.undo_rejected(&[GIT_BUSY, REPO_BUSY]);
        assert_eq!(m.state(), s1, "{}", lock.display());
        assert_eq!(std::fs::read(&lock).unwrap(), b"another git\n");
        std::fs::remove_file(&lock).unwrap();
    }

    m.undo();
    assert_eq!(m.state(), s0);
}

/// A merge stopped on a conflict: the undo is rejected without changing
/// anything; after `git merge --abort`, it brings the work back.
#[test]
fn hostile_half_done_merge_rejects_until_aborted() {
    let m = Machine::new();
    m.f.git(&["checkout", "-q", "-b", "other"]);
    m.f.write("conf.txt", "theirs\n");
    m.f.git(&["add", "conf.txt"]);
    m.f.git(&["commit", "-q", "-m", "other"]);
    m.f.git(&["checkout", "-q", "main"]);
    m.write("conf.txt", b"ours\n");
    m.git(&["add", "conf.txt"]);
    m.git(&["commit", "-q", "-m", "ours"]);
    let (s0, s1, _) = m.lose_work();
    // The captures are asynchronous (the daemon takes a point once the worktree
    // has been quiet): wait for the clean S1 before the merge starts, so what
    // the undo takes back never depends on how fast the daemon is.
    let known = m.snapshot_ids();
    m.captured(&s1, &known);

    let merge =
        m.f.git_command(&m.worktree, &["merge", "-q", "other"])
            .output()
            .unwrap();
    assert!(!merge.status.success(), "the merge must stop on a conflict");
    let stopped = m.state();
    m.undo_rejected(&["a Git operation is in progress"]);
    assert_eq!(m.state(), stopped);

    // The daemon may capture the half-done merge too, whenever it likes: wait
    // for that point, so the next undo has a single possible target.
    let known = m.snapshot_ids();
    m.captured(&stopped, &known);

    // `merge --abort` is raw Git too (a reset to `HEAD`): the first undo
    // takes it back, to the last state the Time Machine captured, the
    // half-done merge (`stopped`; its files and index, a merge in progress is
    // never restored as such); the next takes back the reset. Nothing was
    // lost. The undo only sees the abort once the engine observed it.
    m.git(&["merge", "--abort"]);
    m.events(GitEventKind::Reset, 2);
    m.undo();
    assert_eq!(m.state(), stopped);
    m.undo();
    assert_eq!(m.state(), s0);
}
