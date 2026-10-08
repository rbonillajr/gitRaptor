//! The applier of TS-TMC-003 (ADR-TMC-002 § 3) on temporary repos of the testkit, never this
//! repo or the real profile (NFR-01). Each test plays the protected operation (TS-TMC-004) by
//! hand: an operation recorded and `ready` with its guaranteed prior snapshot. On Windows the
//! executable bit and symbolic links are left out (NTFS has no such bit; links need a
//! privilege): junctions play the link that must never be followed (DS-TS-TMC-003 W1–W5).

mod tm_common;

use std::collections::BTreeMap;
use std::path::Path;

use gitraptor_core::timemachine::apply::{
    Applier, ApplyError, ApplyHooks, ApplyPlan, ApplyReport, ApplyWarning, PathIssue, PlanWorktree,
    RefScope, Refusal,
};
use gitraptor_core::timemachine::oplog::{
    Channel, NewOperation, OperationKind, OperationState, OperationTransition, Requester, Scope,
    SnapshotLevel, Target,
};
use gitraptor_git::resolve::{self, Resolution, ResolveConfig};
use gitraptor_git::tm_write::WriteContext;
use gitraptor_git::{Invoker, SystemGit};
use gitraptor_testkit::Fixture;
#[cfg(unix)]
use gitraptor_testkit::canary::Canary;
use tm_common::{Env, REPO_ID, git};

fn system_git() -> SystemGit {
    match resolve::resolve(&ResolveConfig::for_current_os(None), &Invoker::default()) {
        Resolution::Found { git, .. } => git,
        Resolution::NotFound { diagnostics } => panic!("tests need Git >= 2.38: {diagnostics:?}"),
    }
}

struct Apply {
    env: Env,
    write: WriteContext,
}

impl Apply {
    fn new(f: Fixture) -> Self {
        let env = Env::new(f);
        let write = WriteContext::new(
            system_git(),
            Invoker::default(),
            &env.tm_root().join(REPO_ID),
        )
        .unwrap();
        Self { env, write }
    }

    fn busy() -> Self {
        Self::new(Fixture::busy(&git()))
    }

    fn f(&self) -> &Fixture {
        &self.env.f
    }

    fn snapshot(&self) -> String {
        self.env
            .capture(SnapshotLevel::Observation, None)
            .snapshot_id
    }

    /// The protected operation, up to `ready`: intent, guaranteed prior, ready.
    fn ready(&self, target: &str) -> (String, String) {
        let op = self
            .env
            .oplog
            .lock()
            .unwrap()
            .record_operation(
                &NewOperation {
                    kind: OperationKind::Restore,
                    subtype: None,
                    scope: Scope {
                        worktrees: vec![self.f().repo.display().to_string()],
                        refs: vec![],
                    },
                    requester: Requester::Unattributed,
                    channel: Channel::Cli,
                    confirmed: true,
                    target: Target::Snapshot(target.to_owned()),
                    warnings: vec![],
                    engine_mark: 1,
                },
                10,
            )
            .unwrap();
        let mut req = self.env.request(SnapshotLevel::GuaranteedPrior, None);
        req.cause_operation = Some(op.clone());
        let prior = self
            .env
            .store
            .capture(&self.env.oplog, &req)
            .unwrap()
            .snapshot_id;
        let mut log = self.env.oplog.lock().unwrap();
        log.advance_operation(
            &op,
            OperationTransition::PriorSnapshot {
                snapshot_id: &prior,
            },
            11,
        )
        .unwrap();
        log.advance_operation(&op, OperationTransition::Ready, 12)
            .unwrap();
        (op, prior)
    }

    fn plan(&self, target: &str, prior: &str) -> ApplyPlan {
        ApplyPlan {
            target_snapshot: target.to_owned(),
            prior_snapshot: prior.to_owned(),
            worktrees: vec![PlanWorktree {
                key: "main".into(),
                root: self.f().repo.clone(),
                recreate_id: None,
            }],
            refs: RefScope::All,
        }
    }

    fn applier(&self, hooks: ApplyHooks) -> Applier<'_> {
        Applier::new(
            &self.env.store,
            &self.write,
            &self.env.oplog,
            self.f().repo.clone(),
            self.f().profile.clone(),
        )
        .with_clock(|| 20)
        .with_hooks(hooks)
    }

    fn apply(&self, target: &str, hooks: ApplyHooks) -> (String, Result<ApplyReport, ApplyError>) {
        let (op, prior) = self.ready(target);
        let plan = self.plan(target, &prior);
        let result = self.applier(hooks).apply(&op, &plan);
        (op, result)
    }

    fn state_of(&self, op: &str) -> OperationState {
        self.env
            .oplog
            .lock()
            .unwrap()
            .operation(op)
            .unwrap()
            .unwrap()
            .state
    }

    /// Index, refs and `HEAD` as Git sees them.
    fn git_state(&self) -> (String, String, String) {
        (
            self.f().git(&["ls-files", "-s"]),
            self.f().git(&[
                "for-each-ref",
                "--format=%(refname) %(objectname)",
                "refs/heads",
                "refs/stash",
            ]),
            std::fs::read_to_string(self.f().repo.join(".git/HEAD")).unwrap(),
        )
    }

    /// Working tree content, `.git` and ignored `target/` aside; links by their target.
    fn files(&self) -> BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                if rel == ".git" || rel == "target" {
                    continue;
                }
                let meta = path.symlink_metadata().unwrap();
                if meta.file_type().is_symlink() {
                    let target = std::fs::read_link(&path).unwrap();
                    out.insert(rel, target.into_os_string().into_encoded_bytes());
                } else if meta.is_dir() {
                    walk(root, &path, out);
                } else {
                    out.insert(rel, std::fs::read(&path).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(&self.f().repo, &self.f().repo, &mut out);
        out
    }
}

fn steps(env: &Env, op: &str) -> Vec<String> {
    env.oplog
        .lock()
        .unwrap()
        .journal(op)
        .unwrap()
        .into_iter()
        .map(|e| match (e.state, e.step) {
            (Some(s), Some(n)) => format!("{s}:{n}"),
            (Some(s), None) => s,
            (None, _) => e.entry,
        })
        .collect()
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    path.symlink_metadata().unwrap().permissions().mode()
}

/// Changes of every kind after the target: content, removal, new files and folders, a type
/// change, the exec bit, a link, the index, a commit on `main` and a new branch.
fn change_everything(f: &Fixture) {
    f.write("a.txt", "alpha rewritten\n");
    std::fs::remove_file(f.repo.join("b.txt")).unwrap();
    f.write("new/deep/file.txt", "new\n");
    std::fs::remove_file(f.repo.join("untracked.txt")).unwrap();
    f.write("untracked.txt/inner", "the file became a folder\n");
    #[cfg(unix)]
    {
        std::fs::set_permissions(
            f.repo.join("staged.txt"),
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        std::os::unix::fs::symlink("a.txt", f.repo.join("link")).unwrap();
    }
    f.git(&["add", "-A"]);
    f.git(&["commit", "-q", "-m", "after the target"]);
    f.git(&["branch", "extra"]);
    f.write("a.txt", "dirty again\n");
}

/// A link to a folder at `at`: a symbolic link on Unix, a junction (no privilege needed) on
/// Windows.
fn plant_link(target: &Path, at: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, at).unwrap();
    #[cfg(windows)]
    {
        let out = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(at)
            .arg(target)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
    }
}

mod repo_intact {
    use super::*;

    #[test]
    fn restores_working_tree_index_and_refs_exactly() {
        let t = Apply::busy();
        #[cfg(unix)]
        std::fs::set_permissions(
            t.f().repo.join("b.txt"),
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let target = t.snapshot();
        let files_before = t.files();
        let git_before = t.git_state();
        let ignored = std::fs::read(t.f().repo.join("target/out.bin")).unwrap();

        change_everything(t.f());
        assert_ne!(t.files(), files_before);

        let (op, result) = t.apply(&target, ApplyHooks::default());
        let report = result.unwrap();
        assert!(report.paths.is_empty(), "{report:?}");
        assert_eq!(t.files(), files_before);
        assert_eq!(t.git_state(), git_before);
        #[cfg(unix)]
        {
            assert_eq!(mode(&t.f().repo.join("b.txt")) & 0o111, 0o111);
            assert_eq!(mode(&t.f().repo.join("staged.txt")) & 0o100, 0);
        }
        // Ignored content is never touched; no temporary is left behind.
        assert_eq!(
            std::fs::read(t.f().repo.join("target/out.bin")).unwrap(),
            ignored
        );
        assert!(!t.f().repo.join(".git/index.lock").exists());
        assert!(t.files().keys().all(|p| !p.contains(".gitraptor-tm-")));

        assert_eq!(t.state_of(&op), OperationState::Finished);
        let steps = steps(&t.env, &op);
        let applying: Vec<&str> = steps
            .iter()
            .filter(|s| s.starts_with("applying"))
            .map(String::as_str)
            .collect();
        assert_eq!(
            applying,
            [
                "applying:3",
                "applying:4",
                "applying:5",
                "applying:6",
                "applying:7"
            ]
        );
        // On Windows the lock is not annotated yet: its identity for recovery is XP-15.
        #[cfg(unix)]
        assert!(steps.contains(&"lock-taken".to_owned()), "{steps:?}");
        assert!(steps.contains(&"lock-released".to_owned()), "{steps:?}");
        assert_eq!(steps.last().unwrap(), "finished");
        // `git status` agrees with the snapshot: only what was dirty at the target is dirty.
        let status = t.f().git(&["status", "--porcelain"]);
        assert!(status.contains(" M a.txt"), "{status}");
        assert!(status.contains("A  staged.txt"), "{status}");
    }

    #[test]
    fn objects_the_repo_lost_come_back_from_the_store() {
        let t = Apply::busy();
        t.f().write("lost.txt", "only in a dropped commit\n");
        t.f().git(&["add", "lost.txt"]);
        t.f().git(&["commit", "-q", "-m", "to be dropped"]);
        let target = t.snapshot();
        let dropped = t.f().git(&["rev-parse", "HEAD"]).trim().to_owned();
        t.f().git(&["reset", "-q", "--hard", "HEAD~1"]);
        t.f().git(&["reflog", "expire", "--expire=now", "--all"]);
        t.f().git(&["gc", "-q", "--prune=now"]);
        assert!(
            t.f()
                .git_command(&t.f().repo, &["cat-file", "-e", &dropped])
                .output()
                .unwrap()
                .status
                .code()
                != Some(0)
        );

        let (_, result) = t.apply(&target, ApplyHooks::default());
        result.unwrap();
        assert_eq!(t.f().git(&["rev-parse", "main"]).trim(), dropped);
        t.f().git(&["fsck", "--no-dangling"]);
        assert!(!t.f().git(&["count-objects", "-v"]).is_empty());
        let keeps: Vec<_> = std::fs::read_dir(t.f().repo.join(".git/objects/pack"))
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|x| x == "keep")
            })
            .collect();
        assert!(
            keeps.is_empty(),
            "the keep file must be released after the refs"
        );
    }

    #[test]
    fn a_foreign_lock_rejects_and_stays() {
        let t = Apply::busy();
        let target = t.snapshot();
        change_everything(t.f());
        let (op, prior) = t.ready(&target);
        let lock = t.f().repo.join(".git/index.lock");
        std::fs::write(&lock, b"an agent's git add").unwrap();
        let before = (t.files(), t.git_state());

        let err = t
            .applier(ApplyHooks::default())
            .apply(&op, &t.plan(&target, &prior));
        match err {
            Err(ApplyError::Rejected(r)) => {
                assert!(
                    r.iter()
                        .any(|r| matches!(r, Refusal::GitBusy { lock: l } if *l == lock)),
                    "{r:?}"
                )
            }
            other => panic!("expected a rejection: {other:?}"),
        }
        assert_eq!(std::fs::read(&lock).unwrap(), b"an agent's git add");
        std::fs::remove_file(&lock).unwrap();
        assert_eq!((t.files(), t.git_state()), before);
        assert_eq!(t.state_of(&op), OperationState::Rejected);
    }

    #[test]
    fn an_operation_in_progress_rejects() {
        let t = Apply::busy();
        let target = t.snapshot();
        let (op, prior) = t.ready(&target);
        let head = t.f().git(&["rev-parse", "HEAD"]);
        std::fs::write(t.f().repo.join(".git/MERGE_HEAD"), head).unwrap();
        let applier = t.applier(ApplyHooks::default());
        let plan = t.plan(&target, &prior);
        assert!(matches!(
            applier.check_preconditions(&plan).as_slice(),
            [Refusal::InProgress {
                marker: "MERGE_HEAD",
                ..
            }]
        ));
        assert!(matches!(
            applier.apply(&op, &plan),
            Err(ApplyError::Rejected(_))
        ));
        assert_eq!(t.state_of(&op), OperationState::Rejected);
    }

    #[test]
    fn a_branch_moved_before_applying_rejects_without_changes() {
        let t = Apply::busy();
        let target = t.snapshot();
        change_everything(t.f());
        let (op, prior) = t.ready(&target);
        // `main` is a ref the plan moves; an agent moves it after the prior snapshot.
        t.f().git(&["update-ref", "refs/heads/main", "feature"]);
        let before = (t.files(), t.git_state());
        let result = t
            .applier(ApplyHooks::default())
            .apply(&op, &t.plan(&target, &prior));
        assert!(
            matches!(&result, Err(ApplyError::Rejected(r)) if matches!(r.as_slice(), [Refusal::RefMoved { .. }])),
            "{result:?}"
        );
        assert_eq!((t.files(), t.git_state()), before);
    }

    #[test]
    fn a_branch_moved_while_applying_fails_the_whole_transaction() {
        let t = Apply::busy();
        let target = t.snapshot();
        change_everything(t.f());
        let (op, prior) = t.ready(&target);
        let files_before = t.files();
        let repo = t.f().repo.clone();
        let git_bin = git();
        let hooks = ApplyHooks {
            at_step: Some(Box::new(move |step| {
                if step == 5 {
                    // An agent moves `main` between the planning and the transaction.
                    let out = std::process::Command::new(&git_bin)
                        .current_dir(&repo)
                        .args(["update-ref", "refs/heads/main", "refs/heads/feature"])
                        .output()
                        .unwrap();
                    assert!(out.status.success());
                }
            })),
            ..ApplyHooks::default()
        };
        let feature = t.f().git(&["rev-parse", "feature"]);
        let result = t.applier(hooks).apply(&op, &t.plan(&target, &prior));
        match result {
            Err(ApplyError::Interrupted {
                step: 5,
                changed: false,
                ..
            }) => {}
            other => panic!("expected the ref transaction to fail whole: {other:?}"),
        }
        // No ref of the plan moved, no file was touched; the agent's move stands.
        assert_eq!(t.files(), files_before);
        assert_eq!(t.f().git(&["rev-parse", "main"]), feature);
        assert!(
            !t.f()
                .git(&["rev-parse", "--verify", "-q", "extra"])
                .is_empty()
        );
        assert!(!t.f().repo.join(".git/index.lock").exists());
        assert_eq!(t.state_of(&op), OperationState::Interrupted);
    }

    #[test]
    fn a_write_between_compare_and_exchange_is_overlap_and_kept() {
        let t = Apply::busy();
        let target = t.snapshot();
        change_everything(t.f());
        let hooks = ApplyHooks {
            before_exchange: Some(Box::new(|root: &Path, rel: &[u8]| {
                if rel == b"a.txt" {
                    std::fs::write(root.join("a.txt"), b"an agent wrote this\n").unwrap();
                }
            })),
            ..ApplyHooks::default()
        };
        let (op, result) = t.apply(&target, hooks);
        let report = result.unwrap();
        assert_eq!(
            report.paths,
            vec![gitraptor_core::timemachine::apply::PathReport {
                worktree: "main".into(),
                path: "a.txt".into(),
                issue: PathIssue::Overlap { kept_at: None },
            }]
        );
        assert_eq!(
            std::fs::read(t.f().repo.join("a.txt")).unwrap(),
            b"an agent wrote this\n"
        );
        // Everything else reached the target.
        assert_eq!(std::fs::read(t.f().repo.join("b.txt")).unwrap(), b"beta\n");
        assert!(!t.f().repo.join("new").exists());
        assert_eq!(t.state_of(&op), OperationState::Finished);
    }

    /// Windows (DS-TS-TMC-003 W5): an editor holds the second of three files open without
    /// `FILE_SHARE_DELETE`. The application stops as interrupted, naming the file, which keeps
    /// its content; once the editor closes, undo (back to the prior snapshot) restores
    /// everything as it was before the application.
    #[cfg(windows)]
    #[test]
    fn a_file_open_in_an_editor_interrupts_and_undo_recovers_once_closed() {
        use std::os::windows::fs::OpenOptionsExt;
        let t = Apply::busy();
        for name in ["w1.txt", "w2.txt", "w3.txt"] {
            t.f().write(name, &format!("{name} at the target\n"));
        }
        let target = t.snapshot();
        for name in ["w1.txt", "w2.txt", "w3.txt"] {
            t.f().write(name, &format!("{name} after the target\n"));
        }
        let before = t.files();
        let editor = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1 | 0x2)
            .open(t.f().repo.join("w2.txt"))
            .unwrap();
        let (op, prior) = t.ready(&target);
        let result = t
            .applier(ApplyHooks::default())
            .apply(&op, &t.plan(&target, &prior));
        match result {
            Err(ApplyError::Interrupted { reason, .. }) => {
                assert!(reason.contains("w2.txt"), "{reason}");
            }
            other => panic!("not interrupted: {other:?}"),
        }
        assert_eq!(t.state_of(&op), OperationState::Interrupted);
        assert_eq!(
            std::fs::read(t.f().repo.join("w2.txt")).unwrap(),
            b"w2.txt after the target\n"
        );
        assert!(t.files().keys().all(|p| !p.contains(".gitraptor-tm-")));
        drop(editor);

        let (undo, result) = t.apply(&prior, ApplyHooks::default());
        result.unwrap();
        assert_eq!(t.state_of(&undo), OperationState::Finished);
        assert_eq!(t.files(), before);
    }

    /// M1 exit criterion 3 at the engine's level, on every OS: uncommitted work thrown away by
    /// a real `git reset --hard` comes back by applying the snapshot that captured it.
    #[test]
    fn work_thrown_away_by_a_raw_reset_hard_comes_back() {
        let t = Apply::busy();
        t.f().write("a.txt", "uncommitted work\n");
        let captured = t.snapshot();
        let files = t.files();
        t.f().git(&["reset", "-q", "--hard"]);
        assert_ne!(
            std::fs::read(t.f().repo.join("a.txt")).unwrap(),
            b"uncommitted work\n"
        );
        let (op, result) = t.apply(&captured, ApplyHooks::default());
        let report = result.unwrap();
        assert!(report.paths.is_empty(), "{report:?}");
        assert_eq!(t.state_of(&op), OperationState::Finished);
        assert_eq!(t.files(), files);
    }

    #[test]
    fn a_link_planted_while_applying_never_leads_outside() {
        let t = Apply::busy();
        t.f().write("dir/inside.txt", "inside\n");
        let target = t.snapshot();
        std::fs::remove_dir_all(t.f().repo.join("dir")).unwrap();
        let outside = t.f().root.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let repo = t.f().repo.clone();
        let outside_link = outside.clone();
        let hooks = ApplyHooks {
            at_step: Some(Box::new(move |step| {
                if step == 6 {
                    plant_link(&outside_link, &repo.join("dir"));
                }
            })),
            ..ApplyHooks::default()
        };
        let (_, result) = t.apply(&target, hooks);
        let report = result.unwrap();
        assert!(
            report
                .paths
                .iter()
                .any(|p| p.path == "dir/inside.txt" && matches!(p.issue, PathIssue::Blocked(_))),
            "{report:?}"
        );
        assert_eq!(
            std::fs::read_dir(&outside).unwrap().count(),
            0,
            "a write escaped the worktree"
        );
    }

    #[test]
    fn excluded_paths_are_never_written_or_removed() {
        let t = Apply::busy();
        std::fs::create_dir_all(t.f().repo.join("nested")).unwrap();
        t.f().git_in(&t.f().repo.join("nested"), &["init", "-q"]);
        std::fs::write(t.f().repo.join("nested/inner.txt"), b"v1").unwrap();
        let target = t.snapshot();
        std::fs::write(t.f().repo.join("nested/inner.txt"), b"v2").unwrap();
        let (_, result) = t.apply(&target, ApplyHooks::default());
        let report = result.unwrap();
        assert!(
            report
                .warnings
                .iter()
                .any(|w| matches!(w, ApplyWarning::Excluded { path, .. } if path == "nested"))
        );
        assert_eq!(
            std::fs::read(t.f().repo.join("nested/inner.txt")).unwrap(),
            b"v2"
        );
    }

    /// Unix only: the testkit canary (SEC-09) is not ported to Windows yet (XP-08).
    #[cfg(unix)]
    #[test]
    fn internal_writes_run_no_configurable_program() {
        let c = Canary::arm(Fixture::busy(&git()));
        let t = Apply::new(c.f);
        let markers = c.markers.clone();
        let target = t.snapshot();
        // No `git add` here: the canary's required filter would fail it.
        t.f().write("b.txt", "changed\n");
        t.f().write("new.txt", "new\n");
        std::fs::remove_file(t.f().repo.join("staged.txt")).unwrap();
        t.f().git(&["branch", "extra"]);
        let before_files = t.files();
        let (op, prior) = t.ready(&target);
        // Armed after the captures, which read with gix: a hostile `core.worktree`.
        let elsewhere = t.f().root.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        t.f()
            .git(&["config", "core.worktree", elsewhere.to_str().unwrap()]);
        // The setup's own `git branch` ran the armed hook: only the application counts.
        for m in std::fs::read_dir(&markers).unwrap() {
            std::fs::remove_file(m.unwrap().path()).unwrap();
        }
        let result = t
            .applier(ApplyHooks::default())
            .apply(&op, &t.plan(&target, &prior));
        t.f().git(&["config", "--unset", "core.worktree"]);
        let report = result.unwrap();
        assert!(report.paths.is_empty(), "{report:?}");
        assert_ne!(t.files(), before_files);
        assert_eq!(std::fs::read(t.f().repo.join("b.txt")).unwrap(), b"beta\n");
        assert!(!t.f().repo.join("new.txt").exists());
        assert!(t.f().git(&["branch", "--list", "extra"]).is_empty());
        let fired: Vec<_> = std::fs::read_dir(&markers)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(fired.is_empty(), "fired: {fired:?}");
        assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
    }

    #[test]
    fn a_second_application_on_the_same_repo_is_busy() {
        let t = Apply::busy();
        let target = t.snapshot();
        let (op, prior) = t.ready(&target);
        let key = t.env.store.path().display().to_string();
        let guard = gitraptor_core::timemachine::repo_lock::try_lock(&key).unwrap();
        let result = t
            .applier(ApplyHooks::default())
            .apply(&op, &t.plan(&target, &prior));
        drop(guard);
        assert!(
            matches!(&result, Err(ApplyError::Rejected(r)) if r == &[Refusal::RepoBusy]),
            "{result:?}"
        );
    }

    #[test]
    fn a_deleted_worktree_is_recreated_without_checkout() {
        let f = Fixture::busy(&git());
        let wt = f.add_worktree("w", "feature");
        let t = Apply::new(f);
        std::fs::write(wt.join("wip.txt"), b"work in progress\n").unwrap();
        let mut req = t.env.request(SnapshotLevel::Observation, None);
        req.worktrees
            .push(gitraptor_core::timemachine::store::WorktreeScope {
                key: "w".into(),
                path: wt.clone(),
                hint: None,
            });
        let target = t.env.store.capture(&t.env.oplog, &req).unwrap().snapshot_id;
        t.f()
            .git(&["worktree", "remove", "--force", wt.to_str().unwrap()]);
        assert!(!wt.exists());

        let (op, prior) = t.ready(&target);
        let mut plan = t.plan(&target, &prior);
        plan.worktrees.push(PlanWorktree {
            key: "w".into(),
            root: wt.clone(),
            recreate_id: Some("w".into()),
        });
        let report = t.applier(ApplyHooks::default()).apply(&op, &plan).unwrap();
        assert!(report.paths.is_empty(), "{report:?}");
        assert_eq!(
            std::fs::read(wt.join("wip.txt")).unwrap(),
            b"work in progress\n"
        );
        assert_eq!(std::fs::read(wt.join("c.txt")).unwrap(), b"gamma\n");
        let list = t.f().git(&["worktree", "list", "--porcelain"]);
        assert!(
            list.contains(&format!("worktree {}", wt.display())),
            "{list}"
        );
        assert!(list.contains("branch refs/heads/feature"), "{list}");
        let status = t.f().git_in(&wt, &["status", "--porcelain"]);
        assert_eq!(status.trim(), "?? wip.txt");
    }

    #[test]
    fn without_atomic_exchange_paths_are_reported_and_left_alone() {
        let t = Apply::busy();
        let target = t.snapshot();
        change_everything(t.f());
        let files_before = t.files();
        let hooks = ApplyHooks {
            simulate_no_exchange: true,
            ..ApplyHooks::default()
        };
        let (_, result) = t.apply(&target, hooks);
        let report = result.unwrap();
        assert_eq!(report.written + report.removed, 0, "{report:?}");
        assert!(!report.paths.is_empty());
        assert!(
            report
                .paths
                .iter()
                .all(|p| p.issue == PathIssue::NotGuaranteed),
            "{report:?}"
        );
        let mut expected = vec!["a.txt", "b.txt", "new/deep/file.txt"];
        if cfg!(unix) {
            expected.push("link");
        }
        for path in expected {
            assert!(
                report.paths.iter().any(|p| p.path == path),
                "{path} not reported"
            );
        }
        assert_eq!(t.files(), files_before, "a path was touched");
    }

    #[test]
    fn restoring_the_stash_keeps_every_older_entry() {
        let t = Apply::busy();
        let f = t.f();
        f.write("b.txt", "first stash\n");
        f.git(&["stash", "push", "-q", "--", "b.txt"]);
        let first = f.git(&["rev-parse", "refs/stash"]);
        let target = t.snapshot();
        f.write("b.txt", "second stash\n");
        f.git(&["stash", "push", "-q", "--", "b.txt"]);
        let second = f.git(&["rev-parse", "refs/stash"]);

        let (_, result) = t.apply(&target, ApplyHooks::default());
        let report = result.unwrap();
        assert!(
            report.warnings.contains(&ApplyWarning::StashTopOnly),
            "{report:?}"
        );
        assert_eq!(f.git(&["rev-parse", "refs/stash"]), first);
        let entries = f.git(&["reflog", "show", "--format=%H", "refs/stash"]);
        assert!(
            entries.contains(second.trim()),
            "the newer stash was lost: {entries}"
        );
        assert!(entries.contains(first.trim()));
    }

    #[test]
    fn a_stash_absent_from_the_target_is_never_deleted() {
        let t = Apply::busy();
        let target = t.snapshot();
        t.f().write("b.txt", "stashed later\n");
        t.f().git(&["stash", "push", "-q", "--", "b.txt"]);
        let stash = t.f().git(&["rev-parse", "refs/stash"]);
        let (_, result) = t.apply(&target, ApplyHooks::default());
        let report = result.unwrap();
        assert!(
            report.warnings.contains(&ApplyWarning::StashKept),
            "{report:?}"
        );
        assert_eq!(t.f().git(&["rev-parse", "refs/stash"]), stash);
        assert_eq!(t.f().git(&["stash", "list"]).lines().count(), 1);
    }

    #[test]
    fn hostile_names_in_the_meta_are_rejected_before_git_runs() {
        use gitraptor_git::tm_write::refs::{RefUpdate, branch_ref};
        for name in [
            "refs/heads/--upload-pack=x",
            "refs/remotes/x",
            "refs/heads/a\nb",
        ] {
            assert!(RefUpdate::new(name, None, None).is_err(), "{name:?}");
        }
        assert!(branch_ref("refs/heads/-x").is_err());
    }
}

/// The sweep of temporary entries after a crash between the two renames (DS-TS-TMC-003,
/// Enmienda T; L-02 of XP-12).
mod temp_sweep {
    use super::*;
    use gitraptor_core::timemachine::sweep::{KeptReason, SweepReport, sweep_temps};

    fn temps(root: &Path) -> Vec<String> {
        let mut found: Vec<String> = std::fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(".gitraptor-tm-"))
            .collect();
        found.sort();
        found
    }

    fn sweep(t: &Apply) -> SweepReport {
        sweep_temps(&t.env.oplog.lock().unwrap(), &t.env.store)
    }

    /// The target lacks `extra.txt`; the application dies right after moving it aside.
    fn crashed() -> Apply {
        let t = Apply::busy();
        let target = t.snapshot();
        t.f().write("extra.txt", "agent work\n");
        let (op, result) = t.apply(
            &target,
            ApplyHooks {
                simulate_crash_between_moves: true,
                ..Default::default()
            },
        );
        assert!(
            matches!(result, Err(ApplyError::Interrupted { step: 6, .. })),
            "{result:?}"
        );
        assert_eq!(t.state_of(&op), OperationState::Interrupted);
        assert!(!t.f().repo.join("extra.txt").exists());
        assert_eq!(temps(&t.f().repo).len(), 1);
        t
    }

    #[test]
    fn the_displaced_file_goes_back_with_its_exact_content() {
        let t = crashed();
        let report = sweep(&t);
        assert_eq!(report.restored.len(), 1, "{report:?}");
        assert_eq!(report.restored[0].path, "extra.txt");
        assert!(report.kept.is_empty(), "{report:?}");
        assert_eq!(
            std::fs::read(t.f().repo.join("extra.txt")).unwrap(),
            b"agent work\n"
        );
        assert!(temps(&t.f().repo).is_empty());
        // A second start finds nothing to do.
        let again = sweep(&t);
        assert!(
            again.restored.is_empty() && again.kept.is_empty(),
            "{again:?}"
        );
    }

    #[test]
    fn a_path_taken_meanwhile_is_left_alone_and_reported() {
        let t = crashed();
        t.f().write("extra.txt", "someone else\n");
        let before = temps(&t.f().repo);
        let report = sweep(&t);
        assert!(report.restored.is_empty(), "{report:?}");
        assert_eq!(report.kept.len(), 1, "{report:?}");
        let kept = &report.kept[0];
        assert_eq!(kept.reason, KeptReason::PathOccupied);
        assert_eq!(kept.path.as_deref(), Some("extra.txt"));
        assert!(kept.in_store);
        assert_eq!(
            std::fs::read(t.f().repo.join("extra.txt")).unwrap(),
            b"someone else\n"
        );
        assert_eq!(temps(&t.f().repo), before);
        assert_eq!(
            std::fs::read(t.f().repo.join(&before[0])).unwrap(),
            b"agent work\n"
        );
    }

    #[test]
    fn a_foreign_temporary_file_is_never_deleted() {
        let t = crashed();
        std::fs::write(t.f().repo.join(".gitraptor-tm-42"), b"not ours").unwrap();
        // A user's file with the prefix but not the applier's form is not one of ours.
        std::fs::write(t.f().repo.join(".gitraptor-tm-notes"), b"notes").unwrap();
        let report = sweep(&t);
        assert_eq!(report.restored.len(), 1, "{report:?}");
        assert_eq!(report.kept.len(), 1, "{report:?}");
        let kept = &report.kept[0];
        assert_eq!(kept.temp, ".gitraptor-tm-42");
        assert_eq!(kept.reason, KeptReason::Unknown);
        assert!(!kept.in_store);
        assert_eq!(
            std::fs::read(t.f().repo.join(".gitraptor-tm-42")).unwrap(),
            b"not ours"
        );
        assert!(t.f().repo.join(".gitraptor-tm-notes").exists());
    }

    #[test]
    fn without_an_interrupted_write_nothing_is_swept() {
        let t = Apply::busy();
        std::fs::write(t.f().repo.join(".gitraptor-tm-7"), b"left by someone").unwrap();
        assert_eq!(sweep(&t), SweepReport::default());
        assert!(t.f().repo.join(".gitraptor-tm-7").exists());
    }
}
