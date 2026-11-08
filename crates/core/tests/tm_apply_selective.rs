//! TMC011-APPLY-SELECTIVE (T005 of DS-US-TMC-011): `ApplyPlan.selective`, the applier that writes
//! only the subjects it is given, on temporary repos of the testkit (NFR-01). Modeled on
//! `tm_apply_keep.rs`: the protected operation is played by hand (intent, guaranteed prior,
//! `ready`).
//!
//! These tests call `ApplyPlan::selective`, `SelectiveTarget` and `PathTarget`, which the plan
//! creates in `apply/mod.rs`: until then the file does not compile (missing symbols).

mod tm_common;

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};

use gitraptor_core::timemachine::apply::{
    Applier, ApplyError, ApplyHooks, ApplyPlan, ApplyReport, PathTarget, PlanWorktree, RefScope,
    SelectiveTarget,
};
use gitraptor_core::timemachine::oplog::{
    Channel, NewOperation, OperationKind, OperationTransition, Requester, Scope, SnapshotLevel,
    Target,
};
use gitraptor_git::resolve::{self, Resolution, ResolveConfig};
use gitraptor_git::tm_write::WriteContext;
use gitraptor_git::tm_write::files::Kind;
use gitraptor_git::{Invoker, Oid, SystemGit};
use gitraptor_testkit::Fixture;
use tm_common::{Env, REPO_ID, git};

fn system_git() -> SystemGit {
    match resolve::resolve(&ResolveConfig::for_current_os(None), &Invoker::default()) {
        Resolution::Found { git, .. } => git,
        Resolution::NotFound { diagnostics } => panic!("tests need Git >= 2.38: {diagnostics:?}"),
    }
}

type Shape = (String, Option<SelectiveTarget>, RefScope, BTreeSet<String>);

struct Apply {
    env: Env,
    write: WriteContext,
}

impl Apply {
    fn new() -> Self {
        let env = Env::new(Fixture::with_commit(&git()));
        let write = WriteContext::new(
            system_git(),
            Invoker::default(),
            &env.tm_root().join(REPO_ID),
        )
        .unwrap();
        Self { env, write }
    }

    fn f(&self) -> &Fixture {
        &self.env.f
    }

    fn read(&self, rela: &str) -> Vec<u8> {
        std::fs::read(self.f().repo.join(rela)).unwrap()
    }

    fn index_entry(&self, rela: &str) -> String {
        self.f().git(&["ls-files", "-s", "--", rela])
    }

    fn blob_at_head(&self, rela: &str) -> Oid {
        Oid::from_hex(self.f().git(&["rev-parse", &format!("HEAD:{rela}")]).trim()).unwrap()
    }

    fn commit_id(&self, rev: &str) -> Oid {
        Oid::from_hex(self.f().git(&["rev-parse", rev]).trim()).unwrap()
    }

    fn snapshot(&self) -> String {
        self.env
            .capture(SnapshotLevel::Observation, None)
            .snapshot_id
    }

    fn ref_value(&self, full: &str) -> Option<String> {
        let out = self
            .f()
            .git(&["for-each-ref", "--format=%(objectname)", full]);
        let out = out.trim();
        (!out.is_empty()).then(|| out.to_owned())
    }

    /// The protected operation up to `ready`, then `shape` makes the plan from its prior.
    fn apply(&self, shape: impl FnOnce(&str) -> Shape) -> Result<ApplyReport, ApplyError> {
        let op = self
            .env
            .oplog
            .lock()
            .unwrap()
            .record_operation(
                &NewOperation {
                    kind: OperationKind::Undo,
                    subtype: Some("agent".into()),
                    scope: Scope {
                        worktrees: vec![self.f().repo.display().to_string()],
                        refs: vec![],
                    },
                    requester: Requester::Unattributed,
                    channel: Channel::Cli,
                    confirmed: true,
                    target: Target::Undo(vec![]),
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
        {
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
        }
        let (target_snapshot, selective, refs, keep) = shape(&prior);
        let plan = ApplyPlan {
            target_snapshot,
            prior_snapshot: prior,
            worktrees: vec![PlanWorktree {
                key: "main".into(),
                root: self.f().repo.clone(),
                recreate_id: None,
                keep,
            }],
            refs,
            selective,
        };
        Applier::new(
            &self.env.store,
            &self.write,
            &self.env.oplog,
            self.f().repo.clone(),
            gitraptor_core::observe::locate(&self.f().repo).unwrap(),
            self.f().profile.clone(),
        )
        .with_clock(|| 20)
        .with_hooks(ApplyHooks::default())
        .apply(&op, &plan)
    }
}

fn path_target(file: Option<(Kind, Oid)>, index: Option<(Kind, Oid)>) -> PathTarget {
    PathTarget { file, index }
}

fn selective(paths: &[(&str, PathTarget)], refs: &[(&str, Option<Oid>)]) -> SelectiveTarget {
    SelectiveTarget {
        paths: BTreeMap::from([(
            "main".to_owned(),
            paths
                .iter()
                .map(|(p, t)| ((*p).to_owned(), t.clone()))
                .collect(),
        )]),
        refs: refs.iter().map(|(r, v)| ((*r).to_owned(), *v)).collect(),
        head: BTreeMap::new(),
    }
}

fn only(refs: &[&str]) -> RefScope {
    RefScope::Only(refs.iter().map(|r| (*r).to_owned()).collect())
}

#[test]
fn selective_writes_only_its_paths() {
    let t = Apply::new();
    let alpha = t.blob_at_head("a.txt");
    let committed_index = t.index_entry("a.txt");
    t.f().write("a.txt", "edited a\n");
    t.f().write("b.txt", "edited b\n");
    t.f().write("extra.txt", "untracked\n");
    let head = t.f().git(&["rev-parse", "HEAD"]);

    t.apply(|prior| {
        (
            prior.to_owned(),
            Some(selective(
                &[(
                    "a.txt",
                    path_target(Some((Kind::File, alpha)), Some((Kind::File, alpha))),
                )],
                &[],
            )),
            only(&[]),
            BTreeSet::new(),
        )
    })
    .expect("TMC011-APPLY-SELECTIVE: the selective application proceeds");

    assert_eq!(
        t.read("a.txt"),
        b"alpha\n",
        "TMC011-APPLY-SELECTIVE: the selective path is written with its target"
    );
    assert_eq!(
        t.index_entry("a.txt"),
        committed_index,
        "TMC011-APPLY-SELECTIVE: its index entry is its target's"
    );
    assert_eq!(
        t.read("b.txt"),
        b"edited b\n",
        "TMC011-APPLY-SELECTIVE: a path outside the selective target is not written"
    );
    assert_eq!(
        t.read("extra.txt"),
        b"untracked\n",
        "TMC011-APPLY-SELECTIVE: an untracked path outside it is not removed"
    );
    assert_eq!(
        t.f().git(&["rev-parse", "HEAD"]),
        head,
        "HEAD does not move"
    );
}

#[test]
fn selective_removes_a_path_absent_in_its_target() {
    let t = Apply::new();
    t.f().write("b.txt", "edited b\n");
    t.f().write("a.txt", "edited a\n");

    t.apply(|prior| {
        (
            prior.to_owned(),
            Some(selective(&[("b.txt", path_target(None, None))], &[])),
            only(&[]),
            BTreeSet::new(),
        )
    })
    .expect("TMC011-APPLY-SELECTIVE: the selective application proceeds");

    assert!(
        !t.f().repo.join("b.txt").exists(),
        "TMC011-APPLY-SELECTIVE: None in the target removes the file"
    );
    assert_eq!(
        t.index_entry("b.txt"),
        "",
        "TMC011-APPLY-SELECTIVE: None in the target removes the index entry"
    );
    assert_eq!(
        t.read("a.txt"),
        b"edited a\n",
        "TMC011-APPLY-SELECTIVE: the other path is left as it is"
    );
}

#[test]
fn selective_moves_only_its_refs() {
    let t = Apply::new();
    let first = t.commit_id("HEAD");
    t.f().git(&["branch", "feat", "HEAD"]);
    t.f().git(&["branch", "other", "HEAD"]);
    t.f().git(&["branch", "doomed", "HEAD"]);
    // `feat` moves on, and `other` follows it.
    t.f().git(&["checkout", "-q", "feat"]);
    t.f().write("n.txt", "n\n");
    t.f().git(&["add", "n.txt"]);
    t.f().git(&["commit", "-q", "-m", "on feat"]);
    t.f().git(&["checkout", "-q", "main"]);
    t.f().git(&["branch", "-f", "other", "feat"]);
    let main_before = t.ref_value("refs/heads/main");
    let other_before = t.ref_value("refs/heads/other");
    assert_ne!(other_before, Some(first.to_hex()));

    t.apply(|prior| {
        (
            prior.to_owned(),
            Some(selective(
                &[],
                &[
                    ("refs/heads/feat", Some(first)),
                    ("refs/heads/doomed", None),
                ],
            )),
            only(&["refs/heads/feat", "refs/heads/doomed"]),
            BTreeSet::new(),
        )
    })
    .expect("TMC011-APPLY-SELECTIVE: the selective application proceeds");

    assert_eq!(
        t.ref_value("refs/heads/feat"),
        Some(first.to_hex()),
        "TMC011-APPLY-SELECTIVE: a ref of the selective target moves to its value"
    );
    assert_eq!(
        t.ref_value("refs/heads/doomed"),
        None,
        "TMC011-APPLY-SELECTIVE: a ref whose target is None is deleted"
    );
    assert_eq!(
        t.ref_value("refs/heads/other"),
        other_before,
        "TMC011-APPLY-SELECTIVE: a branch outside the selective target does not move"
    );
    assert_eq!(
        t.ref_value("refs/heads/main"),
        main_before,
        "TMC011-APPLY-SELECTIVE: the checked-out branch does not move either"
    );
}

#[test]
fn selective_with_keep_or_excluded_path_changes_nothing() {
    // A programming error in the plan: debug builds assert, release builds refuse. Either way
    // the repo stays as it was.
    let t = Apply::new();
    let alpha = t.blob_at_head("a.txt");
    t.f().write("a.txt", "edited a\n");
    // An untracked credential file: every capture leaves it out and declares it.
    t.f().write("deploy.pem", "fake-key-material\n");
    let before = t.f().fingerprint();

    // selective together with keep
    let with_keep = catch_unwind(AssertUnwindSafe(|| {
        t.apply(|prior| {
            (
                prior.to_owned(),
                Some(selective(
                    &[(
                        "a.txt",
                        path_target(Some((Kind::File, alpha)), Some((Kind::File, alpha))),
                    )],
                    &[],
                )),
                only(&[]),
                BTreeSet::from(["b.txt".to_owned()]),
            )
        })
    }));
    if let Ok(result) = with_keep {
        assert!(
            result.is_err(),
            "TMC011-APPLY-SELECTIVE: selective with keep is refused, not applied"
        );
    }
    let changes = gitraptor_testkit::diff(&before, &t.f().fingerprint());
    assert!(
        changes.is_empty(),
        "TMC011-APPLY-SELECTIVE: selective with keep changed the repo: {changes:#?}"
    );

    // a path the prior excludes
    let excluded = catch_unwind(AssertUnwindSafe(|| {
        t.apply(|prior| {
            (
                prior.to_owned(),
                Some(selective(
                    &[("deploy.pem", path_target(Some((Kind::File, alpha)), None))],
                    &[],
                )),
                only(&[]),
                BTreeSet::new(),
            )
        })
    }));
    if let Ok(result) = excluded {
        assert!(
            result.is_err(),
            "TMC011-APPLY-SELECTIVE: a path the prior excludes is refused, never written"
        );
    }
    let changes = gitraptor_testkit::diff(&before, &t.f().fingerprint());
    assert!(
        changes.is_empty(),
        "TMC011-APPLY-SELECTIVE: an excluded path changed the repo: {changes:#?}"
    );
}

#[test]
fn no_selective_changes_nothing() {
    // `selective: None` is today's application: the whole target is written back.
    let t = Apply::new();
    let target = t.snapshot();
    t.f().write("a.txt", "edited a\n");
    t.f().write("b.txt", "edited b\n");

    let report = t
        .apply(|_| (target, None, RefScope::All, BTreeSet::new()))
        .expect("TMC011-APPLY-SELECTIVE: a plain application proceeds");

    assert_eq!(
        t.read("a.txt"),
        b"alpha\n",
        "TMC011-APPLY-SELECTIVE: without selective every path goes back to the target"
    );
    assert_eq!(t.read("b.txt"), b"beta\n");
    assert!(report.written >= 2, "{report:?}");
}
