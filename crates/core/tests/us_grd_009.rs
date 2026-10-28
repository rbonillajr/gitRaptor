//! US-GRD-009 in the daemon (DS-US-GRD-009 D2, D10, D12, D14, D15): the `guard.commit-policies`
//! capability both ways, a floor relaxation that waits for its confirmation, the bounds that
//! never allow, and what is read only when a rule needs it. In-process, over temporary repos and
//! a temporary profile (NFR-01). Decisions are read as the wire JSON, so a test names a rule by
//! its code (`policy.diff-size`).
//!
//! Assumed spelling (the plan names the capability, not the caller flag): `Caller.commit_policies:
//! bool`, the analogue of `Caller.policies` for `guard.policies`.
#![cfg(unix)]

mod common;

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use common::{TempProfile, git};
use gitraptor_api::AgentKind;
use gitraptor_api::guard::{
    EvaluateParams, Hook, Operation as GuardOperation, RefUpdate, RefValue,
};
use gitraptor_api::methods::CAP_GUARD_COMMIT_POLICIES;
use gitraptor_core::guardrails::evaluate::{Caller, serve_as};
use gitraptor_core::guardrails::hook::{HookArgs, floor_rules_for_everyone};
use gitraptor_core::guardrails::{GuardEntry, GuardRegistry};
use gitraptor_policy::team::{Confirmed, ConfirmedFloor};
use serde_json::Value;

const REPO_ID: &str = "0000-ffff";
const SIZE: &str = r#"{"policies":{"diffSize":{"maxLines":400}}}"#;

struct Setup {
    profile: TempProfile,
    repo: tempfile::TempDir,
    common: PathBuf,
    base: String,
    blob: String,
}

impl Setup {
    /// `main` carries `team` as `.gitraptor/settings.json` (the floor).
    fn new(team: &str) -> Self {
        let repo = tempfile::tempdir().unwrap();
        let p = repo.path();
        git(p, &["init", "-q", "-b", "main"]);
        let s = Self {
            profile: TempProfile::new(),
            common: gitraptor_git::paths::canonicalize(&p.join(".git")).unwrap(),
            base: String::new(),
            blob: String::new(),
            repo,
        };
        std::fs::create_dir_all(s.profile.dirs().config).unwrap();
        let mut s = s;
        s.set_floor(team);
        s.base = git(s.repo.path(), &["rev-parse", "main"]);
        s
    }

    /// Commits a new floor on `main` and returns its blob.
    fn set_floor(&mut self, json: &str) -> String {
        let p = self.repo.path();
        git(p, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        std::fs::create_dir_all(p.join(".gitraptor")).unwrap();
        std::fs::write(p.join(".gitraptor/settings.json"), json).unwrap();
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "chore: floor"]);
        self.blob = git(p, &["rev-parse", "main:.gitraptor/settings.json"]);
        self.blob.clone()
    }

    fn registry(&self, confirmed_blob: Option<&str>) -> GuardRegistry {
        let registry = GuardRegistry::for_profile(self.profile.dirs());
        registry.set(
            REPO_ID,
            GuardEntry {
                common_dir: self.common.to_string_lossy().into_owned(),
                bases: vec!["main".into()],
                confirmed: confirmed_blob.map(|b| Confirmed {
                    base_branch: gitraptor_git::RefName::new("main").unwrap(),
                    floor: ConfirmedFloor::Blob(b.to_owned()),
                }),
            },
        );
        registry
    }

    /// A branch from the base with one commit that adds `files` (`path`, number of lines).
    fn branch_with(&self, name: &str, files: &[(&str, usize)], message: &str) -> String {
        let p = self.repo.path();
        git(p, &["switch", "-q", "-c", name, &self.base]);
        for (path, lines) in files {
            let text: String = (0..*lines).map(|i| format!("line {i}\n")).collect();
            std::fs::write(p.join(path), text).unwrap();
        }
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", message]);
        let tip = git(p, &["rev-parse", "HEAD"]);
        git(p, &["switch", "-q", "main"]);
        tip
    }

    fn op(&self, branch: &str, new: &str) -> GuardOperation {
        GuardOperation::RefTransaction {
            updates: vec![RefUpdate {
                refname: format!("refs/heads/{branch}"),
                old: RefValue::Oid(self.base.clone()),
                new: RefValue::Oid(new.to_owned()),
            }],
            orphan_head: None,
        }
    }

    fn caller(agent: bool, commit_policies: bool) -> Caller {
        Caller {
            actor: agent.then_some(AgentKind::ClaudeCode),
            cwd: None,
            policies: true,
            permissions: true,
            commit_policies,
            ..Caller::default()
        }
    }

    fn serve(&self, registry: &GuardRegistry, op: GuardOperation, caller: &Caller) -> Value {
        let params = EvaluateParams {
            repo_id: REPO_ID.into(),
            common_dir: self.common.to_string_lossy().into_owned(),
            hook: Hook::ReferenceTransaction,
            operation: op,
            authorship: None,
            exception_token: None,
        };
        serde_json::to_value(serve_as(registry, &params, caller)).unwrap()
    }
}

fn rules(d: &Value) -> Vec<String> {
    d["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["rule"].as_str().unwrap().to_owned())
        .collect()
}

fn reason<'a>(d: &'a Value, rule: &str) -> &'a Value {
    d["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["rule"] == rule)
        .unwrap_or_else(|| panic!("no `{rule}` reason: {d}"))
}

fn param(r: &Value, kind: &str) -> Option<String> {
    r["params"]
        .as_array()?
        .iter()
        .find(|p| p["kind"] == kind)
        .and_then(|p| p["value"]["untrusted"].as_str())
        .map(str::to_owned)
}

/// A commit object whose tree does not exist, written with `--literally`: reading its tree fails,
/// reading its header does not.
fn commit_without_tree(repo: &std::path::Path, parent: &str, message: &str) -> String {
    let body = format!(
        "tree {}\nparent {parent}\nauthor T <t@example.com> 1700000000 +0000\n\
         committer T <t@example.com> 1700000000 +0000\n\n{message}\n",
        "e".repeat(40)
    );
    let mut child = Command::new("git")
        .args([
            "hash-object",
            "-t",
            "commit",
            "-w",
            "--literally",
            "--stdin",
        ])
        .current_dir(repo)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(body.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// D10: with the capability the daemon applies the two keys; without it the decision is the one
/// of before (nothing new), and the client that talks to such a daemon applies the floor's rules
/// for everyone itself (D14), never the ones for agents.
#[test]
fn commit_policies_capability_both_ways() {
    assert_eq!(CAP_GUARD_COMMIT_POLICIES.name, "guard.commit-policies");
    let s = Setup::new(
        r#"{"policies":{
            "diffSize":{"maxLines":400,"appliesTo":"everyone"},
            "commitFormat":{}}}"#,
    );
    let tip = s.branch_with("feat-x", &[("a.txt", 1200)], "wip");
    let registry = s.registry(Some(&s.blob));

    let with = s.serve(&registry, s.op("feat-x", &tip), &Setup::caller(true, true));
    assert_eq!(with["appliedEffect"], "deny", "{with}");
    let found = rules(&with);
    assert!(found.contains(&"policy.diff-size".into()), "{with}");
    assert!(found.contains(&"policy.commit-format".into()), "{with}");

    // Without the capability: as before, nothing of the two keys.
    let without = s.serve(&registry, s.op("feat-x", &tip), &Setup::caller(true, false));
    assert_eq!(without["appliedEffect"], "allow", "{without}");
    assert!(
        !rules(&without)
            .iter()
            .any(|r| r.starts_with("policy.diff") || r == "policy.commit-format"),
        "{without}"
    );
    // The person with `everyone` is held only with the capability too.
    let person = s.serve(&registry, s.op("feat-x", &tip), &Setup::caller(false, true));
    assert_eq!(person["appliedEffect"], "deny", "{person}");
    assert!(
        !rules(&person).contains(&"policy.commit-format".into()),
        "format is for agents: {person}"
    );
    let person = s.serve(
        &registry,
        s.op("feat-x", &tip),
        &Setup::caller(false, false),
    );
    assert_eq!(person["appliedEffect"], "allow", "{person}");

    // The client of a daemon without the capability: the floor's rules for everyone, evaluated
    // by the client with the same code.
    let args = HookArgs {
        template: 3,
        hook: Hook::ReferenceTransaction,
        repo: REPO_ID.into(),
        common: s.common.clone(),
        channel: s.profile.root.path().join("channel"),
        instance: "i".into(),
        state: s.profile.root.path().join("state"),
        prior: String::new(),
        git_args: Vec::new(),
    };
    let outcome = floor_rules_for_everyone(&args, &s.op("feat-x", &tip))
        .expect("the floor's rule for everyone denies a 1200-line commit");
    let d = serde_json::to_value(outcome.decision.expect("a decision")).unwrap();
    assert_eq!(d["appliedEffect"], "deny", "{d}");
    assert!(rules(&d).contains(&"policy.diff-size".into()), "{d}");
    assert!(
        !rules(&d).contains(&"policy.commit-format".into()),
        "an `agents` rule needs the actor: {d}"
    );
}

/// D2: raising `maxLines` or adding an `exclude` in the floor does not relax until the developer
/// confirms it; lowering hardens at once.
#[test]
fn a_floor_relaxation_waits_for_its_confirmation() {
    let agent = Setup::caller(true, true);

    // Raise 400 -> 1000.
    let mut s = Setup::new(SIZE);
    let confirmed = s.blob.clone();
    let tip = s.branch_with("feat-x", &[("a.txt", 500)], "feat: x");
    let raised = s.set_floor(r#"{"policies":{"diffSize":{"maxLines":1000}}}"#);
    let d = s.serve(&s.registry(Some(&confirmed)), s.op("feat-x", &tip), &agent);
    assert_eq!(
        d["appliedEffect"], "deny",
        "the confirmed 400 still holds: {d}"
    );
    assert!(rules(&d).contains(&"policy.diff-size".into()), "{d}");
    let d = s.serve(&s.registry(Some(&raised)), s.op("feat-x", &tip), &agent);
    assert_eq!(
        d["appliedEffect"], "allow",
        "confirmed, the new limit holds: {d}"
    );

    // Add an exclusion of the lockfile.
    let mut s = Setup::new(SIZE);
    let confirmed = s.blob.clone();
    let tip = s.branch_with(
        "feat-x",
        &[("Cargo.lock", 5000), ("a.txt", 10)],
        "feat: lock",
    );
    let excluded =
        s.set_floor(r#"{"policies":{"diffSize":{"maxLines":400,"exclude":["Cargo.lock"]}}}"#);
    let d = s.serve(&s.registry(Some(&confirmed)), s.op("feat-x", &tip), &agent);
    assert_eq!(d["appliedEffect"], "deny", "the exclusion waits: {d}");
    let d = s.serve(&s.registry(Some(&excluded)), s.op("feat-x", &tip), &agent);
    assert_eq!(
        d["appliedEffect"], "allow",
        "confirmed, the lockfile does not count: {d}"
    );

    // Lower 1000 -> 200: at once, with no confirmation.
    let mut s = Setup::new(r#"{"policies":{"diffSize":{"maxLines":1000}}}"#);
    let confirmed = s.blob.clone();
    let tip = s.branch_with("feat-x", &[("a.txt", 300)], "feat: x");
    let d = s.serve(&s.registry(Some(&confirmed)), s.op("feat-x", &tip), &agent);
    assert_eq!(d["appliedEffect"], "allow", "{d}");
    s.set_floor(r#"{"policies":{"diffSize":{"maxLines":200}}}"#);
    let d = s.serve(&s.registry(Some(&confirmed)), s.op("feat-x", &tip), &agent);
    assert_eq!(d["appliedEffect"], "deny", "a lower limit hardens now: {d}");
    assert_eq!(reason(&d, "policy.diff-size")["level"], "floor", "{d}");
}

/// D15: past a bound the daemon never allows. A count that already exceeds the limit says "at
/// least"; otherwise the commit is `unverifiable`.
#[test]
fn bounds_never_allow() {
    let s = Setup::new(SIZE);
    let registry = s.registry(Some(&s.blob));
    let agent = Setup::caller(true, true);
    let person = Setup::caller(false, true);

    // One blob over 16 MiB: the lines of the rest are counted, the big file cannot be.
    let mib = 1024 * 1024;
    let huge = ("huge.txt", 17 * mib / 2); // "x\n" lines: 17 MiB of text below
    let write_huge = |branch: &str, extra: &[(&str, usize)]| {
        let p = s.repo.path();
        git(p, &["switch", "-q", "-c", branch, &s.base]);
        std::fs::write(p.join(huge.0), "x\n".repeat(huge.1)).unwrap();
        for (path, lines) in extra {
            let text: String = (0..*lines).map(|i| format!("l{i}\n")).collect();
            std::fs::write(p.join(path), text).unwrap();
        }
        git(p, &["add", "."]);
        git(p, &["commit", "-q", "-m", "feat: huge"]);
        let tip = git(p, &["rev-parse", "HEAD"]);
        git(p, &["switch", "-q", "main"]);
        tip
    };
    let tip = write_huge("huge-only", &[]);
    let d = s.serve(&registry, s.op("huge-only", &tip), &agent);
    assert_eq!(
        d["appliedEffect"], "deny",
        "a blob over the bound is not 0 lines: {d}"
    );
    assert_eq!(
        reason(&d, "policy.diff-size")["cause"],
        "unverifiable",
        "{d}"
    );

    let tip = write_huge("huge-plus", &[("small.txt", 500)]);
    let d = s.serve(&registry, s.op("huge-plus", &tip), &agent);
    assert_eq!(d["appliedEffect"], "deny", "{d}");
    let r = reason(&d, "policy.diff-size");
    assert!(
        r.get("cause").is_none_or(|c| c != "unverifiable"),
        "the count already exceeds the limit: {d}"
    );
    let lines = param(r, "lines").expect("lines param");
    assert!(
        lines.starts_with('≥'),
        "\"at least N\" when a bound cut the count: {lines}"
    );
    assert!(
        lines.trim_start_matches('≥').parse::<u32>().unwrap() >= 500,
        "{lines}"
    );

    // More new commits than the bound (256 per update): a chain of plumbing commits.
    let p = s.repo.path();
    let tree = git(p, &["rev-parse", "main^{tree}"]);
    let mut tip = s.base.clone();
    for n in 0..300 {
        tip = git(
            p,
            &[
                "commit-tree",
                &tree,
                "-p",
                &tip,
                "-m",
                &format!("feat: c{n}"),
            ],
        );
        assert_eq!(tip.len(), 40, "commit-tree failed: {tip:?}");
    }
    let d = s.serve(&registry, s.op("chain", &tip), &agent);
    assert_eq!(d["appliedEffect"], "deny", "{d}");
    assert_eq!(
        reason(&d, "policy.diff-size")["cause"],
        "unverifiable",
        "{d}"
    );
    // The person, with a rule for agents only: nothing to verify, nothing denied.
    let d = s.serve(&registry, s.op("chain", &tip), &person);
    assert_eq!(d["appliedEffect"], "allow", "{d}");

    // The total read: five files of 14 MiB each stay under the per-blob bound and over the 64 MiB
    // total, so the count is cut.
    let p = s.repo.path();
    git(p, &["switch", "-q", "-c", "total", &s.base]);
    for k in 0..5 {
        let text: String = (0..14 * mib / 12)
            .map(|i| format!("{k}-{i:08}\n"))
            .collect();
        std::fs::write(p.join(format!("f{k}.txt")), text).unwrap();
    }
    git(p, &["add", "."]);
    git(p, &["commit", "-q", "-m", "feat: total"]);
    let tip = git(p, &["rev-parse", "HEAD"]);
    git(p, &["switch", "-q", "main"]);
    let d = s.serve(&registry, s.op("total", &tip), &agent);
    assert_eq!(d["appliedEffect"], "deny", "{d}");
    assert!(rules(&d).contains(&"policy.diff-size".into()), "{d}");
}

/// D15: the commits are read only when a rule of the two keys reaches the actor, and their trees
/// only when a size rule does. A commit whose tree is missing proves it: reading the tree fails.
#[test]
fn nothing_is_read_that_no_rule_needs() {
    let person = Setup::caller(false, true);

    // Only `commitFormat` for everyone: the header is read, no tree.
    let s = Setup::new(r#"{"policies":{"commitFormat":{"appliesTo":"everyone"}}}"#);
    let registry = s.registry(Some(&s.blob));
    let good = commit_without_tree(s.repo.path(), &s.base, "feat: fine");
    let d = s.serve(&registry, s.op("feat-x", &good), &person);
    assert_eq!(
        d["appliedEffect"], "allow",
        "no tree was read for a format rule: {d}"
    );
    let bad = commit_without_tree(s.repo.path(), &s.base, "arreglos varios");
    let d = s.serve(&registry, s.op("feat-y", &bad), &person);
    assert_eq!(d["appliedEffect"], "deny", "{d}");
    let r = reason(&d, "policy.commit-format");
    assert_ne!(
        r["cause"], "unverifiable",
        "the format is judged without the tree: {d}"
    );

    // A size rule for everyone does read the tree: it is missing, so it cannot be verified.
    let s = Setup::new(r#"{"policies":{"diffSize":{"maxLines":400,"appliesTo":"everyone"}}}"#);
    let registry = s.registry(Some(&s.blob));
    let c = commit_without_tree(s.repo.path(), &s.base, "feat: fine");
    let d = s.serve(&registry, s.op("feat-x", &c), &person);
    assert_eq!(d["appliedEffect"], "deny", "{d}");
    assert_eq!(
        reason(&d, "policy.diff-size")["cause"],
        "unverifiable",
        "{d}"
    );

    // Rules for agents only and the person moving the branch: nothing applies, nothing is read
    // (an object the repo cannot read would otherwise be unverifiable).
    let s = Setup::new(r#"{"policies":{"diffSize":{"maxLines":400},"commitFormat":{}}}"#);
    let registry = s.registry(Some(&s.blob));
    let c = commit_without_tree(s.repo.path(), &s.base, "wip");
    let d = s.serve(&registry, s.op("feat-x", &c), &person);
    assert_eq!(
        d["appliedEffect"], "allow",
        "no rule reaches the person: {d}"
    );
    let missing = "d".repeat(40);
    let d = s.serve(&registry, s.op("feat-y", &missing), &person);
    assert_eq!(d["appliedEffect"], "allow", "nothing was read: {d}");

    // No key at all: nothing is read either, for the agent's commit of 5000 lines too.
    let s = Setup::new("{}");
    let registry = s.registry(Some(&s.blob));
    let tip = s.branch_with("feat-x", &[("a.txt", 5000)], "wip");
    let d = s.serve(&registry, s.op("feat-x", &tip), &Setup::caller(true, true));
    assert_eq!(d["appliedEffect"], "allow", "scenario 4: {d}");
    assert!(
        !rules(&d)
            .iter()
            .any(|r| r.starts_with("policy.diff") || r == "policy.commit-format"),
        "{d}"
    );
}
