//! US-CKP-022 (DS-US-CKP-022 § Contrato de API): the wire of `graph.lanes` and its place in the
//! method registry. The types come from `gitraptor_api::graph`, which the expert creates.

use gitraptor_api::capability::CAPABILITIES_PROTOCOL;
use gitraptor_api::graph::{GraphLanes, GraphParams, Lane, LaneCommit};
use gitraptor_api::methods::{self, GROUPS};
use serde_json::{Value, json};

/// The first example of the DS: a ready base, one ready lane and one lane without merge-base.
fn ready_example() -> Value {
    json!({
        "computed_at_utc_ms": 1793091600000_i64,
        "base": {"ready": {"name": {"untrusted": "main"}}},
        "lanes_total": 2,
        "lanes": [
            {"worktree": {"untrusted": "/w/feat-pagos"},
             "head": {"branch": {"name": {"untrusted": "feat-pagos"}}},
             "state": {"ready": {
                 "merge_base": "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678",
                 "total": {"count": 72, "exact": true},
                 "commits": [{
                     "oid": "9f8e7d6c5b4a39281706f5e4d3c2b1a098765432",
                     "summary": {"untrusted": "Add payment retry"},
                     "committed_utc_ms": 1793091500000_i64,
                     "merge": false}]}}},
            {"worktree": {"untrusted": "/w/feat-login"},
             "head": {"branch": {"name": {"untrusted": "feat-login"}}},
             "state": {"not-computable": {"reason": "no-merge-base"}}}
        ]
    })
}

/// The second example: a base that does not exist, so no lanes.
fn missing_base_example() -> Value {
    json!({
        "computed_at_utc_ms": 1793091600000_i64,
        "base": {"not-computable": {"reason": {"base-missing": {"name": {"untrusted": "develop"}}}}},
        "lanes_total": 0,
        "lanes": []
    })
}

#[test]
fn graph_lanes_is_declared_for_the_full_profile_only() {
    let group = GROUPS
        .iter()
        .find(|g| g.module == "graph")
        .expect("the `graph` module is missing from methods::GROUPS (D2)");
    let spec = group
        .methods
        .iter()
        .find(|m| m.name == "graph.lanes")
        .expect("`graph.lanes` is not declared in the graph group (D2)");
    assert_eq!(methods::GRAPH_LANES, "graph.lanes");
    assert!(
        !spec.mcp,
        "graph.lanes must not be offered to raptor-mcp (mcp: false, D2)"
    );
    assert!(!spec.reserved, "graph.lanes is not a reserved command");
    assert_eq!(
        spec.since, CAPABILITIES_PROTOCOL,
        "graph.lanes is declared .since(CAPABILITIES_PROTOCOL) like diff.worktree (D14)"
    );
    assert!(
        group.error_block.is_none() && group.errors.is_empty(),
        "graph.lanes has no error block of its own: everything not computable goes in the result (D2)"
    );
    assert!(
        methods::METHODS.iter().any(|m| m.name == "graph.lanes"),
        "graph.lanes must be in the registry METHODS"
    );
}

#[test]
fn graph_lanes_wire_matches_the_contract_example() {
    for (label, example) in [
        ("ready example", ready_example()),
        ("missing-base example", missing_base_example()),
    ] {
        let parsed: GraphLanes = serde_json::from_value(example.clone())
            .unwrap_or_else(|e| panic!("the {label} of the DS does not deserialize: {e}"));
        let back = serde_json::to_value(&parsed).unwrap();
        assert_eq!(
            back, example,
            "the {label} must serialize back to the same kebab-case wire (cut omitted when absent)"
        );
    }

    // The request.
    let params: GraphParams = serde_json::from_value(json!({"repo_id": "r1"})).unwrap();
    assert_eq!(params.repo_id, "r1");
    assert!(
        serde_json::from_value::<GraphParams>(json!({"repo_id": "r1", "filter": []})).is_err(),
        "GraphParams denies unknown fields"
    );

    // The rest of the vocabulary: every reason and the cut are kebab-case.
    for reason in [
        "worktree-unavailable",
        "no-commits",
        "no-merge-base",
        "too-slow",
        "unreadable",
    ] {
        let mut v = ready_example();
        v["lanes"][1]["state"] = json!({"not-computable": {"reason": reason}});
        let parsed: GraphLanes = serde_json::from_value(v.clone())
            .unwrap_or_else(|e| panic!("lane reason `{reason}` is not in the contract: {e}"));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), v);
    }
    for gap in [
        json!("no-base"),
        json!("repo-unavailable"),
        json!({"base-unconfirmed": {"name": {"untrusted": "main"}}}),
    ] {
        let mut v = missing_base_example();
        v["base"] = json!({"not-computable": {"reason": gap}});
        let parsed: GraphLanes = serde_json::from_value(v.clone())
            .unwrap_or_else(|e| panic!("base gap {gap} is not in the contract: {e}"));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), v);
    }
    for cut in ["lanes", "bytes"] {
        let mut v = ready_example();
        v["cut"] = json!(cut);
        let parsed: GraphLanes = serde_json::from_value(v.clone())
            .unwrap_or_else(|e| panic!("cut `{cut}` is not in the contract: {e}"));
        assert_eq!(serde_json::to_value(&parsed).unwrap(), v);
    }
    let mut detached = ready_example();
    detached["lanes"][0]["head"] = json!("detached");
    let parsed: GraphLanes = serde_json::from_value(detached.clone())
        .unwrap_or_else(|e| panic!("a detached head is not in the contract: {e}"));
    assert_eq!(serde_json::to_value(&parsed).unwrap(), detached);

    // deny_unknown_fields on every struct of the contract.
    let mut top = ready_example();
    top["extra"] = json!(1);
    assert!(
        serde_json::from_value::<GraphLanes>(top).is_err(),
        "GraphLanes denies unknown fields"
    );
    let mut lane = ready_example()["lanes"][0].clone();
    lane["extra"] = json!(1);
    assert!(
        serde_json::from_value::<Lane>(lane).is_err(),
        "Lane denies unknown fields"
    );
    let mut commit = ready_example()["lanes"][0]["state"]["ready"]["commits"][0].clone();
    commit["extra"] = json!(1);
    assert!(
        serde_json::from_value::<LaneCommit>(commit).is_err(),
        "LaneCommit denies unknown fields"
    );
}

/// Every key declared as a property anywhere in a JSON schema.
fn property_names(schema: &Value, into: &mut Vec<String>) {
    match schema {
        Value::Object(map) => {
            if let Some(Value::Object(props)) = map.get("properties") {
                into.extend(props.keys().cloned());
            }
            for v in map.values() {
                property_names(v, into);
            }
        }
        Value::Array(items) => items.iter().for_each(|v| property_names(v, into)),
        _ => {}
    }
}

#[test]
fn graph_types_carry_no_actor_author_or_email() {
    const FORBIDDEN: [&str; 8] = [
        "actor",
        "session",
        "session_id",
        "author",
        "authors",
        "email",
        "mail",
        "committer",
    ];
    let schema = serde_json::to_value(schemars::schema_for!(GraphLanes)).unwrap();
    let mut names = Vec::new();
    property_names(&schema, &mut names);
    assert!(
        names.iter().any(|n| n == "lanes_total"),
        "the schema of GraphLanes was not found, so nothing was checked"
    );
    for name in &names {
        assert!(
            !FORBIDDEN.contains(&name.as_str()),
            "the contract carries a `{name}` field: no actor, session, author or email (D6, D7)"
        );
    }

    // A client that sends them is rejected, never ignored.
    let commit = ready_example()["lanes"][0]["state"]["ready"]["commits"][0].clone();
    for field in ["actor", "author", "email"] {
        let mut with = commit.clone();
        with[field] = json!("someone");
        assert!(
            serde_json::from_value::<LaneCommit>(with).is_err(),
            "a `{field}` field on a commit must be rejected (deny_unknown_fields, D6/D7)"
        );
    }
}
