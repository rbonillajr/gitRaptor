//! TS-TMC-001: the four guarantees of D-TMC-11 (ADR-TMC-001), the intact repo with the testkit
//! harness (INF-GRP-001), seeding without effect on the user's packs and the hostile cases of
//! SEC-TMC-01, 06 and 09.

mod tm_common;

use std::path::{Path, PathBuf};

use gitraptor_core::timemachine::oplog::SnapshotLevel;
use gitraptor_core::timemachine::store::{SnapshotStore, StoreStatus};
use gitraptor_testkit::{Exceptions, Fixture};
use tm_common::{Env, REPO_ID, git, hint};

fn packs(git_dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(git_dir.join("objects/pack"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    v.sort();
    v
}

/// A busy repo whose history is packed, with a stash and a linked worktree.
fn packed_busy() -> Fixture {
    let f = Fixture::busy(&git());
    f.git(&["stash", "push", "-q", "-m", "wip", "--", "a.txt"]);
    f.write("a.txt", "alpha changed again\n");
    f.git(&["gc", "-q"]);
    f.add_worktree("feature", "feature");
    f
}

mod repo_intact {
    use super::*;

    /// Opening, seeding and capturing (prior and observation, full and incremental) leave the
    /// repo, its worktrees, home and everything outside the profile byte-identical.
    #[test]
    fn capture_leaves_the_repo_intact() {
        let f = packed_busy();
        f.git(&["config", "core.fsmonitor", "true"]);
        f.git(&["config", "core.untrackedCache", "true"]);
        let report = gitraptor_testkit::check(
            "tm-capture",
            &f,
            &Exceptions::engine_profile("profile"),
            || {
                let dirs = gitraptor_core::profile::ProfileDirs::under_root(&f.profile);
                let (store, _) = SnapshotStore::open_or_create(&dirs, REPO_ID).unwrap();
                let (oplog, _) =
                    gitraptor_core::timemachine::oplog::Oplog::open(&dirs, REPO_ID, 1).unwrap();
                let oplog = std::sync::Mutex::new(oplog);
                store.seed(&f.repo).unwrap();
                let wt = f.root.join("wt-feature");
                let mut req = gitraptor_core::timemachine::store::CaptureRequest {
                    level: SnapshotLevel::GuaranteedPrior,
                    repo: f.repo.clone(),
                    worktrees: vec![
                        gitraptor_core::timemachine::store::WorktreeScope {
                            key: "main".into(),
                            path: f.repo.clone(),
                            hint: None,
                        },
                        gitraptor_core::timemachine::store::WorktreeScope {
                            key: "feature".into(),
                            path: wt,
                            hint: None,
                        },
                    ],
                    engine_mark: Some(1),
                    cause_operation: None,
                    cause_event_seq: None,
                };
                store.capture(&oplog, &req).unwrap();
                req.level = SnapshotLevel::Observation;
                req.worktrees.truncate(1);
                req.worktrees[0].hint = Some(hint(1, 2, &["a.txt", "untracked.txt"]));
                store.capture(&oplog, &req).unwrap();
                let id = store
                    .capture(&oplog, &req)
                    .map(|o| o.snapshot_id)
                    .unwrap_or_default();
                let _ = store.verify(&id);
            },
        );
        report.assert_intact();
    }
}

#[test]
fn snapshots_are_never_pushed() {
    let env = Env::new(packed_busy());
    let out = env.prior();
    let untracked_blob = env
        .store
        .files(&out.snapshot_id, "main")
        .unwrap()
        .into_iter()
        .find(|(p, _, _)| p == "untracked.txt")
        .unwrap()
        .2;
    let remote = env.f.root.join("remote.git");
    env.f.git_in(
        &env.f.root,
        &["init", "-q", "--bare", remote.to_str().unwrap()],
    );
    let url = remote.to_str().unwrap();
    env.f.git(&["push", "-q", "--mirror", url]);
    env.f.git(&["push", "-q", "--all", url]);
    env.f.git(&["push", "-q", "-f", url, "refs/*:refs/*"]);
    let refs = env.f.git_in(
        &env.f.root,
        &["--git-dir", url, "for-each-ref", "--format=%(refname)"],
    );
    assert!(!refs.contains("refs/tm/"), "{refs}");
    let has = env
        .f
        .git_command(
            &env.f.root,
            &["--git-dir", url, "cat-file", "-e", &untracked_blob.to_hex()],
        )
        .status()
        .unwrap();
    assert!(
        !has.success(),
        "an object only the store has reached the remote"
    );
    // The repo itself has no ref of the store either.
    assert!(!env.f.git(&["for-each-ref"]).contains("refs/tm/"));
    assert!(
        !env.f
            .git(&["log", "--all", "--oneline"])
            .contains("tm snapshot")
    );
}

#[test]
fn aggressive_maintenance_after_a_destructive_reset_keeps_every_snapshot() {
    let env = Env::new(packed_busy());
    env.f
        .write("lost.txt", "only in a commit that will be lost\n");
    env.f.git(&["add", "lost.txt"]);
    env.f.git(&["commit", "-q", "-m", "about to be lost"]);
    let lost = env.f.git(&["rev-parse", "HEAD"]).trim().to_owned();
    let out = env.prior();
    env.f.git(&["reset", "-q", "--hard", "HEAD~1"]);
    env.f.git(&["stash", "clear"]);
    env.f.git(&["reflog", "expire", "--expire=now", "--all"]);
    env.f.git(&["gc", "-q", "--prune=now", "--aggressive"]);
    let gone = env
        .f
        .git_command(&env.f.repo, &["cat-file", "-e", &lost])
        .status()
        .unwrap();
    assert!(
        !gone.success(),
        "setup: the commit should be gone from the repo"
    );
    // The store still has the snapshot, its content and the lost commit (a parent).
    env.store.verify(&out.snapshot_id).unwrap();
    assert_eq!(
        env.file(&out.snapshot_id, "lost.txt").unwrap(),
        b"only in a commit that will be lost\n"
    );
    env.store_git(&["cat-file", "-e", &lost]);
    env.store_git(&["fsck", "--connectivity-only", "--no-dangling"]);
}

#[test]
fn agent_work_in_the_worktree_never_alters_a_snapshot() {
    let env = Env::new(packed_busy());
    let out = env.prior();
    let commit = env
        .store
        .snapshot_commit(&out.snapshot_id)
        .unwrap()
        .unwrap();
    let files = env.files(&out.snapshot_id, "main");
    env.f.git(&["checkout", "-q", "-b", "agent", "feature"]);
    env.f.git(&["reset", "-q", "--hard", "main"]);
    env.f.git(&["clean", "-qfdx"]);
    env.f.write("agent.txt", "agent\n");
    env.f.git(&["add", "agent.txt"]);
    env.f.git(&["commit", "-q", "-m", "agent"]);
    assert_eq!(
        env.store
            .snapshot_commit(&out.snapshot_id)
            .unwrap()
            .unwrap(),
        commit
    );
    assert_eq!(env.files(&out.snapshot_id, "main"), files);
    env.store.verify(&out.snapshot_id).unwrap();
}

#[test]
fn seeding_and_writing_known_objects_leave_the_user_packs_untouched() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let env = Env::new(packed_busy());
        let git_dir = env.f.repo.join(".git");
        let stat = |p: &Path| {
            let m = p.metadata().unwrap();
            (m.ino(), m.mtime(), m.mtime_nsec(), m.len(), m.mode())
        };
        let before: Vec<_> = packs(&git_dir)
            .iter()
            .map(|p| (p.clone(), stat(p)))
            .collect();
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let report = env.store.seed(&env.f.repo).unwrap();
        assert!(report.cloned + report.copied >= 1, "{report:?}");
        assert!(report.skipped.is_empty(), "{report:?}");
        // Captures write objects that are already in the user's packs (the committed blobs).
        env.store.reset_continuity();
        env.prior();
        env.f.write("b.txt", "beta\n"); // back to the committed content
        env.prior();
        let after: Vec<_> = packs(&git_dir)
            .iter()
            .map(|p| (p.clone(), stat(p)))
            .collect();
        assert_eq!(before, after);
        // The store's packs are its own files: other inode, 0600, own index.
        for p in packs(&env.store_path()) {
            if p.extension().is_some_and(|e| e == "pack") {
                let m = p.metadata().unwrap();
                assert_eq!(m.mode() & 0o777, 0o600);
                assert!(!before.iter().any(|(_, s)| s.0 == m.ino()));
                assert!(p.with_extension("idx").exists());
            }
        }
        env.store_git(&["fsck", "--no-dangling"]);
    }
}

#[test]
#[cfg(target_vendor = "apple")]
fn seeded_packs_lose_extended_attributes_and_acls() {
    let f = packed_busy();
    let git_dir = f.repo.join(".git");
    let pack = packs(&git_dir)
        .into_iter()
        .find(|p| p.extension().is_some_and(|e| e == "pack"))
        .unwrap();
    let run = |args: &[&str]| {
        let st = std::process::Command::new(args[0])
            .args(&args[1..])
            .status()
            .unwrap();
        assert!(st.success(), "{args:?}");
    };
    run(&["chmod", "u+w", pack.to_str().unwrap()]);
    run(&[
        "xattr",
        "-w",
        "com.example.mark",
        "user",
        pack.to_str().unwrap(),
    ]);
    run(&["chmod", "+a", "everyone allow read", pack.to_str().unwrap()]);
    let env = Env::new(f);
    env.store.seed(&env.f.repo).unwrap();
    for p in packs(&env.store_path()) {
        // macOS itself tags new files with `com.apple.provenance`; the user's mark must be gone.
        let attrs = std::process::Command::new("xattr")
            .arg(&p)
            .output()
            .unwrap();
        let attrs = String::from_utf8_lossy(&attrs.stdout);
        assert!(
            !attrs.contains("com.example.mark"),
            "{p:?} kept xattrs: {attrs}"
        );
        let acl = std::process::Command::new("ls")
            .args(["-le", p.to_str().unwrap()])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&acl.stdout);
        assert!(!text.contains(" 0: "), "{p:?} kept an ACL: {text}");
    }
}

#[test]
fn hostile_seeding_does_not_contaminate_the_store() {
    #[cfg(unix)]
    {
        let f = packed_busy();
        let git_dir = f.repo.join(".git");
        let pack_dir = git_dir.join("objects/pack");
        let real_pack = packs(&git_dir)
            .into_iter()
            .find(|p| p.extension().is_some_and(|e| e == "pack"))
            .unwrap();
        // A forged index: the pack's `.idx` is replaced by garbage claiming other objects.
        let idx = real_pack.with_extension("idx");
        let mut forged = std::fs::read(&idx).unwrap();
        let n = forged.len();
        for b in &mut forged[n / 2..n - 40] {
            *b ^= 0x5a;
        }
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&idx, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        std::fs::write(&idx, &forged).unwrap();
        // A pack that is a symlink out of `objects/pack`.
        let outside = f.root.join("outside.pack");
        std::fs::copy(&real_pack, &outside).unwrap();
        std::os::unix::fs::symlink(
            &outside,
            pack_dir.join("pack-1111111111111111111111111111111111111111.pack"),
        )
        .unwrap();
        // A pack whose name does not match its checksum.
        std::fs::copy(
            &real_pack,
            pack_dir.join("pack-2222222222222222222222222222222222222222.pack"),
        )
        .unwrap();
        // Alternates pointing to another repository.
        std::fs::write(
            git_dir.join("objects/info/alternates"),
            format!("{}\n", f.other_repo.join(".git/objects").display()),
        )
        .unwrap();
        let env = Env::new(f);
        let report = env.store.seed(&env.f.repo).unwrap();
        let skipped: Vec<_> = report.skipped.iter().map(|s| s.name.as_str()).collect();
        assert!(skipped.contains(&"pack-1111111111111111111111111111111111111111.pack"));
        assert!(skipped.contains(&"pack-2222222222222222222222222222222222222222.pack"));
        assert_eq!(report.cloned + report.copied, 1, "{report:?}");
        // The store regenerated its own index and never got the forged one or alternates.
        let store = env.store_path();
        let own_idx = store.join("objects/pack").join(idx.file_name().unwrap());
        assert_ne!(std::fs::read(own_idx).unwrap(), forged);
        assert!(!store.join("objects/info/alternates").exists());
        env.store_git(&["verify-pack", real_pack_in(&store).to_str().unwrap()]);
        // The forged index breaks the user's own repo; with it repaired, captures work.
        let mut repaired = forged.clone();
        for b in &mut repaired[n / 2..n - 40] {
            *b ^= 0x5a;
        }
        std::fs::write(&idx, &repaired).unwrap();
        let out = env.prior();
        env.store.verify(&out.snapshot_id).unwrap();
    }
}

fn real_pack_in(store: &Path) -> PathBuf {
    packs(store)
        .into_iter()
        .find(|p| p.extension().is_some_and(|e| e == "idx"))
        .unwrap()
}

#[test]
fn a_corrupt_object_is_caught_before_restoring() {
    let env = Env::busy();
    let out = env.prior();
    let blob = env
        .store
        .files(&out.snapshot_id, "main")
        .unwrap()
        .into_iter()
        .find(|(p, _, _)| p == "untracked.txt")
        .unwrap()
        .2
        .to_hex();
    let path = env
        .store_path()
        .join("objects")
        .join(&blob[..2])
        .join(&blob[2..]);
    // Garbage where the loose object was: it must not be handed out for restoring.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    std::fs::write(&path, b"not a zlib stream").unwrap();
    assert!(env.store.verify(&out.snapshot_id).is_err());
}

#[test]
#[cfg(unix)]
fn store_folders_are_private_and_an_untrusted_store_is_set_aside() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::busy();
    let mode = |p: &Path| p.metadata().unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&env.tm_root()), 0o700);
    assert_eq!(mode(&env.tm_root().join(REPO_ID)), 0o700);
    assert_eq!(mode(&env.store_path()), 0o700);
    assert_eq!(mode(&env.store_path().join("config")), 0o600);
    let out = env.prior();

    // Opened by someone else's mode: not trusted, set aside (never deleted), started again.
    std::fs::set_permissions(env.store_path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        SnapshotStore::open_existing(&env.dirs, REPO_ID)
            .unwrap()
            .is_none()
    );
    let (store, status) = SnapshotStore::open_or_create(&env.dirs, REPO_ID).unwrap();
    let StoreStatus::Replaced { set_aside, .. } = status else {
        panic!("not replaced: {status:?}");
    };
    assert!(
        set_aside
            .join("refs/tm/snap")
            .join(&out.snapshot_id)
            .exists()
    );
    assert!(store.snapshot_commit(&out.snapshot_id).unwrap().is_none());
    drop(store);

    // A store that is a symlink to a folder elsewhere is not trusted either.
    let elsewhere = env.f.root.join("elsewhere.git");
    std::fs::rename(env.store_path(), &elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, env.store_path()).unwrap();
    assert!(
        SnapshotStore::open_existing(&env.dirs, REPO_ID)
            .unwrap()
            .is_none()
    );
    // A repo key that could escape the folder is refused.
    assert!(SnapshotStore::open_or_create(&env.dirs, "../x").is_err());
}

#[test]
#[cfg(target_vendor = "apple")]
fn tm_folder_is_excluded_from_backups() {
    let env = Env::busy();
    assert!(env.tm_root().join("CACHEDIR.TAG").exists());
    let out = std::process::Command::new("tmutil")
        .args(["isexcluded", env.tm_root().to_str().unwrap()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("[Excluded]"), "{text}");
}

#[test]
fn store_configuration_has_no_remotes_hooks_or_gc() {
    let env = Env::busy();
    let get = |k: &str| env.store_git(&["config", "--get", k]).trim().to_owned();
    assert_eq!(get("gc.auto"), "0");
    assert_eq!(get("core.logAllRefUpdates"), "false");
    assert_eq!(get("gitraptor.store"), "1");
    assert!(get("core.hooksPath").ends_with("nohooks"));
    assert!(env.store_git(&["remote"]).trim().is_empty());
    assert!(!env.store_path().join("hooks").exists());
    let out = env.prior();
    // No reflog is kept for snapshot refs.
    assert!(
        !env.store_path()
            .join("logs/refs/tm/snap")
            .join(&out.snapshot_id)
            .exists()
    );
}
