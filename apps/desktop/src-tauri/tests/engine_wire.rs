//! Contract Rust <-> TypeScript: one example of each `EngineMessage` variant, serialized by the
//! real `wire.rs` types, must equal the frozen fixture the webview tests also load. This test only
//! compares: a difference means `wire.rs` (or the TS mirror) changed shape, never that the fixture
//! should be rewritten here.

use std::path::Path;

use gitraptor_desktop::engine::wire::{
    AttentionDto, Autostart, CandidateDto, ConnState, CountDto, DaemonDto, EngineDto,
    EngineMessage, EngineStateDto, GlobalDto, RepoStateDto, RepoSummaryDto, ResyncReason, Tier,
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
