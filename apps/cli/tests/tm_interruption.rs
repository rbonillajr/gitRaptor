//! The repo stays recoverable when GitRaptor dies half-way through a snapshot, an undo or a
//! restore, with the real `raptor` binary as daemon and as client over a temporary repo, home and
//! profile; never this repo nor the real profile (NFR-01, NFR-12).
//!
//! The cuts are deterministic: the daemon kills itself with `SIGKILL` at a named crash point of the
//! `chaos` feature (`GITRAPTOR_TEST_TM_CRASH_AT`), or a write is refused by a folder made
//! read-only. No fixed waits: every state is awaited with a deadline. "Intact" is the INF-GRP-001
//! fingerprint of the worktrees; "recoverable" is the state the user sees (files with their bytes,
//! index, `HEAD`, branches and `git status`) back exactly as it was.
//!
//! One test per scenario of the story, plus the delivery of the notice over the CLI (English and
//! Spanish) and its scope (a folder the repo does not register gets nothing and marks nothing).
//!
//! macOS and Linux, like `tm_chaos`: the CI does not run tests with Git on Windows yet and there
//! is no `SIGKILL` there. Pendiente: etapa de validación multiplataforma.
#![cfg(any(target_os = "macos", target_os = "linux"))]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

use gitraptor_api::messages::{ClientKind, EventsHistoryResult, GitEventKind, Snapshot};
use gitraptor_api::rpc::{ScopeRefusal, ScopeRefusedData, code};
use gitraptor_api::timemachine::{
    EntryOrigin, RestoreResult, TimelineOperationKind, TimelineOperationState, TimelineResult,
    UndoResult,
};
use gitraptor_api::{PROTOCOL_VERSION, methods};
use gitraptor_core::client::{Client, ClientError};
use gitraptor_core::profile::{Profile, ProfileDirs};
use gitraptor_core::timemachine::chaos::{self, CRASH_AT_ENV};
use gitraptor_core::timemachine::oplog::{
    NoticeKind, OperationKind, OperationState, Oplog, SnapshotLevel, SnapshotState,
};
use gitraptor_core::timemachine::store::{SnapshotStore, snapshot_refs};
use gitraptor_git::tm_write::store::TreeEntryKind;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fingerprint::{Scope, Snapshot as Fingerprint, diff};
use gitraptor_testkit::fixture::git_from_path;
use serde_json::{Value, json};

const RAPTOR: &str = env!("CARGO_BIN_EXE_raptor");
/// The only agent executable the debug daemon knows: the Claude Code session that may run these
/// tests is not taken for an agent, so every request here is "unattributed".
const FAKE_AGENT: &str = "raptor-fake-agent";
const DEADLINE: Duration = Duration::from_secs(30);
/// Key of the worktree `<root>/wt-feat-login` in the snapshots.
const KEY: &str = "wt-wt-feat-login";

/// The method that hands a client the pending notices of its worktree, once.
const NOTICES: &str = "timemachine.notices";
/// Crash points of a capture that is not a guaranteed prior: its row `pending` without a ref, and
/// its ref created with the row still `pending`.
const CAPTURE_PENDING: &str = "capture:pending";
const CAPTURE_REF: &str = "capture:ref";

const BASE_A: &[u8] = b"alpha\n";
const BASE_B: &[u8] = b"beta\n";
const WORK_A: &[u8] = b"alpha\nwork on a\n";
const WORK_B: &[u8] = b"beta\nwork on b\n";
const FOREIGN_LOCK: &[u8] = b"another git\n";

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
    /// `<root>/wt-feat-login`.
    worktree: PathBuf,
    /// `<root>/wt-feat-pagos`.
    pagos: PathBuf,
    repo_id: String,
    daemon: Option<Child>,
}

impl Drop for Machine {
    fn drop(&mut self) {
        if let Some(mut child) = self.daemon.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        // A folder left read-only would keep the temporary directory from being removed.
        for dir in [self.worktree.join("src"), self.worktree.join("docs")] {
            let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755));
        }
    }
}

impl Machine {
    /// "demo": `main` with `docs/a.txt` and `src/b.txt`, branches `feat-login` and `feat-pagos`
    /// with their worktrees, observed by a fresh profile; the daemon serving.
    fn new() -> Self {
        let f = Fixture::new(&git_from_path());
        f.write("docs/a.txt", std::str::from_utf8(BASE_A).unwrap());
        f.write("src/b.txt", std::str::from_utf8(BASE_B).unwrap());
        f.git(&["add", "docs/a.txt", "src/b.txt"]);
        f.git(&["commit", "-q", "-m", "base"]);
        f.git(&["branch", "feat-login"]);
        f.git(&["branch", "feat-pagos"]);
        let worktree = f
            .add_worktree("feat-login", "feat-login")
            .canonicalize()
            .unwrap();
        let pagos = f
            .add_worktree("feat-pagos", "feat-pagos")
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
            pagos,
            repo_id: String::new(),
            daemon: None,
        };
        m.start(None);
        let snapshot: Snapshot = m
            .client()
            .call(methods::ENGINE_SNAPSHOT, json!({}))
            .unwrap();
        m.repo_id = snapshot.repos[0].repo_id.clone();
        m
    }

    fn dirs(&self) -> ProfileDirs {
        ProfileDirs::under_root(&self.f.profile)
    }

    fn env(&self, lang: &str) -> Vec<(&'static str, OsString)> {
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
            ("LANG", lang.into()),
        ]
    }

    fn spawn(&mut self, crash: Option<&str>) {
        assert!(self.daemon.is_none());
        let mut cmd = Command::new(RAPTOR);
        cmd.arg("daemon")
            .env_clear()
            .envs(self.env("en_US.UTF-8"))
            .current_dir(&self.f.root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(point) = crash {
            cmd.env(CRASH_AT_ENV, point);
        }
        self.daemon = Some(cmd.spawn().unwrap());
    }

    /// Starts the daemon, dying at `crash` if given, and waits until it answers.
    fn start(&mut self, crash: Option<&str>) {
        self.spawn(crash);
        let start = Instant::now();
        while Client::connect(&self.dirs(), ClientKind::Cli, PROTOCOL_VERSION).is_err() {
            assert!(start.elapsed() < DEADLINE, "the daemon never answered");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Starts the daemon armed at `point` and waits until it answers or dies (a point reached
    /// while starting is a valid cut too).
    fn start_armed(&mut self, point: &str) {
        self.spawn(Some(point));
        let start = Instant::now();
        loop {
            if Client::connect(&self.dirs(), ClientKind::Cli, PROTOCOL_VERSION).is_ok() {
                return;
            }
            if self.daemon.as_mut().unwrap().try_wait().unwrap().is_some() {
                return;
            }
            assert!(start.elapsed() < DEADLINE, "the daemon never answered");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The armed daemon died by `SIGKILL`: the proof that its point ran.
    fn assert_killed(&mut self, point: &str) {
        let mut dead = self.daemon.take().unwrap();
        assert_eq!(wait_exit(&mut dead).signal(), Some(9), "{point}");
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

    /// Restarts the daemon armed at `point`, runs `raptor <args>` from feat-login, which must
    /// fail because the daemon died there, and starts it again unarmed: the recovery ran.
    fn kill_during(&mut self, point: &str, args: &[&str]) {
        self.stop();
        self.start(Some(point));
        let out = self.raptor(&self.worktree, args, "en_US.UTF-8");
        assert!(!out.status.success(), "{point}: {}", text(&out));
        self.assert_killed(point);
        self.start(None);
    }

    fn client(&self) -> Client {
        Client::connect(&self.dirs(), ClientKind::Cli, PROTOCOL_VERSION).unwrap()
    }

    /// `raptor <args>` from `dir`, as the developer, in `lang`.
    fn raptor(&self, dir: &Path, args: &[&str], lang: &str) -> Output {
        Command::new(RAPTOR)
            .args(args)
            .env_clear()
            .envs(self.env(lang))
            .current_dir(dir)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn undo(&self) -> UndoResult {
        let out = self.raptor(&self.worktree, &["undo", "--json"], "en_US.UTF-8");
        assert!(out.status.success(), "{}", text(&out));
        serde_json::from_slice(&out.stdout).unwrap()
    }

    /// The pending notices of `worktree`, asked as the CLI would.
    fn notices(&self, worktree: &Path) -> Result<Value, ClientError> {
        self.client().call(
            NOTICES,
            json!({ "worktree": worktree.to_string_lossy(), "surface": "cli" }),
        )
    }

    fn timeline(&self) -> TimelineResult {
        let value: Value = self
            .client()
            .call(
                methods::TM_TIMELINE,
                json!({ "worktree": self.worktree.to_string_lossy() }),
            )
            .unwrap();
        serde_json::from_value(value).unwrap()
    }

    /// The fixture's Git in feat-login, as an agent would run it.
    fn git(&self, args: &[&str]) -> String {
        self.f.git_in(&self.worktree, args)
    }

    fn write(&self, rela: &str, content: &[u8]) {
        let path = self.worktree.join(rela);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn snapshot_ids(&self) -> BTreeSet<String> {
        SnapshotStore::open_existing(&self.dirs(), &self.repo_id)
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

    /// Waits for a snapshot not in `known` whose feat-login holds exactly the files of `state`.
    fn captured(&self, state: &State, known: &BTreeSet<String>) -> String {
        let start = Instant::now();
        loop {
            if let Ok(Some(store)) = SnapshotStore::open_existing(&self.dirs(), &self.repo_id) {
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

    /// Waits until the history has `n` events of `kind` in feat-login.
    fn events(&self, kind: GitEventKind, n: usize) {
        let start = Instant::now();
        loop {
            let page: EventsHistoryResult = self
                .client()
                .call(methods::EVENTS_HISTORY, json!({ "repo_id": self.repo_id }))
                .unwrap();
            let seen = page
                .events
                .iter()
                .filter(|e| e.kind == kind && Path::new(e.worktree.raw()) == self.worktree)
                .count();
            if seen >= n {
                return;
            }
            assert!(start.elapsed() < DEADLINE, "{seen} of {n} {kind:?}");
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

    /// INF-GRP-001 fingerprint of both worktrees (their files and their `.git` link). Never
    /// taken across a `git` of the test, which may refresh an index.
    fn fingerprint(&self) -> Fingerprint {
        Fingerprint::take(
            &[
                Scope::new("feat-login", &self.worktree),
                Scope::new("feat-pagos", &self.pagos),
            ],
            &BTreeSet::new(),
        )
    }

    /// Uncommitted work on both files of feat-login, captured by observation (S0); then a raw
    /// `git reset --hard` throws it away (S1).
    fn lose_work(&self) -> (State, State, String) {
        self.write("docs/a.txt", WORK_A);
        self.write("src/b.txt", WORK_B);
        let s0 = self.state();
        let captured = self.captured(&s0, &BTreeSet::new());
        self.git(&["reset", "-q", "--hard"]);
        self.events(GitEventKind::Reset, 1);
        let s1 = self.state();
        assert_eq!(s1.files["docs/a.txt"], BASE_A);
        assert_eq!(s1.files["src/b.txt"], BASE_B);
        (s0, s1, captured)
    }

    /// Stops the daemon and opens the repo's oplog; the daemon starts again.
    fn oplog(&mut self) -> Oplog {
        self.stop();
        let oplog = Oplog::open(&self.dirs(), &self.repo_id, 1).unwrap().0;
        self.start(None);
        oplog
    }

    /// The Git admin folder of a worktree, from its `.git` link.
    fn admin(&self, worktree: &Path) -> PathBuf {
        let link = std::fs::read_to_string(worktree.join(".git")).unwrap();
        PathBuf::from(link.trim().strip_prefix("gitdir: ").unwrap())
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

fn assert_intact(before: &Fingerprint, after: &Fingerprint, what: &str) {
    let changes = diff(before, after);
    assert!(changes.is_empty(), "{what}: {changes:#?}");
}

/// The notices of a `timemachine.notices` answer.
fn notice_list(answer: &Value) -> Vec<Value> {
    answer["notices"]
        .as_array()
        .unwrap_or_else(|| panic!("no notice list: {answer}"))
        .clone()
}

/// The only operation of `kind` in the oplog.
fn only_operation(
    oplog: &Oplog,
    kind: OperationKind,
) -> gitraptor_core::timemachine::oplog::OperationView {
    let ops: Vec<_> = oplog
        .operations(&Default::default())
        .unwrap()
        .into_iter()
        .filter(|op| op.record.kind == kind)
        .collect();
    assert_eq!(ops.len(), 1, "{ops:#?}");
    ops.into_iter().next().unwrap()
}

// ----- Scenario 1: a snapshot cut half-way --------------------------------------------------

/// The daemon dies at `point` while it captures feat-login by observation: the worktree does not
/// change and the incomplete snapshot is never offered as a point to restore.
fn capture_cut(point: &str) {
    let mut m = Machine::new();
    // A first capture of the clean worktree, so the next one is the one that dies.
    let clean = m.state();
    m.captured(&clean, &BTreeSet::new());
    let known = m.snapshot_ids();

    m.stop();
    // Every snapshot row so far, whatever its state: the cut one is the only row added after.
    let rows: BTreeSet<String> = Oplog::open(&m.dirs(), &m.repo_id, 1)
        .unwrap()
        .0
        .snapshots(&Default::default())
        .unwrap()
        .into_iter()
        .map(|s| s.record.snapshot_id)
        .collect();
    m.start_armed(point);
    m.write("docs/a.txt", WORK_A);
    let before = m.fingerprint();
    // The capture of that change kills the daemon; a daemon still alive at the deadline means
    // the point never ran. At `capture:ref` the new ref exists a moment before the daemon dies,
    // so a new ref alone proves nothing: the oplog after the restart is the proof.
    let start = Instant::now();
    while m.daemon.as_mut().unwrap().try_wait().unwrap().is_none() {
        let new: Vec<_> = m.snapshot_ids().difference(&known).cloned().collect();
        assert!(
            start.elapsed() < DEADLINE,
            "{point}: the daemon did not die there (new refs: {new:?})"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    m.assert_killed(point);
    assert_intact(&before, &m.fingerprint(), point);

    m.start(None);
    let oplog = m.oplog();
    let refs = m.snapshot_ids();
    let snapshots = oplog.snapshots(&Default::default()).unwrap();
    assert!(
        snapshots.iter().all(|s| s.state != SnapshotState::Pending),
        "{point}: {snapshots:#?}"
    );
    let cut: Vec<String> = snapshots
        .iter()
        .filter(|s| {
            !rows.contains(&s.record.snapshot_id)
                && s.state == SnapshotState::Discarded
                && s.record.level == SnapshotLevel::Observation
        })
        .map(|s| s.record.snapshot_id.clone())
        .collect();
    assert_eq!(cut.len(), 1, "{point}: {snapshots:#?}");
    let cut = &cut[0];
    assert!(
        !refs.contains(cut),
        "{point}: its ref is still in the store"
    );

    // Not a point to restore: the timeline does not show it and a restore to it is refused
    // without changing anything.
    let shown = m.raptor(&m.worktree, &["timeline"], "en_US.UTF-8");
    assert!(shown.status.success(), "{}", text(&shown));
    assert!(!text(&shown).contains(cut.as_str()), "{}", text(&shown));
    let before = m.fingerprint();
    let restore = m.raptor(&m.worktree, &["restore", cut, "--json"], "en_US.UTF-8");
    assert!(!restore.status.success(), "{point}: {}", text(&restore));
    assert_intact(&before, &m.fingerprint(), point);
}

#[test]
fn s1_a_capture_cut_before_its_ref_is_never_a_point() {
    capture_cut(CAPTURE_PENDING);
}

#[test]
fn s1_a_capture_cut_after_its_ref_is_never_a_point() {
    capture_cut(CAPTURE_REF);
}

// ----- Scenario 2: an undo or a restore cut half-way ----------------------------------------

/// The daemon dies between the two file writes of `args` (an undo or a restore from S1): the
/// operation is `interrupted`, the next client of feat-login receives one notice, only once and
/// only there, and `raptor undo` brings feat-login back to S1, then the next one to S0.
fn apply_cut(args: &[&str], kind: OperationKind, wire_kind: &str) {
    let mut m = Machine::new();
    let (s0, s1, _) = m.lose_work();
    m.kill_during(chaos::APPLY_MID_FILES, args);

    let oplog = m.oplog();
    let cut = only_operation(&oplog, kind);
    assert_eq!(cut.state, OperationState::Interrupted);
    let pending: Vec<_> = oplog
        .notices()
        .unwrap()
        .into_iter()
        .filter(|n| n.kind == NoticeKind::Interruption)
        .collect();
    assert_eq!(pending.len(), 1, "{pending:#?}");
    assert!(pending[0].first_delivered_ms.is_none());

    // Another worktree's client gets nothing.
    let pagos = m
        .notices(&m.pagos)
        .expect("timemachine.notices from feat-pagos");
    assert!(notice_list(&pagos).is_empty(), "{pagos}");
    // The next client of feat-login gets the notice, once.
    let first = m
        .notices(&m.worktree)
        .expect("timemachine.notices from feat-login");
    let list = notice_list(&first);
    assert_eq!(list.len(), 1, "{first}");
    let notice = &list[0];
    assert_eq!(notice["kind"], "interruption", "{notice}");
    assert_eq!(
        notice["operation_id"].as_str(),
        Some(cut.record.operation_id.as_str())
    );
    assert_eq!(notice["operation_kind"], wire_kind, "{notice}");
    assert_eq!(
        notice["prior_snapshot_id"].as_str(),
        cut.prior_snapshot.as_deref(),
        "{notice}"
    );
    let again = m.notices(&m.worktree).unwrap();
    assert!(notice_list(&again).is_empty(), "shown twice: {again}");

    // The way back, for a requester that is "unattributed" (scenario 5).
    let back = m.undo();
    assert_eq!(back.undone_operation_id, cut.record.operation_id);
    assert_eq!(m.state(), s1);
    m.undo();
    assert_eq!(m.state(), s0);
}

#[test]
fn s2_an_undo_cut_half_way_is_recoverable_and_noticed_once() {
    apply_cut(&["undo", "--json"], OperationKind::Undo, "undo");
}

#[test]
fn s2_a_restore_cut_half_way_is_recoverable_and_noticed_once() {
    // The restore point is the capture of S0, taken before the reset.
    let mut m = Machine::new();
    let (s0, s1, point) = m.lose_work();
    let args = ["restore", point.as_str(), "--json"];
    m.kill_during(chaos::APPLY_MID_FILES, &args);

    let oplog = m.oplog();
    let cut = only_operation(&oplog, OperationKind::Restore);
    assert_eq!(cut.state, OperationState::Interrupted);

    let first = m
        .notices(&m.worktree)
        .expect("timemachine.notices from feat-login");
    let list = notice_list(&first);
    assert_eq!(list.len(), 1, "{first}");
    assert_eq!(list[0]["kind"], "interruption");
    assert_eq!(list[0]["operation_kind"], "restore");
    assert_eq!(
        list[0]["operation_id"].as_str(),
        Some(cut.record.operation_id.as_str())
    );
    assert!(notice_list(&m.notices(&m.worktree).unwrap()).is_empty());

    let back = m.undo();
    assert_eq!(back.undone_operation_id, cut.record.operation_id);
    assert_eq!(m.state(), s1);
    // The restore itself works again: back to S0.
    let out = m.raptor(&m.worktree, &args, "en_US.UTF-8");
    assert!(out.status.success(), "{}", text(&out));
    let _: RestoreResult = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(m.state(), s0);
}

/// The CLI shows the notice to the next command run from feat-login, in the user's language, and
/// never again.
fn cli_notice(lang: &str, said: &str) {
    let mut m = Machine::new();
    let (_, s1, _) = m.lose_work();
    m.kill_during(chaos::APPLY_MID_FILES, &["undo", "--json"]);
    let cut = only_operation(&m.oplog(), OperationKind::Undo);
    let id = cut.record.operation_id.as_str();

    let first = m.raptor(&m.worktree, &["timeline"], lang);
    assert!(first.status.success(), "{}", text(&first));
    let stderr = String::from_utf8_lossy(&first.stderr).into_owned();
    assert!(stderr.contains(said), "{lang}: {stderr}");
    assert!(stderr.contains(id), "{lang}: {stderr}");
    assert!(stderr.contains("raptor undo"), "{lang}: {stderr}");

    let second = m.raptor(&m.worktree, &["timeline"], lang);
    assert!(second.status.success(), "{}", text(&second));
    let stderr = String::from_utf8_lossy(&second.stderr).into_owned();
    assert!(!stderr.contains(said), "{lang}: shown twice: {stderr}");

    m.undo();
    assert_eq!(m.state(), s1);
}

#[test]
fn s2_the_cli_shows_the_notice_once_in_english() {
    cli_notice("en_US.UTF-8", "was interrupted");
}

#[test]
fn s2_the_cli_shows_the_notice_once_in_spanish() {
    cli_notice("es_ES.UTF-8", "se interrumpió");
}

/// A folder that claims feat-login's admin entry without being registered by the repo is
/// refused before anything is read or marked (the `.git` gate of the Time Machine): the notice
/// is still there for the real worktree, and nothing changed.
#[test]
fn s2_a_folder_the_repo_does_not_register_gets_no_notice() {
    let mut m = Machine::new();
    m.lose_work();
    m.kill_during(chaos::APPLY_MID_FILES, &["undo", "--json"]);
    let ghost = m.f.root.join("ghost");
    std::fs::create_dir(&ghost).unwrap();
    std::fs::write(
        ghost.join(".git"),
        format!("gitdir: {}\n", m.admin(&m.worktree).display()),
    )
    .unwrap();
    let ghost = ghost.canonicalize().unwrap();

    let before = m.fingerprint();
    match m.notices(&ghost) {
        Err(ClientError::Rpc(e)) => {
            assert_eq!(e.code, code::SCOPE_REFUSED, "{e:?}");
            let data: ScopeRefusedData = serde_json::from_value(e.data.unwrap()).unwrap();
            assert_eq!(data.reason, ScopeRefusal::NotObserved);
        }
        other => panic!("the ghost folder was not refused: {other:?}"),
    }
    assert_intact(&before, &m.fingerprint(), "ghost");
    let real = m.notices(&m.worktree).unwrap();
    assert_eq!(notice_list(&real).len(), 1, "{real}");
}

/// The notice only reaches a client that passes the `.git` gate of feat-login itself: a client
/// of another repo observed by the same daemon gets nothing, and nothing is marked delivered.
#[test]
fn s2_another_repo_gets_no_notice() {
    let mut m = Machine::new();
    m.lose_work();
    m.kill_during(chaos::APPLY_MID_FILES, &["undo", "--json"]);

    m.stop();
    let other = m.f.root.join("other");
    std::fs::create_dir(&other).unwrap();
    m.f.git_in(&other, &["init", "-q", "-b", "main"]);
    std::fs::write(other.join("c.txt"), "gamma\n").unwrap();
    m.f.git_in(&other, &["add", "c.txt"]);
    m.f.git_in(&other, &["commit", "-q", "-m", "other"]);
    let other = other.canonicalize().unwrap();
    let (mut profile, _) = Profile::open(m.dirs()).unwrap();
    profile.add_repo(&other.join(".git"), None, 1).unwrap();
    drop(profile);
    m.start(None);

    let before = m.fingerprint();
    let answer = m
        .notices(&other)
        .expect("timemachine.notices from the other repo");
    assert!(notice_list(&answer).is_empty(), "{answer}");
    assert_intact(&before, &m.fingerprint(), "other repo");
    let real = m.notices(&m.worktree).unwrap();
    assert_eq!(notice_list(&real).len(), 1, "{real}");
}

// ----- Scenario 3: a failure detected half-way ----------------------------------------------

/// The second file of an undo cannot be written (its folder refuses writes, as a file another
/// program holds would on Windows): the undo stops `interrupted`, what it already wrote stays
/// (no automatic rollback), the requester is told that `raptor undo` returns to the state
/// before, and once the folder is writable again `raptor undo` does exactly that.
#[test]
fn s3_a_write_refused_half_way_interrupts_without_rollback() {
    let mut m = Machine::new();
    let (s0, s1, _) = m.lose_work();
    let src = m.worktree.join("src");
    std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o555)).unwrap();

    let out = m.raptor(&m.worktree, &["undo"], "en_US.UTF-8");
    assert!(!out.status.success(), "{}", text(&out));
    let said = text(&out);
    assert!(said.contains("interrupted"), "{said}");
    assert!(said.contains("raptor undo"), "{said}");
    // Applied in order, docs/ first: that write stays, the refused one never happened.
    assert_eq!(
        std::fs::read(m.worktree.join("docs/a.txt")).unwrap(),
        WORK_A
    );
    assert_eq!(std::fs::read(m.worktree.join("src/b.txt")).unwrap(), BASE_B);

    let cut = only_operation(&m.oplog(), OperationKind::Undo);
    assert_eq!(cut.state, OperationState::Interrupted);
    let timeline = m.timeline();
    assert!(
        timeline.entries.iter().any(|e| matches!(
            &e.origin,
            EntryOrigin::Operation { operation_id, kind: TimelineOperationKind::Undo, state: TimelineOperationState::Interrupted, .. }
                if *operation_id == cut.record.operation_id
        )),
        "{timeline:#?}"
    );
    let shown = m.raptor(&m.worktree, &["timeline"], "en_US.UTF-8");
    assert!(text(&shown).contains("interrupted"), "{}", text(&shown));

    std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o755)).unwrap();
    let back = m.undo();
    assert_eq!(back.undone_operation_id, cut.record.operation_id);
    assert_eq!(m.state(), s1);
    m.undo();
    assert_eq!(m.state(), s0);
}

// ----- Scenario 4: only the own Git lock is released at start -------------------------------

/// The daemon dies with its own `index.lock` taken in feat-login; meanwhile another Git holds its
/// own `index.lock` in feat-pagos. At the next start the own lock is released without changing a
/// file, and the foreign one stays, the same file with the same bytes.
#[test]
fn s4_only_the_own_git_lock_is_released_at_start() {
    let mut m = Machine::new();
    let (_, s1, _) = m.lose_work();
    m.stop();
    m.start(Some("apply:step-4"));
    let out = m.raptor(&m.worktree, &["undo", "--json"], "en_US.UTF-8");
    assert!(!out.status.success(), "{}", text(&out));
    m.assert_killed("apply:step-4");
    let own = m.admin(&m.worktree).join("index.lock");
    assert!(own.exists(), "the undo died without its own lock");

    let foreign = m.admin(&m.pagos).join("index.lock");
    std::fs::write(&foreign, FOREIGN_LOCK).unwrap();
    let foreign_ino = std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(&foreign).unwrap());
    // What the start may change: the undo itself probed its worktree before it died.
    let before = m.fingerprint();

    m.start(None);
    assert_eq!(m.git_locks(), vec![foreign.clone()]);
    assert_eq!(std::fs::read(&foreign).unwrap(), FOREIGN_LOCK);
    assert_eq!(
        std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(&foreign).unwrap()),
        foreign_ino
    );
    assert_intact(&before, &m.fingerprint(), "own lock released");
    std::fs::remove_file(&foreign).unwrap();
    assert_eq!(m.state(), s1);
}

// ----- Scenario 6: no interruption, no notice ------------------------------------------------

/// An orderly stop with nothing half-done: at the next start the client receives no notice of
/// recovery, nothing changed, and the history is the same as before, complete.
#[test]
fn s6_a_clean_restart_gives_no_notice_and_a_complete_history() {
    let mut m = Machine::new();
    let (s0, _, _) = m.lose_work();
    let known = m.snapshot_ids();
    let undo = m.undo();
    assert_eq!(m.state(), s0);
    // The anchor of the undo: its echo is settled before the history is read.
    m.captured(&s0, &known);
    let history = m.timeline();
    assert!(
        history.entries.iter().any(|e| matches!(
            &e.origin,
            EntryOrigin::Operation { operation_id, state: TimelineOperationState::Finished, .. }
                if *operation_id == undo.operation_id
        )),
        "{history:#?}"
    );
    assert!(
        history.entries.iter().any(|e| matches!(
            &e.origin,
            EntryOrigin::GitEvent {
                kind: GitEventKind::Reset,
                ..
            }
        )),
        "{history:#?}"
    );
    let before = m.fingerprint();

    m.stop();
    m.start(None);
    let after = m.timeline();
    let ids = |t: &TimelineResult| -> Vec<(String, EntryOrigin)> {
        t.entries
            .iter()
            .map(|e| (e.id.clone(), e.origin.clone()))
            .collect()
    };
    assert_eq!(ids(&after), ids(&history));
    assert_intact(&before, &m.fingerprint(), "clean restart");
    let oplog = m.oplog();
    assert!(
        oplog
            .operations(&Default::default())
            .unwrap()
            .iter()
            .all(|op| {
                !matches!(
                    op.state,
                    OperationState::Interrupted | OperationState::Aborted
                )
            })
    );
    assert!(oplog.notices().unwrap().is_empty());

    let answer = m
        .notices(&m.worktree)
        .expect("timemachine.notices after a clean restart");
    assert!(notice_list(&answer).is_empty(), "{answer}");
    let shown = m.raptor(&m.worktree, &["timeline"], "en_US.UTF-8");
    assert!(shown.status.success(), "{}", text(&shown));
    assert!(
        !String::from_utf8_lossy(&shown.stderr).contains("interrupted"),
        "{}",
        text(&shown)
    );
}
