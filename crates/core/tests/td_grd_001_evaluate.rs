//! `guard.evaluate` of a push in the daemon: the forbidden paths reach every pushed ref (tags,
//! notes and the other refs Guardrails does not govern), the protected branches stay on
//! `refs/heads/*`, and what cannot be verified is denied only when a path rule governs the actor.
//! Temporary repos only, no channel (NFR-01).
#![cfg(unix)]

mod common;

use common::git;
use gitraptor_api::AgentKind;
use gitraptor_api::Untrusted;
use gitraptor_api::guard::{
    Cause, Decision, Effect, EvaluateParams, Hook, Operation, PushUpdate, RefValue, Rule,
};
use gitraptor_core::guardrails::evaluate::{Caller, serve_as};
use gitraptor_core::guardrails::{GuardEntry, GuardRegistry};

const SETTINGS: &str = r#"{"policies":{
    "protectedBranches":{"patterns":["main"]},
    "forbiddenPaths":{"patterns":["secrets/"]}}}"#;

struct Repo {
    _tmp: tempfile::TempDir,
    dir: std::path::PathBuf,
    common: std::path::PathBuf,
    /// A commit that touches `secrets/new.txt`.
    bad: String,
    /// A commit (another line of history from the base) that touches nothing forbidden.
    clean: String,
}

fn rev(dir: &std::path::Path, what: &str) -> String {
    git(dir, &["rev-parse", what])
}

fn repo() -> Repo {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().to_path_buf();
    git(&dir, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(dir.join(".gitraptor")).unwrap();
    std::fs::write(dir.join(".gitraptor/settings.json"), SETTINGS).unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "base"]);
    git(&dir, &["checkout", "-q", "-b", "clean"]);
    std::fs::write(dir.join("ok.txt"), "ok\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "clean"]);
    let clean = rev(&dir, "HEAD");
    git(&dir, &["checkout", "-q", "main"]);
    std::fs::create_dir_all(dir.join("secrets")).unwrap();
    std::fs::write(dir.join("secrets/new.txt"), "key\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "bad"]);
    let bad = rev(&dir, "HEAD");
    let common = dir.join(".git").canonicalize().unwrap();
    Repo {
        _tmp: tmp,
        dir,
        common,
        bad,
        clean,
    }
}

fn registry(common: &std::path::Path) -> GuardRegistry {
    let r = GuardRegistry::default();
    r.set(
        "0000-ffff",
        GuardEntry {
            common_dir: common.to_string_lossy().into_owned(),
            bases: vec!["main".into()],
            confirmed: None,
        },
    );
    r
}

fn new_ref(remote_ref: &str, oid: &str) -> PushUpdate {
    PushUpdate {
        local_ref: Some("HEAD".into()),
        local: RefValue::Oid(oid.to_owned()),
        remote_ref: remote_ref.to_owned(),
        remote: RefValue::Zero,
    }
}

fn push(common: &std::path::Path, updates: Vec<PushUpdate>) -> EvaluateParams {
    EvaluateParams {
        repo_id: "0000-ffff".into(),
        common_dir: common.to_string_lossy().into_owned(),
        hook: Hook::PrePush,
        operation: Operation::Push {
            remote: Untrusted::new("origin".to_owned()),
            updates,
        },
        authorship: None,
    }
}

fn caller(repo: &Repo, agent: bool, policies: bool) -> Caller {
    Caller {
        actor: agent.then_some(AgentKind::ClaudeCode),
        cwd: Some(repo.dir.clone()),
        policies,
        ..Caller::default()
    }
}

fn serve(repo: &Repo, updates: Vec<PushUpdate>, agent: bool, policies: bool) -> Decision {
    serve_as(
        &registry(&repo.common),
        &push(&repo.common, updates),
        &caller(repo, agent, policies),
    )
}

fn count(d: &Decision, rule: Rule) -> usize {
    d.reasons.iter().filter(|r| r.rule == rule).count()
}

#[test]
fn forbidden_paths_apply_to_every_pushed_ref() {
    let r = repo();
    for refname in [
        "refs/tags/x",
        "refs/notes/x",
        "refs/remotes/mirror/x",
        "refs/bisect/x",
    ] {
        let d = serve(&r, vec![new_ref(refname, &r.bad)], true, true);
        assert_eq!(d.applied_effect, Effect::Deny, "{refname}: {d:?}");
        assert!(count(&d, Rule::ForbiddenPath) >= 1, "{refname}: {d:?}");
        let d = serve(&r, vec![new_ref(refname, &r.clean)], true, true);
        assert_eq!(d.applied_effect, Effect::Allow, "{refname} clean: {d:?}");
        // The person is not governed by an `agents` rule.
        let d = serve(&r, vec![new_ref(refname, &r.bad)], false, true);
        assert_eq!(d.applied_effect, Effect::Allow, "{refname} person: {d:?}");
    }
}

#[test]
fn protected_branch_stays_on_branches_and_forbidden_paths_reach_tags_and_notes() {
    let r = repo();
    let base = rev(&r.dir, "main~1");
    // Only tags and notes named `main`: the protected branch is not theirs.
    let d = serve(
        &r,
        vec![
            new_ref("refs/tags/main", &r.bad),
            new_ref("refs/notes/main", &r.bad),
        ],
        true,
        true,
    );
    assert_eq!(d.applied_effect, Effect::Deny, "{d:?}");
    assert_eq!(count(&d, Rule::ProtectedBranch), 0, "{d:?}");
    assert!(count(&d, Rule::ForbiddenPath) >= 1, "{d:?}");

    // In one push: the branch is evaluated as a branch, the tag and the note for their paths.
    let branch = PushUpdate {
        local_ref: Some("clean".into()),
        local: RefValue::Oid(r.clean.clone()),
        remote_ref: "refs/heads/main".into(),
        remote: RefValue::Oid(base),
    };
    let d = serve(
        &r,
        vec![
            branch,
            new_ref("refs/tags/main", &r.bad),
            new_ref("refs/notes/main", &r.bad),
        ],
        true,
        true,
    );
    assert_eq!(d.applied_effect, Effect::Deny, "{d:?}");
    assert_eq!(count(&d, Rule::ProtectedBranch), 1, "{d:?}");
    assert!(count(&d, Rule::ForbiddenPath) >= 1, "{d:?}");
}

#[test]
fn ungoverned_refs_fail_closed_only_when_a_path_rule_governs_the_actor() {
    let r = repo();
    // 257 new tags at the same clean commit: one read each, and the bound is 256.
    let many = || -> Vec<PushUpdate> {
        (0..257)
            .map(|n| new_ref(&format!("refs/tags/t{n}"), &r.clean))
            .collect()
    };
    let d = serve(&r, many(), true, true);
    assert_eq!(d.applied_effect, Effect::Deny, "{d:?}");
    assert!(
        d.reasons
            .iter()
            .any(|x| x.rule == Rule::ForbiddenPath && x.cause == Some(Cause::Unverifiable)),
        "{d:?}"
    );
    let d = serve(&r, many(), false, true);
    assert_eq!(d.applied_effect, Effect::Allow, "the person: {d:?}");

    // An annotated tag is read as its commit.
    git(&r.dir, &["tag", "-a", "-m", "m", "annotated", &r.clean]);
    let annotated = rev(&r.dir, "annotated");
    let d = serve(&r, vec![new_ref("refs/tags/a", &annotated)], true, true);
    assert_eq!(d.applied_effect, Effect::Allow, "annotated: {d:?}");

    // A tag of a tree cannot be read as commits.
    let tree = rev(&r.dir, "clean^{tree}");
    let d = serve(&r, vec![new_ref("refs/tags/tree", &tree)], true, true);
    assert_eq!(d.applied_effect, Effect::Deny, "tree: {d:?}");
    assert!(
        d.reasons
            .iter()
            .any(|x| x.rule == Rule::ForbiddenPath && x.cause == Some(Cause::Unverifiable)),
        "tree: {d:?}"
    );
}

/// A connection that did not ask for `guard.policies` (an older hook) gets no rule applied.
#[test]
fn a_daemon_without_guard_policies_evaluates_no_ungoverned_ref() {
    let r = repo();
    let d = serve(&r, vec![new_ref("refs/tags/x", &r.bad)], true, false);
    assert_eq!(d.applied_effect, Effect::Allow, "{d:?}");
}
