//! US-GRD-007 in the daemon, the fail-closed edges: an `ask` passes only for a person the daemon
//! proved, and a team level the daemon cannot load never leaves less than the hook client's own
//! hardening. In-process, over temporary repos and a temporary profile (NFR-01).
#![cfg(unix)]

mod common;

use std::path::PathBuf;

use common::{TempProfile, git};
use gitraptor_api::AgentKind;
use gitraptor_api::guard::{
    EvaluateParams, Hook, Operation as GuardOperation, RefUpdate, RefValue,
};
use gitraptor_core::guardrails::evaluate::{AskedBy, Caller, serve_as};
use gitraptor_core::guardrails::{GuardEntry, GuardRegistry};
use serde_json::Value;

const REPO_ID: &str = "0000-ffff";

/// A repo whose `main` carries `team` as the floor and a `feat-x` one commit ahead.
struct Setup {
    _profile: TempProfile,
    repo: tempfile::TempDir,
    common: PathBuf,
    main: String,
    feat: String,
    registry: GuardRegistry,
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
        let main = git(p, &["rev-parse", "main"]);
        git(p, &["switch", "-q", "-c", "feat-x"]);
        git(p, &["commit", "-q", "--allow-empty", "-m", "feat"]);
        let feat = git(p, &["rev-parse", "HEAD"]);
        git(p, &["switch", "-q", "main"]);
        let common = gitraptor_git::paths::canonicalize(&p.join(".git")).unwrap();
        let profile = TempProfile::new();
        std::fs::create_dir_all(profile.dirs().config).unwrap();
        let registry = GuardRegistry::for_profile(profile.dirs());
        registry.set(
            REPO_ID,
            GuardEntry {
                common_dir: common.to_string_lossy().into_owned(),
                bases: vec!["main".into()],
                confirmed: None,
            },
        );
        Self {
            _profile: profile,
            repo,
            common,
            main,
            feat,
            registry,
        }
    }

    fn caller(&self, actor: Option<AgentKind>, asked_by: Option<AskedBy>) -> Caller {
        Caller {
            actor,
            cwd: Some(self.repo.path().to_path_buf()),
            policies: true,
            permissions: true,
            asked_by,
            ..Caller::default()
        }
    }

    fn delete(&self, caller: &Caller, branch: &str, old: &str) -> Value {
        let params = EvaluateParams {
            exception_token: None,
            repo_id: REPO_ID.into(),
            common_dir: self.common.to_string_lossy().into_owned(),
            hook: Hook::ReferenceTransaction,
            operation: GuardOperation::RefTransaction {
                updates: vec![RefUpdate {
                    refname: format!("refs/heads/{branch}"),
                    old: RefValue::Oid(old.into()),
                    new: RefValue::Zero,
                }],
                orphan_head: None,
            },
            authorship: None,
        };
        serde_json::to_value(serve_as(&self.registry, &params, caller)).unwrap()
    }
}

fn has_rule(d: &Value, rule: &str) -> bool {
    d["reasons"]
        .as_array()
        .is_some_and(|r| r.iter().any(|r| r["rule"] == rule))
}

const ASKS_DELETE: &str = r#"{"permissions":{"ask":["branch-delete"]}}"#;

/// An actor the daemon could not verify is nobody proved: its `ask` is denied, every reason with
/// `ask-unavailable`, as for an agent. It never passes as the person.
#[test]
fn an_unverified_actor_does_not_pass_an_ask() {
    let s = Setup::new(ASKS_DELETE);
    let d = s.delete(&s.caller(None, Some(AskedBy::Unproved)), "feat-x", &s.feat);
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

    // The same client proved to be the person goes ahead with a notice.
    let d = s.delete(&s.caller(None, Some(AskedBy::Person)), "feat-x", &s.feat);
    assert_eq!(d["appliedEffect"], "allow", "{d}");
    assert!(
        d["notices"]
            .as_array()
            .is_some_and(|n| n.iter().any(|r| r["rule"] == "permission.branch-delete")),
        "{d}"
    );

    // What the channel resolves wins over the actor field: a resolved agent is denied.
    let d = s.delete(&s.caller(None, Some(AskedBy::Agent)), "feat-x", &s.feat);
    assert_eq!(d["appliedEffect"], "deny", "{d}");
}

/// A team level the daemon cannot load (here a remote whose name is not a valid ref) leaves the
/// hook client's own hardening with the capability: the minimum still denies deleting the base.
#[test]
fn an_unloadable_team_level_keeps_the_degraded_guarantee() {
    let s = Setup::new(r#"{"permissions":{"deny":["branch-delete"]}}"#);
    let config = s.common.join("config");
    let mut text = std::fs::read_to_string(&config).unwrap();
    text.push_str("[remote \"a..b\"]\n\turl = /nowhere\n");
    std::fs::write(&config, text).unwrap();

    let agent = s.caller(Some(AgentKind::ClaudeCode), Some(AskedBy::Agent));
    let d = s.delete(&agent, "main", &s.main);
    assert_eq!(d["appliedEffect"], "deny", "{d}");
    assert!(has_rule(&d, "minimum.base-branch-delete"), "{d}");
}
