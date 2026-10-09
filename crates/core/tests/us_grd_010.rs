//! Layered configuration on the daemon's evaluation, without a channel: the profile and the
//! repo's local level only harden what the team set, and between the two personal levels the
//! local one wins (BR-CONS-001). Each test builds temporary repos and a temporary profile; nothing
//! touches the real profile or this repo (NFR-01).

mod common;

use std::path::{Path, PathBuf};

use common::{TempProfile, git};
use gitraptor_api::AgentKind;
use gitraptor_api::guard::{
    Effect, EvaluateParams, Hook, Level, Operation as GuardOperation, RefUpdate, RefValue, Rule,
};
use gitraptor_core::guardrails::evaluate::{Caller, open, serve_as};
use gitraptor_core::guardrails::layers::{self, Layers};
use gitraptor_core::guardrails::{GuardEntry, GuardRegistry};
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::profile::settings::local_settings_path;
use gitraptor_policy::layers::{IgnoredRelaxation, RelaxKey};
use gitraptor_policy::settings::SourceKind;
use gitraptor_policy::settings::model::Operation;
use gitraptor_policy::team::{Confirmed, ConfirmedFloor, Permission, RuleSource};

/// A team that lets everyone force-push: it only takes effect once the developer confirmed it.
const TEAM_ALLOWS_FORCE_PUSH: &str =
    r#"{"permissions":{"disableSafeMinimum":true,"allow":["force-push"]}}"#;
const REPO_ID: &str = "0000-ffff";

/// A repo whose `main` carries `team` as `.gitraptor/settings.json` (the floor), and a profile.
struct Setup {
    profile: TempProfile,
    repo: tempfile::TempDir,
    common: PathBuf,
    blob: String,
}

impl Setup {
    fn new(team: &str) -> Self {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "-q", "-b", "main"]);
        std::fs::create_dir_all(repo.path().join(".gitraptor")).unwrap();
        std::fs::write(repo.path().join(".gitraptor/settings.json"), team).unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-q", "-m", "a"]);
        let blob = git(repo.path(), &["rev-parse", "main:.gitraptor/settings.json"]);
        let common = repo.path().join(".git").canonicalize().unwrap();
        let profile = TempProfile::new();
        std::fs::create_dir_all(profile.dirs().config).unwrap();
        Self {
            profile,
            repo,
            common,
            blob,
        }
    }

    fn dirs(&self) -> ProfileDirs {
        self.profile.dirs()
    }

    fn confirmed(&self) -> Confirmed {
        Confirmed {
            base_branch: gitraptor_git::RefName::new("main").unwrap(),
            floor: ConfirmedFloor::Blob(self.blob.clone()),
        }
    }

    fn profile_settings(&self, json: &str) {
        std::fs::write(self.dirs().config.join("settings.json"), json).unwrap();
    }

    fn local_settings(&self, repo_id: &str, json: &str) {
        let path = local_settings_path(&self.dirs(), repo_id).expect("a valid repo id");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, json).unwrap();
    }

    fn load(&self, repo_id: &str) -> Layers {
        let reader = open(&self.common).expect("the repo opens");
        layers::load(
            &reader,
            Some(&self.confirmed()),
            Some(&self.dirs()),
            Some(repo_id),
        )
        .expect("the team level is readable")
    }
}

fn ignored(level: Level, key: RelaxKey) -> IgnoredRelaxation {
    IgnoredRelaxation { level, key }
}

/// US-GRD-010 row 1: the team denies, the local allows: deny, and the attempt is ignored.
#[test]
fn precedence_row1_team_deny_local_allow_is_deny() {
    let s = Setup::new(r#"{"permissions":{"deny":["force-push"]}}"#);
    s.local_settings(REPO_ID, r#"{"permissions":{"allow":["force-push"]}}"#);
    let layers = s.load(REPO_ID);
    assert!(
        layers.local.applicable().is_some(),
        "the local level is read"
    );
    assert_eq!(
        layers.permissions().permission(Operation::ForcePush),
        Permission::Deny
    );
    assert!(
        layers.ignored().contains(&ignored(
            Level::Local,
            RelaxKey::Permission(Operation::ForcePush)
        )),
        "{:?}",
        layers.ignored()
    );
}

/// US-GRD-010 row 2: the confirmed team allows, the profile denies: deny, from the profile.
#[test]
fn precedence_row2_team_allow_profile_deny_is_deny() {
    let s = Setup::new(TEAM_ALLOWS_FORCE_PUSH);
    s.profile_settings(r#"{"permissions":{"deny":["force-push"]}}"#);
    let layers = s.load(REPO_ID);
    // The fixture itself: the confirmed team does let force-push through on its own.
    assert_eq!(
        layers.team.permissions.permission(Operation::ForcePush),
        Permission::Allow
    );
    let permissions = layers.permissions();
    assert_eq!(
        permissions.permission(Operation::ForcePush),
        Permission::Deny
    );
    assert!(
        permissions.operations[&Operation::ForcePush]
            .sources
            .contains(&RuleSource::Source(SourceKind::Profile)),
        "{:?}",
        permissions.operations[&Operation::ForcePush]
    );
}

/// US-GRD-010 row 3: the team allows, the profile denies and the local allows: the local wins
/// between the personal levels, so the effective permission is allow and nothing was relaxed.
#[test]
fn precedence_row3_team_allow_profile_deny_local_allow_is_allow() {
    let s = Setup::new(TEAM_ALLOWS_FORCE_PUSH);
    s.profile_settings(r#"{"permissions":{"deny":["force-push"]}}"#);
    s.local_settings(REPO_ID, r#"{"permissions":{"allow":["force-push"]}}"#);
    let layers = s.load(REPO_ID);
    assert!(layers.profile.applicable().is_some(), "the profile is read");
    assert!(
        layers.local.applicable().is_some(),
        "the local level is read"
    );
    assert_eq!(
        layers.permissions().permission(Operation::ForcePush),
        Permission::Allow
    );
    assert!(layers.ignored().is_empty(), "{:?}", layers.ignored());
}

/// US-GRD-010 row 4: the team asks for `rebase`, the local allows it: ask, and it is ignored.
#[test]
fn precedence_row4_team_ask_local_allow_is_ask() {
    let s = Setup::new(r#"{"permissions":{"ask":["rebase"]}}"#);
    s.local_settings(REPO_ID, r#"{"permissions":{"allow":["rebase"]}}"#);
    let layers = s.load(REPO_ID);
    assert!(
        layers.local.applicable().is_some(),
        "the local level is read"
    );
    assert_eq!(
        layers.permissions().permission(Operation::Rebase),
        Permission::Ask
    );
    assert!(
        layers.ignored().contains(&ignored(
            Level::Local,
            RelaxKey::Permission(Operation::Rebase)
        )),
        "{:?}",
        layers.ignored()
    );
}

/// US-GRD-010 row 7: the team protects `main` and the profile `release`: the union holds, and an
/// agent cannot move `release`.
#[test]
fn precedence_row7_protected_branches_union() {
    let s = Setup::new(r#"{"policies":{"protectedBranches":{"patterns":["main"]}}}"#);
    s.profile_settings(r#"{"policies":{"protectedBranches":{"patterns":["release"]}}}"#);
    let layers = s.load(REPO_ID);
    let policies = layers.policies();
    let has = |level: Level, pattern: &str| {
        policies
            .branches
            .iter()
            .any(|r| r.level == level && r.patterns.iter().any(|p| p.raw() == pattern))
    };
    assert!(has(Level::Floor, "main"), "{policies:?}");
    assert!(has(Level::Profile, "release"), "{policies:?}");

    // The agent's attempt to move `release`, through the daemon's evaluation.
    let repo = s.repo.path();
    git(repo, &["branch", "release"]);
    let old = git(repo, &["rev-parse", "release"]);
    git(repo, &["commit", "-q", "--allow-empty", "-m", "b"]);
    let new = git(repo, &["rev-parse", "HEAD"]);
    let registry = GuardRegistry::for_profile(s.dirs());
    registry.set(
        REPO_ID,
        GuardEntry {
            common_dir: s.common.to_string_lossy().into_owned(),
            bases: vec!["main".into()],
            confirmed: Some(s.confirmed()),
        },
    );
    let params = EvaluateParams {
        repo_id: REPO_ID.into(),
        common_dir: s.common.to_string_lossy().into_owned(),
        hook: Hook::ReferenceTransaction,
        operation: GuardOperation::RefTransaction {
            updates: vec![RefUpdate {
                refname: "refs/heads/release".into(),
                old: RefValue::Oid(old),
                new: RefValue::Oid(new),
            }],
            orphan_head: None,
        },
        authorship: None,
    };
    let agent = Caller {
        actor: Some(AgentKind::ClaudeCode),
        cwd: Some(repo.to_path_buf()),
        policies: true,
        ..Caller::default()
    };
    let d = serve_as(&registry, &params, &agent);
    assert_eq!(d.applied_effect, Effect::Deny, "{d:?}");
    assert!(
        d.reasons
            .iter()
            .any(|r| r.rule == Rule::ProtectedBranch && r.level == Level::Profile),
        "{d:?}"
    );
}

/// The local level is per clone: a hardening in one clone's `settings.local.json` does not reach
/// another clone of the same repo, because the file lives under the clone's own `repo_id`. Read on
/// the effective permission (the guard applies it with US-GRD-007).
#[test]
fn a_local_hardening_does_not_reach_another_clone() {
    let first = Setup::new(r#"{}"#);
    let parent = tempfile::tempdir().unwrap();
    let second_path: PathBuf = parent.path().join("second");
    git(
        parent.path(),
        &[
            "clone",
            "-q",
            first.repo.path().to_str().unwrap(),
            second_path.to_str().unwrap(),
        ],
    );
    let second = Setup {
        profile: TempProfile {
            root: tempfile::tempdir().unwrap(),
        },
        repo: parent,
        common: Path::new(&second_path).join(".git").canonicalize().unwrap(),
        blob: first.blob.clone(),
    };
    // One profile for both clones: only the `repo_id` tells them apart.
    let dirs = first.dirs();
    let (id_first, id_second) = ("aaaa-0001", "bbbb-0002");
    first.local_settings(id_first, r#"{"permissions":{"deny":["push"]}}"#);

    let load = |setup: &Setup, id: &str| {
        let reader = open(&setup.common).expect("the repo opens");
        layers::load(&reader, Some(&setup.confirmed()), Some(&dirs), Some(id)).unwrap()
    };
    let in_first = load(&first, id_first);
    let in_second = load(&second, id_second);
    assert!(
        in_first.local.applicable().is_some(),
        "the local level is read"
    );
    assert_eq!(
        in_first.permissions().permission(Operation::Push),
        Permission::Deny
    );
    assert_eq!(
        in_second.permissions().permission(Operation::Push),
        Permission::Allow
    );
}

/// ADR-GRD-004 § 3: the base branch comes from the team floor only. A personal level that names
/// one is ignored, and the attempt is reported.
#[test]
fn a_base_branch_in_a_personal_level_is_ignored() {
    let s = Setup::new(r#"{}"#);
    s.local_settings(REPO_ID, r#"{"engine":{"baseBranch":"develop"}}"#);
    let layers = s.load(REPO_ID);
    let guarded: Vec<&str> = layers
        .team
        .guarded_base_branches()
        .iter()
        .map(gitraptor_git::RefName::as_str)
        .collect();
    assert_eq!(guarded, ["main"]);
    assert!(
        layers
            .ignored()
            .contains(&ignored(Level::Local, RelaxKey::BaseBranch)),
        "{:?}",
        layers.ignored()
    );
}
