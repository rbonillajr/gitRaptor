//! US-GRD-007 in the daemon (DS-US-GRD-007 D2, D4, D6, D10, D18, D19): `disableSafeMinimum` only
//! from the confirmed and readable floor, one reason per level of the maximum, the
//! `guard.permissions` capability both ways, a classification that cannot be proved, and `ask`
//! by actor. In-process, over temporary repos and a temporary profile (NFR-01). The decisions
//! are read as the wire JSON, so a test names a rule by its code (`permission.push`).
#![cfg(unix)]

mod common;

use std::path::PathBuf;

use common::{TempProfile, git};
use gitraptor_api::guard::{
    Cause, Effect, EvaluateParams, GuardLogResult, Hook, Level, Operation as GuardOperation,
    PushUpdate, Reason, RefUpdate, RefValue, Rule,
};
use gitraptor_api::{AgentKind, Untrusted};
use gitraptor_core::guardrails::evaluate::{Caller, decision, open, serve_as};
use gitraptor_core::guardrails::{GuardEntry, GuardRegistry, install, permissions};
use gitraptor_policy::guard::Evaluation;
use gitraptor_policy::settings::model::Operation;
use gitraptor_policy::team::{Confirmed, ConfirmedFloor, Permission};
use serde_json::{Value, json};

const REPO_ID: &str = "0000-ffff";

/// A repo whose `main` carries `team` as `.gitraptor/settings.json` (the floor), a `feat-x` one
/// commit ahead, an `alt` that diverged, and a profile.
struct Setup {
    profile: TempProfile,
    repo: tempfile::TempDir,
    common: PathBuf,
    blob: String,
    main: String,
    feat: String,
    alt: String,
}

impl Setup {
    fn new(team: &str) -> Self {
        let repo = tempfile::tempdir().unwrap();
        let p = repo.path();
        git(p, &["init", "-q", "-b", "main"]);
        std::fs::create_dir_all(p.join(".gitraptor")).unwrap();
        std::fs::write(p.join(".gitraptor/settings.json"), team).unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "a"]);
        let blob = git(p, &["rev-parse", "main:.gitraptor/settings.json"]);
        let main = git(p, &["rev-parse", "main"]);
        git(p, &["switch", "-q", "-c", "feat-x"]);
        git(p, &["commit", "-q", "--allow-empty", "-m", "feat"]);
        let feat = git(p, &["rev-parse", "HEAD"]);
        git(p, &["switch", "-q", "main"]);
        git(p, &["switch", "-q", "-c", "alt"]);
        git(p, &["commit", "-q", "--allow-empty", "-m", "alt"]);
        let alt = git(p, &["rev-parse", "HEAD"]);
        git(p, &["switch", "-q", "main"]);
        let common = gitraptor_git::paths::canonicalize(&p.join(".git")).unwrap();
        let profile = TempProfile::new();
        std::fs::create_dir_all(profile.dirs().config).unwrap();
        Self {
            profile,
            repo,
            common,
            blob,
            main,
            feat,
            alt,
        }
    }

    fn confirmed(&self) -> Confirmed {
        Confirmed {
            base_branch: gitraptor_git::RefName::new("main").unwrap(),
            floor: ConfirmedFloor::Blob(self.blob.clone()),
        }
    }

    fn profile_settings(&self, json: &str) {
        std::fs::write(self.profile.dirs().config.join("settings.json"), json).unwrap();
    }

    fn registry(&self, confirmed: bool) -> GuardRegistry {
        let registry = GuardRegistry::for_profile(self.profile.dirs());
        registry.set(
            REPO_ID,
            GuardEntry {
                common_dir: self.common.to_string_lossy().into_owned(),
                bases: vec!["main".into()],
                confirmed: confirmed.then(|| self.confirmed()),
            },
        );
        registry
    }

    fn caller(&self, actor: Option<AgentKind>, permissions: bool) -> Caller {
        Caller {
            actor,
            cwd: Some(self.repo.path().to_path_buf()),
            policies: true,
            permissions,
            ..Caller::default()
        }
    }

    fn params(&self, hook: Hook, operation: GuardOperation) -> EvaluateParams {
        EvaluateParams {
            exception_token: None,
            repo_id: REPO_ID.into(),
            common_dir: self.common.to_string_lossy().into_owned(),
            hook,
            operation,
            authorship: None,
        }
    }

    fn serve(&self, registry: &GuardRegistry, op: GuardOperation, caller: &Caller) -> Value {
        let hook = match op {
            GuardOperation::Push { .. } => Hook::PrePush,
            _ => Hook::ReferenceTransaction,
        };
        let d = serve_as(registry, &self.params(hook, op), caller);
        serde_json::to_value(d).unwrap()
    }

    fn delete(&self, branch: &str, old: &str) -> GuardOperation {
        GuardOperation::RefTransaction {
            updates: vec![RefUpdate {
                refname: format!("refs/heads/{branch}"),
                old: RefValue::Oid(old.into()),
                new: RefValue::Zero,
            }],
            orphan_head: None,
        }
    }

    fn create(&self, branch: &str, at: &str) -> GuardOperation {
        GuardOperation::RefTransaction {
            updates: vec![RefUpdate {
                refname: format!("refs/heads/{branch}"),
                old: RefValue::Zero,
                new: RefValue::Oid(at.into()),
            }],
            orphan_head: None,
        }
    }

    fn push(&self, local: &str, remote: &str) -> GuardOperation {
        GuardOperation::Push {
            remote: Untrusted::new("origin"),
            updates: vec![PushUpdate {
                local_ref: Some("refs/heads/feat-x".into()),
                local: RefValue::Oid(local.into()),
                remote_ref: "refs/heads/feat-x".into(),
                remote: RefValue::Oid(remote.into()),
            }],
        }
    }
}

fn rules(decision: &Value) -> Vec<(String, String)> {
    decision["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                r["rule"].as_str().unwrap().to_owned(),
                r["level"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn has_rule(decision: &Value, rule: &str) -> bool {
    rules(decision).iter().any(|(r, _)| r == rule)
}

const DISABLES_MINIMUM: &str = r#"{"permissions":{"disableSafeMinimum":true}}"#;

/// D4: `disableSafeMinimum` turns the minimum off only from the floor the developer confirmed
/// and that reads whole. Deleting the base branch is what the minimum denies.
#[test]
fn disable_safe_minimum_only_from_the_confirmed_readable_floor() {
    // Confirmed and readable: the base branch can go.
    let s = Setup::new(DISABLES_MINIMUM);
    let registry = s.registry(true);
    let agent = s.caller(Some(AgentKind::ClaudeCode), true);
    let d = s.serve(&registry, s.delete("main", &s.main), &agent);
    assert_eq!(d["appliedEffect"], "allow", "{d}");
    assert!(!has_rule(&d, "minimum.base-branch-delete"), "{d}");

    // The same floor, not confirmed: the minimum holds.
    let registry = s.registry(false);
    let d = s.serve(&registry, s.delete("main", &s.main), &agent);
    assert_eq!(d["appliedEffect"], "deny", "{d}");
    assert!(has_rule(&d, "minimum.base-branch-delete"), "{d}");

    // A personal level cannot turn it off, even over a confirmed floor.
    let s = Setup::new("{}");
    s.profile_settings(DISABLES_MINIMUM);
    let registry = s.registry(true);
    let d = s.serve(&registry, s.delete("main", &s.main), &agent);
    assert_eq!(d["appliedEffect"], "deny", "{d}");
    assert!(has_rule(&d, "minimum.base-branch-delete"), "{d}");

    // A floor that does not read whole (a broken document) does not turn it off either.
    let s = Setup::new(r#"{"permissions":{"disableSafeMinimum":true,"deny":"#);
    let registry = s.registry(true);
    let d = s.serve(&registry, s.delete("main", &s.main), &agent);
    assert_eq!(d["appliedEffect"], "deny", "{d}");
}

/// D18: `minimum_set` is `disabled-by-team` only with a confirmed floor that reads and disables
/// the minimum.
#[test]
fn minimum_set_status_follows_the_confirmed_floor() {
    use gitraptor_api::guard::MinimumSetStatus;
    let s = Setup::new(DISABLES_MINIMUM);
    let reader = open(&s.common).expect("the repo opens");
    let confirmed = s.confirmed();
    assert_eq!(
        install::minimum_set(&reader, Some(&confirmed)).status,
        MinimumSetStatus::DisabledByTeam
    );
    assert_eq!(
        install::minimum_set(&reader, None).status,
        MinimumSetStatus::Active
    );
    let s = Setup::new(r#"{"permissions":{"deny":["push"]}}"#);
    let reader = open(&s.common).expect("the repo opens");
    assert_eq!(
        install::minimum_set(&reader, Some(&s.confirmed())).status,
        MinimumSetStatus::Active
    );
}

/// D2: one reason per level that produces the maximum, and the safe minimum is its own reason
/// (`minimum.force-push`), never a permission reason.
#[test]
fn reasons_name_every_level_of_the_maximum() {
    let s = Setup::new(r#"{"permissions":{"deny":["push"]}}"#);
    s.profile_settings(r#"{"permissions":{"deny":["push"]}}"#);
    let registry = s.registry(true);
    let agent = s.caller(Some(AgentKind::ClaudeCode), true);
    let d = s.serve(&registry, s.push(&s.feat, &s.main), &agent);
    assert_eq!(d["appliedEffect"], "deny", "{d}");
    let mut found = rules(&d);
    found.sort();
    assert_eq!(
        found,
        [
            ("permission.push".to_owned(), "floor".to_owned()),
            ("permission.push".to_owned(), "profile".to_owned())
        ],
        "{d}"
    );

    // A forced push denied by the minimum and by the team names both.
    let s = Setup::new(r#"{"permissions":{"deny":["force-push"]}}"#);
    let registry = s.registry(false);
    let d = s.serve(&registry, s.push(&s.feat, &s.alt), &agent);
    assert_eq!(d["appliedEffect"], "deny", "{d}");
    assert!(has_rule(&d, "minimum.force-push"), "{d}");
    let permission: Vec<_> = rules(&d)
        .into_iter()
        .filter(|(r, _)| r == "permission.force-push")
        .collect();
    assert_eq!(
        permission,
        [("permission.force-push".to_owned(), "floor".to_owned())],
        "{d}"
    );
}

/// D10: without `guard.permissions` the daemon evaluates as before (no permissions) and the
/// client applies the team's permissions only to harden; `guard.log` hides `permission.*` from a
/// connection without the capability but keeps counting them.
#[test]
fn permissions_capability_both_ways() {
    let s = Setup::new(r#"{"permissions":{"deny":["push"]}}"#);
    let registry = s.registry(true);
    // With the capability the permission applies.
    let with = s.caller(Some(AgentKind::ClaudeCode), true);
    let d = s.serve(&registry, s.push(&s.feat, &s.main), &with);
    assert_eq!(d["appliedEffect"], "deny", "{d}");
    assert!(has_rule(&d, "permission.push"), "{d}");
    // Without it, the decision is the one of before the capability existed.
    let without = s.caller(Some(AgentKind::ClaudeCode), false);
    let d = s.serve(&registry, s.push(&s.feat, &s.main), &without);
    assert_eq!(d["appliedEffect"], "allow", "{d}");
    assert!(!has_rule(&d, "permission.push"), "{d}");

    // The hook client of a daemon without the capability (D10, D12): the floor only hardens.
    let team = Setup::new(
        r#"{"permissions":{"disableSafeMinimum":true,"allow":["force-push"],"deny":["commit"],"ask":["branch-delete"]}}"#,
    );
    let reader = open(&team.common).expect("the repo opens");
    let perms = permissions::team_only(&reader, Some(team.repo.path()));
    assert!(perms.safe_minimum_active, "the minimum is forced");
    assert_eq!(perms.permission(Operation::ForcePush), Permission::Deny);
    assert_eq!(perms.permission(Operation::Commit), Permission::Deny);
    assert_eq!(perms.permission(Operation::BranchDelete), Permission::Ask);
    assert_eq!(perms.permission(Operation::Push), Permission::Allow);

    // `guard.log` for a connection without the capability.
    let mut log: GuardLogResult = serde_json::from_value(json!({
        "sinceMs": 1,
        "summary": {"blocked": 2, "notices": 0, "rateLimited": 0},
        "entries": [
            {
                "atMs": 10, "utcOffsetS": 0, "lastMs": 10, "count": 1,
                "actor": null,
                "operation": {"kind": "ref-transaction", "refs": [{"name": {"untrusted": "feat-x"}, "change": "delete"}]},
                "kind": "denial", "detail": "full",
                "effect": "deny", "appliedEffect": "deny",
                "reasons": [{"rule": "permission.branch-delete", "level": "floor"}],
                "layer": "hooks", "origin": "daemon", "decisionId": "d-1"
            },
            {
                "atMs": 11, "utcOffsetS": 0, "lastMs": 11, "count": 1,
                "actor": null,
                "operation": {"kind": "ref-transaction", "refs": [{"name": {"untrusted": "main"}, "change": "delete"}]},
                "kind": "denial", "detail": "full",
                "effect": "deny", "appliedEffect": "deny",
                "reasons": [{"rule": "minimum.base-branch-delete", "level": "minimum"}],
                "layer": "hooks", "origin": "daemon", "decisionId": "d-2"
            }
        ],
        "unloggedPeriods": []
    }))
    .unwrap();
    permissions::legacy_log(&mut log);
    let wire = serde_json::to_value(&log).unwrap();
    let entries = wire["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1, "{wire}");
    assert_eq!(
        entries[0]["reasons"][0]["rule"], "minimum.base-branch-delete",
        "{wire}"
    );
    assert_eq!(wire["summary"]["blocked"], 2, "they still count: {wire}");
}

/// D6: a transaction whose operation cannot be proved (no `git` process read) is denied only
/// when the permission that needs the proof is not `allow`.
#[test]
fn an_unclassifiable_transaction_is_denied_only_when_it_matters() {
    let agent = |s: &Setup| s.caller(Some(AgentKind::ClaudeCode), true);
    // `worktree-add` in deny and a branch created by a `git` nobody can read: denied, unverifiable.
    let s = Setup::new(r#"{"permissions":{"deny":["worktree-add"]}}"#);
    let registry = s.registry(true);
    let d = s.serve(&registry, s.create("new-branch", &s.main), &agent(&s));
    assert_eq!(d["appliedEffect"], "deny", "{d}");
    let reason = d["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["rule"] == "permission.worktree-add")
        .unwrap_or_else(|| panic!("{d}"));
    assert_eq!(reason["cause"], "unverifiable", "{d}");

    // The same creation with `worktree-add` allowed is not held up by what cannot be proved.
    let s = Setup::new(r#"{"permissions":{"deny":["push"]}}"#);
    let registry = s.registry(true);
    let d = s.serve(&registry, s.create("new-branch", &s.main), &agent(&s));
    assert_eq!(d["appliedEffect"], "allow", "{d}");
}

/// D19: `ask` by actor. The pure rule gives `ask` whoever acts; the daemon applies `deny` with
/// `ask-unavailable` to an agent, lets the person through with a notice, and an actor nobody
/// proved (the client deciding alone) is an agent. `allow` and `deny` do not depend on the actor.
#[test]
fn ask_depends_on_the_actor() {
    let s = Setup::new(r#"{"permissions":{"ask":["branch-delete"],"deny":["push"]}}"#);
    let registry = s.registry(true);
    let delete = || s.delete("feat-x", &s.feat);

    // Agent: applied deny, every reason of the ask says why.
    let agent = s.caller(Some(AgentKind::ClaudeCode), true);
    let d = s.serve(&registry, delete(), &agent);
    assert_eq!(d["effect"], "ask", "{d}");
    assert_eq!(d["appliedEffect"], "deny", "{d}");
    assert!(has_rule(&d, "permission.branch-delete"), "{d}");
    assert!(
        d["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["cause"] == "ask-unavailable"),
        "{d}"
    );

    // Person: the effect stays ask, it is applied as allow, and a notice says so.
    let person = s.caller(None, true);
    let d = s.serve(&registry, delete(), &person);
    assert_eq!(d["effect"], "ask", "{d}");
    assert_eq!(d["appliedEffect"], "allow", "{d}");
    assert!(
        d["notices"]
            .as_array()
            .is_some_and(|n| n.iter().any(|r| r["rule"] == "permission.branch-delete")),
        "the person is told the ask was not applied: {d}"
    );
    assert!(
        d["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r.get("cause").is_none_or(|c| c != "ask-unavailable")),
        "{d}"
    );

    // Not proved: the client's own decision denies everyone.
    let mut eval = Evaluation::allow();
    eval.add(
        Effect::Ask,
        Reason {
            rule: Rule::PermissionBranchDelete,
            level: Level::Floor,
            cause: None,
            params: Vec::new(),
        },
    );
    let d = serde_json::to_value(decision(eval)).unwrap();
    assert_eq!(d["effect"], "ask", "{d}");
    assert_eq!(d["appliedEffect"], "deny", "{d}");
    assert_eq!(
        d["reasons"][0]["cause"],
        serde_json::to_value(Cause::AskUnavailable).unwrap(),
        "{d}"
    );

    // `deny` does not depend on the actor.
    for caller in [&agent, &person] {
        let d = s.serve(&registry, s.push(&s.feat, &s.main), caller);
        assert_eq!(d["appliedEffect"], "deny", "{d}");
        assert!(d["reasons"][0].get("cause").is_none(), "{d}");
    }
}
