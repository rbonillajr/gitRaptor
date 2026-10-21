//! US-GRD-014 (D3, D4, D5): the pure plan of `guard confirm`, over temporary repos (NFR-01).
//!
//! API surface these tests assume, in `gitraptor_policy::team::confirm` (names from § 6.1 and
//! § 6.3 of the Dev Spec; the implementer provides exactly this):
//! - `plan(&TeamConfig, Option<&Confirmed>) -> Result<ConfirmPlan, Refusal>`;
//! - `ConfirmPlan { base: BaseChange, relaxations: Vec<Relaxation>, fingerprint: Fingerprint, .. }`
//!   and `ConfirmPlan::needs_window(&self) -> bool` (D4: the base differs from the reference or
//!   there is a relaxation);
//! - `BaseChange { from: Option<RefName>, to: RefName }`: `from` is the base of the reference
//!   (the confirmed one, or `main` when nothing is confirmed), `to` the resolved one;
//! - `Relaxation { key: RelaxedKey, before, after }`, `before` and `after` of one type with
//!   `PartialEq + Debug`;
//! - `RelaxedKey::as_str(self) -> &'static str`, the key path of the closed list:
//!   `permissions.allow`, `permissions.ask`, `permissions.deny`,
//!   `permissions.disableSafeMinimum`, `policies.protectedBranches`, `policies.forbiddenPaths`,
//!   `policies.commitAuthorship` (`engine.baseBranch` is the `base` change);
//! - `Fingerprint: Clone + PartialEq + Debug`;
//! - `Refusal::{NothingToConfirm, BaseInvalid, FloorUnreadable, UnshowableRelaxation}` (`Debug`).

mod common;

use common::Repo;
use gitraptor_git::RefName;
use gitraptor_policy::settings::Code;
use gitraptor_policy::team::confirm::{ConfirmPlan, Refusal, plan};
use gitraptor_policy::team::{Confirmed, ConfirmedFloor, TeamConfig, TeamLoader};

fn r(name: &str) -> RefName {
    RefName::new(name).unwrap()
}

fn load(repo: &Repo, conf: Option<&Confirmed>) -> TeamConfig {
    TeamLoader::default().load(&repo.reader(), conf).unwrap()
}

/// `origin/main` at a new commit with `settings` as the team settings.
fn publish(repo: &Repo, settings: &str) -> String {
    let commit = repo.commit_settings(settings);
    repo.set_origin_main(&commit);
    commit
}

/// A repo whose `origin/main` holds `settings`, confirmed with base `main`.
fn confirmed_repo(settings: &str) -> (Repo, Confirmed) {
    let repo = Repo::new();
    let commit = publish(&repo, settings);
    let conf = Confirmed {
        base_branch: r("main"),
        floor: ConfirmedFloor::Blob(repo.settings_blob(&commit)),
    };
    (repo, conf)
}

fn plan_of(repo: &Repo, conf: Option<&Confirmed>) -> ConfirmPlan {
    let config = load(repo, conf);
    match plan(&config, conf) {
        Ok(p) => p,
        Err(e) => panic!("expected a plan, got {e:?}"),
    }
}

fn keys(p: &ConfirmPlan) -> Vec<&'static str> {
    p.relaxations.iter().map(|x| x.key.as_str()).collect()
}

#[test]
fn every_closed_list_key_is_shown_when_it_relaxes() {
    // (confirmed floor, new floor, keys the plan must name)
    let cases: [(&str, &str, &[&str]); 7] = [
        (
            r#"{"permissions":{"deny":["push"]}}"#,
            r#"{}"#,
            &["permissions.deny"],
        ),
        (
            r#"{"permissions":{"ask":["merge"]}}"#,
            r#"{}"#,
            &["permissions.ask"],
        ),
        (
            r#"{"permissions":{"deny":["rebase"]}}"#,
            r#"{"permissions":{"allow":["rebase"]}}"#,
            &["permissions.allow", "permissions.deny"],
        ),
        (
            r#"{}"#,
            r#"{"permissions":{"disableSafeMinimum":true}}"#,
            &["permissions.disableSafeMinimum"],
        ),
        (
            r#"{"policies":{"protectedBranches":{"patterns":["release/*"]}}}"#,
            r#"{}"#,
            &["policies.protectedBranches"],
        ),
        (
            r#"{"policies":{"forbiddenPaths":{"patterns":["secrets/"]}}}"#,
            r#"{}"#,
            &["policies.forbiddenPaths"],
        ),
        (
            r#"{"policies":{"commitAuthorship":{"mode":"human-author"}}}"#,
            r#"{"policies":{"commitAuthorship":{"mode":"flexible"}}}"#,
            &["policies.commitAuthorship"],
        ),
    ];
    for (before, after, expected) in cases {
        let (repo, conf) = confirmed_repo(before);
        publish(&repo, after);
        let p = plan_of(&repo, Some(&conf));
        let shown = keys(&p);
        for key in expected {
            assert!(shown.contains(key), "{before} -> {after}: {shown:?}");
        }
        for rel in &p.relaxations {
            assert_ne!(
                rel.before,
                rel.after,
                "{before} -> {after}: {:?}",
                rel.key.as_str()
            );
        }
        assert!(p.needs_window(), "{before} -> {after}");
        assert_eq!(p.base.to, r("main"));
    }

    // engine.baseBranch: shown with its before and after as the base change.
    let (repo, conf) = confirmed_repo(r#"{}"#);
    publish(&repo, r#"{"engine":{"baseBranch":"develop"}}"#);
    let p = plan_of(&repo, Some(&conf));
    assert_eq!(p.base.from, Some(r("main")));
    assert_eq!(p.base.to, r("develop"));
    assert!(p.needs_window());
}

#[test]
fn removing_a_protected_branch_pattern_is_floor_relax_pending() {
    let (repo, conf) = confirmed_repo(
        r#"{"policies":{"protectedBranches":{"patterns":["release/*","hotfix/*"]}}}"#,
    );
    publish(
        &repo,
        r#"{"policies":{"protectedBranches":{"patterns":["hotfix/*"]}}}"#,
    );
    let config = load(&repo, Some(&conf));
    assert!(
        config.has(Code::FloorRelaxPending),
        "{:?}",
        config.team_diagnostics()
    );
    let p = plan(&config, Some(&conf)).expect("a plan");
    assert!(
        keys(&p).contains(&"policies.protectedBranches"),
        "{:?}",
        keys(&p)
    );
    assert!(p.needs_window());

    // Adding a pattern only hardens: no pending relaxation, nothing to confirm.
    let (repo, conf) =
        confirmed_repo(r#"{"policies":{"protectedBranches":{"patterns":["hotfix/*"]}}}"#);
    publish(
        &repo,
        r#"{"policies":{"protectedBranches":{"patterns":["hotfix/*","release/*"]}}}"#,
    );
    let config = load(&repo, Some(&conf));
    assert!(!config.has(Code::FloorRelaxPending));
}

#[test]
fn a_floor_that_only_hardens_needs_no_confirmation() {
    let (repo, conf) = confirmed_repo(r#"{"permissions":{"deny":["push"]}}"#);
    publish(
        &repo,
        r#"{"permissions":{"deny":["push","merge"]},"policies":{"forbiddenPaths":{"patterns":["secrets/"]}}}"#,
    );
    let config = load(&repo, Some(&conf));
    let got = plan(&config, Some(&conf));
    assert!(
        matches!(got, Err(Refusal::NothingToConfirm)),
        "{:?}",
        got.err()
    );

    // The same floor as the confirmed one: nothing to confirm either.
    let (repo, conf) = confirmed_repo(r#"{"engine":{"baseBranch":"main"}}"#);
    let got = plan(&load(&repo, Some(&conf)), Some(&conf));
    assert!(
        matches!(got, Err(Refusal::NothingToConfirm)),
        "{:?}",
        got.err()
    );
}

#[test]
fn a_base_other_than_the_reference_needs_the_window() {
    // Nothing confirmed: the reference is `main` with the floor `Absent`.
    let repo = Repo::new();
    publish(&repo, r#"{"permissions":{"deny":["push"]}}"#);
    let p = plan_of(&repo, None);
    assert_eq!(p.base.from, Some(r("main")));
    assert_eq!(p.base.to, r("main"));
    assert!(p.relaxations.is_empty(), "{:?}", keys(&p));
    assert!(
        !p.needs_window(),
        "a hardening floor on main applies at once"
    );

    // Against an Absent reference, disabling the minimum is a relaxation.
    let repo = Repo::new();
    publish(&repo, r#"{"permissions":{"disableSafeMinimum":true}}"#);
    let p = plan_of(&repo, None);
    assert!(keys(&p).contains(&"permissions.disableSafeMinimum"));
    assert!(p.needs_window());

    // A first `develop`: no relaxation, but it stops guarding `main`, so it waits.
    let repo = Repo::new();
    publish(&repo, r#"{"engine":{"baseBranch":"develop"}}"#);
    let p = plan_of(&repo, None);
    assert_eq!(p.base.from, Some(r("main")));
    assert_eq!(p.base.to, r("develop"));
    assert!(p.relaxations.is_empty(), "{:?}", keys(&p));
    assert!(p.needs_window());

    // A confirmed `develop` moving to `release` waits too.
    let (repo, conf) = confirmed_repo(r#"{"engine":{"baseBranch":"develop"}}"#);
    let conf = Confirmed {
        base_branch: r("develop"),
        ..conf
    };
    publish(&repo, r#"{"engine":{"baseBranch":"release"}}"#);
    let p = plan_of(&repo, Some(&conf));
    assert_eq!(p.base.from, Some(r("develop")));
    assert_eq!(p.base.to, r("release"));
    assert!(p.needs_window());
}

#[test]
fn invalid_base_and_unreadable_floor_are_refused() {
    for bad in ["refs/heads/develop", "--upload-pack=x"] {
        let settings = format!(r#"{{"engine":{{"baseBranch":"{bad}"}}}}"#);
        // Unconfirmed (base-unconfirmed with an invalid name).
        let repo = Repo::new();
        publish(&repo, &settings);
        let got = plan(&load(&repo, None), None);
        assert!(
            matches!(got, Err(Refusal::BaseInvalid)),
            "{bad}: {:?}",
            got.err()
        );
        // Confirmed `main` (base-change-pending-invalid): the rest of the floor is not confirmed.
        let (repo, conf) = confirmed_repo(r#"{}"#);
        publish(&repo, &settings);
        let got = plan(&load(&repo, Some(&conf)), Some(&conf));
        assert!(
            matches!(got, Err(Refusal::BaseInvalid)),
            "{bad}: {:?}",
            got.err()
        );
    }

    // An ignored floor (a symlink in place of the settings).
    let (repo, conf) = confirmed_repo(r#"{}"#);
    let target = repo.hash_blob(b"/etc/hosts");
    let link = repo.commit_entry("120000", &target);
    repo.set_origin_main(&link);
    for c in [None, Some(&conf)] {
        let got = plan(&load(&repo, c), c);
        assert!(
            matches!(got, Err(Refusal::FloorUnreadable)),
            "{:?}",
            got.err()
        );
    }

    // A partial floor (an unknown policy).
    let repo = Repo::new();
    publish(
        &repo,
        r#"{"engine":{"baseBranch":"develop"},"policies":{"maxDiffLines":10}}"#,
    );
    let got = plan(&load(&repo, None), None);
    assert!(
        matches!(got, Err(Refusal::FloorUnreadable)),
        "{:?}",
        got.err()
    );
}

#[test]
fn the_fingerprint_covers_source_floor_base_and_previous() {
    let repo = Repo::new();
    publish(&repo, r#"{"engine":{"baseBranch":"develop"}}"#);
    let first = plan_of(&repo, None).fingerprint;
    // Stable while nothing changes.
    assert_eq!(plan_of(&repo, None).fingerprint, first);

    // Another source commit with the same floor blob.
    repo.git(&["commit", "-q", "--allow-empty", "-m", "unrelated"]);
    repo.set_origin_main(&repo.head());
    let other_commit = plan_of(&repo, None).fingerprint;
    assert_ne!(other_commit, first);

    // Another floor blob with the same resolved base.
    publish(
        &repo,
        r#"{"engine":{"baseBranch":"develop"},"permissions":{"deny":["push"]}}"#,
    );
    let other_blob = plan_of(&repo, None).fingerprint;
    assert_ne!(other_blob, other_commit);

    // Another resolved base.
    publish(&repo, r#"{"engine":{"baseBranch":"release"}}"#);
    let other_base = plan_of(&repo, None).fingerprint;
    assert_ne!(other_base, other_blob);

    // The same repo state, but something else confirmed before.
    let main_absent = Confirmed {
        base_branch: r("main"),
        floor: ConfirmedFloor::Absent,
    };
    let after_main = plan_of(&repo, Some(&main_absent)).fingerprint;
    assert_ne!(after_main, other_base);
    let develop_absent = Confirmed {
        base_branch: r("develop"),
        floor: ConfirmedFloor::Absent,
    };
    let after_develop = plan_of(&repo, Some(&develop_absent)).fingerprint;
    assert_ne!(after_develop, after_main);
    assert_eq!(
        plan_of(&repo, Some(&develop_absent)).fingerprint,
        after_develop
    );
}
