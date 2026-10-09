//! #223 I-03 (NFR-01, NFR-02, ADR-MCP-001): the Time Machine never reads a `.git` the repo does
//! not own. A capture (manual, continuous or a guaranteed prior: all of them go through the
//! store) of a worktree whose `.git` was rewritten to point at another repo, of a folder the
//! repo does not register, or of a worktree whose `.git` is a symlink to outside, fails before
//! anything is recorded, and both repos stay intact (INF-GRP-001 fingerprint). A real linked
//! worktree (as Orca creates them) and a submodule's checkout are still captured. Temporary
//! repos only (NFR-01).

mod tm_common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gitraptor_core::observe::{locate, open_registered_worktree, registered_worktrees};
use gitraptor_core::timemachine::manual::{self, ManualAsk, ManualError};
use gitraptor_core::timemachine::oplog::{Channel, Requester, RequesterOrigin, SnapshotLevel};
use gitraptor_core::timemachine::store::{CaptureError, CaptureRequest, WorktreeScope};
use gitraptor_git::ReadError;
use gitraptor_testkit::Fixture;
use gitraptor_testkit::fingerprint::{Scope as FpScope, Snapshot, diff};
use tm_common::{Env, REPO_ID, git};

fn canonical(p: &Path) -> PathBuf {
    gitraptor_core::observe::canonical(p)
}

/// The observed repo (in an [`Env`]) with a linked worktree `wt-a`, and another repo (the one an
/// attacker points at) with a linked worktree `wt-x` and an untracked file only it has.
struct Pair {
    env: Env,
    theirs: Fixture,
    common: PathBuf,
    a: PathBuf,
    x: PathBuf,
}

fn pair() -> Pair {
    let env = Env::new(Fixture::with_commit(&git()));
    env.f.git(&["branch", "a"]);
    let a = canonical(&env.f.add_worktree("a", "a"));
    std::fs::write(a.join("ours.txt"), "ours\n").unwrap();
    let theirs = Fixture::with_commit(&git());
    theirs.git(&["branch", "x"]);
    let x = canonical(&theirs.add_worktree("x", "x"));
    std::fs::write(theirs.repo.join("secret.txt"), "theirs\n").unwrap();
    std::fs::write(x.join("secret.txt"), "theirs\n").unwrap();
    let common = locate(&env.f.repo).unwrap();
    Pair {
        env,
        theirs,
        common,
        a,
        x,
    }
}

impl Pair {
    /// Both repos and their worktrees, `.git` included.
    fn fingerprint(&self) -> Snapshot {
        let scopes = [
            FpScope::new("ours", self.env.f.repo.clone()),
            FpScope::new("ours-a", self.a.clone()),
            FpScope::new("theirs", self.theirs.repo.clone()),
            FpScope::new("theirs-x", self.x.clone()),
        ];
        Snapshot::take(&scopes, &BTreeSet::new())
    }

    fn theirs_admin(&self) -> PathBuf {
        locate(&self.theirs.repo)
            .unwrap()
            .join("worktrees")
            .join("wt-x")
    }

    fn request(&self, path: &Path) -> CaptureRequest {
        CaptureRequest {
            level: SnapshotLevel::Observation,
            common_dir: self.common.clone(),
            worktrees: vec![WorktreeScope {
                key: "wt-a".into(),
                path: path.to_path_buf(),
                hint: None,
            }],
            engine_mark: None,
            cause_operation: None,
            cause_event_seq: None,
            include_credentials: false,
            still_valid: None,
            give_way: None,
        }
    }

    fn ask(&self, path: &Path) -> ManualAsk {
        ManualAsk {
            repo_id: REPO_ID.into(),
            worktree: path.to_path_buf(),
            common_dir: self.common.clone(),
            label: "before".into(),
            requester: Requester::Agent {
                name: "claude".into(),
                origin: RequesterOrigin::Detected,
                session_id: "s1".into(),
            },
            channel: Channel::Mcp,
        }
    }

    /// The store's refs: a capture that fails records none.
    fn store_refs(&self) -> String {
        self.env.store_git(&["for-each-ref"])
    }

    /// Every way a snapshot of `path` is taken is refused as untrusted, records nothing and
    /// leaves both repos as they were.
    fn assert_refused(&self, path: &Path) {
        // `git status` refreshes a stat-dirty index: the "before" is taken after it.
        for root in [&self.env.f.repo, &self.a, &self.theirs.repo, &self.x] {
            let _ = self
                .env
                .f
                .git_command(root, &["status", "--porcelain"])
                .output();
        }
        let before = self.fingerprint();
        let refs = self.store_refs();

        let captured = self.env.store.capture(&self.env.oplog, &self.request(path));
        assert!(
            matches!(captured, Err(CaptureError::Read(ReadError::Untrusted(_)))),
            "{captured:?}"
        );
        let manual = manual::capture_in_store(
            &self.env.store,
            &self.env.oplog,
            &self.ask(path),
            None,
            false,
            None,
            1_728_000_000_000,
            Instant::now() + Duration::from_secs(25),
        );
        assert!(
            matches!(
                manual,
                Err(ManualError::Capture(CaptureError::Read(
                    ReadError::Untrusted(_)
                )))
            ),
            "{manual:?}"
        );
        assert!(open_registered_worktree(&self.common, path).is_err());

        assert_eq!(self.store_refs(), refs, "a snapshot was recorded");
        let changes = diff(&before, &self.fingerprint());
        assert!(changes.is_empty(), "a repo changed: {changes:#?}");
    }
}

#[test]
fn a_registered_linked_worktree_and_the_main_one_are_still_captured() {
    let p = pair();
    let listed = registered_worktrees(&p.common).unwrap();
    assert!(listed.contains(&(canonical(&p.env.f.repo), None)));
    assert!(listed.contains(&(p.a.clone(), Some("wt-a".into()))));

    let out = p.env.store.capture(&p.env.oplog, &p.request(&p.a)).unwrap();
    assert!(
        p.env
            .store
            .files(&out.snapshot_id, "wt-a")
            .unwrap()
            .iter()
            .any(|(path, _, _)| path == "ours.txt")
    );
    p.env.prior();
}

#[test]
fn a_linked_git_file_rewritten_to_point_at_another_repo_is_not_captured() {
    let p = pair();
    std::fs::write(
        p.a.join(".git"),
        format!("gitdir: {}\n", p.theirs_admin().display()),
    )
    .unwrap();
    p.assert_refused(&p.a);
}

#[test]
fn a_folder_the_repo_does_not_register_is_not_captured() {
    let p = pair();
    // Another repo's worktree, captured as if it were one of ours.
    p.assert_refused(&p.x);
    // A folder that claims our registered worktree's admin entry: the entry names `wt-a`'s root,
    // not this one.
    let ghost = p.env.f.root.join("ghost");
    std::fs::create_dir(&ghost).unwrap();
    let admin = p.common.join("worktrees").join("wt-a");
    std::fs::write(ghost.join(".git"), format!("gitdir: {}\n", admin.display())).unwrap();
    p.assert_refused(&canonical(&ghost));
}

#[cfg(unix)]
#[test]
fn a_git_that_is_a_symlink_to_outside_is_not_captured() {
    let p = pair();
    std::fs::remove_file(p.a.join(".git")).unwrap();
    std::os::unix::fs::symlink(p.theirs_admin(), p.a.join(".git")).unwrap();
    p.assert_refused(&p.a);
}

#[test]
fn a_submodules_checkout_is_still_opened() {
    let sub = Fixture::with_commit(&git());
    let f = Fixture::with_commit(&git());
    f.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "-q",
        sub.repo.to_str().unwrap(),
        "sub",
    ]);
    let checkout = canonical(&f.repo.join("sub"));
    let common = locate(&checkout).unwrap();
    assert!(common.ends_with("modules/sub"), "{common:?}");
    let reader = open_registered_worktree(&common, &checkout).unwrap();
    assert_eq!(canonical(&reader.workdir().unwrap()), checkout);
}
