//! US-MCP-010 (DS-US-MCP-010 T004): the daemon checks every `safe_commit` path against the
//! worktree. The paths are literal names the worktree has, inside it and inside this repo:
//! a parent link, a nested repo, a folder, a gitlink, a skip-worktree entry or a link named like
//! a Git special file is refused before any effect; a link inside is committed as a link; a
//! deletion of a tracked file is committed; another case is not found; an NFD name matches its
//! NFC entry when Git precomposes; and a `.git` or a link changed between `prepare` and `run`
//! commits nothing.
//!
//! The executor runs over the production `DaemonBackend` with the production commit arm
//! (DS-US-MCP-009), the real Guardrails engine and a real oplog, in temporary repos and profiles
//! (NFR-01). Nothing here waits.
#![cfg(unix)]
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::os::unix::fs::symlink;

use gitraptor_testkit::fingerprint::{Scope as FpScope, Snapshot, diff};

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use gitraptor_api::catalog::{Layer, OperationId, PrepareParams, RejectReason, RunParams};
use gitraptor_api::timemachine::{RequestChannel, ResolvedVia};
use gitraptor_api::{Actor, actor};
use gitraptor_core::channel::marks::ExecutorMarks;
use gitraptor_core::channel::peer::SystemProcs;
use gitraptor_core::channel::requester::{Resolution, Who};
use gitraptor_core::executor::ops::CommitEnv;
use gitraptor_core::executor::{
    Caller, ExecError, Executed, Executor, GuardrailsGate, PrepareInput, Prepared, RunDone, RunEnv,
    RunInput,
};
use gitraptor_core::guardrails::plan_gate::GuardrailsEngine;
use gitraptor_core::guardrails::registry::GuardRegistry;
use gitraptor_core::observe::locate;
use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::manual::{ManualAsk, ManualCaptured, ManualError};
use gitraptor_core::timemachine::oplog::{Oplog, Requester, RequesterOrigin};
use gitraptor_core::timemachine::protected::backend::Presence;
use gitraptor_core::timemachine::protected::{
    DaemonBackend, McpRepos, OperationsWiring, ProtectedBackend, TmRepos,
};
use gitraptor_git::SystemGit;
use gitraptor_testkit::Fixture;
use serde_json::{Map, Value, json};

const REPO: &str = "0a1b2c3d-0000-4000-8000-0000000000aa";
const SESSION: &str = "session-1";

fn git() -> PathBuf {
    gitraptor_testkit::fixture::git_from_path()
}

fn system_git() -> SystemGit {
    match gitraptor_git::resolve::resolve(
        &gitraptor_git::resolve::ResolveConfig::for_current_os(None),
        &gitraptor_git::Invoker::default(),
    ) {
        gitraptor_git::resolve::Resolution::Found { git, .. } => git,
        other => panic!("no Git: {other:?}"),
    }
}

/// An executor over the production `DaemonBackend` with the production commit arm, the real
/// Guardrails engine and a real oplog, in a temporary profile (NFR-01).
struct Rig {
    f: Fixture,
    /// The worktree the commit runs in.
    wt: PathBuf,
    backend: DaemonBackend,
    executor: Executor,
    resolution: Resolution,
    marks: Arc<ExecutorMarks>,
    stopping: AtomicBool,
}

/// Nobody else is in the worktree.
fn alone() -> Presence {
    Arc::new(|_: &Path| Some(Vec::new()))
}

impl Rig {
    /// A fixture with a commit, on a feature branch.
    fn new() -> Self {
        let f = Fixture::with_commit(&git());
        f.git(&["checkout", "-q", "-b", "feat/x"]);
        let wt = f.repo.clone();
        Self::build(f, wt, alone(), None)
    }

    fn build(f: Fixture, wt: PathBuf, presence: Presence, limit: Option<Duration>) -> Self {
        let dirs = ProfileDirs::under_root(&f.profile);
        let common = locate(&f.repo).unwrap();
        let (oplog, _) = Oplog::open(&dirs, REPO, 1).unwrap();
        let tm = Arc::new(TmRepos::new(dirs.clone()));
        tm.insert(REPO, &common, oplog);
        let engine = Arc::new(GuardrailsEngine::default());
        let registry = Arc::new(GuardRegistry::for_profile(dirs));
        engine.attach(&registry, Arc::new(|_entry| true));
        let mut wiring = OperationsWiring::production_from(&|_| None).with_commit_env(CommitEnv {
            git: system_git(),
            home: Some(f.home.clone()),
        });
        wiring.gate = engine as Arc<dyn GuardrailsGate>;
        let mut executor = Executor::new(Arc::clone(&wiring.gate));
        if let Some(limit) = limit {
            executor = executor.with_limits(Duration::from_secs(600), limit);
        }
        let backend = DaemonBackend::new(tm, wiring, Arc::new(McpRepos::new([REPO.to_owned()])))
            .with_presence(presence);
        let who = Who {
            actor: Actor::Agent {
                kind: actor::AgentKind::ClaudeCode,
                name: None,
                origin: actor::AgentOrigin::Detected,
            },
            requester: Requester::Agent {
                name: "claude".into(),
                origin: RequesterOrigin::Detected,
                session_id: SESSION.into(),
            },
        };
        Self {
            f,
            wt,
            backend,
            executor,
            resolution: Resolution {
                who,
                via: ResolvedVia::Ancestry,
                executor_operation: None,
                confirmable: false,
            },
            marks: Arc::new(ExecutorMarks::default()),
            stopping: AtomicBool::new(false),
        }
    }

    fn caller() -> Caller {
        Caller {
            connection: 7,
            pid: 4_242,
            start_us: 1,
            mcp: true,
        }
    }

    /// `safe_commit` as the MCP asks it: `paths` `None` is "everything staged".
    fn try_prepare(&self, paths: Option<&[&str]>) -> Result<String, ExecError> {
        self.try_prepared(paths).map(|p| p.plan_id)
    }

    fn try_prepared(&self, paths: Option<&[&str]>) -> Result<Prepared, ExecError> {
        let mut args = Map::new();
        args.insert(
            "message".into(),
            json!("feat: a change\n\nCo-Authored-By: Claude <noreply@anthropic.com>"),
        );
        if let Some(paths) = paths {
            args.insert("paths".into(), Value::from(paths.to_vec()));
        }
        let params = PrepareParams {
            operation: OperationId::Commit,
            worktree: None,
            args,
            surface: None,
            session_env: Vec::new(),
        };
        let repo = self
            .backend
            .repo_of(&self.wt)
            .expect("the worktree is observed");
        self.executor.prepare(
            &self.backend,
            PrepareInput {
                caller: Self::caller(),
                resolution: &self.resolution,
                layer: Layer::Mcp,
                channel: RequestChannel::Mcp,
                params: &params,
                repo,
                confirm_refusal: None,
            },
        )
    }

    fn prepare(&self, paths: Option<&[&str]>) -> String {
        self.try_prepare(paths)
            .unwrap_or_else(|e| panic!("prepare of a commit plan over {paths:?}: {e:?}"))
    }

    fn run(&self, plan_id: &str) -> Result<RunDone, ExecError> {
        let params = RunParams {
            plan_id: plan_id.to_owned(),
            accepted_warnings: Vec::new(),
            confirmation: None,
        };
        let again = || Ok((self.resolution.clone(), Layer::Mcp, None));
        let capture = |ask: &ManualAsk| {
            Ok::<_, ManualError>(ManualCaptured {
                snapshot_id: "00000000-0000-4000-8000-000000000001".into(),
                worktree: ask.worktree.clone(),
            })
        };
        let engine_mark = |_repo: &str, _worktrees: &[PathBuf]| Some(1);
        let after_step = |_repo: &str, _worktrees: &[PathBuf], _operation: &str| {};
        let publish = |_kind: &str, _data| {};
        let env = RunEnv {
            marks: &self.marks,
            procs: &SystemProcs,
            stopping: &self.stopping,
            prior_deadline: Duration::from_secs(30),
            engine_mark: &engine_mark,
            engine: None,
            after_step: &after_step,
            publish: &publish,
            capture: &capture,
        };
        self.executor.run_any(
            &self.backend,
            RunInput {
                caller: Self::caller(),
                resolution: &self.resolution,
                params: &params,
                resolve_again: &again,
                rescope: &|_repo: &gitraptor_core::timemachine::protected::RepoHandle| Ok(()),
            },
            &env,
        )
    }

    /// Prepare and run; the commit it made.
    fn commit(&self, paths: Option<&[&str]>) -> Executed {
        let plan = self.prepare(paths);
        match self.run(&plan) {
            Ok(RunDone::Protected(done)) => done,
            other => panic!("commit over {paths:?} did not land: {other:?}"),
        }
    }

    fn g(&self, args: &[&str]) -> String {
        self.f.git_in(&self.wt, args)
    }

    fn commits(&self) -> usize {
        self.g(&["rev-list", "--count", "HEAD"])
            .trim()
            .parse()
            .unwrap()
    }

    /// The index as Git has it.
    fn index(&self) -> String {
        self.g(&["ls-files", "-s"])
    }

    /// Prepare is refused with `reason` and nothing moved: no commit, same index.
    fn assert_refused(&self, paths: &[&str], reason: RejectReason) {
        let (commits, index) = (self.commits(), self.index());
        let got = self.try_prepare(Some(paths));
        assert_eq!(
            got,
            Err(ExecError::Rejected(reason)),
            "paths {paths:?} must be refused with {reason:?}"
        );
        assert_eq!(self.commits(), commits, "a refused plan made no commit");
        assert_eq!(
            self.index(),
            index,
            "a refused plan left the index as it was"
        );
    }

    /// The paths a commit touched, as `name-status` lines.
    fn changed_by_head(&self) -> String {
        self.g(&["show", "--name-status", "--format=", "HEAD"])
            .trim()
            .to_owned()
    }
}

/// A `pre-commit` hook in the repo's own hooks folder.
fn hook(f: &Fixture, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let p = f.repo.join(".git/hooks/pre-commit");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn canonical(p: &Path) -> PathBuf {
    gitraptor_core::observe::canonical(p)
}

// ----- Guards ------------------------------------------------------------------------------

/// MCP010-G41 (⛔4.1): a path with other case is not found. On APFS `SRC/A.rs` opens
/// `src/a.rs`; accepted, `git add` would create a second index entry for the same file.
#[test]
fn a_path_with_other_case_is_not_found() {
    let rig = Rig::new();
    rig.f.write("a.txt", "alpha changed\n");

    rig.assert_refused(&["A.TXT"], RejectReason::PathNotFound);

    assert_eq!(
        rig.g(&["ls-files", "a.txt", "A.TXT"]).lines().count(),
        1,
        "one entry for the file: the case was never folded"
    );
}

/// MCP010-G42 (⛔4.2): the `.git` of the worktree is rewritten to point at another repo after
/// `prepare`; `run` commits nothing, in either repo (#223 I-03).
#[test]
fn a_dot_git_pointed_at_another_repo_after_prepare_commits_nothing() {
    let ours = Fixture::with_commit(&git());
    ours.git(&["branch", "a"]);
    let a = canonical(&ours.add_worktree("a", "a"));
    std::fs::write(a.join("ours.txt"), "ours\n").unwrap();
    let theirs = Fixture::with_commit(&git());
    theirs.git(&["branch", "x"]);
    let x = canonical(&theirs.add_worktree("x", "x"));
    std::fs::write(theirs.repo.join("secret.txt"), "theirs\n").unwrap();
    std::fs::write(x.join("secret.txt"), "theirs\n").unwrap();
    let theirs_admin = locate(&theirs.repo).unwrap().join("worktrees").join("wt-x");
    let rig = Rig::build(ours, a.clone(), alone(), None);
    let on_a = rig.g(&["rev-list", "--count", "a"]);
    let plan = rig.prepare(Some(&["ours.txt"]));

    // The attacker's move, between prepare and run.
    std::fs::write(
        a.join(".git"),
        format!("gitdir: {}\n", theirs_admin.display()),
    )
    .unwrap();
    let scopes = [
        FpScope::new("ours", rig.f.repo.clone()),
        FpScope::new("ours-a", a.clone()),
        FpScope::new("theirs", theirs.repo.clone()),
        FpScope::new("theirs-x", x.clone()),
    ];
    let before = Snapshot::take(&scopes, &BTreeSet::new());

    let got = rig.run(&plan);

    assert!(
        matches!(
            got,
            Err(ExecError::Rejected(
                RejectReason::StateChanged | RejectReason::RepoIdentityChanged
            ))
        ),
        "a .git pointed at another repo after prepare must refuse the run: {got:?}"
    );
    let changes = diff(&before, &Snapshot::take(&scopes, &BTreeSet::new()));
    assert!(changes.is_empty(), "both repos stay intact: {changes:?}");
    assert_eq!(
        rig.f.git(&["rev-list", "--count", "a"]),
        on_a,
        "the branch of the worktree did not move"
    );
}

/// MCP010-G43 (⛔4.3): a skip-worktree entry, or one outside the sparse checkout, is not on the
/// disk though the index has it: committing it `--only` would record its deletion.
#[test]
fn a_skip_worktree_or_sparse_entry_is_invalid_and_commits_no_deletion() {
    let rig = Rig::new();
    rig.g(&["update-index", "--skip-worktree", "b.txt"]);
    std::fs::remove_file(rig.wt.join("b.txt")).unwrap();
    rig.assert_refused(&["b.txt"], RejectReason::PathInvalid);
    assert!(
        rig.g(&["ls-tree", "HEAD", "b.txt"]).contains("b.txt"),
        "HEAD still has b.txt"
    );

    let rig = Rig::new();
    rig.g(&["sparse-checkout", "set", "--no-cone", "a.txt"]);
    assert!(
        !rig.wt.join("b.txt").exists(),
        "the sparse checkout removed b.txt"
    );
    rig.assert_refused(&["b.txt"], RejectReason::PathInvalid);
    assert!(
        rig.g(&["ls-tree", "HEAD", "b.txt"]).contains("b.txt"),
        "HEAD still has b.txt"
    );
}

// ----- DS-010 § 9.3 -------------------------------------------------------------------------

#[test]
fn a_parent_symlink_is_outside() {
    let rig = Rig::new();
    std::fs::write(rig.f.other_repo.join("secret.txt"), "theirs\n").unwrap();
    symlink(&rig.f.other_repo, rig.wt.join("link")).unwrap();

    rig.assert_refused(&["link/secret.txt"], RejectReason::PathOutsideWorktree);

    assert_eq!(
        rig.f
            .git_in(&rig.f.other_repo, &["status", "--porcelain"])
            .trim(),
        "?? secret.txt",
        "the other repo was not touched"
    );
}

#[test]
fn a_link_inside_is_committed_as_a_link() {
    let rig = Rig::new();
    symlink("a.txt", rig.wt.join("alias")).unwrap();

    let done = rig.commit(Some(&["alias"]));

    assert_eq!(rig.changed_by_head(), "A\talias", "only the link entered");
    assert!(
        rig.g(&["ls-tree", "HEAD", "alias"]).starts_with("120000 "),
        "the entry is a link, not the content of its target"
    );
    assert_eq!(rig.g(&["cat-file", "-p", "HEAD:alias"]), "a.txt");
    let new = done.outcome.output.new_commit.expect("the commit it made");
    assert_eq!(new.paths, vec!["alias".to_owned()]);
}

#[test]
fn a_symlink_named_gitmodules_is_invalid() {
    let rig = Rig::new();
    symlink("a.txt", rig.wt.join(".gitmodules")).unwrap();

    rig.assert_refused(&[".gitmodules"], RejectReason::PathInvalid);
}

#[test]
fn a_path_into_a_nested_repo_is_outside() {
    let rig = Rig::new();
    let nested = rig.wt.join("nested");
    std::fs::create_dir(&nested).unwrap();
    rig.f.git_in(&nested, &["init", "-q"]);
    std::fs::write(nested.join("file.txt"), "inner\n").unwrap();

    rig.assert_refused(&["nested/file.txt"], RejectReason::PathOutsideWorktree);

    assert_eq!(
        rig.f.git_in(&nested, &["status", "--porcelain"]).trim(),
        "?? file.txt",
        "the nested repo was not touched"
    );
}

#[test]
fn a_folder_is_an_invalid_path() {
    let rig = Rig::new();
    rig.f.write("sub/x.txt", "x\n");
    rig.g(&["add", "sub"]);
    rig.g(&["commit", "-q", "-m", "sub"]);
    rig.f.write("sub/x.txt", "changed\n");

    rig.assert_refused(&["sub"], RejectReason::PathInvalid);
}

#[test]
fn a_deleted_tracked_file_is_committed_as_a_deletion() {
    let rig = Rig::new();
    std::fs::remove_file(rig.wt.join("b.txt")).unwrap();
    let before = rig.commits();

    rig.commit(Some(&["b.txt"]));

    assert_eq!(rig.commits(), before + 1, "one commit");
    assert_eq!(rig.changed_by_head(), "D\tb.txt");
}

/// The folder of a tracked file is replaced by a link out of the worktree after `prepare`: the
/// plan is planned again under the lock and `run` is `state-changed`.
#[test]
fn a_link_swapped_in_between_prepare_and_run_is_state_changed() {
    let rig = Rig::new();
    rig.f.write("sub/f.txt", "one\n");
    rig.g(&["add", "sub"]);
    rig.g(&["commit", "-q", "-m", "sub"]);
    rig.f.write("sub/f.txt", "two\n");
    let plan = rig.prepare(Some(&["sub/f.txt"]));
    let (commits, index) = (rig.commits(), rig.index());

    std::fs::rename(rig.wt.join("sub"), rig.wt.join("sub-away")).unwrap();
    symlink(&rig.f.other_repo, rig.wt.join("sub")).unwrap();
    let got = rig.run(&plan);

    assert_eq!(
        got.err(),
        Some(ExecError::Rejected(RejectReason::StateChanged)),
        "a link swapped in after prepare must give state-changed"
    );
    assert_eq!(rig.commits(), commits, "nothing was committed");
    assert_eq!(rig.index(), index, "the index is as it was");
}

#[test]
fn an_nfd_name_matches_its_nfc_entry_when_git_precomposes() {
    let nfc = "caf\u{e9}.txt";
    let nfd = "cafe\u{301}.txt";

    let rig = Rig::new();
    rig.g(&["config", "core.precomposeUnicode", "true"]);
    rig.f.write(nfc, "one\n");
    rig.g(&["add", "--", nfc]);
    rig.g(&["commit", "-q", "-m", "cafe"]);
    rig.f.write(nfc, "two\n");
    let before = rig.commits();
    let done = rig.commit(Some(&[nfd]));
    assert_eq!(
        rig.commits(),
        before + 1,
        "the NFD name found its NFC entry"
    );
    let new = done.outcome.output.new_commit.expect("the commit it made");
    assert_eq!(new.paths, vec![nfc.to_owned()], "Git gets the NFC form");

    // Without the option, bytes are exact: the NFD name is another name.
    let rig = Rig::new();
    rig.g(&["config", "core.precomposeUnicode", "false"]);
    rig.f.write(nfc, "one\n");
    rig.g(&["add", "--", nfc]);
    rig.g(&["commit", "-q", "-m", "cafe"]);
    rig.f.write(nfc, "two\n");
    rig.assert_refused(&[nfd], RejectReason::PathNotFound);
}

/// The two forms of the `.git` rule, `check_commit_path` side and `tree_path` side, agree.
#[test]
fn dotgit_rules_match_the_tree_path_rules() {
    let corpus = [
        ".git",
        ".GIT",
        ".Git",
        "GIT~1",
        "git~1",
        ".git.",
        ".git ",
        ".git. .",
        ".git::$INDEX_ALLOCATION",
        ".git:stream",
        "\u{200C}.git",
        ".g\u{200D}it",
        ".g\u{206A}it",
        ".git\u{206F}",
        "\u{FEFF}.git",
        ".gitignore",
        ".gitmodules",
        ".github",
        ".gitt",
        "git",
        "a.git",
        "gitt~1",
        "git~2",
        ".gi t",
        "..",
        ".",
        "a.txt",
        "ñandú",
    ];
    for c in corpus {
        assert_eq!(
            gitraptor_api::commit_paths::is_dotgit_alias(c),
            gitraptor_git::tm_write::tree_path::is_dotgit(c.as_bytes()),
            "is_dotgit_alias and tree_path::is_dotgit disagree on {c:?}"
        );
    }
    for c in [
        ".git",
        ".GIT",
        "GIT~1",
        ".git.",
        ".git ",
        ".g\u{200D}it",
        "\u{206A}.git",
    ] {
        assert!(
            gitraptor_api::commit_paths::is_dotgit_alias(c),
            "{c:?} names .git"
        );
    }
    for c in [".gitignore", ".github", "git", "a.git"] {
        assert!(
            !gitraptor_api::commit_paths::is_dotgit_alias(c),
            "{c:?} is not .git"
        );
    }
}

/// Ignored files, by `info/exclude` and by the user's ignore file, are refused by name: the
/// commit never adds them (`add` without `-f` is the second barrier).
#[test]
fn the_ignore_rules_of_info_exclude_and_the_user_apply() {
    let rig = Rig::new();
    let exclude = rig.wt.join(".git/info/exclude");
    std::fs::create_dir_all(exclude.parent().unwrap()).unwrap();
    std::fs::write(&exclude, "*.secret\n").unwrap();
    // The user's ignore file of the fixture's home (`*.swp`), as `core.excludesFile`.
    let user_ignore = rig.f.home.join(".config/git/ignore");
    rig.g(&["config", "core.excludesFile", user_ignore.to_str().unwrap()]);
    rig.f.write("k.secret", "canary\n");
    rig.f.write("x.swp", "canary\n");
    rig.f.write("new.txt", "fine\n");

    rig.assert_refused(&["k.secret"], RejectReason::PathIgnored);
    rig.assert_refused(&["x.swp"], RejectReason::PathIgnored);
    rig.commit(Some(&["new.txt"]));
    assert_eq!(
        rig.changed_by_head(),
        "A\tnew.txt",
        "only the named, not ignored, file"
    );
}
