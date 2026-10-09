//! The protection of the Guardrails configuration cannot be walked around by parking a commit
//! where no evaluation looks (under `refs/remotes/*`), nor over-denies an agent whose commits do
//! not touch it. Regression tests of the review round; temporary repos only (NFR-01).

mod common;

use common::git;
use gitraptor_api::guard::{
    Cause, Decision, Effect, EvaluateParams, Hook, Operation, PushUpdate, RefUpdate, RefValue, Rule,
};
use gitraptor_api::{AgentKind, Untrusted};
use gitraptor_core::guardrails::evaluate::{Caller, serve_as};
use gitraptor_core::guardrails::{GuardEntry, GuardRegistry};

const SETTINGS: &str = r#"{"policies":{"protectedBranches":{"patterns":["release/*"]}}}"#;

struct Repo {
    dir: tempfile::TempDir,
    registry: GuardRegistry,
    common: String,
}

impl Repo {
    /// `main` holds `a.txt` and `.gitraptor/settings.json`; `main` and `feature` start there.
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        git(path, &["init", "-q", "-b", "main"]);
        std::fs::create_dir_all(path.join(".gitraptor")).unwrap();
        std::fs::write(path.join(".gitraptor/settings.json"), SETTINGS).unwrap();
        std::fs::write(path.join("a.txt"), "a\n").unwrap();
        git(path, &["add", "."]);
        git(path, &["commit", "-q", "-m", "base"]);
        let common = path.join(".git").canonicalize().unwrap();
        let common = common.to_string_lossy().into_owned();
        let registry = GuardRegistry::default();
        registry.set(
            "0000-ffff",
            GuardEntry {
                common_dir: common.clone(),
                bases: vec!["main".into()],
                confirmed: None,
            },
        );
        Self {
            dir,
            registry,
            common,
        }
    }

    fn path(&self) -> &std::path::Path {
        self.dir.path()
    }

    fn git(&self, args: &[&str]) -> String {
        git(self.path(), args)
    }

    fn rev(&self, what: &str) -> String {
        self.git(&["rev-parse", what])
    }

    /// A commit on top of `parent` that applies `change` to a scratch branch, left unreferenced
    /// (only its objects stay), with the working tree and `main` back where they were.
    fn orphan_commit(&self, parent: &str, change: impl FnOnce(&Repo)) -> String {
        self.git(&["checkout", "-q", "-b", "scratch", parent]);
        change(self);
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", "scratch"]);
        let tip = self.rev("HEAD");
        self.git(&["checkout", "-q", "main"]);
        self.git(&["branch", "-q", "-D", "scratch"]);
        tip
    }

    /// A commit that empties the configuration.
    fn bad(&self, parent: &str) -> String {
        self.orphan_commit(parent, |r| {
            std::fs::write(r.path().join(".gitraptor/settings.json"), "{}\n").unwrap();
        })
    }

    fn params(&self, operation: Operation) -> EvaluateParams {
        EvaluateParams {
            repo_id: "0000-ffff".into(),
            common_dir: self.common.clone(),
            hook: Hook::ReferenceTransaction,
            operation,
            authorship: None,
        }
    }

    fn decide(&self, operation: Operation, agent: bool) -> Decision {
        let caller = Caller {
            actor: agent.then_some(AgentKind::ClaudeCode),
            cwd: Some(self.path().to_path_buf()),
            policies: true,
            ..Caller::default()
        };
        serve_as(&self.registry, &self.params(operation), &caller)
    }

    /// An agent moves `refname` from `old` to `new` (`None` is a creation).
    fn tx(&self, refname: &str, old: Option<&str>, new: &str) -> Decision {
        self.tx_as(refname, old, new, true)
    }

    fn tx_as(&self, refname: &str, old: Option<&str>, new: &str, agent: bool) -> Decision {
        self.decide(
            Operation::RefTransaction {
                updates: vec![RefUpdate {
                    refname: refname.into(),
                    old: old.map_or(RefValue::Zero, |o| RefValue::Oid(o.into())),
                    new: RefValue::Oid(new.into()),
                }],
                orphan_head: None,
            },
            agent,
        )
    }

    /// An agent pushes `local` to `remote_ref`, whose value on the remote is `old`.
    fn push(&self, local: &str, remote_ref: &str, old: Option<&str>) -> Decision {
        self.decide(
            Operation::Push {
                remote: Untrusted::new("origin"),
                updates: vec![PushUpdate {
                    local_ref: None,
                    local: RefValue::Oid(local.into()),
                    remote_ref: remote_ref.into(),
                    remote: old.map_or(RefValue::Zero, |o| RefValue::Oid(o.into())),
                }],
            },
            true,
        )
    }
}

fn denied_config(d: &Decision) {
    assert_eq!(d.applied_effect, Effect::Deny, "{d:?}");
    assert!(
        d.reasons.iter().any(|r| r.rule == Rule::ConfigProtected),
        "{d:?}"
    );
}

fn allowed(d: &Decision) {
    assert_eq!(d.applied_effect, Effect::Allow, "{d:?}");
}

#[test]
fn a_commit_parked_under_remote_tracking_cannot_be_walked_onto_a_branch() {
    let r = Repo::new();
    let main = r.rev("main");
    let bad = r.bad(&main);
    // Step one is not governed: the ref is written, nothing is evaluated.
    r.git(&["update-ref", "refs/remotes/origin/x", &bad]);
    // Step two moves a branch to it.
    denied_config(&r.tx("refs/heads/main", Some(&main), &bad));
    // A creation at it too.
    denied_config(&r.tx("refs/heads/other", None, &bad));
    // And with no expected old value (Git sends zero).
    denied_config(&r.tx("refs/heads/main", None, &bad));
}

#[test]
fn a_push_of_a_commit_parked_under_remote_tracking_is_denied() {
    let r = Repo::new();
    let main = r.rev("main");
    let bad = r.bad(&main);
    r.git(&["update-ref", "refs/remotes/origin/x", &bad]);
    denied_config(&r.push(&bad, "refs/heads/main", Some(&main)));
    denied_config(&r.push(&bad, "refs/heads/fresh", None));
}

#[test]
fn a_fetch_into_remote_tracking_then_a_fast_forward_is_denied() {
    let r = Repo::new();
    let main = r.rev("main");
    let bad = r.bad(&main);
    r.git(&["branch", "bad-src", &bad]);
    r.git(&["fetch", "-q", ".", "bad-src:refs/remotes/origin/x"]);
    r.git(&["branch", "-q", "-D", "bad-src"]);
    assert_eq!(r.rev("refs/remotes/origin/x"), bad);
    denied_config(&r.tx("refs/heads/main", Some(&main), &bad));
}

#[test]
fn deleting_renaming_or_replacing_the_configuration_is_denied() {
    let r = Repo::new();
    let main = r.rev("main");
    let deleted = r.orphan_commit(&main, |r| {
        r.git(&["rm", "-rq", ".gitraptor"]);
    });
    denied_config(&r.tx("refs/heads/main", Some(&main), &deleted));

    let renamed = r.orphan_commit(&main, |r| {
        r.git(&["mv", ".gitraptor", "tmp-name"]);
        r.git(&["mv", "tmp-name", ".GitRaptor"]);
    });
    denied_config(&r.tx("refs/heads/main", Some(&main), &renamed));

    // A symlink and a submodule in the place of the directory.
    let blob = {
        std::fs::write(r.path().join("target.tmp"), "x").unwrap();
        let id = r.git(&["hash-object", "-w", "target.tmp"]);
        std::fs::remove_file(r.path().join("target.tmp")).unwrap();
        id
    };
    for (mode, id) in [("120000", blob.clone()), ("160000", main.clone())] {
        let replaced = r.orphan_commit(&main, |r| {
            r.git(&["rm", "-rq", "--cached", ".gitraptor"]);
            std::fs::remove_dir_all(r.path().join(".gitraptor")).unwrap();
            r.git(&[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("{mode},{id},.gitraptor"),
            ]);
        });
        denied_config(&r.tx("refs/heads/main", Some(&main), &replaced));
    }
}

/// A chain of `n` commits written with plumbing on top of `base`, the last one with `tree`.
fn chain(r: &Repo, base: &str, n: usize, tree_of_all: &str, last_tree: &str) -> String {
    let mut tip = base.to_owned();
    for i in 0..n {
        let tree = if i + 1 == n { last_tree } else { tree_of_all };
        tip = r.git(&["commit-tree", tree, "-p", &tip, "-m", &format!("c{i}")]);
    }
    tip
}

fn tree_of(r: &Repo, commit: &str) -> String {
    r.rev(&format!("{commit}^{{tree}}"))
}

#[test]
fn a_different_configuration_behind_more_commits_than_the_bound_cannot_be_verified() {
    let r = Repo::new();
    let main = r.rev("main");
    let bad = r.bad(&main);
    let tip = chain(&r, &main, 300, &tree_of(&r, &main), &tree_of(&r, &bad));
    let d = r.tx("refs/heads/main", Some(&main), &tip);
    denied_config(&d);
    assert_eq!(d.reasons[0].cause, Some(Cause::Unverifiable), "{d:?}");
    // The first push of such a chain (the remote has nothing) is the same.
    let d = r.push(&tip, "refs/heads/fresh", None);
    denied_config(&d);
    assert_eq!(d.reasons[0].cause, Some(Cause::Unverifiable), "{d:?}");
    // The person is never held.
    allowed(&r.tx_as("refs/heads/main", Some(&main), &tip, false));
}

#[test]
fn a_long_rebase_and_a_first_push_of_many_commits_stay_unverifiable() {
    let r = Repo::new();
    let main = r.rev("main");
    let same = tree_of(&r, &main);
    // No remote-tracking branch anywhere in this repo. A rebase of 300 commits whose tree never
    // touches the configuration: the commits are all new and past the bound.
    let tip = chain(&r, &main, 300, &same, &same);
    let d = r.tx("refs/heads/main", Some(&main), &tip);
    assert_eq!(d.applied_effect, Effect::Deny, "{d:?}");
    assert_eq!(d.reasons[0].cause, Some(Cause::Unverifiable), "{d:?}");
    let d = r.push(&tip, "refs/heads/big", None);
    assert_eq!(d.reasons[0].cause, Some(Cause::Unverifiable), "{d:?}");
    // Once the person put them on a local branch, they are held: pushing it is free.
    r.git(&["branch", "big", &tip]);
    allowed(&r.push(&tip, "refs/heads/big", None));
}

#[test]
fn a_shallow_clone_is_judged_like_any_other() {
    let origin = Repo::new();
    origin.git(&["commit", "-q", "--allow-empty", "-m", "second"]);
    let clone = tempfile::tempdir().unwrap();
    let url = format!("file://{}", origin.path().display());
    let dest = clone.path().join("c");
    git(
        clone.path(),
        &["clone", "-q", "--depth", "1", &url, dest.to_str().unwrap()],
    );
    assert_eq!(
        git(&dest, &["rev-parse", "--is-shallow-repository"]),
        "true"
    );
    let common = dest.join(".git").canonicalize().unwrap();
    let common = common.to_string_lossy().into_owned();
    let registry = GuardRegistry::default();
    registry.set(
        "0000-ffff",
        GuardEntry {
            common_dir: common.clone(),
            bases: vec!["main".into()],
            confirmed: None,
        },
    );
    let main = git(&dest, &["rev-parse", "main"]);
    let tree = git(&dest, &["rev-parse", "main^{tree}"]);
    let harmless = git(&dest, &["commit-tree", &tree, "-p", &main, "-m", "h"]);
    // The same tree with the configuration emptied.
    git(&dest, &["rm", "-rq", "--cached", ".gitraptor"]);
    let bad_tree = git(&dest, &["write-tree"]);
    let bad = git(&dest, &["commit-tree", &bad_tree, "-p", &main, "-m", "b"]);
    let eval = |new: &str| {
        let params = EvaluateParams {
            repo_id: "0000-ffff".into(),
            common_dir: common.clone(),
            hook: Hook::ReferenceTransaction,
            operation: Operation::RefTransaction {
                updates: vec![RefUpdate {
                    refname: "refs/heads/main".into(),
                    old: RefValue::Oid(main.clone()),
                    new: RefValue::Oid(new.into()),
                }],
                orphan_head: None,
            },
            authorship: None,
        };
        let caller = Caller {
            actor: Some(AgentKind::ClaudeCode),
            cwd: Some(dest.clone()),
            policies: true,
            ..Caller::default()
        };
        serve_as(&registry, &params, &caller)
    };
    allowed(&eval(&harmless));
    denied_config(&eval(&bad));
}

#[test]
fn what_does_not_touch_the_configuration_is_free() {
    let r = Repo::new();
    let main = r.rev("main");

    // A: `git checkout -b` in a small repo (a creation at an existing commit).
    allowed(&r.tx("refs/heads/feat", None, &main));

    // The person changes the configuration on a branch and keeps it there.
    r.git(&["checkout", "-q", "-b", "feature"]);
    std::fs::write(r.path().join(".gitraptor/settings.json"), "{}\n").unwrap();
    r.git(&["commit", "-q", "-am", "person changes the config"]);
    let changed = r.rev("HEAD");
    r.git(&["checkout", "-q", "main"]);

    // B: the agent fast-forwards `main` to what a local branch already holds.
    allowed(&r.tx("refs/heads/main", Some(&main), &changed));

    // D: a new branch pushed from `main`, and from a base before the change.
    allowed(&r.push(&main, "refs/heads/new", None));
    r.git(&["update-ref", "refs/heads/main", &changed]);
    allowed(&r.push(&main, "refs/heads/older", None));
    allowed(&r.push(&changed, "refs/heads/newer", None));
}

#[test]
fn a_change_that_a_later_commit_reverts_is_no_change() {
    let r = Repo::new();
    let main = r.rev("main");
    let bad = r.bad(&main);
    let reverted = r.orphan_commit(&bad, |r| {
        std::fs::write(r.path().join(".gitraptor/settings.json"), SETTINGS).unwrap();
    });
    assert_eq!(tree_of(&r, &reverted), tree_of(&r, &main));
    allowed(&r.tx("refs/heads/main", Some(&main), &reverted));
}

#[test]
fn a_tag_push_is_not_read_and_the_person_is_never_governed() {
    let r = Repo::new();
    let main = r.rev("main");
    let bad = r.bad(&main);
    allowed(&r.push(&bad, "refs/tags/v1", None));
    allowed(&r.tx_as("refs/heads/main", Some(&main), &bad, false));
    r.git(&["update-ref", "refs/remotes/origin/x", &bad]);
    allowed(&r.tx_as("refs/heads/main", Some(&main), &bad, false));
}
