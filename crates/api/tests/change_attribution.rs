//! US-GRP-008 (T001): `WorktreeView::changes_actor` and its capability
//! `observation.change-attribution`. The views are built from JSON on
//! purpose: the field is read and written through the wire form.

use gitraptor_api::capability;
use gitraptor_api::messages::{RepoView, WorktreeStateData, WorktreeView};
use gitraptor_api::methods::GROUPS;
use serde_json::{Value, json};

const CAPABILITY: &str = "observation.change-attribution";

fn worktree_json(with_actor: bool) -> Value {
    let mut w = json!({
        "path": {"untrusted": "/w/feat"},
        "main": false,
        "admin_name": null,
        "status": {
            "state": "ready",
            "head": {"kind": "detached"},
            "counts": {"staged": 0, "unstaged": 1, "untracked": 0},
            "changes": [{
                "path": {"untrusted": "pagos.txt"},
                "area": "unstaged",
                "kind": "modified"
            }],
            "divergence": {"state": "no-commits"}
        }
    });
    if with_actor {
        w["changes_actor"] = json!({"actor": "unattributed"});
    }
    w
}

fn worktree(with_actor: bool) -> WorktreeView {
    serde_json::from_value(worktree_json(with_actor))
        .expect("WorktreeView must accept changes_actor: {\"actor\":\"unattributed\"}")
}

fn state_json(with_actor: bool) -> Value {
    json!({"repo_id": "r1", "worktrees": [worktree_json(with_actor)]})
}

fn repo_json(with_actor: bool) -> Value {
    json!({
        "repo_id": "r1",
        "state": "observed",
        "path": {"untrusted": "/r/.git"},
        "base": {"name": null, "status": "unconfirmed"},
        "worktrees": [worktree_json(with_actor)]
    })
}

fn to_json<T: serde::Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap()
}

#[test]
fn change_attribution_is_a_capability_of_observation() {
    let observation = GROUPS
        .iter()
        .find(|g| g.module == "observation")
        .expect("observation group");
    assert!(
        observation
            .capabilities
            .iter()
            .any(|c| c.name == CAPABILITY),
        "{CAPABILITY} is not listed in the observation group"
    );
    assert!(
        capability::all().any(|c| c.name == CAPABILITY),
        "{CAPABILITY} is not in the capability catalog"
    );
}

#[test]
fn changes_actor_wire_form() {
    let with = to_json(&worktree(true));
    assert_eq!(
        with["changes_actor"],
        json!({"actor": "unattributed"}),
        "changes_actor must travel as {{\"actor\":\"unattributed\"}}: {with}"
    );
    let back: WorktreeView = serde_json::from_value(with.clone()).unwrap();
    assert_eq!(to_json(&back), with, "changes_actor must round-trip");

    // Absent when missing, and a view from before the capability still reads.
    let without = to_json(&worktree(false));
    assert!(
        !without.as_object().unwrap().contains_key("changes_actor"),
        "an absent changes_actor must not be serialized: {without}"
    );
    // The strictness stays: an unknown field is still refused.
    let mut unknown = worktree_json(false);
    unknown["conflicts"] = json!(3);
    assert!(serde_json::from_value::<WorktreeView>(unknown).is_err());
}

#[test]
fn without_change_attribution_drops_it_in_every_view() {
    let mut w = worktree(true);
    w.without_change_attribution();
    assert_eq!(to_json(&w), worktree_json(false), "WorktreeView keeps it");

    let mut data: WorktreeStateData = serde_json::from_value(state_json(true))
        .expect("WorktreeStateData must accept changes_actor in its worktrees");
    data.without_change_attribution();
    assert_eq!(
        to_json(&data)["worktrees"][0],
        worktree_json(false),
        "WorktreeStateData keeps it"
    );

    let mut repo: RepoView = serde_json::from_value(repo_json(true))
        .expect("RepoView must accept changes_actor in its worktrees");
    repo.without_change_attribution();
    assert_eq!(
        to_json(&repo)["worktrees"][0],
        worktree_json(false),
        "RepoView keeps it"
    );
}
