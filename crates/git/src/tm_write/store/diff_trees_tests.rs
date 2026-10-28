//! TMC012-DIFF-TREES (T002 of DS-US-TMC-012): `StoreHandle::diff_trees` walks both trees at once
//! and skips every subtree with the same id. Temporary store, never the real profile (NFR-01).
//! Child module of `tm_write::store`.

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

/// A tree with one blob per `(path, content)`.
fn tree(handle: &StoreHandle, files: &[(&str, &str)]) -> Oid {
    let mut edit = handle.edit_tree(Oid::empty_tree()).unwrap();
    for (path, content) in files {
        let (blob, _) = handle.write_blob(content.as_bytes()).unwrap();
        edit.upsert((*path).into(), TreeEntryKind::Blob, blob)
            .unwrap();
    }
    edit.write().unwrap()
}

/// Deletes the loose object `id` from the store: a missing object on purpose.
fn remove_object(handle: &StoreHandle, id: Oid) {
    let path = handle.object_path(id);
    assert!(path.exists(), "the object is loose in a fresh store");
    std::fs::remove_file(path).unwrap();
}

fn subtree(handle: &StoreHandle, root: Oid, name: &str) -> Oid {
    let (kind, id) = handle
        .tree_entry(root, name.into())
        .unwrap()
        .expect("the subtree exists");
    assert_eq!(kind, TreeEntryKind::Tree);
    id
}

fn sorted(mut paths: Vec<String>) -> Vec<String> {
    paths.sort();
    paths
}

#[test]
fn diff_trees_lists_only_the_paths_that_differ() {
    let f = fixture();
    let h = f.store.handle();
    let a = tree(
        &h,
        &[("shared/x.rs", "x"), ("diff/y.rs", "one"), ("same.rs", "s")],
    );
    let b = tree(
        &h,
        &[
            ("shared/x.rs", "x"),
            ("diff/y.rs", "two"),
            ("diff/z.rs", "new"),
            ("same.rs", "s"),
        ],
    );

    let paths = sorted(h.diff_trees(a, b).expect("two intact trees"));

    assert_eq!(paths, vec!["diff/y.rs".to_owned(), "diff/z.rs".to_owned()]);
}

#[test]
fn diff_trees_skips_equal_subtrees() {
    let f = fixture();
    let h = f.store.handle();
    let a = tree(&h, &[("shared/deep/x.rs", "x"), ("diff/y.rs", "one")]);
    let b = tree(&h, &[("shared/deep/x.rs", "x"), ("diff/y.rs", "two")]);
    let shared = subtree(&h, a, "shared");
    assert_eq!(shared, subtree(&h, b, "shared"), "same content, same id");
    remove_object(&h, shared);

    let paths = h
        .diff_trees(a, b)
        .expect("an object missing inside an equal subtree is never read: no Err");

    assert_eq!(paths, vec!["diff/y.rs".to_owned()]);
}

#[test]
fn a_missing_object_in_a_differing_subtree_is_an_error() {
    let f = fixture();
    let h = f.store.handle();
    let a = tree(&h, &[("shared/x.rs", "x"), ("diff/y.rs", "one")]);
    let b = tree(&h, &[("shared/x.rs", "x"), ("diff/y.rs", "two")]);
    remove_object(&h, subtree(&h, a, "diff"));

    let result = h.diff_trees(a, b);

    assert!(
        result.is_err(),
        "a missing object where the trees differ must be an Err, never a partial list: {result:?}"
    );
}

#[test]
fn identical_roots_have_no_difference() {
    let f = fixture();
    let h = f.store.handle();
    let a = tree(&h, &[("x.rs", "x")]);

    assert_eq!(h.diff_trees(a, a).expect("same id"), Vec::<String>::new());
}
