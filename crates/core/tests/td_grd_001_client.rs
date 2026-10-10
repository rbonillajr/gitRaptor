//! The hook client (`raptor hook`) of a `pre-push` that carries refs Guardrails does not govern:
//! which refs go to the daemon and to the floor, which templates it accepts, and when a signal
//! (no daemon, another repo) blocks a push of only such refs. Temporary repos only (NFR-01).
#![cfg(unix)]

mod common;

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use common::git;
use gitraptor_api::Untrusted;
use gitraptor_api::guard::{Hook, Operation, PushUpdate, RefValue, Rule};
use gitraptor_core::guardrails::hook::{Degraded, HookArgs, HookEnv, HookOutcome, push_scope, run};

const ZERO: &str = "0000000000000000000000000000000000000000";

fn update(remote_ref: &str) -> PushUpdate {
    PushUpdate {
        local_ref: Some("HEAD".into()),
        local: RefValue::Oid("1".repeat(40)),
        remote_ref: remote_ref.to_owned(),
        remote: RefValue::Zero,
    }
}

fn op(refs: &[&str]) -> Operation {
    Operation::Push {
        remote: Untrusted::new("origin".to_owned()),
        updates: refs.iter().map(|r| update(r)).collect(),
    }
}

fn refs_of(op: Option<Operation>) -> Option<Vec<String>> {
    op.map(|op| match op {
        Operation::Push { updates, .. } => updates.into_iter().map(|u| u.remote_ref).collect(),
        other => panic!("not a push: {other:?}"),
    })
}

#[test]
fn capability_and_template_decide_which_pushed_refs_are_sent() {
    let mixed = || op(&["refs/heads/feat", "refs/tags/x", "refs/notes/x"]);
    let only_tags = || op(&["refs/tags/x"]);
    let governed = Some(vec!["refs/heads/feat".to_owned()]);
    let all = Some(vec![
        "refs/heads/feat".to_owned(),
        "refs/tags/x".to_owned(),
        "refs/notes/x".to_owned(),
    ]);

    // What the daemon without `guard.policies` receives: only the governed ones.
    assert_eq!(refs_of(push_scope(mixed(), 3, false)), governed);
    assert_eq!(push_scope(only_tags(), 3, false), None);
    // What it receives with the capability, and what the floor for everyone evaluates without it.
    assert_eq!(refs_of(push_scope(mixed(), 3, true)), all);
    assert_eq!(
        refs_of(push_scope(only_tags(), 3, true)),
        Some(vec!["refs/tags/x".to_owned()])
    );
    // Templates 1 and 2 never hand over what is not governed.
    for template in [1, 2] {
        assert_eq!(
            refs_of(push_scope(mixed(), template, true)),
            governed,
            "template {template}"
        );
        assert_eq!(push_scope(only_tags(), template, true), None);
    }
    // Any other operation is returned as it is.
    let rebase = Operation::Rebase {
        upstream: None,
        branch: None,
    };
    assert_eq!(push_scope(rebase.clone(), 3, false), Some(rebase));
}

#[test]
fn the_hook_accepts_templates_1_2_and_3() {
    let abs = |p: &str| std::env::temp_dir().join(p).to_string_lossy().into_owned();
    let (common, channel, state) = (abs("r/.git"), abs("run"), abs("state"));
    let parse = |template: &str| {
        let argv: Vec<OsString> = [
            template, "pre-push", "abc", &common, &channel, "inst", &state, "", "--", "origin",
        ]
        .iter()
        .map(OsString::from)
        .collect();
        HookArgs::parse(&argv)
    };
    for good in ["1", "2", "3"] {
        let args = parse(good).unwrap_or_else(|| panic!("template {good} is accepted"));
        assert_eq!(args.template.to_string(), good);
    }
    for bad in ["0", "4"] {
        assert!(parse(bad).is_none(), "template {bad} is rejected");
    }
}

struct Repo {
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    common: PathBuf,
    state: PathBuf,
    channel: PathBuf,
    /// A commit that touches `secrets/new.txt`.
    bad: String,
}

/// A repo whose `main` holds the floor `settings` and one more commit with a forbidden path.
fn repo(settings: &str) -> Repo {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let dir = root.join("repo");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(dir.join(".gitraptor")).unwrap();
    std::fs::write(dir.join(".gitraptor/settings.json"), settings).unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "base"]);
    std::fs::create_dir_all(dir.join("secrets")).unwrap();
    std::fs::write(dir.join("secrets/new.txt"), "key\n").unwrap();
    git(&dir, &["add", "."]);
    git(&dir, &["commit", "-q", "-m", "bad"]);
    let bad = git(&dir, &["rev-parse", "HEAD"]);
    // An empty channel folder: no daemon to ask.
    let channel = root.join("run");
    let state = root.join("state");
    std::fs::create_dir_all(&channel).unwrap();
    std::fs::create_dir_all(&state).unwrap();
    let common = dir.join(".git").canonicalize().unwrap();
    Repo {
        _tmp: tmp,
        dir,
        common,
        state,
        channel,
        bad,
    }
}

fn args(repo: &Repo, template: u32) -> HookArgs {
    HookArgs {
        template,
        hook: Hook::PrePush,
        repo: "abc-123".into(),
        common: repo.common.clone(),
        channel: repo.channel.clone(),
        instance: "inst".into(),
        state: repo.state.clone(),
        prior: String::new(),
        git_args: vec!["origin".into(), "/remote.git".into()],
    }
}

fn env_of(dir: &Path) -> HookEnv {
    HookEnv {
        git_dir: Some(dir.join(".git")),
        cwd: dir.to_path_buf(),
    }
}

fn tag_push(repo: &Repo) -> Vec<u8> {
    format!("HEAD {} refs/tags/x {ZERO}\n", repo.bad).into_bytes()
}

fn rules_of(outcome: &HookOutcome) -> Vec<Rule> {
    outcome
        .decision
        .as_ref()
        .map(|d| d.reasons.iter().map(|r| r.rule).collect())
        .unwrap_or_default()
}

const NO_PATHS: &str = r#"{"policies":{"protectedBranches":{"patterns":["main"]}}}"#;
const AGENT_PATHS: &str = r#"{"policies":{"forbiddenPaths":{"patterns":["secrets/"]}}}"#;
const EVERYONE_PATHS: &str =
    r#"{"policies":{"forbiddenPaths":{"patterns":["secrets/"],"appliesTo":"everyone"}}}"#;

#[test]
fn an_ungoverned_push_signal_blocks_only_with_an_applicable_rule() {
    // Case D: the transaction is in another repo than the dispatcher's.
    let other = repo(NO_PATHS);
    for (settings, blocks) in [(NO_PATHS, false), (AGENT_PATHS, true)] {
        let ours = repo(settings);
        let out = run(&args(&ours, 3), &env_of(&other.dir), &tag_push(&ours));
        if blocks {
            assert!(!out.allowed(), "a rule exists: {out:?}");
            assert!(rules_of(&out).contains(&Rule::RepoMismatch), "{out:?}");
        } else {
            assert!(out.allowed(), "no rule applies: {out:?}");
        }
    }

    // Case B: no daemon at the channel.
    let ours = repo(NO_PATHS);
    let out = run(&args(&ours, 3), &env_of(&ours.dir), &tag_push(&ours));
    assert!(out.allowed(), "no rule, no daemon: {out:?}");
    assert_eq!(out.degraded, Some(Degraded::DaemonUnreachable), "{out:?}");

    // A rule for agents cannot be told apart without the daemon: the path is denied.
    let ours = repo(AGENT_PATHS);
    let out = run(&args(&ours, 3), &env_of(&ours.dir), &tag_push(&ours));
    assert!(!out.allowed(), "a rule, no daemon: {out:?}");
    assert!(rules_of(&out).contains(&Rule::ForbiddenPath), "{out:?}");
    assert!(rules_of(&out).contains(&Rule::Degraded), "{out:?}");
}

/// Regression of the strict degraded mode at the client: the template decides the scope.
#[test]
fn degraded_mode_scopes_ungoverned_refs_by_template() {
    let ours = repo(EVERYONE_PATHS);
    let out = run(&args(&ours, 3), &env_of(&ours.dir), &tag_push(&ours));
    assert!(!out.allowed(), "template 3: {out:?}");
    assert!(out.degraded.is_some(), "{out:?}");
    assert!(rules_of(&out).contains(&Rule::ForbiddenPath), "{out:?}");

    let out = run(&args(&ours, 2), &env_of(&ours.dir), &tag_push(&ours));
    assert!(out.allowed(), "template 2 is as before: {out:?}");
}
