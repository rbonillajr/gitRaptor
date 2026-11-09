//! TMC011-ATTRIBUTION (T002 of DS-US-TMC-011): what the planner reads from the engine. Child
//! module of `daemon::tm` (`use super::*`), mounted there with
//! `#[cfg(test)] #[path = "tm_attribution_tests.rs"] mod attribution_tests;`.
//!
//! A real `Daemon` over a temporary repo and profile (NFR-01), started and not run: the store is
//! seeded through its own write batch, then the loop-side reads (`Daemon::raw_events`,
//! `Daemon::session_attribution`) are called as the daemon loop calls them.

use std::time::Duration;

use super::*;
use crate::daemon::{DaemonConfig, DaemonEnv, LogLimits};
use crate::profile::{
    Agent, AgentKind, Author, GapCause, Origin, ProfileDirs, RecordKind, Timestamp,
};
use crate::profile::{NewEvent, WriteOp};
use crate::timemachine::engine::SessionAttribution;
use gitraptor_testkit::Fixture;

const OLD: &str = "1111111111111111111111111111111111111111";
const NEW: &str = "2222222222222222222222222222222222222222";

struct Rig {
    daemon: Daemon,
    repo_id: String,
    worktree: PathBuf,
    _fx: Fixture,
    _tmp: tempfile::TempDir,
}

fn rig() -> Rig {
    let fx = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    // `canonical_dir`: on Windows `canonicalize` gives a verbatim `\\?\C:\…` path, which the
    // profile refuses (`InvalidPath`); the testkit strips that prefix.
    let worktree = gitraptor_testkit::fixture::canonical_dir(&fx.repo);
    let tmp = tempfile::tempdir().unwrap();
    let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
    let (mut profile, _) = crate::profile::Profile::open(dirs.clone()).unwrap();
    let (entry, _) = profile
        .add_repo(
            &gitraptor_testkit::fixture::canonical_dir(&worktree.join(".git")),
            None,
            1,
        )
        .unwrap();
    drop(profile);
    let env = DaemonEnv::from_vars(std::env::vars_os());
    let daemon = Daemon::start(DaemonConfig {
        dirs,
        git: env.git_resolve_config(None),
        env,
        heartbeat: Duration::from_secs(3600),
        log: LogLimits::default(),
        stop_deadline: None,
        channel: Default::default(),
        protected: None,
        operations: None,
        tm_prior_layer: None,
        tiers: Default::default(),
        discovery: Default::default(),
        power: Default::default(),
        predict: Default::default(),
        tm_capture: Default::default(),
    })
    .unwrap();
    Rig {
        daemon,
        repo_id: entry.repo_id,
        worktree,
        _fx: fx,
        _tmp: tmp,
    }
}

fn ts(ms: i64) -> Timestamp {
    Timestamp {
        utc_ms: ms,
        offset_s: 0,
    }
}

impl Rig {
    fn store(&mut self) -> &mut RepoStore {
        let id = self.repo_id.clone();
        &mut self
            .daemon
            .stores
            .iter_mut()
            .find(|(repo, _)| *repo == id)
            .expect("the repo's store is open")
            .1
    }

    fn write(&mut self, ops: &[WriteOp]) {
        self.store().write_batch(ops).unwrap();
    }

    fn start_session(&mut self, id: &str) {
        let worktree = self.worktree.clone();
        self.write(&[
            WriteOp::UpsertWorktree {
                path: worktree.clone(),
                admin_name: None,
                seen_ms: 1,
            },
            WriteOp::StartSession {
                session_id: id.into(),
                worktree,
                agent: Agent {
                    kind: AgentKind::ClaudeCode,
                    name: None,
                },
                origin: Origin::Detected,
                detection_key: Some(id.into()),
                started_ms: 1,
            },
        ]);
    }

    fn event(&self, metadata: &str, session: Option<&str>, gap: Option<&str>) -> WriteOp {
        WriteOp::AppendEvent(NewEvent {
            worktree: self.worktree.clone(),
            kind: "commit".into(),
            metadata: metadata.into(),
            observed: ts(1_000),
            session_id: session.map(str::to_owned),
            evidence: None,
            gap_id: gap.map(str::to_owned),
            authorship: None,
        })
    }

    fn raw(&self) -> Vec<RawGitEvent> {
        self.daemon
            .raw_events(&self.repo_id, &self.worktree)
            .expect("the repo's events are readable")
    }
}

fn agent_name(requester: &Requester) -> Option<&str> {
    match requester {
        Requester::Agent { name, .. } => Some(name),
        Requester::Unattributed => None,
    }
}

/// TMC011-ATTRIBUTION: the gap, the inferred worktree and both commits reach the planner.
#[test]
fn raw_events_carry_the_gap_and_the_commits() {
    let mut r = rig();
    r.start_session("100:1");
    let plain = format!(r#"{{"branch":"main","old_commit":"{OLD}","new_commit":"{NEW}"}}"#);
    let inferred = format!(
        r#"{{"branch":"main","old_commit":"{OLD}","new_commit":"{NEW}","worktree_inferred":true}}"#
    );
    r.write(&[
        WriteOp::OpenGap {
            gap_id: "gap-1".into(),
            started_ms: 10,
            cause: GapCause::DaemonDown,
            requested_by: None,
        },
        WriteOp::CloseGap {
            gap_id: "gap-1".into(),
            ended_ms: 20,
        },
    ]);
    let ops = [
        r.event(&plain, Some("100:1"), None),
        r.event(&inferred, Some("100:1"), None),
        r.event(&plain, Some("100:1"), Some("gap-1")),
        r.event(r#"{"branch":"main"}"#, None, None),
    ];
    r.write(&ops);

    let events = r.raw();
    let mine: Vec<&RawGitEvent> = events
        .iter()
        .filter(|e| e.branch.as_deref() == Some("main") && e.kind == GitEventKind::Commit)
        .collect();
    assert!(
        mine.len() >= 4,
        "the four seeded commit events are read: {mine:#?}"
    );
    let plain_ev = mine[mine.len() - 4];
    assert!(
        !plain_ev.in_gap && !plain_ev.inferred,
        "TMC011-ATTRIBUTION: a plain event is neither in a gap nor inferred"
    );
    assert_eq!(
        (
            plain_ev.old_commit.as_deref(),
            plain_ev.new_commit.as_deref()
        ),
        (Some(OLD), Some(NEW)),
        "TMC011-ATTRIBUTION: raw_events carries old_commit and new_commit"
    );
    assert!(
        mine[mine.len() - 3].inferred,
        "TMC011-ATTRIBUTION: worktree_inferred of the event details reaches RawGitEvent.inferred"
    );
    assert!(
        mine[mine.len() - 2].in_gap,
        "TMC011-ATTRIBUTION: an event with a gap id is in_gap even with a session"
    );
    let last = mine[mine.len() - 1];
    assert_eq!(
        (last.old_commit.as_deref(), last.new_commit.as_deref()),
        (None, None),
        "TMC011-ATTRIBUTION: no commits in the details, none in the event"
    );
}

/// TMC011-ATTRIBUTION: one read gives the current actor of every session, and follows a
/// correction and its withdrawal.
#[test]
fn session_attribution_follows_a_correction() {
    let mut r = rig();
    r.start_session("100:1");
    r.start_session("200:1");
    let read = |r: &Rig| -> SessionAttribution {
        r.daemon
            .session_attribution(&r.repo_id)
            .expect("TMC011-ATTRIBUTION: the daemon reads the sessions' attribution")
    };

    let before = read(&r);
    assert_eq!(
        agent_name(&before.0["100:1"]),
        Some("claude-code"),
        "TMC011-ATTRIBUTION: a detected Claude Code session is claude-code"
    );
    assert_eq!(before.0.len(), 2, "one entry per session");

    r.write(&[WriteOp::AppendAttribution {
        session_id: "100:1".into(),
        kind: RecordKind::Correct,
        agent: Agent {
            kind: AgentKind::Other,
            name: Some("codex-1".into()),
        },
        author: Author::Developer,
        recorded_ms: 5,
    }]);
    let corrected = read(&r);
    assert_eq!(
        agent_name(&corrected.0["100:1"]),
        Some("codex-1"),
        "TMC011-ATTRIBUTION: the corrected session is its corrected agent"
    );
    assert_eq!(
        agent_name(&corrected.0["200:1"]),
        Some("claude-code"),
        "the other session is untouched"
    );

    r.write(&[WriteOp::AppendAttribution {
        session_id: "100:1".into(),
        kind: RecordKind::WithdrawCorrection,
        agent: Agent {
            kind: AgentKind::ClaudeCode,
            name: None,
        },
        author: Author::Developer,
        recorded_ms: 6,
    }]);
    assert_eq!(
        agent_name(&read(&r).0["100:1"]),
        Some("claude-code"),
        "TMC011-ATTRIBUTION: withdrawing the correction brings the detected agent back"
    );
}
