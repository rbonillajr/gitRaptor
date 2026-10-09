//! BR-CONS-001: the personal levels harden and never relax, and what they tried to relax is
//! reported without changing anything.

mod common;

use common::Repo;
use gitraptor_api::guard::Level;
use gitraptor_policy::layers::{
    IgnoredRelaxation, Personal, RelaxKey, harden, ignored_relaxations,
};
use gitraptor_policy::settings::{
    Level as DocLevel, Operation, Parsed, SourceKind, parse_document,
};
use gitraptor_policy::team::{Confirmed, ConfirmedFloor, Permission, TeamConfig, TeamLoader};

const DENY_PUSH: &str = r#"{"permissions":{"deny":["push"]}}"#;
const LAX: &str = r#"{"permissions":{"allow":["push","force-push"],"disableSafeMinimum":true},
                      "policies":{"commitAuthorship":{"mode":"flexible"}}}"#;

fn confirmed_repo(settings: &str) -> (Repo, Confirmed) {
    let repo = Repo::new();
    let commit = repo.commit_settings(settings);
    repo.set_origin_main(&commit);
    let conf = Confirmed {
        base_branch: gitraptor_git::RefName::new("main").unwrap(),
        floor: ConfirmedFloor::Blob(repo.settings_blob(&commit)),
    };
    (repo, conf)
}

fn load(repo: &Repo, conf: &Confirmed) -> TeamConfig {
    TeamLoader::default()
        .load(&repo.reader(), Some(conf))
        .unwrap()
}

fn personal(json: &str, level: DocLevel, kind: SourceKind) -> Parsed {
    parse_document(json.as_bytes(), level, kind)
}

fn profile(json: &str) -> Parsed {
    personal(json, DocLevel::Profile, SourceKind::Profile)
}

fn local(json: &str) -> Parsed {
    personal(json, DocLevel::Local, SourceKind::Local)
}

fn ir(level: Level, key: RelaxKey) -> IgnoredRelaxation {
    IgnoredRelaxation { level, key }
}

#[test]
fn a_personal_allow_below_the_team_is_ignored_and_reported() {
    let (repo, conf) =
        confirmed_repo(r#"{"permissions":{"deny":["force-push"],"ask":["rebase"]}}"#);
    let team = load(&repo, &conf);
    let l = local(r#"{"permissions":{"allow":["force-push","rebase","merge"]}}"#);
    let got = ignored_relaxations(&team, &Parsed::absent(), &l, true);
    assert_eq!(
        got,
        [
            ir(Level::Local, RelaxKey::Permission(Operation::ForcePush)),
            ir(Level::Local, RelaxKey::Permission(Operation::Rebase)),
        ]
    );
    // The value: never below the team.
    let h = harden(
        &team.permissions,
        Personal {
            profile: None,
            local: l.applicable(),
        },
    );
    assert_eq!(h.permission(Operation::ForcePush), Permission::Deny);
    assert_eq!(h.permission(Operation::Rebase), Permission::Ask);
}

#[test]
fn a_local_that_relaxes_the_profile_without_going_below_the_team_is_valid() {
    // Row 3: the team allows, the profile denies, the local allows.
    let (repo, conf) = confirmed_repo("{}");
    let team = load(&repo, &conf);
    let p = profile(r#"{"permissions":{"deny":["merge"]}}"#);
    let l = local(r#"{"permissions":{"allow":["merge"]}}"#);
    assert!(ignored_relaxations(&team, &p, &l, true).is_empty());
    // Empty lists are a union, not a relaxation.
    let empty = local(
        r#"{"permissions":{"allow":[],"deny":[]},"policies":{"protectedBranches":{"patterns":[]}}}"#,
    );
    assert!(ignored_relaxations(&team, &Parsed::absent(), &empty, true).is_empty());
}

#[test]
fn the_profile_value_the_local_covers_is_not_reported() {
    let (repo, conf) = confirmed_repo(DENY_PUSH);
    let team = load(&repo, &conf);
    // The local covers the profile's relaxation with a value that is not below the team.
    let p = profile(r#"{"permissions":{"allow":["push"]}}"#);
    let l = local(r#"{"permissions":{"deny":["push"]}}"#);
    assert!(ignored_relaxations(&team, &p, &l, true).is_empty());
    // Without the local, the profile's own relaxation is reported at its level.
    assert_eq!(
        ignored_relaxations(&team, &p, &Parsed::absent(), true),
        [ir(Level::Profile, RelaxKey::Permission(Operation::Push))]
    );
}

#[test]
fn keys_only_the_floor_may_set_are_reported_at_the_personal_level() {
    let (repo, conf) = confirmed_repo("{}");
    let team = load(&repo, &conf);
    let doc = r#"{"permissions":{"disableSafeMinimum":true},"engine":{"baseBranch":"develop"}}"#;
    assert_eq!(
        ignored_relaxations(&team, &Parsed::absent(), &local(doc), true),
        [
            ir(Level::Local, RelaxKey::SafeMinimum),
            ir(Level::Local, RelaxKey::BaseBranch)
        ]
    );
    // The profile counts when the local does not declare the key; the local covers it otherwise.
    let got = ignored_relaxations(&team, &profile(doc), &Parsed::absent(), true);
    assert_eq!(got[0].level, Level::Profile);
    let got = ignored_relaxations(&team, &profile(doc), &local(doc), true);
    assert!(got.iter().all(|r| r.level == Level::Local));
}

#[test]
fn a_lax_worktree_is_reported_and_changes_nothing() {
    let (repo, conf) = confirmed_repo(DENY_PUSH);
    let x = repo.worktree("feat-x", &repo.head());
    repo.commit_settings_in(&x, LAX);
    let team = TeamLoader::default()
        .load(&repo.reader_at(&x), Some(&conf))
        .unwrap();
    let before = team.clone();
    let got = ignored_relaxations(&team, &Parsed::absent(), &Parsed::absent(), true);
    assert_eq!(
        got,
        [
            ir(Level::Worktree, RelaxKey::Permission(Operation::Push)),
            ir(Level::Worktree, RelaxKey::Permission(Operation::ForcePush)),
            ir(Level::Worktree, RelaxKey::SafeMinimum),
            ir(Level::Worktree, RelaxKey::CommitAuthorship),
        ]
    );
    // Pure: the team level is what it was.
    assert_eq!(team, before);
    assert_eq!(
        team.permissions.permission(Operation::Push),
        Permission::Deny
    );
    assert!(team.permissions.safe_minimum_active);
}

#[test]
fn a_different_base_branch_in_the_worktree_is_reported() {
    let (repo, conf) = confirmed_repo("{}");
    let x = repo.worktree("feat-x", &repo.head());
    repo.commit_settings_in(&x, r#"{"engine":{"baseBranch":"develop"}}"#);
    let team = TeamLoader::default()
        .load(&repo.reader_at(&x), Some(&conf))
        .unwrap();
    assert_eq!(
        ignored_relaxations(&team, &Parsed::absent(), &Parsed::absent(), true),
        [ir(Level::Worktree, RelaxKey::BaseBranch)]
    );
}

#[test]
fn authorship_is_a_relaxation_only_against_a_team_that_did_not_grant_it() {
    let flexible = r#"{"policies":{"commitAuthorship":{"mode":"flexible"}}}"#;
    // Default team: a personal `flexible` is ignored.
    let (repo, conf) = confirmed_repo("{}");
    let team = load(&repo, &conf);
    assert_eq!(
        ignored_relaxations(&team, &profile(flexible), &Parsed::absent(), true),
        [ir(Level::Profile, RelaxKey::CommitAuthorship)]
    );
    // Local before profile.
    assert_eq!(
        ignored_relaxations(&team, &profile(flexible), &local(flexible), true),
        [ir(Level::Local, RelaxKey::CommitAuthorship)]
    );
    // A confirmed floor that already is `flexible`: nothing was relaxed against the team.
    let (repo, conf) = confirmed_repo(flexible);
    let team = load(&repo, &conf);
    assert!(ignored_relaxations(&team, &profile(flexible), &local(flexible), true).is_empty());
    // The same floor unconfirmed (`floor_may_relax = false`): the team's value is the default,
    // and the worktree (the same commit here) declares the same `flexible`.
    assert_eq!(
        ignored_relaxations(&team, &Parsed::absent(), &local(flexible), false),
        [
            ir(Level::Worktree, RelaxKey::CommitAuthorship),
            ir(Level::Local, RelaxKey::CommitAuthorship)
        ]
    );
}

#[test]
fn the_report_is_sorted_by_level_then_key_and_has_no_duplicates() {
    let (repo, conf) = confirmed_repo(r#"{"permissions":{"deny":["push","merge"]}}"#);
    let team = load(&repo, &conf);
    let l = local(r#"{"permissions":{"allow":["push","merge"],"disableSafeMinimum":true}}"#);
    let got = ignored_relaxations(&team, &Parsed::absent(), &l, true);
    assert_eq!(
        got,
        [
            ir(Level::Local, RelaxKey::Permission(Operation::Push)),
            ir(Level::Local, RelaxKey::Permission(Operation::Merge)),
            ir(Level::Local, RelaxKey::SafeMinimum),
        ]
    );
}
