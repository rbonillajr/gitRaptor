//! US-GRD-014, D4 amended: confirming waits for the window also when a branch guarded today
//! stops being guarded. A repo whose main branch is `master`, with nothing confirmed and the team
//! base `main`, guards {`main`, `master`} today and only `main` afterwards. Temporary repos only
//! (NFR-01).

mod common;

use common::Repo;
use gitraptor_git::RefName;
use gitraptor_policy::team::TeamLoader;
use gitraptor_policy::team::confirm::plan;

fn r(name: &str) -> RefName {
    RefName::new(name).unwrap()
}

/// `origin/<principal>` at a commit with `settings`, and `origin/HEAD` naming it.
fn repo_with_principal(principal: &str, settings: &str) -> Repo {
    let repo = Repo::new();
    let commit = repo.commit_settings(settings);
    repo.set_origin_main(&commit);
    repo.git(&[
        "update-ref",
        &format!("refs/remotes/origin/{principal}"),
        &commit,
    ]);
    repo.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        &format!("refs/remotes/origin/{principal}"),
    ]);
    repo
}

#[test]
fn a_principal_that_stops_being_guarded_waits_for_the_window() {
    let repo = repo_with_principal("master", r#"{"engine":{"baseBranch":"main"}}"#);
    let config = TeamLoader::default().load(&repo.reader(), None).unwrap();
    assert!(config.guarded_base_branches().contains(&r("master")));
    let p = plan(&config, None).expect("a plan");
    assert_eq!(p.base.from, Some(r("main")));
    assert_eq!(p.base.to, r("main"));
    assert!(p.relaxations.is_empty());
    assert_eq!(p.base.unguarded, [r("master")]);
    assert!(p.needs_window());
}

#[test]
fn with_main_as_principal_nothing_stops_being_guarded() {
    let repo = repo_with_principal("main", r#"{"permissions":{"deny":["push"]}}"#);
    let config = TeamLoader::default().load(&repo.reader(), None).unwrap();
    let p = plan(&config, None).expect("a plan");
    assert!(p.base.unguarded.is_empty(), "{:?}", p.base.unguarded);
    assert!(!p.needs_window());
}

#[test]
fn the_fingerprint_covers_what_stops_being_guarded() {
    let settings = r#"{"engine":{"baseBranch":"main"}}"#;
    let master = repo_with_principal("master", settings);
    let a = plan(
        &TeamLoader::default().load(&master.reader(), None).unwrap(),
        None,
    )
    .unwrap();
    // The same repo once the principal is `main`: nothing else is guarded.
    master.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/main",
    ]);
    let b = plan(
        &TeamLoader::default().load(&master.reader(), None).unwrap(),
        None,
    )
    .unwrap();
    assert!(b.base.unguarded.is_empty());
    assert_ne!(a.fingerprint, b.fingerprint);
}
