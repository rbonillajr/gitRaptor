//! A kept path the prior holds in conflict keeps its stages 1–3 in the index: the applier
//! never drops another actor's unresolved conflict. Temporary repos of the testkit only; the
//! protected operation is played by hand (intent, guaranteed prior, `ready`).

mod tm_common;

use std::collections::BTreeSet;
use std::io::Write as _;
use std::process::Stdio;

use gitraptor_core::timemachine::apply::{
    Applier, ApplyError, ApplyHooks, ApplyPlan, ApplyReport, PlanWorktree, RefScope,
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

    /// `git ls-files -s` of one path: one line per stage.
    fn stages(&self, rela: &str) -> Vec<String> {
        self.f()
            .git(&["ls-files", "-s", "--", rela])
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// Feeds `input` to `git update-index --index-info`.
    fn index_info(&self, input: &str) {
        let mut child = self
            .f()
            .git_command(&self.f().repo, &["update-index", "--index-info"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "update-index: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn blob(&self, content: &str) -> String {
        let name = format!("blob-{}.tmp", content.len());
        let path = self.f().root.join(name);
        std::fs::write(&path, content).unwrap();
        let id = self.f().git(&["hash-object", "-w", path.to_str().unwrap()]);
        std::fs::remove_file(path).unwrap();
        id.trim().to_owned()
    }

    /// Puts `rela` in conflict in the index (stages 1, 2 and 3, no stage 0), as a merge that
    /// stopped leaves it, with the markers in the file.
    fn conflict(&self, rela: &str) {
        let base = self.blob("base\n");
        let ours = self.blob("ours side\n");
        let theirs = self.blob("theirs side, longer\n");
        self.index_info(&format!(
            "0 {zero}\t{rela}\n100644 {base} 1\t{rela}\n100644 {ours} 2\t{rela}\n100644 {theirs} 3\t{rela}\n",
            zero = "0".repeat(base.len()),
        ));
        self.f().write(
            rela,
            "<<<<<<< ours\nours side\n=======\ntheirs side, longer\n>>>>>>> theirs\n",
        );
    }

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

#[test]
fn a_kept_path_in_conflict_keeps_its_stages() {
    let t = Apply::new();
    let target = t.snapshot();
    // After the target another actor left `a.txt` in conflict.
    t.conflict("a.txt");
    let before = t.stages("a.txt");
    assert_eq!(before.len(), 3, "stages 1, 2 and 3: {before:?}");

    let report = t
        .apply(&target, &["a.txt"])
        .expect("the application proceeds");

    assert!(
        report.kept.iter().any(|(_, p)| p == "a.txt"),
        "a.txt is reported kept: {report:?}"
    );
    assert_eq!(
        t.stages("a.txt"),
        before,
        "a kept path in conflict keeps its stages 1–3, never the target's entry nor none"
    );
    assert_eq!(
        std::fs::read_to_string(t.f().repo.join("a.txt")).unwrap(),
        "<<<<<<< ours\nours side\n=======\ntheirs side, longer\n>>>>>>> theirs\n",
        "and its file is not written"
    );
}
