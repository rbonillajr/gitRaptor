//! D2, D17, SEC-12 (DS-US-DSK-005 T007): `ingest::graph` turns the daemon's `GraphLanes` into the
//! `GraphDto` the webview draws, and `ingest::graph_trigger` says which repo events make the
//! graph stale.
//!
//! The expected values come from the story and the contract, never from the code under test: the
//! wire shape of the DTO is written out as JSON, texts are compared with `visible` (the app's
//! sanitizer, `gitraptor_api::display`) and with the raw bytes of the hostile inputs, and the
//! worktree key with `ingest::key` over the raw root. Pure functions: no daemon, no file system.

use gitraptor_api::display::{NAME_MAX_CHARS, TEXT_MAX_CHARS, visible};
use gitraptor_api::event::{self, Event};
use gitraptor_api::graph::{
    BaseGap, GraphBase, GraphCut, GraphLanes, Lane, LaneCommit, LaneGap, LaneHead, LaneState,
};
use gitraptor_api::messages::CommitCountView;
use gitraptor_api::{Untrusted, UntrustedName};
use gitraptor_desktop::engine::ingest::{graph, graph_trigger, key};
use gitraptor_desktop::engine::wire::{GraphDto, LaneStateDto};
use serde_json::{Value, json};

const MERGE_BASE: &str = "0123456789abcdef0123456789abcdef01234567";
const OID_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OID_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SHA256_OID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const ROOT: &str = "/w/wt/feat-pagos";

fn commit(oid: &str, summary: &str, committed_utc_ms: i64, merge: bool) -> LaneCommit {
    LaneCommit {
        oid: oid.to_owned(),
        summary: Untrusted::new(summary),
        committed_utc_ms,
        merge,
    }
}

fn ready(merge_base: &str, commits: Vec<LaneCommit>, total: u64, exact: bool) -> LaneState {
    LaneState::Ready {
        merge_base: merge_base.to_owned(),
        commits,
        total: CommitCountView {
            count: total,
            exact,
        },
    }
}

fn lane(root: &str, branch: &str, state: LaneState) -> Lane {
    Lane {
        worktree: Untrusted::new(root),
        head: LaneHead::Branch {
            name: UntrustedName::new(branch),
        },
        state,
    }
}

fn graph_of(base: GraphBase, lanes: Vec<Lane>) -> GraphLanes {
    let lanes_total = u32::try_from(lanes.len()).unwrap();
    GraphLanes {
        computed_at_utc_ms: 1_700_000_400_000,
        base,
        lanes,
        lanes_total,
        cut: None,
    }
}

fn main_base() -> GraphBase {
    GraphBase::Ready {
        name: UntrustedName::new("main"),
    }
}

fn wire(lanes: &GraphLanes) -> Value {
    serde_json::to_value(graph(lanes)).expect("the DTO serialises")
}

fn first_lane_state(lanes: GraphLanes) -> LaneStateDto {
    graph(&lanes)
        .lanes
        .into_iter()
        .next()
        .expect("the lane is kept")
        .state
}

// ---------------------------------------------------------------- D2-DTO

#[test]
fn a_ready_graph_has_the_wire_shape_of_the_contract() {
    let input = GraphLanes {
        cut: Some(GraphCut::Lanes),
        lanes_total: 40,
        ..graph_of(
            main_base(),
            vec![
                lane(
                    ROOT,
                    "feat-pagos",
                    ready(
                        MERGE_BASE,
                        vec![
                            commit(OID_A, "add the payment form", 1_700_000_300_000, false),
                            commit(OID_B, "merge main", 1_700_000_200_000, true),
                        ],
                        72,
                        false,
                    ),
                ),
                Lane {
                    worktree: Untrusted::new("/w/wt/scratch"),
                    head: LaneHead::Detached,
                    state: LaneState::NotComputable {
                        reason: LaneGap::TooSlow,
                    },
                },
            ],
        )
    };
    assert_eq!(
        wire(&input),
        json!({
            "computedUtcMs": 1_700_000_400_000_i64,
            "base": {"state": "ready", "name": "main"},
            "lanes": [
                {
                    "worktreeKey": key(ROOT),
                    "head": {"kind": "branch", "name": "feat-pagos"},
                    "state": {
                        "state": "ready",
                        "mergeBase": MERGE_BASE,
                        "total": {"count": 72, "exact": false},
                        "commits": [
                            {"oid": OID_A, "summary": "add the payment form",
                             "committedUtcMs": 1_700_000_300_000_i64, "merge": false},
                            {"oid": OID_B, "summary": "merge main",
                             "committedUtcMs": 1_700_000_200_000_i64, "merge": true}
                        ]
                    }
                },
                {
                    "worktreeKey": key("/w/wt/scratch"),
                    "head": {"kind": "detached"},
                    "state": {"state": "not-computable", "reason": "too-slow"}
                }
            ],
            "lanesTotal": 40,
            "cut": "lanes"
        }),
        "D2-DTO: GraphDto is the shape of § Contratos compartidos, camelCase fields, kebab-case tags"
    );
}

#[test]
fn every_lane_gap_and_cut_keeps_its_name() {
    let gaps = [
        (LaneGap::WorktreeUnavailable, "worktree-unavailable"),
        (LaneGap::NoCommits, "no-commits"),
        (LaneGap::NoMergeBase, "no-merge-base"),
        (LaneGap::TooSlow, "too-slow"),
        (LaneGap::Unreadable, "unreadable"),
    ];
    for (gap, name) in gaps {
        let state = first_lane_state(graph_of(
            main_base(),
            vec![lane(ROOT, "b", LaneState::NotComputable { reason: gap })],
        ));
        assert_eq!(
            serde_json::to_value(state).unwrap(),
            json!({"state": "not-computable", "reason": name}),
            "D2-DTO: the lane gap {name} reaches the webview by its wire name"
        );
    }
    for (cut, name) in [(GraphCut::Lanes, "lanes"), (GraphCut::Bytes, "bytes")] {
        let input = GraphLanes {
            cut: Some(cut),
            ..graph_of(main_base(), Vec::new())
        };
        assert_eq!(wire(&input)["cut"], json!(name), "D2-DTO: cut {name}");
    }
    assert_eq!(
        wire(&graph_of(main_base(), Vec::new()))["cut"],
        Value::Null,
        "D2-DTO: no cut is null, not absent"
    );
}

#[test]
fn a_base_that_is_not_computable_names_its_reason_and_the_base_when_it_has_one() {
    let cases = [
        (
            BaseGap::BaseUnconfirmed {
                name: UntrustedName::new("develop"),
            },
            json!({"state": "not-computable", "reason": "base-unconfirmed", "name": "develop"}),
        ),
        (
            BaseGap::BaseMissing {
                name: UntrustedName::new("develop"),
            },
            json!({"state": "not-computable", "reason": "base-missing", "name": "develop"}),
        ),
        (
            BaseGap::NoBase,
            json!({"state": "not-computable", "reason": "no-base", "name": null}),
        ),
        (
            BaseGap::RepoUnavailable,
            json!({"state": "not-computable", "reason": "repo-unavailable", "name": null}),
        ),
    ];
    for (gap, expected) in cases {
        let input = graph_of(GraphBase::NotComputable { reason: gap }, Vec::new());
        assert_eq!(
            wire(&input)["base"],
            expected,
            "D2-DTO: the base gap and its name (null for no-base and repo-unavailable)"
        );
    }
}

#[test]
fn the_base_name_is_sanitized_and_bounded() {
    let raw = format!("de\u{1b}[31mvelop{}", "x".repeat(NAME_MAX_CHARS * 2));
    let input = graph_of(
        GraphBase::NotComputable {
            reason: BaseGap::BaseMissing {
                name: UntrustedName::new(raw.clone()),
            },
        },
        Vec::new(),
    );
    let dto = wire(&input);
    let shown = dto["base"]["name"].as_str().expect("a base name");
    assert_eq!(
        shown,
        visible(&raw, NAME_MAX_CHARS),
        "SEC-12: the base name goes through `visible` at NAME_MAX_CHARS"
    );
    assert!(
        !shown.contains('\u{1b}'),
        "SEC-12: no raw escape in {shown:?}"
    );
}

// ---------------------------------------------------------------- SEC-12

#[test]
fn summary_and_branch_reach_the_webview_escaped() {
    let hostile_summary = "fix\u{1b}]0;x\u{7} \u{202e}txt.exe";
    let hostile_branch = "feat/\u{1b}[2Jpwn\u{202e}";
    let input = graph_of(
        main_base(),
        vec![lane(
            ROOT,
            hostile_branch,
            ready(
                MERGE_BASE,
                vec![commit(OID_A, hostile_summary, 1_700_000_300_000, false)],
                1,
                true,
            ),
        )],
    );
    let dto = wire(&input);
    let summary = dto["lanes"][0]["state"]["commits"][0]["summary"]
        .as_str()
        .expect("a summary");
    let branch = dto["lanes"][0]["head"]["name"].as_str().expect("a branch");
    assert_eq!(
        summary,
        visible(hostile_summary, TEXT_MAX_CHARS),
        "SEC-12: the summary goes through `visible` at TEXT_MAX_CHARS"
    );
    assert_eq!(
        branch,
        visible(hostile_branch, NAME_MAX_CHARS),
        "SEC-12: the branch goes through `visible` at NAME_MAX_CHARS"
    );
    for (what, text) in [("summary", summary), ("branch", branch)] {
        assert!(
            !text.chars().any(|c| c.is_control() || c == '\u{202e}'),
            "SEC-12: the {what} carries a raw control or bidi character: {text:?}"
        );
    }
    assert!(
        summary.contains("\\x1b") || summary.contains("\\u{1b}"),
        "SEC-12: the escape is shown as text, not dropped silently: {summary:?}"
    );
}

#[test]
fn a_long_summary_is_cut_at_the_text_bound() {
    let long = "s".repeat(TEXT_MAX_CHARS + 500);
    let input = graph_of(
        main_base(),
        vec![lane(
            ROOT,
            "b",
            ready(
                MERGE_BASE,
                vec![commit(OID_A, &long, 1_700_000_300_000, false)],
                1,
                true,
            ),
        )],
    );
    let dto = wire(&input);
    let summary = dto["lanes"][0]["state"]["commits"][0]["summary"]
        .as_str()
        .expect("a summary");
    assert_eq!(summary, visible(&long, TEXT_MAX_CHARS));
    assert!(
        summary.chars().count() <= TEXT_MAX_CHARS + 1,
        "SEC-12: the summary is bounded at TEXT_MAX_CHARS characters plus the ellipsis that `visible` appends"
    );
}

#[test]
fn the_raw_worktree_path_never_reaches_the_webview() {
    let raw_root = "/Users/ana/secret-client/wt/fe\u{202e}at";
    let input = graph_of(
        main_base(),
        vec![lane(
            raw_root,
            "feat",
            ready(
                MERGE_BASE,
                vec![commit(OID_A, "a commit", 1_700_000_300_000, false)],
                1,
                true,
            ),
        )],
    );
    let dto = wire(&input);
    assert_eq!(
        dto["lanes"][0]["worktreeKey"],
        json!(key(raw_root)),
        "SEC-12: the lane is identified by the key of the raw root, as the fleet row is"
    );
    let text = dto.to_string();
    for needle in ["secret-client", "/Users/ana", "wt/fe"] {
        assert!(
            !text.contains(needle),
            "SEC-12: the DTO carries part of the worktree path ({needle}): {text}"
        );
    }
}

// ---------------------------------------------------------------- D2-VALID

#[test]
fn an_invalid_oid_drops_the_commit() {
    let bad_oids = [
        "",
        "abc123",
        "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\u{1b}",
    ];
    for bad in bad_oids {
        let state = first_lane_state(graph_of(
            main_base(),
            vec![lane(
                ROOT,
                "b",
                ready(
                    MERGE_BASE,
                    vec![
                        commit(bad, "bad", 1_700_000_300_000, false),
                        commit(OID_A, "good", 1_700_000_200_000, false),
                    ],
                    2,
                    true,
                ),
            )],
        ));
        let LaneStateDto::Ready { commits, .. } = state else {
            panic!("D2-VALID: an invalid commit oid ({bad:?}) must not make the lane unreadable");
        };
        let oids: Vec<&str> = commits.iter().map(|c| c.oid.as_str()).collect();
        assert_eq!(
            oids,
            [OID_A],
            "D2-VALID: the commit with oid {bad:?} is omitted, the others stay"
        );
    }
}

#[test]
fn a_40_or_64_digit_hex_oid_is_kept_in_either_case() {
    let upper = OID_A.to_uppercase();
    let state = first_lane_state(graph_of(
        main_base(),
        vec![lane(
            ROOT,
            "b",
            ready(
                SHA256_OID,
                vec![
                    commit(SHA256_OID, "sha-256", 3, false),
                    commit(&upper, "upper", 2, false),
                    commit(OID_B, "sha-1", 1, false),
                ],
                3,
                true,
            ),
        )],
    ));
    let LaneStateDto::Ready {
        merge_base,
        commits,
        ..
    } = state
    else {
        panic!("D2-VALID: valid ids keep the lane ready");
    };
    assert_eq!(
        merge_base, SHA256_OID,
        "D2-VALID: a 64-digit merge-base is valid"
    );
    assert_eq!(
        commits.iter().map(|c| c.oid.as_str()).collect::<Vec<_>>(),
        [SHA256_OID, upper.as_str(), OID_B],
        "D2-VALID: 40 and 64 hex digits, any case, in order"
    );
}

#[test]
fn an_invalid_merge_base_makes_the_lane_not_computable_unreadable() {
    for bad in [
        "",
        "main",
        "0123",
        "not hex not hex not hex not hex not hex!",
    ] {
        let dto = wire(&graph_of(
            main_base(),
            vec![lane(
                ROOT,
                "b",
                ready(
                    bad,
                    vec![commit(OID_A, "a", 1_700_000_300_000, false)],
                    1,
                    true,
                ),
            )],
        ));
        assert_eq!(
            dto["lanes"][0]["state"],
            json!({"state": "not-computable", "reason": "unreadable"}),
            "D2-VALID: a lane whose merge-base is {bad:?} becomes not-computable unreadable"
        );
    }
}

#[test]
fn a_negative_commit_time_drops_the_commit() {
    let state = first_lane_state(graph_of(
        main_base(),
        vec![lane(
            ROOT,
            "b",
            ready(
                MERGE_BASE,
                vec![
                    commit(OID_A, "before the epoch", -1, false),
                    commit(OID_B, "at the epoch", 0, false),
                ],
                2,
                true,
            ),
        )],
    ));
    let LaneStateDto::Ready { commits, .. } = state else {
        panic!("D2-VALID: a negative time does not make the lane unreadable");
    };
    assert_eq!(
        commits.iter().map(|c| c.oid.as_str()).collect::<Vec<_>>(),
        [OID_B],
        "D2-VALID: committed_utc_ms < 0 drops the commit; 0 stays"
    );
}

#[test]
fn the_commit_total_keeps_its_exactness_and_saturates() {
    let dto = wire(&graph_of(
        main_base(),
        vec![
            lane(
                ROOT,
                "a",
                ready(MERGE_BASE, vec![commit(OID_A, "a", 1, false)], 72, false),
            ),
            lane(
                "/w/wt/b",
                "b",
                ready(
                    MERGE_BASE,
                    vec![commit(OID_A, "a", 1, false)],
                    u64::MAX,
                    true,
                ),
            ),
        ],
    ));
    assert_eq!(
        dto["lanes"][0]["state"]["total"],
        json!({"count": 72, "exact": false}),
        "D2-DTO: an inexact total says so, for \"at least N more\""
    );
    assert_eq!(
        dto["lanes"][1]["state"]["total"],
        json!({"count": u32::MAX, "exact": true}),
        "D2-DTO: a count over u32 saturates, as every other count of the app"
    );
}

#[test]
fn the_graph_dto_is_what_the_conversion_returns_for_an_empty_graph() {
    let dto: GraphDto = graph(&graph_of(main_base(), Vec::new()));
    assert_eq!(dto.lanes.len(), 0);
    assert_eq!(dto.lanes_total, 0);
    assert_eq!(dto.computed_utc_ms, 1_700_000_400_000);
}

// ---------------------------------------------------------------- D3-TRIGGER

fn event_of(kind: &str) -> Event {
    Event {
        seq: 1,
        kind: kind.to_owned(),
        version: 1,
        wall_ms: 0,
        timings: None,
        data: json!({}),
    }
}

#[test]
fn only_a_git_event_is_a_graph_trigger() {
    assert!(
        graph_trigger(&event_of(event::GIT_EVENT)),
        "D3-TRIGGER: a git.event marks the graph stale"
    );
    for other in [
        event::WORKTREE_STATE,
        event::SESSION_STATE,
        "prediction.changed",
        "operation.queued",
        "something.else",
    ] {
        assert!(
            !graph_trigger(&event_of(other)),
            "D3-TRIGGER: {other} does not mark the graph stale by itself"
        );
    }
}

#[test]
fn the_protocol_bounds_of_lanes_and_commits_are_enforced_here_too() {
    let commits: Vec<LaneCommit> = (0..80)
        .map(|i| commit(OID_A, "c", 1_700_000_000_000 + i, false))
        .collect();
    let lanes: Vec<Lane> = (0..40)
        .map(|i| {
            lane(
                &format!("/r/wt{i}"),
                "b",
                ready(MERGE_BASE, commits.clone(), 80, true),
            )
        })
        .collect();
    let dto = wire(&graph_of(main_base(), lanes));
    let lanes = dto["lanes"].as_array().expect("lanes");
    assert_eq!(lanes.len(), 32, "at most MAX_GRAPH_LANES lanes");
    let shown = lanes[0]["state"]["commits"].as_array().expect("commits");
    assert_eq!(shown.len(), 50, "at most MAX_LANE_COMMITS commits per lane");
}
