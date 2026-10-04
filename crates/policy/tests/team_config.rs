//! TS-GRD-001: committed team settings, floor on the main branch and confirmed base branch.
//! One test per criterion of the TS Plan de Verificación (and ADR-GRD-004 Validación).

mod common;

use common::Repo;
use gitraptor_git::RefName;
use gitraptor_policy::settings::{Code, Location, Operation, SourceKind, SourceStatus};
use gitraptor_policy::team::{
    BaseStatus, Confirmed, ConfirmedFloor, Permission, ResolvedBase, TeamConfig, TeamLoader,
    affects_team_config,
};

const DENY_PUSH: &str = r#"{"permissions":{"deny":["push"]}}"#;
const LAX: &str = r#"{"permissions":{"allow":["push","force-push"],"disableSafeMinimum":true}}"#;

fn r(name: &str) -> RefName {
    RefName::new(name).unwrap()
}

fn names(c: &TeamConfig) -> Vec<String> {
    c.guarded_base_branches()
        .iter()
        .map(ToString::to_string)
        .collect()
}

fn confirmed(base: &str, floor: Option<String>) -> Confirmed {
    Confirmed {
        base_branch: r(base),
        floor: floor.map_or(ConfirmedFloor::Absent, ConfirmedFloor::Blob),
    }
}

/// A repo whose `origin/main` holds `settings`, confirmed by the developer.
fn confirmed_repo(settings: &str) -> (Repo, Confirmed) {
    let repo = Repo::new();
    let commit = repo.commit_settings(settings);
    repo.set_origin_main(&commit);
    let conf = confirmed("main", Some(repo.settings_blob(&commit)));
    (repo, conf)
}

fn load(repo: &Repo, conf: Option<&Confirmed>) -> TeamConfig {
    TeamLoader::default().load(&repo.reader(), conf).unwrap()
}

// ── Sin commitear ──────────────────────────────────────────────────────────────────────────

#[test]
fn uncommitted_edit_or_conflict_does_not_change_the_team_level() {
    let (repo, conf) = confirmed_repo(DENY_PUSH);
    repo.write(&repo.path, ".gitraptor/settings.json", LAX);
    let c = load(&repo, Some(&conf));
    assert_eq!(c.permissions.permission(Operation::Push), Permission::Deny);
    assert!(c.permissions.safe_minimum_active);

    // A conflicted, unparseable file in the working tree is not "unreadable".
    repo.write(
        &repo.path,
        ".gitraptor/settings.json",
        "<<<<<<< ours\n{}\n=======\n{\"x\":1}\n>>>>>>> theirs\n",
    );
    let c = load(&repo, Some(&conf));
    assert_eq!(c.floor.status(), SourceStatus::Readable);
    assert_eq!(c.worktree.status(), SourceStatus::Readable);
    assert!(!c.has(Code::InvalidJson));
}

#[test]
fn a_real_merge_conflict_in_the_worktree_is_not_read() {
    let (repo, conf) = confirmed_repo(DENY_PUSH);
    let base = repo.head();
    repo.git(&["checkout", "-q", "-b", "a"]);
    repo.commit_settings(r#"{"permissions":{"deny":["push","merge"]}}"#);
    repo.git(&["checkout", "-q", "-b", "b", &base]);
    repo.commit_settings(r#"{"permissions":{"deny":["push","rebase"]}}"#);
    let out = repo
        .command(&repo.path, &["merge", "-q", "a"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "the merge must conflict");
    let c = load(&repo, Some(&conf));
    assert_eq!(c.worktree.status(), SourceStatus::Readable);
    assert_eq!(
        c.permissions.permission(Operation::Rebase),
        Permission::Deny
    );
    assert_eq!(
        c.permissions.permission(Operation::Merge),
        Permission::Allow
    );
}

// ── Por worktree (D6) ──────────────────────────────────────────────────────────────────────

#[test]
fn each_worktree_adds_its_own_hardening_and_never_relaxes() {
    let (repo, conf) = confirmed_repo(DENY_PUSH);
    let main = repo.head();
    let x = repo.worktree("feat-x", &main);
    let y = repo.worktree("feat-y", &main);
    repo.commit_settings_in(&x, r#"{"permissions":{"deny":["push","merge"]}}"#);
    repo.commit_settings_in(&y, r#"{"permissions":{"deny":["push","rebase"]}}"#);
    let loader = TeamLoader::default();
    let cx = loader.load(&repo.reader_at(&x), Some(&conf)).unwrap();
    let cy = loader.load(&repo.reader_at(&y), Some(&conf)).unwrap();
    assert_eq!(
        cx.permissions.permission(Operation::Merge),
        Permission::Deny
    );
    assert_eq!(
        cx.permissions.permission(Operation::Rebase),
        Permission::Allow
    );
    assert_eq!(
        cy.permissions.permission(Operation::Rebase),
        Permission::Deny
    );
    assert_eq!(
        cy.permissions.permission(Operation::Merge),
        Permission::Allow
    );

    // A relaxation committed on the worktree's branch has no effect.
    repo.commit_settings_in(&x, LAX);
    let cx = loader.load(&repo.reader_at(&x), Some(&conf)).unwrap();
    assert!(cx.permissions.safe_minimum_active);
    assert_eq!(cx.permissions.permission(Operation::Push), Permission::Deny);
    assert_eq!(
        cx.permissions.permission(Operation::ForcePush),
        Permission::Deny
    );
    let floor_only: Vec<_> = cx
        .diagnostics()
        .filter(|d| d.code == Code::FloorOnlyKey)
        .collect();
    assert_eq!(floor_only.len(), 1);
    assert_eq!(floor_only[0].source, SourceKind::Worktree);
}

#[test]
fn an_old_checkout_or_an_orphan_branch_keeps_the_floor_rules() {
    let (repo, conf) = confirmed_repo(DENY_PUSH);
    let initial = repo.git(&["rev-list", "--max-parents=0", "HEAD"]);
    repo.git(&["checkout", "-q", "--detach", &initial]);
    let c = load(&repo, Some(&conf));
    assert_eq!(c.worktree.status(), SourceStatus::Absent);
    assert_eq!(c.permissions.permission(Operation::Push), Permission::Deny);

    repo.git(&["checkout", "-q", "--orphan", "empty"]);
    let c = load(&repo, Some(&conf));
    assert_eq!(c.worktree.status(), SourceStatus::Absent, "unborn HEAD");
    assert_eq!(c.permissions.permission(Operation::Push), Permission::Deny);
}

#[test]
fn a_hardening_on_the_main_branch_applies_in_every_worktree() {
    let (repo, conf) = confirmed_repo(DENY_PUSH);
    let x = repo.worktree("feat-x", &repo.head());
    let stricter = repo.commit_settings(r#"{"permissions":{"deny":["push","commit"]}}"#);
    repo.set_origin_main(&stricter);
    let c = TeamLoader::default()
        .load(&repo.reader_at(&x), Some(&conf))
        .unwrap();
    assert_eq!(
        c.permissions.permission(Operation::Commit),
        Permission::Deny
    );
    assert!(!c.has(Code::FloorRelaxPending));
}

// ── Suelo forjado (D7, J3) ─────────────────────────────────────────────────────────────────

#[test]
fn a_forged_lax_floor_keeps_the_most_restrictive_combination() {
    let (repo, conf) = confirmed_repo(DENY_PUSH);
    let lax = repo.commit_settings(LAX);
    repo.git(&["update-ref", "refs/remotes/origin/main", &lax]);
    repo.git(&["checkout", "-q", "--detach", "HEAD~1"]);
    let c = load(&repo, Some(&conf));
    assert!(c.has(Code::FloorRelaxPending));
    assert!(c.permissions.safe_minimum_active);
    assert_eq!(c.permissions.permission(Operation::Push), Permission::Deny);
    assert_eq!(
        c.permissions.permission(Operation::ForcePush),
        Permission::Deny
    );
    assert!(c.permissions.operations[&Operation::Push].sources.contains(
        &gitraptor_policy::team::RuleSource::Source(SourceKind::ConfirmedFloor)
    ));

    // Once the developer confirms it, the new floor rules (from a worktree that does not
    // harden push itself).
    repo.git(&["checkout", "-q", "--detach", &lax]);
    let accepted = confirmed("main", Some(repo.settings_blob(&lax)));
    let c = load(&repo, Some(&accepted));
    assert!(!c.has(Code::FloorRelaxPending));
    assert!(!c.permissions.safe_minimum_active);
    assert_eq!(c.permissions.permission(Operation::Push), Permission::Allow);
}

#[test]
fn a_forged_floor_without_settings_or_with_broken_settings_is_a_relaxation() {
    let (repo, conf) = confirmed_repo(DENY_PUSH);
    let base = repo.git(&["rev-list", "--max-parents=0", "HEAD"]);
    repo.git(&["update-ref", "refs/remotes/origin/main", &base]);
    let c = load(&repo, Some(&conf));
    assert_eq!(c.floor.status(), SourceStatus::Absent);
    assert!(c.has(Code::FloorRelaxPending));
    assert_eq!(c.permissions.permission(Operation::Push), Permission::Deny);

    let broken = repo.commit_settings("{ not json");
    repo.git(&["update-ref", "refs/remotes/origin/main", &broken]);
    let c = load(&repo, Some(&conf));
    assert_eq!(c.floor.status(), SourceStatus::Ignored);
    assert!(c.has(Code::FloorRelaxPending));
    assert_eq!(c.permissions.permission(Operation::Push), Permission::Deny);
}

#[test]
fn without_confirmation_the_floor_only_hardens() {
    let repo = Repo::new();
    let lax = repo.commit_settings(
        r#"{"permissions":{"deny":["merge"],"allow":["force-push"],"disableSafeMinimum":true}}"#,
    );
    repo.set_origin_main(&lax);
    let c = load(&repo, None);
    assert!(c.has(Code::BaseUnconfirmed));
    assert!(c.permissions.safe_minimum_active);
    assert_eq!(
        c.permissions.permission(Operation::ForcePush),
        Permission::Deny
    );
    assert_eq!(c.permissions.permission(Operation::Merge), Permission::Deny);
}

#[test]
fn a_confirmed_floor_lost_to_gc_leaves_the_floor_hardening_only() {
    let (repo, _) =
        confirmed_repo(r#"{"permissions":{"deny":["merge"],"disableSafeMinimum":true}}"#);
    let gone = confirmed("main", Some("1".repeat(40)));
    let c = load(&repo, Some(&gone));
    assert!(c.has(Code::ConfirmedFloorMissing));
    assert!(c.permissions.safe_minimum_active);
    assert_eq!(c.permissions.permission(Operation::Merge), Permission::Deny);
}

// ── Cambio de rama base (D6) ───────────────────────────────────────────────────────────────

#[test]
fn a_base_change_keeps_the_confirmed_branch_and_guards_both() {
    let (repo, conf) = confirmed_repo("{}");
    let dev = repo.commit_settings(r#"{"engine":{"baseBranch":"develop"}}"#);
    repo.set_origin_main(&dev);
    let c = load(&repo, Some(&conf));
    assert_eq!(c.base_branch().name, Some(r("main")));
    assert_eq!(c.base_branch().status, BaseStatus::Confirmed);
    assert_eq!(c.resolved_base, ResolvedBase::Valid(r("develop")));
    assert!(c.has(Code::BaseChangePending { invalid: false }));
    assert_eq!(names(&c), ["main", "develop"]);

    let after = confirmed("develop", Some(repo.settings_blob(&dev)));
    let c = load(&repo, Some(&after));
    assert_eq!(c.base_branch().name, Some(r("develop")));
    assert_eq!(names(&c), ["develop"]);
    assert!(!c.has(Code::BaseChangePending { invalid: false }));
}

#[test]
fn an_invalid_base_branch_never_reaches_git() {
    let repo = Repo::new();
    let bad = repo.commit_settings(r#"{"engine":{"baseBranch":"--upload-pack=x"}}"#);
    repo.set_origin_main(&bad);
    // Q42: nothing confirmed → no base branch at all.
    let c = load(&repo, None);
    assert_eq!(c.base_branch().name, None);
    assert_eq!(c.base_branch().status, BaseStatus::Invalid);
    assert!(c.has(Code::InvalidBaseBranch));
    assert_eq!(names(&c), ["main"]);
    // With `main` confirmed: the engine keeps `main`, a pending invalid change is reported and
    // Guardrails guards {confirmed, main, main branch}.
    let c = load(&repo, Some(&confirmed("main", None)));
    assert_eq!(c.base_branch().name, Some(r("main")));
    assert!(c.has(Code::BaseChangePending { invalid: true }));
    assert_eq!(names(&c), ["main"]);
    let c = load(&repo, Some(&confirmed("release", None)));
    assert_eq!(names(&c), ["release", "main"]);
}

#[test]
fn unconfirmed_base_is_the_resolved_one_marked_and_guards_the_union() {
    let repo = Repo::new();
    repo.git(&["remote", "add", "origin", "/nonexistent/origin.git"]);
    let dev = repo.commit_settings(r#"{"engine":{"baseBranch":"develop"}}"#);
    repo.git(&["update-ref", "refs/remotes/origin/trunk", &dev]);
    repo.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/trunk",
    ]);
    let c = load(&repo, None);
    assert_eq!(c.base_branch().name, Some(r("develop")));
    assert_eq!(c.base_branch().status, BaseStatus::Unconfirmed);
    assert!(c.has(Code::BaseUnconfirmed));
    assert_eq!(names(&c), ["main", "trunk", "develop"]);
}

// ── Objetos de reemplazo y grafts (SEC-GRD-17) ─────────────────────────────────────────────

#[test]
fn replacement_objects_and_grafts_do_not_change_the_document() {
    let (repo, conf) = confirmed_repo(DENY_PUSH);
    let floor_commit = repo.git(&["rev-parse", "refs/remotes/origin/main"]);
    let blob = repo.settings_blob(&floor_commit);
    let lax = repo.hash_blob(LAX.as_bytes());
    repo.git(&["replace", &blob, &lax]);
    assert_eq!(
        repo.git(&["cat-file", "-p", &blob]),
        LAX,
        "Git sees the lax blob"
    );
    // A whole lax commit replacing the floor commit, and a graft.
    let lax_commit = repo.commit_settings(LAX);
    repo.git(&["checkout", "-q", "--detach", &floor_commit]);
    repo.git(&["replace", &floor_commit, &lax_commit]);
    std::fs::create_dir_all(repo.path.join(".git/info")).unwrap();
    std::fs::write(
        repo.path.join(".git/info/grafts"),
        format!("{floor_commit} {lax_commit}\n"),
    )
    .unwrap();
    // The inverted `core.useReplaceRefs` of gix 0.88 cannot turn them back on.
    repo.git(&["config", "core.useReplaceRefs", "false"]);
    let c = load(&repo, Some(&conf));
    assert_eq!(c.floor.blob.as_deref(), Some(blob.as_str()));
    assert!(!c.has(Code::FloorRelaxPending));
    assert!(c.permissions.safe_minimum_active);
    assert_eq!(c.permissions.permission(Operation::Push), Permission::Deny);
}

// ── Rama principal (decisión 2) ────────────────────────────────────────────────────────────

#[test]
fn main_branch_resolution_order() {
    // Remote HEAD → origin/trunk.
    let repo = Repo::new();
    let commit = repo.commit_settings(DENY_PUSH);
    repo.git(&["remote", "add", "origin", "/nonexistent/origin.git"]);
    repo.git(&["update-ref", "refs/remotes/origin/trunk", &commit]);
    repo.git(&[
        "symbolic-ref",
        "refs/remotes/origin/HEAD",
        "refs/remotes/origin/trunk",
    ]);
    let c = load(&repo, None);
    assert_eq!(c.main_branch.name, r("trunk"));
    assert_eq!(
        c.main_branch.copy.as_ref().unwrap().reference,
        r("refs/remotes/origin/trunk")
    );

    // No remote → the local branch.
    let repo = Repo::new();
    repo.commit_settings(DENY_PUSH);
    let c = load(&repo, None);
    assert_eq!(c.main_branch.remote, None);
    assert_eq!(
        c.main_branch.copy.as_ref().unwrap().reference,
        r("refs/heads/main")
    );
    assert_eq!(c.floor.status(), SourceStatus::Readable);

    // Several remotes and none is `origin` → no remote, the local branch.
    repo.git(&["remote", "add", "a", "/nonexistent/a.git"]);
    repo.git(&["remote", "add", "b", "/nonexistent/b.git"]);
    let empty = repo.git(&["rev-list", "--max-parents=0", "HEAD"]);
    repo.git(&["update-ref", "refs/remotes/a/main", &empty]);
    let c = load(&repo, None);
    assert_eq!(c.main_branch.remote, None);
    assert_eq!(
        c.main_branch.copy.as_ref().unwrap().reference,
        r("refs/heads/main")
    );

    // A single remote other than `origin` is the remote.
    let repo = Repo::new();
    repo.git(&["remote", "add", "upstream", "/nonexistent/u.git"]);
    let commit = repo.commit_settings(DENY_PUSH);
    repo.git(&["update-ref", "refs/remotes/upstream/main", &commit]);
    let c = load(&repo, None);
    assert_eq!(c.main_branch.remote, Some(r("upstream")));
    assert_eq!(
        c.main_branch.copy.as_ref().unwrap().reference,
        r("refs/remotes/upstream/main")
    );

    // Nothing at all → `main` by default and an absent floor.
    let repo = Repo::new();
    repo.git(&["branch", "-m", "work"]);
    let c = load(&repo, None);
    assert_eq!(c.main_branch.name, r("main"));
    assert_eq!(c.main_branch.copy, None);
    assert_eq!(c.floor.status(), SourceStatus::Absent);
    assert_eq!(c.base_branch().name, Some(r("main")));
}

#[test]
fn a_bare_repository_reads_the_floor_the_same_way() {
    let repo = Repo::new();
    repo.commit_settings(DENY_PUSH);
    let bare = repo.root.join("bare.git");
    repo.git(&[
        "clone",
        "-q",
        "--bare",
        repo.path.to_str().unwrap(),
        bare.to_str().unwrap(),
    ]);
    let c = TeamLoader::default()
        .load(&repo.reader_at(&bare), Some(&confirmed("main", None)))
        .unwrap();
    assert_eq!(c.floor.status(), SourceStatus::Readable);
    assert_eq!(c.permissions.permission(Operation::Push), Permission::Deny);
}

#[test]
fn a_local_commit_without_push_does_not_change_the_floor() {
    let (repo, conf) = confirmed_repo(r#"{"engine":{"baseBranch":"main"}}"#);
    repo.commit_settings(r#"{"engine":{"baseBranch":"develop"},"permissions":{"allow":["push"]}}"#);
    let c = load(&repo, Some(&conf));
    assert_eq!(
        c.main_branch.copy.as_ref().unwrap().reference,
        r("refs/remotes/origin/main")
    );
    assert_eq!(c.resolved_base, ResolvedBase::Valid(r("main")));
    assert!(!c.has(Code::BaseChangePending { invalid: false }));
    assert!(!c.has(Code::FloorRelaxPending));
}

// ── Dos consumidores ───────────────────────────────────────────────────────────────────────

#[test]
fn engine_and_guardrails_get_the_same_confirmed_base_and_source_state() {
    let (repo, conf) = confirmed_repo("{}");
    let dev = repo.commit_settings(r#"{"engine":{"baseBranch":"develop"}}"#);
    repo.set_origin_main(&dev);
    let other = repo.worktree("feat-x", &dev);
    // Two independent loads, as the engine (ahead/behind) and Guardrails would make them.
    let engine = TeamLoader::default()
        .load(&repo.reader(), Some(&conf))
        .unwrap();
    let guardrails = TeamLoader::default()
        .load(&repo.reader_at(&other), Some(&conf))
        .unwrap();
    assert_eq!(engine.base_branch(), guardrails.base_branch());
    assert_eq!(engine.base_branch().name, Some(r("main")));
    assert_eq!(engine.floor.status(), guardrails.floor.status());
    assert_eq!(engine.floor.blob, guardrails.floor.blob);
    // Only Guardrails reads the wider set.
    assert_eq!(names(&guardrails), ["main", "develop"]);
}

// ── Entradas hostiles (SEC-11, L-03) ───────────────────────────────────────────────────────

fn assert_ignored_without_content(c: &TeamConfig, code: Code) {
    assert_eq!(c.floor.status(), SourceStatus::Ignored);
    assert!(c.has(code), "{:?}", c.floor.parsed.diagnostics);
    assert!(c.permissions.safe_minimum_active);
    for d in c.diagnostics() {
        assert!(!matches!(&d.location, Some(Location::Pointer(p)) if p.contains("hosts")));
    }
}

#[test]
fn hostile_entries_give_an_ignored_floor_and_the_fail_safe_union() {
    let repo = Repo::new();
    let target = repo.hash_blob(b"/etc/hosts");
    let link = repo.commit_entry("120000", &target);
    repo.set_origin_main(&link);
    let c = load(&repo, Some(&confirmed("release", None)));
    assert_ignored_without_content(&c, Code::Symlink);
    // Fail-safe (§ 3.6): {main, main branch, confirmed}; the engine keeps the confirmed one.
    assert_eq!(names(&c), ["main", "release"]);
    assert_eq!(c.base_branch().name, Some(r("release")));

    let head = repo.head();
    let submodule = repo.commit_entry("160000", &head);
    repo.set_origin_main(&submodule);
    assert_ignored_without_content(&load(&repo, None), Code::Submodule);

    let huge = repo.hash_blob(format!("{{\"x\":\"{}\"}}", "a".repeat(10 << 20)).as_bytes());
    let big = repo.commit_entry("100644", &huge);
    repo.set_origin_main(&big);
    assert_ignored_without_content(
        &load(&repo, None),
        Code::LimitExceeded(gitraptor_policy::settings::Limit::Size),
    );

    let deep = format!("{}{}", "[".repeat(1000), "]".repeat(1000));
    let deep = repo.commit_entry("100644", &repo.hash_blob(deep.as_bytes()));
    repo.set_origin_main(&deep);
    assert_ignored_without_content(
        &load(&repo, None),
        Code::LimitExceeded(gitraptor_policy::settings::Limit::Depth),
    );
}

#[test]
fn a_partial_floor_forces_the_minimum() {
    let (repo, conf) = confirmed_repo(
        r#"{"permissions":{"deny":["push"],"disableSafeMinimum":true},"policies":{"maxDiffLines":10}}"#,
    );
    let c = load(&repo, Some(&conf));
    assert_eq!(c.floor.status(), SourceStatus::Partial);
    assert!(c.has(Code::PolicyNotSupported));
    assert!(c.permissions.safe_minimum_active);
    assert_eq!(c.permissions.permission(Operation::Push), Permission::Deny);
}

// ── Sin escrituras ni red; recarga; caché; presupuesto ─────────────────────────────────────

#[test]
fn loading_writes_nothing() {
    let (repo, conf) = confirmed_repo(DENY_PUSH);
    let x = repo.worktree("feat-x", &repo.head());
    let before = repo.fingerprint();
    let loader = TeamLoader::default();
    for _ in 0..3 {
        loader.load(&repo.reader(), Some(&conf)).unwrap();
        loader.load(&repo.reader_at(&x), None).unwrap();
    }
    assert_eq!(before, repo.fingerprint());
}

/// "Sin red" by construction: gix is built without any network transport (ADR-GRP-001/009).
#[test]
fn gix_has_no_network_feature() {
    let manifest = include_str!("../../../Cargo.toml");
    let gix = manifest
        .lines()
        .find(|l| l.starts_with("gix ="))
        .expect("gix in workspace dependencies");
    assert!(gix.contains("default-features = false"), "{gix}");
    for network in [
        "blocking-network-client",
        "async-network-client",
        "http",
        "curl",
        "reqwest",
    ] {
        assert!(!gix.contains(network), "network feature {network} in {gix}");
    }
}

#[test]
fn a_ref_change_reloads_the_floor() {
    let (repo, conf) = confirmed_repo(DENY_PUSH);
    let loader = TeamLoader::default();
    let before = loader.load(&repo.reader(), Some(&conf)).unwrap();
    assert_eq!(
        before.permissions.permission(Operation::Merge),
        Permission::Allow
    );
    // The watcher reports the ref that changed; the daemon reloads when it matters.
    let stricter = repo.commit_settings(r#"{"permissions":{"deny":["push","merge"]}}"#);
    repo.set_origin_main(&stricter);
    let event = "refs/remotes/origin/main";
    assert!(affects_team_config(event));
    let after = loader.load(&repo.reader(), Some(&conf)).unwrap();
    assert_eq!(
        after.permissions.permission(Operation::Merge),
        Permission::Deny
    );
    // A working-tree edit is not a trigger.
    assert!(!affects_team_config("index"));
}

#[test]
fn the_cache_is_bounded_and_keyed_by_blob() {
    let (repo, conf) = confirmed_repo(DENY_PUSH);
    let loader = TeamLoader::new(2);
    loader.load(&repo.reader(), Some(&conf)).unwrap();
    assert_eq!(loader.cached(), 1, "floor and worktree share one blob");
    for deny in ["merge", "rebase", "commit"] {
        let c = repo.commit_settings(&format!(r#"{{"permissions":{{"deny":["{deny}"]}}}}"#));
        repo.set_origin_main(&c);
        loader.load(&repo.reader(), Some(&conf)).unwrap();
    }
    assert_eq!(loader.cached(), 2);
}

/// NFR-GRD-04: the warm load must leave room for the evaluation. ⚠️ ASSUMPTION (Arquitecto,
/// 2026-10-04): warm ≤ 5 ms p95; cold measured and reported. Timing is not stable in CI, so
/// run it by hand: `cargo test --release -p gitraptor-policy --test team_config budget -- --ignored --nocapture`.
#[test]
#[ignore = "timing; run by hand in release"]
fn budget_warm_load_p95() {
    let (repo, conf) = confirmed_repo(DENY_PUSH);
    let x = repo.worktree("feat-x", &repo.head());
    let loader = TeamLoader::default();
    let time = |f: &dyn Fn()| {
        let start = std::time::Instant::now();
        f();
        start.elapsed()
    };
    let cold = time(&|| {
        TeamLoader::default()
            .load(&repo.reader_at(&x), Some(&conf))
            .unwrap();
    });
    let mut warm: Vec<_> = (0..200)
        .map(|_| {
            time(&|| {
                loader.load(&repo.reader_at(&x), Some(&conf)).unwrap();
            })
        })
        .collect();
    warm.sort();
    let p95 = warm[warm.len() * 95 / 100];
    println!(
        "cold (open + load): {cold:?}; warm p50 {:?}, p95 {p95:?}",
        warm[warm.len() / 2]
    );
    assert!(
        p95 <= std::time::Duration::from_millis(5),
        "warm p95 {p95:?}"
    );
}
