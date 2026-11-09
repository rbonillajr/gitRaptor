//! TMC011-SET / GUARD-4.1 / GUARD-4.2 / NOT-SELECTABLE / CHANGE (T004 of DS-US-TMC-011): which
//! operations an undo by agent takes back and what it writes. Child module of
//! `timemachine::undo_agent`, mounted there as `mod tests`.
//!
//! Temporary repo and profile only. The rejection tests give the undo no Git: the plan goes
//! through and `git-unavailable` stops it, so what is read is the rejection the undo records and
//! the `Target::Undo` it carries. The change tests give it a real Git and read the worktree.
//! The agent's work is raw Git events over real commits of the repo; their owners come from the
//! engine's `session_attribution`.

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
use crate::timemachine::oplog::{
    Channel, OpRef, OperationKind, OperationState, Oplog, Requester, RequesterOrigin, Target,
};
use crate::timemachine::protected::{McpRepos, TimeMachineBackend, TmRepos};
use crate::timemachine::undo::{
    Selection, TmRepoHandle, UndoDone, UndoEnv, UndoError, undo_selected,
};

const REPO: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";
const A: &str = "100:1";
const B: &str = "200:1";
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

fn agent(name: &str, session: &str) -> Requester {
    Requester::Agent {
        name: name.into(),
        origin: RequesterOrigin::Detected,
        session_id: session.into(),
    }
}

fn who(name: &str, session: &str) -> Who {
    Who {
        actor: Actor::Agent {
            kind: AgentKind::ClaudeCode,
            name: None,
            origin: AgentOrigin::Detected,
        },
        requester: agent(name, session),
    }
}

fn attribution(entries: &[(&str, &str)]) -> SessionAttribution {
    SessionAttribution(HashMap::from_iter(
        entries
            .iter()
            .map(|(session, name)| ((*session).to_owned(), agent(name, session))),
    ))
}

struct Engine {
    events: Vec<RawGitEvent>,
    sessions: Option<SessionAttribution>,
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
        self.sessions.clone()
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

fn event(seq: i64, branch: &str, session: &str, name: &str, ago_ms: i64) -> RawGitEvent {
    RawGitEvent {
        seq,
        kind: GitEventKind::Commit,
        branch: Some(branch.into()),
        actor: agent(name, session),
        observed_ms: now_ms() - ago_ms,
        in_gap: false,
        inferred: false,
        old_commit: Some("a".repeat(40)),
        new_commit: Some("b".repeat(40)),
    }
}

impl Rig {
    fn head(&self) -> String {
        self.f.git(&["rev-parse", "HEAD"]).trim().to_owned()
    }

    /// A real commit on main by the agent: `files` are written, everything is committed.
    /// Returns `(old, new)`.
    fn commit(&self, files: &[(&str, &str)], remove: &[&str], message: &str) -> (String, String) {
        let old = self.head();
        for (path, content) in files {
            self.f.write(path, content);
        }
        for path in remove {
            self.f.git(&["rm", "-q", path]);
        }
        self.f.git(&["add", "-A"]);
        self.f.git(&["commit", "-q", "-m", message]);
        (old, self.head())
    }

    fn real_event(&self, seq: i64, commits: &(String, String), ago_ms: i64) -> RawGitEvent {
        RawGitEvent {
            old_commit: Some(commits.0.clone()),
            new_commit: Some(commits.1.clone()),
            ..event(seq, "main", A, "claude-code", ago_ms)
        }
    }

    fn deps(&self, events: Vec<RawGitEvent>, sessions: Option<SessionAttribution>) -> CaptureDeps {
        CaptureDeps {
            repos: Arc::new(TmRepos::new(self.dirs.clone())),
            engine: Arc::new(Engine { events, sessions }),
            profile: self.dirs.clone(),
            logger: Logger::open(&self.dirs.state, LogLimits::default()).unwrap(),
            layer: None,
            free_space_floor: None,
            after_mark: None,
            seals: Default::default(),
        }
    }

    fn run(
        &self,
        requester: &Who,
        events: Vec<RawGitEvent>,
        sessions: Option<SessionAttribution>,
        selector: &str,
        git: Option<&SystemGit>,
    ) -> Result<UndoDone, UndoError> {
        let deps = self.deps(events, sessions);
        let marks = Arc::new(ExecutorMarks::default());
        let stopping = AtomicBool::new(false);
        let invoker = Invoker::default();
        let env = UndoEnv {
            marks: &marks,
            procs: &NoProcs,
            stopping: &stopping,
            prior_deadline: Duration::from_secs(30),
            git,
            invoker: &invoker,
            engine: Some(&deps),
            fallback_mark: 1_000,
        };
        undo_selected(
            &self.repo,
            requester,
            Channel::Cli,
            &Confirmation::unavailable(),
            &env,
            Selection::Agent {
                selector: selector.to_owned(),
                span_ms: SPAN_MS,
            },
        )
    }

    /// The rejection (Git is not given) and the set the oplog recorded for it.
    fn attempt(
        &self,
        requester: &Who,
        events: Vec<RawGitEvent>,
        sessions: Option<SessionAttribution>,
        selector: &str,
    ) -> (TmRejectReason, Vec<OpRef>) {
        let err = self
            .run(requester, events, sessions, selector, None)
            .expect_err("no Git was given: the undo cannot finish");
        let UndoError::Rejected {
            reason,
            operation_id,
            ..
        } = err
        else {
            panic!("expected a rejection, got {err:?}");
        };
        let view = self
            .repo
            .repo
            .oplog
            .lock()
            .unwrap()
            .operation(&operation_id.expect("a rejection is recorded"))
            .unwrap()
            .expect("the rejection is in the oplog");
        assert_eq!(view.record.kind, OperationKind::Undo);
        assert_eq!(view.state, OperationState::Rejected);
        let Target::Undo(set) = view.record.target else {
            panic!("a rejected undo records Target::Undo");
        };
        (reason, set)
    }

    fn read(&self, rela: &str) -> Option<String> {
        std::fs::read_to_string(self.f.repo.join(rela)).ok()
    }
}

#[test]
fn the_set_is_the_agents_operations_in_the_period() {
    let r = rig();
    let first = r.commit(&[("api.rs", "v1\n")], &[], "one");
    let second = r.commit(&[("api.rs", "v2\n")], &[], "two");
    let events = vec![
        // The agent's, but an hour old: outside the period.
        event(1, "old-branch", A, "claude-code", 3_600_000),
        r.real_event(2, &first, 2_000),
        r.real_event(3, &second, 1_000),
    ];
    let sessions = attribution(&[(A, "claude-code")]);

    let (reason, set) = r.attempt(&who("claude-code", A), events, Some(sessions), A);

    assert_eq!(
        reason,
        TmRejectReason::GitUnavailable,
        "TMC011-SET: the plan goes through and only the missing Git stops it"
    );
    assert_eq!(
        set,
        vec![OpRef::GitEvent(2), OpRef::GitEvent(3)],
        "TMC011-SET: the set is the agent's operations with a moment in the period, in stack order"
    );
}

#[test]
fn unattributed_and_other_agents_never_enter() {
    let r = rig();
    let first = r.commit(&[("api.rs", "v1\n")], &[], "one");
    let mut unattributed = event(3, "u-branch", A, "claude-code", 1_500);
    unattributed.actor = Requester::Unattributed;
    let events = vec![
        r.real_event(1, &first, 2_000),
        event(2, "b-branch", B, "codex-1", 1_800),
        unattributed,
    ];
    let sessions = attribution(&[(A, "claude-code"), (B, "codex-1")]);

    let (_, set) = r.attempt(
        &who("claude-code", A),
        events.clone(),
        Some(sessions.clone()),
        "claude-code",
    );
    assert_eq!(
        set,
        vec![OpRef::GitEvent(1)],
        "TMC011-SET: neither the other agent's work nor the unattributed one enters the set"
    );

    // Nothing of the agent in the period: the others do not become the set.
    let only_others = events[1..].to_vec();
    let (reason, set) = r.attempt(
        &Who::unattributed(),
        only_others,
        Some(sessions),
        "claude-code",
    );
    assert_eq!(
        reason,
        TmRejectReason::NothingToUndo,
        "TMC011-SET: with nothing of the agent the undo says so"
    );
    assert!(set.is_empty(), "{set:?}");
}

#[test]
fn owners_come_from_one_attribution_read() {
    let r = rig();
    let first = r.commit(&[("api.rs", "v1\n")], &[], "one");
    // The event was frozen as claude-code, but the one read of the attribution now says the
    // session is codex-1 (a correction): the owner is what that read says.
    let events = vec![r.real_event(1, &first, 2_000)];
    let sessions = attribution(&[(A, "codex-1")]);

    let (reason, set) = r.attempt(
        &who("codex-1", A),
        events.clone(),
        Some(sessions.clone()),
        "codex-1",
    );
    assert_eq!(reason, TmRejectReason::GitUnavailable);
    assert_eq!(
        set,
        vec![OpRef::GitEvent(1)],
        "TMC011-SET: the event is codex-1's by the current attribution, not claude-code's by its \
         stale actor"
    );

    let (reason, set) = r.attempt(&Who::unattributed(), events, Some(sessions), "claude-code");
    assert_eq!(
        reason,
        TmRejectReason::NothingToUndo,
        "TMC011-SET: claude-code no longer owns that session's work"
    );
    assert!(set.is_empty(), "{set:?}");
}

#[test]
fn an_agent_naming_another_is_rejected_before_the_set() {
    let r = rig();
    let first = r.commit(&[("api.rs", "v1\n")], &[], "one");
    // B has work in the period; A asks for B's.
    let mut theirs = r.real_event(1, &first, 2_000);
    theirs.actor = agent("claude-code", B);
    let sessions = attribution(&[(A, "claude-code"), (B, "claude-code")]);

    let (reason, set) = r.attempt(&who("claude-code", A), vec![theirs], Some(sessions), B);

    assert_eq!(
        reason,
        TmRejectReason::OtherActor,
        "TMC011-GUARD-4.1: an agent that names another gets other-actor"
    );
    assert!(
        set.is_empty(),
        "TMC011-GUARD-4.1: the rejection is recorded before the set is computed (empty \
         Target::Undo), so it does not tell whether the other agent has work: {set:?}"
    );
}

#[test]
fn a_gap_event_never_enters() {
    let r = rig();
    let first = r.commit(&[("api.rs", "v1\n")], &[], "one");
    let mut gap = r.real_event(2, &first, 1_000);
    gap.in_gap = true;
    let sessions = attribution(&[(A, "claude-code")]);

    let ok_and_gap = vec![r.real_event(1, &first, 2_000), gap.clone()];
    let (_, set) = r.attempt(
        &who("claude-code", A),
        ok_and_gap,
        Some(sessions.clone()),
        A,
    );
    assert_eq!(
        set,
        vec![OpRef::GitEvent(1)],
        "TMC011-GUARD-4.2: an event with in_gap never enters, even carrying the agent's session"
    );

    let (reason, set) = r.attempt(&who("claude-code", A), vec![gap], Some(sessions), A);
    assert_eq!(
        reason,
        TmRejectReason::NothingToUndo,
        "TMC011-GUARD-4.2: with only a gap event there is nothing to undo"
    );
    assert!(set.is_empty(), "{set:?}");
}

#[test]
fn an_inferred_worktree_event_is_not_selectable() {
    let r = rig();
    let first = r.commit(&[("api.rs", "v1\n")], &[], "one");
    let mut inferred = r.real_event(1, &first, 2_000);
    inferred.inferred = true;
    let sessions = attribution(&[(A, "claude-code")]);

    let (reason, _) = r.attempt(&who("claude-code", A), vec![inferred], Some(sessions), A);

    assert_eq!(
        reason,
        TmRejectReason::NotSelectable,
        "TMC011-NOT-SELECTABLE: an event whose worktree the engine inferred rejects the whole \
         request: it is not known to be this worktree's"
    );
}

#[test]
fn a_commit_change_is_its_tree_diff() {
    let r = rig();
    let main_before = r.head();
    let commits = r.commit(
        &[("new.rs", "fresh\n"), ("a.txt", "alpha2\n")],
        &["b.txt"],
        "agent",
    );
    // Someone else's uncommitted file: not part of any change.
    r.f.write("mine.txt", "not the agent's\n");
    let git = system_git();
    let events = vec![r.real_event(1, &commits, 1_000)];

    let done = r
        .run(
            &who("claude-code", A),
            events,
            Some(attribution(&[(A, "claude-code")])),
            A,
            Some(&git),
        )
        .expect("TMC011-CHANGE: the undo of the agent's commit runs");

    assert_eq!(done.undone_all.len(), 1);
    assert_eq!(
        r.read("new.rs"),
        None,
        "TMC011-CHANGE: a path the commit added goes away"
    );
    assert_eq!(
        r.read("a.txt").as_deref(),
        Some("alpha\n"),
        "TMC011-CHANGE: a path the commit modified goes back to its value before"
    );
    assert_eq!(
        r.read("b.txt").as_deref(),
        Some("beta\n"),
        "TMC011-CHANGE: a path the commit deleted comes back"
    );
    assert_eq!(
        r.head(),
        main_before,
        "TMC011-CHANGE: the commit's branch goes back to old_commit"
    );
    assert_eq!(
        r.f.git(&["status", "--porcelain", "--", "a.txt", "b.txt", "new.rs"]),
        "",
        "TMC011-CHANGE: the index agrees with the files for every path of the change"
    );
    assert_eq!(
        r.read("mine.txt").as_deref(),
        Some("not the agent's\n"),
        "TMC011-CHANGE: a path outside the change is never written"
    );
}

#[test]
fn the_target_of_a_path_is_before_the_oldest_change() {
    let r = rig();
    let main_before = r.head();
    let first = r.commit(&[("api.rs", "v1\n"), ("b.txt", "beta1\n")], &[], "one");
    let second = r.commit(&[("api.rs", "v2\n"), ("a.txt", "alpha2\n")], &[], "two");
    r.f.write("keep.rs", "fn keep() { someone_else(); }\n");
    let git = system_git();
    let events = vec![
        r.real_event(1, &first, 2_000),
        r.real_event(2, &second, 1_000),
    ];

    let done = r
        .run(
            &who("claude-code", A),
            events,
            Some(attribution(&[(A, "claude-code")])),
            A,
            Some(&git),
        )
        .expect("TMC011-CHANGE: the undo of both commits runs");

    assert_eq!(done.undone_all.len(), 2);
    assert_eq!(
        r.read("api.rs"),
        None,
        "TMC011-CHANGE: api.rs did not exist before the oldest change that touched it"
    );
    assert_eq!(
        r.read("b.txt").as_deref(),
        Some("beta\n"),
        "TMC011-CHANGE: a path only the first commit touched goes back to before the first"
    );
    assert_eq!(
        r.read("a.txt").as_deref(),
        Some("alpha\n"),
        "TMC011-CHANGE: a path only the second commit touched goes back to before the second"
    );
    assert_eq!(
        r.head(),
        main_before,
        "TMC011-CHANGE: the branch goes back to before the first commit"
    );
    assert_eq!(
        r.read("keep.rs").as_deref(),
        Some("fn keep() { someone_else(); }\n"),
        "TMC011-CHANGE: a path the agent did not touch keeps its edit"
    );
}
