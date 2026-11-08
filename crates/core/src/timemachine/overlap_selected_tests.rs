//! TMC011-OVERLAP-SUBJECT (T006 of DS-US-TMC-011): `overlap::check_selected` and
//! `check_selected_tail` over real captures of a temporary repo and synthetic raw events (NFR-01).
//! Child module of `timemachine::overlap`, mounted there as `mod selected_tests`.
//!
//! Marks: the agent's first change has its base at mark 5 and its commits are events 6 and 7; a
//! point captured at mark 20 shows the state "now". An event of another actor between the
//! base and the point has seq 8 (after) or 3 (before).

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::*;
use crate::profile::ProfileDirs;
use crate::timemachine::engine::RawGitEvent;
use crate::timemachine::oplog::{
    OpRef, Oplog, Requester, RequesterOrigin, SnapshotLevel,
};
use crate::timemachine::store::{CaptureRequest, SnapshotStore, WorktreeScope};
use crate::timemachine::undo_agent::{Subject as Subj, Value};
use gitraptor_api::messages::GitEventKind;
use gitraptor_git::tm_write::files::Kind;
use gitraptor_git::Oid;
use gitraptor_testkit::Fixture;

const REPO_ID: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";
const KEY: &str = "main";
const MAIN: &str = "refs/heads/main";

fn actor(session: &str) -> Requester {
    Requester::Agent {
        name: "claude-code".into(),
        origin: RequesterOrigin::Detected,
        session_id: session.into(),
    }
}

struct World {
    f: Fixture,
    store: SnapshotStore,
    oplog: Mutex<Oplog>,
}

impl World {
    fn new() -> Self {
        let f = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
        f.write("api.rs", "v0\n");
        f.git(&["add", "api.rs"]);
        f.git(&["commit", "-q", "-m", "api"]);
        let dirs = ProfileDirs::under_root(&f.profile);
        let (store, _) = SnapshotStore::open_or_create(&dirs, REPO_ID).unwrap();
        let (oplog, _) = Oplog::open(&dirs, REPO_ID, 1).unwrap();
        Self {
            f,
            store,
            oplog: Mutex::new(oplog),
        }
    }

    /// The agent's commit: `api.rs` becomes `content`, committed on main.
    fn agent_commit(&self, content: &str) {
        self.f.write("api.rs", content);
        self.f.git(&["add", "api.rs"]);
        self.f.git(&["commit", "-q", "-m", "agent"]);
    }

    fn point(&self, mark: i64) -> String {
        let req = CaptureRequest {
            level: SnapshotLevel::Observation,
            echo_settled: None,
            common_dir: crate::observe::locate(&self.f.repo).unwrap(),
            worktrees: vec![WorktreeScope {
                key: KEY.into(),
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
        self.store.capture(&self.oplog, &req).unwrap().snapshot_id
    }

    fn blob(&self, content: &str) -> Oid {
        std::fs::write(self.f.repo.join("blob.tmp"), content).unwrap();
        let id = self.f.git(&["hash-object", "-w", "blob.tmp"]);
        std::fs::remove_file(self.f.repo.join("blob.tmp")).unwrap();
        Oid::from_hex(id.trim()).unwrap()
    }

    fn head(&self) -> Oid {
        Oid::from_hex(self.f.git(&["rev-parse", "HEAD"]).trim()).unwrap()
    }

    /// The final value of `api.rs` for an agent that committed `content`.
    fn api_final(&self, content: &str) -> Value {
        let entry = Some((Kind::File, self.blob(content)));
        Value::Entry {
            file: entry,
            index: entry,
        }
    }

    fn check(&self, input: &Inputs<'_>) -> OverlapReport {
        let selected = SelectedInput {
            worktree_key: KEY,
            finals: &input.finals,
            first_base: &input.first_base,
            set: &input.set,
            events: &input.events,
            last_point: input.last_point.as_deref(),
            deadline: Instant::now() + Duration::from_secs(60),
        };
        match &input.prior {
            Some(prior) => check_selected_tail(&self.store, &selected, prior),
            None => check_selected(&self.store, &selected),
        }
    }
}

struct Inputs<'a> {
    finals: BTreeMap<Subj, (Value, i64)>,
    first_base: BTreeMap<String, i64>,
    set: Vec<OpRef>,
    events: Vec<RawGitEvent>,
    last_point: Option<String>,
    prior: Option<String>,
    _w: std::marker::PhantomData<&'a ()>,
}

impl Inputs<'_> {
    fn new() -> Self {
        Self {
            finals: BTreeMap::new(),
            first_base: BTreeMap::new(),
            set: vec![OpRef::GitEvent(6), OpRef::GitEvent(7)],
            events: Vec::new(),
            last_point: None,
            prior: None,
            _w: std::marker::PhantomData,
        }
    }

    fn path(mut self, value: Value, mark: i64) -> Self {
        self.finals.insert(
            Subj::Path {
                worktree_key: KEY.into(),
                path: "api.rs".into(),
            },
            (value, mark),
        );
        self
    }

    fn branch(mut self, tip: Oid, mark: i64) -> Self {
        self.finals
            .insert(Subj::Ref(MAIN.into()), (Value::Ref(Some(tip)), mark));
        self.first_base.insert(MAIN.into(), 5);
        self
    }
}

fn commit_event(seq: i64, who: Requester) -> RawGitEvent {
    RawGitEvent {
        seq,
        kind: GitEventKind::Commit,
        branch: Some("main".into()),
        actor: who,
        observed_ms: 0,
        in_gap: false,
        inferred: false,
        old_commit: None,
        new_commit: None,
    }
}

fn on_path(report: &OverlapReport) -> usize {
    report
        .conflicts
        .iter()
        .filter(|c| matches!(&c.subject, Subject::Path { path, .. } if path == "api.rs"))
        .count()
}

fn on_branch(report: &OverlapReport) -> usize {
    report
        .conflicts
        .iter()
        .filter(|c| matches!(&c.subject, Subject::Ref(name) if name.ends_with("main")))
        .count()
}

#[test]
fn an_edit_after_the_agents_last_change_conflicts() {
    let w = World::new();
    w.agent_commit("v1\n");
    let final_api = w.api_final("v1\n");
    // Someone edits the file after the agent's last commit (mark 7).
    w.f.write("api.rs", "v2-by-someone-else\n");
    let point = w.point(20);

    let mut input = Inputs::new().path(final_api, 7);
    input.last_point = Some(point);
    let report = w.check(&input);

    assert_eq!(
        on_path(&report),
        1,
        "TMC011-OVERLAP-SUBJECT: an edit after the agent's last change is a conflict: {report:#?}"
    );
    assert!(report.stops());
    assert!(!report.unverified, "the check was possible");
}

#[test]
fn the_agents_own_commits_on_one_file_do_not_conflict() {
    let w = World::new();
    w.agent_commit("v1\n");
    w.agent_commit("v2\n");
    let final_api = w.api_final("v2\n");
    let tip = w.head();
    let point = w.point(20);

    let mut input = Inputs::new().path(final_api, 7).branch(tip, 7);
    input.events = vec![commit_event(6, actor("s1")), commit_event(7, actor("s1"))];
    input.last_point = Some(point);
    let report = w.check(&input);

    assert!(
        report.conflicts.is_empty() && !report.unverified,
        "TMC011-OVERLAP-SUBJECT: the agent's own commits on one file (and its branch moves) \
         never overlap with themselves: {report:#?}"
    );
}

#[test]
fn another_actors_commit_on_the_branch_after_the_first_change_conflicts() {
    let w = World::new();
    w.agent_commit("v1\n");
    let final_api = w.api_final("v1\n");
    let tip = w.head();
    let point = w.point(20);

    let mut input = Inputs::new().path(final_api, 7).branch(tip, 7);
    input.events = vec![
        commit_event(6, actor("s1")),
        // Another actor's commit between the agent's: the MVP does not rewrite history (D-TMC-34).
        commit_event(8, Requester::Unattributed),
        commit_event(7, actor("s1")),
    ];
    input.last_point = Some(point);
    let report = w.check(&input);

    assert_eq!(
        on_branch(&report),
        1,
        "TMC011-OVERLAP-SUBJECT: another actor's commit on the branch after the agent's first \
         change is a conflict on the branch: {report:#?}"
    );
    assert_eq!(on_path(&report), 0, "the file itself is as the agent left it");
}

#[test]
fn an_earlier_change_of_another_actor_does_not_conflict() {
    let w = World::new();
    w.agent_commit("v1\n");
    let final_api = w.api_final("v1\n");
    let tip = w.head();
    let point = w.point(20);

    let mut input = Inputs::new().path(final_api, 7).branch(tip, 7);
    input.events = vec![
        // Before the base of the agent's first change (mark 5): not counted.
        commit_event(3, Requester::Unattributed),
        commit_event(6, actor("s1")),
        commit_event(7, actor("s1")),
    ];
    input.last_point = Some(point);
    let report = w.check(&input);

    assert!(
        report.conflicts.is_empty() && !report.unverified,
        "TMC011-OVERLAP-SUBJECT: what another actor did before the agent's first change does \
         not conflict: {report:#?}"
    );
}

#[test]
fn a_point_older_than_the_last_change_defers_to_phase_2() {
    let w = World::new();
    // A point at mark 3 still shows `v0`; the agent's last change (mark 7) is later than it.
    let old_point = w.point(3);
    w.agent_commit("v1\n");
    let final_api = w.api_final("v1\n");

    let mut input = Inputs::new().path(final_api.clone(), 7);
    input.last_point = Some(old_point);
    let phase1 = w.check(&input);
    assert!(
        phase1.conflicts.is_empty() && !phase1.unverified,
        "TMC011-OVERLAP-SUBJECT: a point older than the agent's last change has not got its \
         final value yet: phase 1 must leave the subject for phase 2, not report a false \
         conflict: {phase1:#?}"
    );

    // Phase 2, against the prior: the state is as the agent left it.
    let prior = w.point(20);
    let mut tail = Inputs::new().path(final_api.clone(), 7);
    tail.prior = Some(prior);
    let ok = w.check(&tail);
    assert!(
        ok.conflicts.is_empty() && !ok.unverified,
        "TMC011-OVERLAP-SUBJECT: phase 2 compares every subject against the prior: {ok:#?}"
    );

    // Phase 2 with an edit in between does stop.
    w.f.write("api.rs", "v3-late-edit\n");
    let prior = w.point(30);
    let mut late = Inputs::new().path(final_api, 7);
    late.prior = Some(prior);
    let stops = w.check(&late);
    assert_eq!(
        on_path(&stops),
        1,
        "TMC011-OVERLAP-SUBJECT: phase 2 reports the edit the deferred subject missed: {stops:#?}"
    );
}
