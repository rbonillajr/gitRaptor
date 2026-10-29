//! TMC012-STORE-DIFF (T002 of DS-US-TMC-012): `changed_between` over real captures of a
//! temporary repo, never this one (NFR-01). Child module of `timemachine::store`.

use std::collections::BTreeSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::*;
use crate::profile::ProfileDirs;
use crate::timemachine::oplog::{Oplog, SnapshotLevel};
use gitraptor_testkit::Fixture;

const REPO_ID: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";
const KEY: &str = "main";

struct Env {
    f: Fixture,
    store: SnapshotStore,
    oplog: Mutex<Oplog>,
}

fn env() -> Env {
    let f = Fixture::with_commit(&gitraptor_testkit::fixture::git_from_path());
    let dirs = ProfileDirs::under_root(&f.profile);
    let (store, _) = SnapshotStore::open_or_create(&dirs, REPO_ID).unwrap();
    let (oplog, _) = Oplog::open(&dirs, REPO_ID, 1).unwrap();
    Env {
        f,
        store,
        oplog: Mutex::new(oplog),
    }
}

impl Env {
    fn write(&self, rela: &str, content: &str) {
        let p = self.f.repo.join(rela);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    fn capture(&self) -> String {
        let req = CaptureRequest {
            level: SnapshotLevel::Observation,
            echo_settled: None,
            common_dir: crate::observe::locate(&self.f.repo).unwrap(),
            worktrees: vec![WorktreeScope {
                key: KEY.into(),
                path: self.f.repo.clone(),
                hint: None,
            }],
            engine_mark: None,
            cause_operation: None,
            cause_event_seq: None,
            include_credentials: false,
            still_valid: None,
            give_way: None,
        };
        self.store.capture(&self.oplog, &req).unwrap().snapshot_id
    }

    fn changes(&self, from: &str, to: &str) -> Result<PointChanges, CaptureError> {
        self.store
            .changed_between(from, to, KEY, Instant::now() + Duration::from_secs(30))
    }
}

fn set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn diff_trees_skips_equal_subtrees() {
    let e = env();
    for i in 0..5 {
        e.write(&format!("big/dir{i}/file.rs"), "same\n");
    }
    e.write("deep/inner/leaf.rs", "one\n");
    let a = e.capture();
    e.write("deep/inner/leaf.rs", "two\n");
    let b = e.capture();

    let changes = e.changes(&a, &b).expect("two real points");

    assert_eq!(
        changes.paths,
        set(&["deep/inner/leaf.rs"]),
        "only the changed leaf, never the equal subtrees: changed_between must list just it"
    );
    assert!(changes.refs.is_empty());
}

#[test]
fn identical_points_have_no_changes() {
    let e = env();
    e.write("a.rs", "x\n");
    let a = e.capture();
    let b = e.capture();

    let changes = e.changes(&a, &b).expect("real points");

    assert!(changes.paths.is_empty(), "{:?}", changes.paths);
    assert!(changes.refs.is_empty());
}

#[test]
fn added_and_removed_files_are_changes() {
    let e = env();
    e.write("keep.rs", "k\n");
    e.write("gone.rs", "g\n");
    let a = e.capture();
    std::fs::remove_file(e.f.repo.join("gone.rs")).unwrap();
    e.write("new.rs", "n\n");
    let b = e.capture();

    let changes = e.changes(&a, &b).unwrap();

    assert_eq!(changes.paths, set(&["gone.rs", "new.rs"]));
}

#[test]
fn an_index_only_change_is_a_change() {
    let e = env();
    e.write("api.rs", "fn login() {}\n");
    e.f.git(&["add", "api.rs"]);
    e.f.git(&["commit", "-q", "-m", "api"]);
    let a = e.capture();
    // Stage another content, then put the worktree file back: only the index differs.
    e.write("api.rs", "fn login() { staged(); }\n");
    e.f.git(&["add", "api.rs"]);
    e.write("api.rs", "fn login() {}\n");
    let b = e.capture();

    let changes = e.changes(&a, &b).unwrap();

    assert_eq!(
        changes.paths,
        set(&["api.rs"]),
        "a path that changed only in the index counts"
    );
}

#[test]
fn a_branch_that_differs_between_points_is_listed() {
    let e = env();
    e.f.git(&["branch", "feat-pagos"]);
    let a = e.capture();
    e.write("x.rs", "x\n");
    e.f.git(&["add", "x.rs"]);
    e.f.git(&["commit", "-q", "-m", "x"]);
    e.f.git(&["branch", "-f", "feat-pagos", "HEAD"]);
    e.f.git(&["branch", "feat-new"]);
    let b = e.capture();

    let changes = e.changes(&a, &b).unwrap();

    assert!(
        changes.refs.contains("feat-pagos") && changes.refs.contains("feat-new"),
        "branches moved or created between the points: {:?}",
        changes.refs
    );
}

#[test]
fn a_missing_object_is_an_error_not_empty() {
    let e = env();
    e.write("a.rs", "x\n");
    let a = e.capture();

    let result = e.changes(&a, "00000000-0000-0000-0000-00000000dead");

    assert!(
        result.is_err(),
        "an unknown point must be an Err, never an empty set: {result:?}"
    );
}

#[test]
fn an_expired_deadline_is_an_error_not_a_partial_set() {
    let e = env();
    e.write("a.rs", "x\n");
    let a = e.capture();
    e.write("a.rs", "y\n");
    let b = e.capture();
    let past = Instant::now()
        .checked_sub(Duration::from_secs(5))
        .unwrap_or_else(Instant::now);

    let result = e.store.changed_between(&a, &b, KEY, past);

    assert!(
        result.is_err(),
        "an expired deadline must be an Err, never a partial or empty set: {result:?}"
    );
}
