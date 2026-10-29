//! `StoreHandle::diff_trees_until` is bounded: past its deadline, or with more differing paths
//! than its cap, it gives an `Err`, never a partial list. Temporary store, never the real
//! profile. Child module of `tm_write::store`.

use std::time::{Duration, Instant};

use super::*;

const REPO_ID: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";

struct Fixture {
    _tmp: tempfile::TempDir,
    store: StoreRepo,
}

fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let tm = crate::paths::canonicalize(tmp.path()).unwrap().join("tm");
    durable::create_private_dir(&tm).unwrap();
    durable::create_private_dir(&tm.join(REPO_ID)).unwrap();
    let store = StoreRepo::create(&tm, REPO_ID).unwrap();
    Fixture { _tmp: tmp, store }
}

fn tree(handle: &StoreHandle, files: &[(&str, &str)]) -> Oid {
    let mut edit = handle.edit_tree(Oid::empty_tree()).unwrap();
    for (path, content) in files {
        let (blob, _) = handle.write_blob(content.as_bytes()).unwrap();
        edit.upsert((*path).into(), TreeEntryKind::Blob, blob)
            .unwrap();
    }
    edit.write().unwrap()
}

fn two_trees(h: &StoreHandle) -> (Oid, Oid) {
    let a = tree(h, &[("d/x.rs", "1"), ("y.rs", "1"), ("z.rs", "1")]);
    let b = tree(h, &[("d/x.rs", "2"), ("y.rs", "2"), ("z.rs", "2")]);
    (a, b)
}

#[test]
fn a_deadline_already_passed_is_an_err() {
    let f = fixture();
    let h = f.store.handle();
    let (a, b) = two_trees(&h);
    let past = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .unwrap_or_else(Instant::now);

    let result = h.diff_trees_until(a, b, Some(past), MAX_DIFF_PATHS);

    assert!(
        result.is_err(),
        "a deadline already passed fails closed, never a partial list: {result:?}"
    );
}

#[test]
fn more_paths_than_the_cap_is_an_err() {
    let f = fixture();
    let h = f.store.handle();
    let (a, b) = two_trees(&h);
    let later = Instant::now() + Duration::from_secs(60);

    let over = h.diff_trees_until(a, b, Some(later), 2);
    assert!(
        over.is_err(),
        "three differing paths over a cap of two fail closed: {over:?}"
    );

    let at_cap = h
        .diff_trees_until(a, b, Some(later), 3)
        .expect("exactly the cap is still a full list");
    assert_eq!(at_cap, vec!["d/x.rs", "y.rs", "z.rs"]);
}

#[test]
fn the_default_cap_is_the_documented_one() {
    assert_eq!(MAX_DIFF_PATHS, 100_000);
}
