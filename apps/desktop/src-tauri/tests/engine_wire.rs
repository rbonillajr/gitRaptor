//! Contract Rust <-> TypeScript: one example of each `EngineMessage` variant, serialized by the
//! real `wire.rs` types, must equal the frozen fixture the webview tests also load. This test only
//! compares: a difference means `wire.rs` (or the TS mirror) changed shape, never that the fixture
//! should be rewritten here.

use std::path::Path;

use gitraptor_desktop::engine::wire::{
    AgentDto, AttentionDto, Autostart, BaseDto, BaseStatus, BoundedCountDto, CandidateDto,
    ConfirmCommandDto, ConflictFileDto, ConflictFilesDto, ConflictKind, ConnState, CountDto,
    CountsDto, DaemonDto, DivergenceDto, EndCause, EngineDto, EngineMessage, EngineStateDto,
    FleetPerf, GlobalDto, HeadDto, LimitCode, LineRangeDto, PairDetailDto, PairDto, PairLevel,
    PairReason, PairState, ReadyDto, RepoFleetDto, RepoStateDto, RepoSummaryDto, ResyncReason,
    SessionDto, SessionOrigin, SessionState, Tier, UnavailableReason, WorktreeDto,
    WorktreeStatusDto,
};
use serde_json::{Map, Value};

fn repo(
    id: &str,
    name: &str,
    state: RepoStateDto,
    tier: Option<Tier>,
    conflicts: CountDto,
) -> RepoSummaryDto {
    RepoSummaryDto {
        repo_id: id.to_owned(),
        name: name.to_owned(),
        path: format!("/Users/me/code/{name}"),
        state,
        tier,
        attention: AttentionDto {
            conflicts,
            denials: CountDto::Unavailable,
        },
    }
}

fn ready(head: HeadDto, unstaged: u32, divergence: DivergenceDto) -> WorktreeStatusDto {
    WorktreeStatusDto::Ready(ReadyDto {
        head,
        counts: CountsDto {
            staged: 0,
            unstaged,
            untracked: 0,
        },
        divergence,
    })
}

fn counted(ahead: u32, behind: u32, exact: bool) -> DivergenceDto {
    DivergenceDto::Counted {
        ahead: BoundedCountDto {
            count: ahead,
            exact,
        },
        behind: BoundedCountDto {
            count: behind,
            exact: true,
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn worktree(
    key: &str,
    folder: &str,
    main: bool,
    status: WorktreeStatusDto,
    activity: Option<u64>,
    detached_at: Option<&str>,
    operation: Option<&str>,
) -> WorktreeDto {
    WorktreeDto {
        key: key.to_owned(),
        folder: folder.to_owned(),
        path: format!("/w/{folder}"),
        main,
        status,
        last_activity_utc_ms: activity,
        last_activity_in_gap: activity.is_none() && detached_at.is_some(),
        detached_at: detached_at.map(str::to_owned),
        operation: operation.map(str::to_owned),
    }
}

fn fleet_worktrees() -> Vec<WorktreeDto> {
    vec![
        worktree(
            "1111111111111111",
            "tienda",
            true,
            ready(
                HeadDto::Branch {
                    name: "main".to_owned(),
                },
                0,
                counted(0, 0, true),
            ),
            Some(1_762_697_000_000),
            None,
            None,
        ),
        worktree(
            "2222222222222222",
            "feat-pagos",
            false,
            ready(
                HeadDto::Branch {
                    name: "feat-pagos".to_owned(),
                },
                2,
                counted(3, 1, true),
            ),
            Some(1_762_697_090_000),
            None,
            Some("rebase"),
        ),
        worktree(
            "3333333333333333",
            "feat-login",
            false,
            ready(HeadDto::Detached, 0, counted(10_000, 0, false)),
            None,
            Some("0123456789abcdef0123456789abcdef01234567"),
            None,
        ),
        worktree(
            "4444444444444444",
            "spike",
            false,
            ready(
                HeadDto::Unborn {
                    name: "spike".to_owned(),
                },
                0,
                DivergenceDto::NoCommits,
            ),
            None,
            None,
            None,
        ),
        worktree(
            "5555555555555555",
            "gone",
            false,
            WorktreeStatusDto::Unavailable {
                reason: UnavailableReason::Missing,
            },
            None,
            None,
            None,
        ),
        worktree(
            "6666666666666666",
            "base-less",
            false,
            ready(
                HeadDto::Branch {
                    name: "x".to_owned(),
                },
                0,
                DivergenceDto::BaseMissing,
            ),
            None,
            None,
            None,
        ),
    ]
}

fn session(id: &str, key: &str, agent: AgentDto, state: SessionState) -> SessionDto {
    let ended = state == SessionState::Ended;
    SessionDto {
        session_id: id.to_owned(),
        worktree_key: key.to_owned(),
        agent,
        origin: if matches!(state, SessionState::Active) {
            SessionOrigin::Detected
        } else {
            SessionOrigin::Registered
        },
        state,
        started_utc_ms: 1_762_696_000_000,
        state_since_utc_ms: 1_762_697_000_000,
        ended_utc_ms: ended.then_some(1_762_697_000_000),
        end_cause: ended.then_some(EndCause::ProcessGone),
    }
}

fn pair(id: &str, a: &str, b: &str, state: PairState, level: Option<PairLevel>) -> PairDto {
    let pending = state == PairState::Pending;
    PairDto {
        pair_id: id.to_owned(),
        rev: if pending { 11 } else { 12 },
        a: a.to_owned(),
        b: b.to_owned(),
        state,
        reason: pending.then_some(PairReason::BaseUnconfirmed),
        level,
        computed_at_utc_ms: (!pending).then_some(1_762_697_200_000),
        overlap_files: u32::from(!pending),
        overlap_partial: false,
        conflict_files: u32::from(level == Some(PairLevel::Conflict)),
        conflict_truncated: false,
    }
}

fn recalculating_pair() -> PairDto {
    PairDto {
        rev: 13,
        computed_at_utc_ms: Some(1_762_697_300_000),
        overlap_files: 2,
        conflict_files: 0,
        ..pair(
            "pair-1",
            "3333333333333333",
            "2222222222222222",
            PairState::Recalculating,
            Some(PairLevel::Overlap),
        )
    }
}

fn examples() -> Vec<(&'static str, EngineMessage)> {
    let engine = |state, git: Option<&str>| EngineDto {
        state,
        git_version: git.map(str::to_owned),
    };
    vec![
        (
            "conn",
            EngineMessage::Conn {
                state: ConnState::Reconnecting {
                    attempt: 2,
                    retry_in_ms: 500,
                },
                stale_since_ms: Some(1_762_697_100_000),
                resync: Some(ResyncReason::SlowConsumer),
            },
        ),
        (
            "session",
            EngineMessage::Session {
                daemon_version: "0.1.0".to_owned(),
                protocol: 9,
                restricted: false,
                activity: true,
                discovery: true,
            },
        ),
        (
            "global",
            EngineMessage::Global(GlobalDto {
                engine: engine(EngineStateDto::Observing, Some("2.45.1")),
                daemon: DaemonDto {
                    version: "0.1.0".to_owned(),
                    protocol: 9,
                },
                autostart: Autostart::NotRegistered,
                repos: vec![
                    repo(
                        "repo-alpha",
                        "alpha",
                        RepoStateDto::Observed,
                        Some(Tier::Active),
                        CountDto::Counted { count: 2 },
                    ),
                    repo(
                        "repo-beta",
                        "beta",
                        RepoStateDto::Unavailable,
                        None,
                        CountDto::Unavailable,
                    ),
                ],
            }),
        ),
        (
            "engineState",
            EngineMessage::EngineState {
                engine: engine(EngineStateDto::WaitingForGit, None),
            },
        ),
        (
            "repoObserved",
            EngineMessage::RepoObserved {
                repo: repo(
                    "repo-gamma",
                    "gamma",
                    RepoStateDto::Observed,
                    Some(Tier::Waking),
                    CountDto::Counted { count: 0 },
                ),
            },
        ),
        (
            "repoRetired",
            EngineMessage::RepoRetired {
                repo_id: "repo-beta".to_owned(),
            },
        ),
        (
            "repoTier",
            EngineMessage::RepoTier {
                repo_id: "repo-alpha".to_owned(),
                tier: Tier::Dormant,
            },
        ),
        (
            "repoAttention",
            EngineMessage::RepoAttention {
                repo_id: "repo-alpha".to_owned(),
                attention: AttentionDto {
                    conflicts: CountDto::Counted { count: 3 },
                    denials: CountDto::Counted { count: 1 },
                },
            },
        ),
        (
            "discovered",
            EngineMessage::Discovered {
                candidates: vec![CandidateDto {
                    name: "delta".to_owned(),
                    path: "/Users/me/code/delta".to_owned(),
                }],
                replace: true,
            },
        ),
        (
            "engineOnly",
            EngineMessage::EngineOnly {
                engine: engine(EngineStateDto::NoRepos, None),
            },
        ),
        (
            "repo",
            EngineMessage::Repo {
                fleet: RepoFleetDto {
                    repo_id: "repo-alpha".to_owned(),
                    name: "tienda".to_owned(),
                    base: BaseDto {
                        name: Some("main".to_owned()),
                        status: BaseStatus::Confirmed,
                    },
                    fetched_utc_ms: Some(1_762_690_000_000),
                    tier: Some(Tier::Active),
                    worktrees: fleet_worktrees(),
                    pairs: vec![
                        pair(
                            "pair-1",
                            "3333333333333333",
                            "2222222222222222",
                            PairState::Current,
                            Some(PairLevel::Conflict),
                        ),
                        pair(
                            "pair-2",
                            "2222222222222222",
                            "base",
                            PairState::Pending,
                            None,
                        ),
                    ],
                    pairs_truncated: false,
                    limits: vec![LimitCode::OverlapIncludesUncommitted, LimitCode::LocalOnly],
                    confirm_command: ConfirmCommandDto::Offered {
                        command: "'/Applications/GitRaptor.app/Contents/MacOS/raptor' 'guard' 'confirm' '/Users/me/code/tienda'".to_owned(),
                        with_path: true,
                    },
                },
            },
        ),
        (
            "worktrees",
            EngineMessage::Worktrees {
                repo_id: "repo-alpha".to_owned(),
                worktrees: fleet_worktrees(),
                fetched_utc_ms: None,
                perf: FleetPerf {
                    backend_ns: 1_500_000,
                },
            },
        ),
        (
            "sessions",
            EngineMessage::Sessions {
                repo_id: "repo-alpha".to_owned(),
                detection_available: true,
                sessions: vec![
                    session(
                        "claude-1",
                        "2222222222222222",
                        AgentDto::ClaudeCode,
                        SessionState::Active,
                    ),
                    session(
                        "agent-x",
                        "3333333333333333",
                        AgentDto::Other {
                            name: "Aider".to_owned(),
                        },
                        SessionState::Ended,
                    ),
                ],
            },
        ),
        (
            "repoSession",
            EngineMessage::RepoSession {
                repo_id: "repo-alpha".to_owned(),
                session: SessionDto {
                    state_since_utc_ms: 1_762_697_500_000,
                    ..session(
                        "claude-1",
                        "2222222222222222",
                        AgentDto::ClaudeCode,
                        SessionState::Inactive,
                    )
                },
                perf: FleetPerf {
                    backend_ns: 800_000,
                },
            },
        ),
        (
            "prediction",
            EngineMessage::Prediction {
                repo_id: "repo-alpha".to_owned(),
                pairs: vec![recalculating_pair()],
                removed: vec!["pair-old".to_owned()],
                perf: FleetPerf {
                    backend_ns: 600_000,
                },
            },
        ),
        (
            "pairDetails",
            EngineMessage::PairDetails {
                repo_id: "repo-alpha".to_owned(),
                details: vec![PairDetailDto {
                    pair: recalculating_pair(),
                    overlap: vec!["README.md".to_owned()],
                    conflict: Some(ConflictFilesDto {
                        files: vec![ConflictFileDto {
                            path: "pagos.rs".to_owned(),
                            kind: ConflictKind::Content,
                            ours: vec![LineRangeDto { start: 40, len: 19 }],
                            theirs: vec![LineRangeDto { start: 44, len: 18 }],
                            hunks_truncated: false,
                            has_merge_rule: true,
                        }],
                        truncated: false,
                    }),
                }],
                filled: true,
                perf: FleetPerf {
                    backend_ns: 700_000,
                },
            },
        ),
        (
            "repoClosed",
            EngineMessage::RepoClosed {
                repo_id: "repo-beta".to_owned(),
            },
        ),
    ]
}

#[test]
fn every_variant_serializes_exactly_as_the_frozen_fixture() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/engine-messages.json");
    let text = std::fs::read_to_string(&path).expect("the frozen fixture must exist");
    let fixture: Value = serde_json::from_str(&text).expect("the fixture must be JSON");
    let mut built = Map::new();
    for (name, message) in examples() {
        let value = serde_json::to_value(&message).expect("a message serializes");
        built.insert(name.to_owned(), value);
    }
    assert_eq!(
        Value::Object(built),
        fixture,
        "wire.rs no longer serializes as apps/desktop/tests/fixtures/engine-messages.json"
    );
}
