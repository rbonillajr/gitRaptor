//! TMC012-APPLY-KEEP (T004 of DS-US-TMC-012): `PlanWorktree.keep`, the paths the applier neither
//! writes nor removes, on temporary repos of the testkit (NFR-01). Modeled on `tm_apply.rs`: the
//! protected operation is played by hand (intent, guaranteed prior, `ready`).

mod tm_common;

use std::collections::BTreeSet;

use gitraptor_core::timemachine::apply::{
    Applier, ApplyError, ApplyHooks, ApplyPlan, ApplyReport, ApplyWarning, PlanWorktree, RefScope,
};
use gitraptor_core::timemachine::oplog::{
    Channel, NewOperation, OperationKind, OperationTransition, Requester, Scope, SnapshotLevel,
    Target,
};
use gitraptor_git::resolve::{self, Resolution, ResolveConfig};
use gitraptor_git::tm_write::WriteContext;
use gitraptor_git::{Invoker, SystemGit};
use gitraptor_testkit::Fixture;
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
    fn new() -> Self {
        let env = Env::new(Fixture::busy(&git()));
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

    fn snapshot(&self) -> String {
        self.env
            .capture(SnapshotLevel::Observation, None)
            .snapshot_id
    }

    fn read(&self, rela: &str) -> Vec<u8> {
        std::fs::read(self.f().repo.join(rela)).unwrap()
    }

    fn index_entry(&self, rela: &str) -> String {
        self.f().git(&["ls-files", "-s", "--", rela])
    }

    /// Applies `target` over the current state with `keep` left alone.
    fn apply(&self, target: &str, keep: &[&str]) -> Result<ApplyReport, ApplyError> {
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
        let plan = ApplyPlan {
            target_snapshot: target.to_owned(),
            prior_snapshot: prior,
            worktrees: vec![PlanWorktree {
                key: "main".into(),
                root: self.f().repo.clone(),
                recreate_id: None,
                keep: keep
                    .iter()
                    .map(|p| (*p).to_owned())
                    .collect::<BTreeSet<_>>(),
            }],
            refs: RefScope::All,
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

fn kept_of(report: &ApplyReport) -> BTreeSet<(String, String)> {
    report.kept.iter().cloned().collect()
}

#[test]
fn a_kept_path_is_neither_written_nor_removed() {
    let t = Apply::new();
    let target = t.snapshot();
    // After the target: one file rewritten (the target would write it back), one created (the
    // target would remove it) and a third that is not kept.
    t.f().write("a.txt", "someone else's edit\n");
    t.f().write("mine.txt", "created by someone else\n");
    t.f().write("b.txt", "changed, not kept\n");

    let report = t
        .apply(&target, &["a.txt", "mine.txt"])
        .expect("the application proceeds");

    assert_eq!(
        t.read("a.txt"),
        b"someone else's edit\n",
        "a kept path is not written"
    );
    assert_eq!(
        t.read("mine.txt"),
        b"created by someone else\n",
        "a kept path is not removed"
    );
    assert_eq!(
        t.read("b.txt"),
        b"beta\n",
        "a path outside keep is restored as always"
    );
    assert_eq!(
        kept_of(&report),
        BTreeSet::from([
            ("main".to_owned(), "a.txt".to_owned()),
            ("main".to_owned(), "mine.txt".to_owned()),
        ]),
        "the kept paths are reported in ApplyReport.kept: {report:?}"
    );
    assert!(
        !report.warnings.iter().any(|w| matches!(
            w,
            ApplyWarning::Excluded { path, .. } if path == "a.txt" || path == "mine.txt"
        )),
        "a kept path is not an ApplyWarning::Excluded: {:?}",
        report.warnings
    );
    assert!(
        report.paths.is_empty(),
        "nothing is left as not restored: {:?}",
        report.paths
    );
}

#[test]
fn a_kept_path_keeps_its_current_index_entry() {
    let t = Apply::new();
    let target = t.snapshot();
    t.f().write("a.txt", "staged by someone else\n");
    t.f().git(&["add", "a.txt"]);
    let before = t.index_entry("a.txt");
    let target_entry = {
        // What the target's index holds for the file, for contrast.
        t.f().git(&["ls-tree", "HEAD", "a.txt"])
    };
    assert!(!before.is_empty() && !target_entry.is_empty());

    t.apply(&target, &["a.txt"])
        .expect("the application proceeds");

    assert_eq!(
        t.index_entry("a.txt"),
        before,
        "the index entry of a kept path is the one it has now, never the target's"
    );
    assert_eq!(t.read("a.txt"), b"staged by someone else\n");
}

#[test]
fn an_empty_keep_changes_nothing() {
    let t = Apply::new();
    let target = t.snapshot();
    t.f().write("a.txt", "edited\n");
    t.f().write("mine.txt", "created\n");

    let report = t.apply(&target, &[]).expect("the application proceeds");

    assert!(
        report.kept.is_empty(),
        "no keep, nothing reported as kept: {:?}",
        report.kept
    );
    assert_eq!(
        t.read("a.txt"),
        b"alpha changed\n",
        "written back as before"
    );
    assert!(!t.f().repo.join("mine.txt").exists(), "removed as before");
}
