//! `repack` and `prune` of the snapshot store through the write profile (ADR-TMC-007 § 4,
//! ADR-TMC-002 § 2): on a temporary store, never the real profile (NFR-01).
#![cfg(unix)]

mod common;

use std::os::unix::fs::DirBuilderExt;

use gitraptor_git::Invoker;
use gitraptor_git::tm_write::WriteContext;
use gitraptor_git::tm_write::store::StoreRepo;

const REPO_ID: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";

#[test]
fn repack_and_prune_run_on_the_store_and_keep_referenced_objects() {
    let tmp = tempfile::tempdir().unwrap();
    let tm = tmp.path().canonicalize().unwrap().join("tm");
    std::fs::DirBuilder::new().mode(0o700).create(&tm).unwrap();
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(tm.join(REPO_ID))
        .unwrap();
    let store = StoreRepo::create(&tm, REPO_ID).unwrap();
    let handle = store.handle();
    let (blob, _) = handle.write_blob(b"kept by a snapshot ref\n").unwrap();
    let mut edit = handle.edit_tree(gitraptor_git::Oid::empty_tree()).unwrap();
    edit.upsert(
        "f".into(),
        gitraptor_git::tm_write::store::TreeEntryKind::Blob,
        blob,
    )
    .unwrap();
    let tree = edit.write().unwrap();
    let commit = handle.commit(tree, &[], "snapshot").unwrap();
    handle
        .create_ref("00000000-0000-0000-0000-000000000001", commit)
        .unwrap();

    let ctx =
        WriteContext::new(common::system_git(), Invoker::default(), &tm.join(REPO_ID)).unwrap();
    store.repack(&ctx).unwrap();
    store.prune(&ctx).unwrap();
    let packs = std::fs::read_dir(store.path().join("objects/pack"))
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|x| x == "pack")
        })
        .count();
    assert!(packs >= 1, "repack wrote no pack");
    let reopened = StoreRepo::open(&tm, REPO_ID).unwrap();
    assert_eq!(
        reopened.handle().read_blob(blob).unwrap(),
        b"kept by a snapshot ref\n"
    );
    // The store's own hooks folder stays empty.
    assert_eq!(
        std::fs::read_dir(tm.join(REPO_ID).join("nohooks"))
            .unwrap()
            .count(),
        0
    );
}
