//! The undo by agent of a reset: its change is what differs between the point before it and its
//! base, read from the store. Child module of `timemachine::undo_agent`, mounted there as
//! `mod more_tests`. Temporary repo and profile only.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use gitraptor_api::actor::{Actor, AgentKind, AgentOrigin};
use gitraptor_api::messages::GitEventKind;
use gitraptor_api::timemachine::TmRejectReason;
use gitraptor_git::resolve::{self, Resolution, ResolveConfig};
use gitraptor_git::{Invoker, SystemGit};
use gitraptor_testkit::Fixture;

use crate::channel::marks::ExecutorMarks;
use crate::channel::peer::{ProcError, ProcInfo, ProcSource};
use crate::channel::requester::Who;
use crate::daemon::{LogLimits, Logger};
use crate::profile::ProfileDirs;
use crate::timemachine::confirm::Confirmation;
use crate::timemachine::continuous::CaptureDeps;
use crate::timemachine::engine::{EngineLink, RawGitEvent, SessionAttribution};
use crate::timemachine::oplog::{Channel, Oplog, Requester, RequesterOrigin, SnapshotLevel};
use crate::timemachine::protected::{McpRepos, TimeMachineBackend, TmRepos};
use crate::timemachine::store::{CaptureRequest, WorktreeScope};
use crate::timemachine::undo::{
    Selection, TmRepoHandle, UndoDone, UndoEnv, UndoError, undo_selected,
};

const REPO: &str = "1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d";
const A: &str = "100:1";
const SPAN_MS: i64 = 600_000;

fn now_ms() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap()
}

fn agent() -> Requester {
    Requester::Agent {
        name: "claude-code".into(),
        origin: RequesterOrigin::Detected,
        session_id: A.into(),
    }
}

fn who() -> Who {
    Who {
        actor: Actor::Agent {
            kind: AgentKind::ClaudeCode,
            name: None,
            origin: AgentOrigin::Detected,
        },
        requester: agent(),
    }
}

struct Engine {
    events: Vec<RawGitEvent>,
}

impl EngineLink for Engine {
    fn mark(&self, _repo_id: &str) -> i64 {
        1_000
    }
    fn settle(&self, _repo_id: &str, _worktrees: &[PathBuf], _limit: Duration) -> Option<i64> {
        Some(1_000)
    }
    fn raw_events(&self, _repo_id: &str, _worktree: &Path) -> Option<Vec<RawGitEvent>> {
        Some(self.events.clone())
    }
    fn session_attribution(&self, _repo_id: &str) -> Option<SessionAttribution> {
        Some(SessionAttribution(HashMap::from([(A.to_owned(), agent())])))
    }
    fn generation_floor(&self, _repo_id: &str) -> i64 {
        0
    }
    fn activity(&self, _repo_id: &str) -> u64 {
        0
    }
}

struct NoProcs;

impl ProcSource for NoProcs {
    fn read(&self, _pid: u32) -> Result<ProcInfo, ProcError> {
        Err(ProcError::Gone)
    }
    fn foreign_to(&self, _pid: u32, _uid: u32) -> Option<bool> {
        None
    }
}

fn system_git() -> SystemGit {
    match resolve::resolve(&ResolveConfig::for_current_os(None), &Invoker::default()) {
        Resolution::Found { git, .. } => git,
        Resolution::NotFound { diagnostics } => panic!("tests need Git >= 2.38: {diagnostics:?}"),
    }
}

struct Rig {
    f: Fixture,
    dirs: ProfileDirs,
    repo: TmRepoHandle,
}

fn rig() -> Rig {
    let f = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    f.write("keep.rs", "fn keep() {}\n");
    f.git(&["add", "keep.rs"]);
    f.git(&["commit", "-q", "-m", "keep"]);
    let dirs = ProfileDirs::under_root(&f.profile);
    let common = crate::observe::locate(&f.repo).unwrap();
    let (oplog, _) = Oplog::open(&dirs, REPO, 1).unwrap();
    let repos = Arc::new(TmRepos::new(dirs.clone()));
    repos.insert(REPO, &common, oplog);
    let backend = TimeMachineBackend::new(repos, None, Arc::new(McpRepos::new([])));
    let repo = backend
        .repo_of(&f.repo)
        .expect("the temporary repo is observed");
    Rig { f, dirs, repo }
}

impl Rig {
    fn head(&self) -> String {
        self.f.git(&["rev-parse", "HEAD"]).trim().to_owned()
    }

    fn commit(&self, path: &str, content: &str) -> String {
        self.f.write(path, content);
        self.f.git(&["add", path]);
        self.f.git(&["commit", "-q", "-m", path]);
        self.head()
    }

    /// An observation point of the worktree at engine mark `mark`.
    fn point(&self, mark: i64) -> String {
        let req = CaptureRequest {
            level: SnapshotLevel::Observation,
            echo_settled: None,
            common_dir: crate::observe::locate(&self.f.repo).unwrap(),
            worktrees: vec![WorktreeScope {
                key: "main".into(),
                path: self.f.repo.clone(),
                hint: None,
            }],
            engine_mark: Some(mark),
            cause_operation: None,
            cause_event_seq: None,
            include_credentials: false,
            still_valid: None,
            give_way: None,
        };
        self.repo
            .store
            .as_ref()
            .expect("the store opens")
            .capture(&self.repo.repo.oplog, &req)
            .unwrap()
            .snapshot_id
    }

    /// The agent's `git reset --hard <to>` on main, as the engine reports it with `seq`.
    fn reset(&self, seq: i64, to: &str) -> RawGitEvent {
        let old = self.head();
        self.f.git(&["reset", "-q", "--hard", to]);
        RawGitEvent {
            seq,
            kind: GitEventKind::Reset,
            branch: Some("main".into()),
            actor: agent(),
            observed_ms: now_ms() - 1_000,
            in_gap: false,
            inferred: false,
            old_commit: Some(old),
            new_commit: Some(to.to_owned()),
        }
    }

    fn run(&self, events: Vec<RawGitEvent>, git: &SystemGit) -> Result<UndoDone, UndoError> {
        let deps = CaptureDeps {
            repos: Arc::new(TmRepos::new(self.dirs.clone())),
            engine: Arc::new(Engine { events }),
            profile: self.dirs.clone(),
            logger: Logger::open(&self.dirs.state, LogLimits::default()).unwrap(),
            layer: None,
            free_space_floor: None,
            after_mark: None,
        };
        let marks = Arc::new(ExecutorMarks::default());
        let stopping = AtomicBool::new(false);
        let invoker = Invoker::default();
        let env = UndoEnv {
            marks: &marks,
            procs: &NoProcs,
            stopping: &stopping,
            prior_deadline: Duration::from_secs(30),
            git: Some(git),
            invoker: &invoker,
            engine: Some(&deps),
            fallback_mark: 1_000,
        };
        undo_selected(
            &self.repo,
            &who(),
            Channel::Cli,
            &Confirmation::unavailable(),
            &env,
            Selection::Agent {
                selector: A.to_owned(),
                span_ms: SPAN_MS,
            },
        )
    }

    fn read(&self, rela: &str) -> Option<String> {
        std::fs::read_to_string(self.f.repo.join(rela)).ok()
    }
}

#[test]
fn a_reset_by_the_agent_is_undone_to_its_prior_subject_by_subject() {
    let r = rig();
    let first = r.commit("api.rs", "v1\n");
    let second = r.commit("api.rs", "v2\n");
    r.point(1);
    let reset = r.reset(2, &first);
    r.point(2);
    // Someone else's work after the reset: a tracked edit and a new file.
    r.f.write("keep.rs", "fn keep() { someone_else(); }\n");
    r.f.write("other.txt", "not the agent's\n");
    let git = system_git();

    let done = r
        .run(vec![reset], &git)
        .expect("the undo of the agent's reset runs");

    assert_eq!(done.undone_all.len(), 1);
    assert_eq!(
        r.head(),
        second,
        "the branch the reset moved goes back to where it was before it"
    );
    assert_eq!(
        r.read("api.rs").as_deref(),
        Some("v2\n"),
        "a path the reset changed goes back to its value before the reset"
    );
    assert_eq!(
        r.f.git(&["status", "--porcelain", "--", "api.rs"]),
        "",
        "the index agrees with the file for the path of the change"
    );
    assert_eq!(
        r.read("keep.rs").as_deref(),
        Some("fn keep() { someone_else(); }\n"),
        "another actor's edit of a path the reset did not touch is never written"
    );
    assert_eq!(
        r.read("other.txt").as_deref(),
        Some("not the agent's\n"),
        "another actor's new file is never written"
    );
}

#[test]
fn a_reset_without_a_base_is_overlap_unverified() {
    let r = rig();
    let first = r.commit("api.rs", "v1\n");
    r.commit("api.rs", "v2\n");
    r.point(1);
    // No point holds the state the reset left: its change cannot be told from what came after.
    let reset = r.reset(2, &first);
    r.f.write("other.txt", "not the agent's\n");
    let git = system_git();

    let err = r
        .run(vec![reset], &git)
        .expect_err("a reset without a base is not undone");

    let UndoError::Rejected { reason, .. } = err else {
        panic!("expected a rejection, got {err:?}");
    };
    assert_eq!(reason, TmRejectReason::OverlapUnverified);
    assert_eq!(r.head(), first, "nothing changed: the branch stays");
    assert_eq!(r.read("api.rs").as_deref(), Some("v1\n"));
    assert_eq!(r.read("other.txt").as_deref(), Some("not the agent's\n"));
}
