//! TS-TMC-001: capture of a snapshot — round trip of raw content, ignored and excluded paths,
//! continuity, incremental capture, fast path, validity point and priority of the prior.
//!
//! Unix only for now: on Windows the store is not supported yet and `open_or_create` fails
//! with `Unsupported` (Pendiente: etapa de validación multiplataforma).
#![cfg(unix)]

mod tm_common;

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use gitraptor_core::timemachine::oplog::{
    RecoveryOptions, SnapshotFilter, SnapshotLevel, SystemProbe,
};
use gitraptor_core::timemachine::store::{CaptureError, Detection, SnapshotStore};
use gitraptor_testkit::Fixture;
use tm_common::{Env, git, hint, write};

fn level_obs() -> SnapshotLevel {
    SnapshotLevel::Observation
}

#[test]
fn snapshot_holds_raw_working_tree_index_and_refs() {
    let env = Env::busy();
    let out = env.prior();
    assert_eq!(
        out.detection,
        vec![("main".into(), Detection::Full("first-capture"))]
    );
    let id = &out.snapshot_id;
    // Modified, staged and untracked content as on disk; ignored never.
    assert_eq!(env.file(id, "a.txt").unwrap(), b"alpha changed\n");
    assert_eq!(env.file(id, "staged.txt").unwrap(), b"staged\n");
    assert_eq!(env.file(id, "untracked.txt").unwrap(), b"u\n");
    assert!(env.file(id, "target/out.bin").is_none());
    // The index tree holds what is staged: a.txt as committed, staged.txt added.
    let staged: Vec<String> = env
        .store
        .staged(id, "main")
        .unwrap()
        .into_iter()
        .map(|(p, _, _)| p)
        .collect();
    assert!(staged.contains(&"staged.txt".to_owned()));
    assert!(!staged.contains(&"untracked.txt".to_owned()));
    let meta = env.store.meta(id).unwrap();
    assert_eq!(meta.scope, ["main"]);
    assert_eq!(meta.worktrees[0].head_branch.as_deref(), Some("main"));
    assert!(meta.branches.contains_key("main") && meta.branches.contains_key("feature"));
    // Parents are the commits of HEAD and every branch, anchored in the store.
    let commit = env.store.snapshot_commit(id).unwrap().unwrap();
    let parents: Vec<String> = env
        .store
        .commit_parents(commit)
        .unwrap()
        .iter()
        .map(|p| p.to_hex())
        .collect();
    for c in meta.branches.values() {
        assert!(parents.contains(c));
    }
    env.store.verify(id).unwrap();
}

#[test]
#[cfg(unix)]
fn content_round_trips_bit_for_bit() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::with_commit(&git());
    // Conversions that would change bytes between disk and index: autocrlf, eol, ident and a
    // clean filter that leaves a mark if it ever runs.
    let marker = f.root.join("filter-ran");
    f.git(&["config", "core.autocrlf", "true"]);
    f.git(&[
        "config",
        "filter.mark.clean",
        &format!("touch {} && cat", marker.display()),
    ]);
    write(
        &f.repo,
        ".gitattributes",
        b"*.crlf text eol=crlf\n*.id ident\n*.flt filter=mark\n",
    );
    write(&f.repo, "win.crlf", b"one\r\ntwo\r\n");
    write(&f.repo, "auto.txt", b"crlf\r\nline\r\n");
    write(&f.repo, "kw.id", b"$Id$\n");
    write(&f.repo, "x.flt", b"filtered?\n");
    write(&f.repo, "run.sh", b"#!/bin/sh\necho hi\n");
    std::fs::set_permissions(
        f.repo.join("run.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::os::unix::fs::symlink("a.txt", f.repo.join("link")).unwrap();
    write(&f.repo, "ñandú/日本語 ü.txt", "unicode ✓\n".as_bytes());
    write(&f.repo, "bin.dat", &[0, 159, 146, 150, 13, 10, 255]);
    f.git(&["add", "."]);
    f.git(&["commit", "-q", "-m", "conversions"]);
    // `git add` above ran the filter on purpose; from here on nothing may run it.
    std::fs::remove_file(&marker).unwrap();
    // Unchanged since `git add`, so the stat matches the index: reuse is tempting but wrong.
    let env = Env::new(f);
    for detection_round in 0..2 {
        let out = env.prior();
        let id = &out.snapshot_id;
        for (path, bytes) in env.files(id, "main") {
            let disk = env.f.repo.join(&path);
            let expected = if disk.symlink_metadata().unwrap().file_type().is_symlink() {
                use std::os::unix::ffi::OsStrExt;
                std::fs::read_link(&disk)
                    .unwrap()
                    .as_os_str()
                    .as_bytes()
                    .to_vec()
            } else {
                std::fs::read(&disk).unwrap()
            };
            assert_eq!(bytes, expected, "{path} differs (round {detection_round})");
        }
        let kinds: std::collections::HashMap<String, _> = env
            .store
            .files(id, "main")
            .unwrap()
            .into_iter()
            .map(|(p, k, _)| (p, k))
            .collect();
        use gitraptor_git::tm_write::store::TreeEntryKind;
        assert_eq!(kinds["run.sh"], TreeEntryKind::Executable);
        assert_eq!(kinds["link"], TreeEntryKind::Symlink);
        assert_eq!(kinds["ñandú/日本語 ü.txt"], TreeEntryKind::Blob);
    }
    assert!(
        !marker.exists(),
        "a filter of the user ran during a capture"
    );
}

#[test]
fn ignored_credentials_nested_repos_and_submodules_are_left_out_and_declared() {
    let f = Fixture::with_commit(&git());
    write(&f.repo, ".gitignore", b"node_modules/\n");
    write(&f.repo, "node_modules/pkg/index.js", b"x");
    write(&f.repo, ".env", b"TOKEN=secret\n");
    write(&f.repo, "config/id_rsa", b"-----BEGIN-----\n");
    write(&f.repo, "secrets.txt", b"not on the list\n");
    // A nested repository that is not a submodule.
    std::fs::create_dir_all(f.repo.join("vendor/lib")).unwrap();
    f.git_in(&f.repo.join("vendor/lib"), &["init", "-q"]);
    write(&f.repo, "vendor/lib/file.txt", b"nested\n");
    // A submodule: only its gitlink is kept.
    let sub_src = f.root.join("sub-src");
    std::fs::create_dir_all(&sub_src).unwrap();
    f.git_in(&sub_src, &["init", "-q"]);
    write(&sub_src, "s.txt", b"s\n");
    f.git_in(&sub_src, &["add", "."]);
    f.git_in(&sub_src, &["commit", "-q", "-m", "s"]);
    f.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        "-q",
        sub_src.to_str().unwrap(),
        "sub",
    ]);
    let env = Env::new(f);
    let out = env.prior();
    let paths = env.paths(&out.snapshot_id);
    assert!(paths.contains(&"secrets.txt".to_owned()));
    for absent in [
        ".env",
        "config/id_rsa",
        "node_modules/pkg/index.js",
        "vendor/lib/file.txt",
        "sub/s.txt",
    ] {
        assert!(!paths.contains(&absent.to_owned()), "{absent} captured");
    }
    let kinds: Vec<_> = env.store.files(&out.snapshot_id, "main").unwrap();
    assert!(kinds.iter().any(
        |(p, k, _)| p == "sub" && *k == gitraptor_git::tm_write::store::TreeEntryKind::Gitlink
    ));
    let reasons: Vec<(String, String)> = out
        .exclusions
        .iter()
        .map(|e| (e.path.clone(), e.reason.clone()))
        .collect();
    for (path, reason) in [
        ("main:.env", "credential"),
        ("main:config/id_rsa", "credential"),
        ("main:vendor/lib", "nested-repo"),
        ("main:sub", "submodule"),
    ] {
        assert!(
            reasons.contains(&(path.into(), reason.into())),
            "{path} not declared as {reason}: {reasons:?}"
        );
    }
    // Ignored files are not declared one by one.
    assert!(!reasons.iter().any(|(p, _)| p.contains("node_modules")));
    // The same exclusions are in the meta and in the oplog row.
    assert_eq!(
        env.store.meta(&out.snapshot_id).unwrap().exclusions,
        out.exclusions
    );
    let row = env
        .oplog
        .lock()
        .unwrap()
        .snapshot(&out.snapshot_id)
        .unwrap()
        .unwrap();
    assert_eq!(row.complete.unwrap().exclusions, out.exclusions);
}

#[test]
fn observation_leaves_out_large_files_and_is_partial_but_prior_keeps_them() {
    let env = Env::new(Fixture::with_commit(&git()));
    let big = env.f.repo.join("big.bin");
    let file = std::fs::File::create(&big).unwrap();
    file.set_len(51 * 1024 * 1024).unwrap();
    drop(file);
    let obs = env.capture(level_obs(), None);
    assert!(obs.partial);
    assert!(env.file(&obs.snapshot_id, "big.bin").is_none());
    assert!(
        obs.exclusions
            .iter()
            .any(|e| e.path == "main:big.bin" && e.reason == "too-large")
    );
    let prior = env.prior();
    assert!(!prior.partial);
    assert_eq!(
        env.file(&prior.snapshot_id, "big.bin").unwrap().len(),
        51 * 1024 * 1024
    );
}

#[test]
fn incremental_capture_reads_only_the_changed_path_and_reuses_the_rest() {
    let env = Env::busy();
    let first = env.capture_at(level_obs(), 1, None);
    write(&env.f.repo, "b.txt", b"beta changed\n");
    let second = env.capture_at(level_obs(), 2, Some(hint(1, 2, &["b.txt"])));
    assert_eq!(second.detection[0].1, Detection::Engine);
    assert_eq!(second.timings.files_read, 1);
    assert_eq!(
        env.file(&second.snapshot_id, "b.txt").unwrap(),
        b"beta changed\n"
    );
    let before = env.store.files(&first.snapshot_id, "main").unwrap();
    let after = env.store.files(&second.snapshot_id, "main").unwrap();
    for (p, k, id) in &before {
        if p != "b.txt" {
            assert!(after.contains(&(p.clone(), *k, *id)), "{p} not reused");
        }
    }
}

#[test]
fn fast_path_reuses_the_tree_when_nothing_changed() {
    let env = Env::busy();
    let first = env.capture_at(level_obs(), 1, None);
    let second = env.capture_at(SnapshotLevel::GuaranteedPrior, 2, Some(hint(1, 2, &[])));
    assert!(second.fast_path);
    assert_eq!(second.timings.files_read, 0);
    let tree = |id: &str| {
        let c = env.store.snapshot_commit(id).unwrap().unwrap();
        env.store_git(&["rev-parse", &format!("{}^{{tree}}", c.to_hex())])
    };
    assert_eq!(tree(&first.snapshot_id), tree(&second.snapshot_id));
    assert_ne!(first.snapshot_id, second.snapshot_id);
}

#[test]
fn broken_continuity_never_misses_a_changed_file() {
    let env = Env::busy();
    env.capture_at(level_obs(), 1, None);

    // A gap: the hint does not start at our mark, and it does not list `a.txt`.
    write(&env.f.repo, "a.txt", b"changed during the gap\n");
    let out = env.capture_at(level_obs(), 3, Some(hint(2, 3, &[])));
    assert_eq!(out.detection[0].1, Detection::Full("mark-mismatch"));
    assert_eq!(
        env.file(&out.snapshot_id, "a.txt").unwrap(),
        b"changed during the gap\n"
    );

    // The engine says it was not continuous.
    write(&env.f.repo, "c-new.txt", b"new\n");
    let mut h = hint(3, 4, &[]);
    h.continuous = false;
    let out = env.capture_at(level_obs(), 4, Some(h));
    assert_eq!(out.detection[0].1, Detection::Full("not-continuous"));
    assert!(env.file(&out.snapshot_id, "c-new.txt").is_some());

    // The ignore rules change without an event in the worktree: `info/exclude`.
    write(&env.f.repo, "local.log", b"log\n");
    let exclude = env.f.repo.join(".git/info/exclude");
    std::fs::create_dir_all(exclude.parent().unwrap()).unwrap();
    std::fs::write(&exclude, "*.log\n").unwrap();
    let out = env.capture_at(level_obs(), 5, Some(hint(4, 5, &["local.log"])));
    assert_eq!(out.detection[0].1, Detection::Full("ignore-rules-changed"));
    assert!(env.file(&out.snapshot_id, "local.log").is_none());
    std::fs::write(&exclude, "").unwrap();
    let out = env.capture_at(level_obs(), 6, Some(hint(5, 6, &[])));
    assert_eq!(out.detection[0].1, Detection::Full("ignore-rules-changed"));
    assert!(env.file(&out.snapshot_id, "local.log").is_some());

    // A `.gitignore` in the hint breaks continuity too.
    write(&env.f.repo, ".gitignore", b"target/\n*.txt\n");
    let out = env.capture_at(level_obs(), 7, Some(hint(6, 7, &[".gitignore"])));
    assert_eq!(out.detection[0].1, Detection::Full("ignore-rules-changed"));
    // Tracked files stay even if they match an ignore rule, as in Git.
    assert!(env.file(&out.snapshot_id, "a.txt").is_some());
    assert!(env.file(&out.snapshot_id, "untracked.txt").is_none());

    // A restart forgets everything: full again.
    env.store.reset_continuity();
    let out = env.capture_at(level_obs(), 8, Some(hint(7, 8, &[])));
    assert_eq!(out.detection[0].1, Detection::Full("first-capture"));
}

#[test]
fn racy_files_are_read_again_even_with_an_equal_stat() {
    let env = Env::new(Fixture::with_commit(&git()));
    let path = env.f.repo.join("a.txt");
    // Written now: its mtime is at or after the capture start, so its cache entry is racy.
    std::fs::write(&path, "first\n").unwrap();
    let mtime = path.metadata().unwrap().modified().unwrap();
    let first = env.capture_at(level_obs(), 1, None);
    assert_eq!(env.file(&first.snapshot_id, "a.txt").unwrap(), b"first\n");
    // Same size, same mtime: only the content differs.
    std::fs::write(&path, "secnd\n").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
    let second = env.capture_at(level_obs(), 2, None);
    assert_eq!(env.file(&second.snapshot_id, "a.txt").unwrap(), b"secnd\n");
    let third = env.capture_at(level_obs(), 3, Some(hint(2, 3, &["a.txt"])));
    assert_eq!(env.file(&third.snapshot_id, "a.txt").unwrap(), b"secnd\n");
}

#[test]
fn a_racy_index_entry_is_never_trusted() {
    let f = Fixture::new(&git());
    let path = f.repo.join("r.txt");
    std::fs::write(&path, "staged\n").unwrap();
    f.git(&["add", "r.txt"]);
    // Rewritten in the same tick as the index, same size and mtime: Git's racy case.
    let mtime = path.metadata().unwrap().modified().unwrap();
    std::fs::write(&path, "edited\n").unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
    let index = f.repo.join(".git/index");
    std::fs::File::options()
        .write(true)
        .open(&index)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
    let env = Env::new(f);
    let out = env.prior();
    assert_eq!(env.file(&out.snapshot_id, "r.txt").unwrap(), b"edited\n");
}

#[test]
fn stale_untracked_cache_does_not_hide_new_files() {
    let f = Fixture::with_commit(&git());
    f.git(&["config", "core.untrackedCache", "true"]);
    std::fs::create_dir_all(f.repo.join("d")).unwrap();
    write(&f.repo, "d/old.txt", b"old\n");
    f.git(&["update-index", "--untracked-cache"]);
    f.git(&["status", "--porcelain"]);
    let dir_mtime = f.repo.join("d").metadata().unwrap().modified().unwrap();
    write(&f.repo, "d/new.txt", b"new\n");
    // The folder looks unchanged to the untracked cache.
    std::fs::File::open(f.repo.join("d"))
        .unwrap()
        .set_modified(dir_mtime)
        .unwrap();
    let env = Env::new(f);
    let out = env.prior();
    assert_eq!(env.file(&out.snapshot_id, "d/new.txt").unwrap(), b"new\n");
}

#[test]
fn deleted_and_renamed_paths_follow_the_working_tree() {
    let env = Env::busy();
    env.capture_at(level_obs(), 1, None);
    std::fs::remove_file(env.f.repo.join("a.txt")).unwrap();
    std::fs::rename(
        env.f.repo.join("untracked.txt"),
        env.f.repo.join("moved.txt"),
    )
    .unwrap();
    let out = env.capture_at(
        level_obs(),
        2,
        Some(hint(1, 2, &["a.txt", "untracked.txt", "moved.txt"])),
    );
    let paths = env.paths(&out.snapshot_id);
    assert!(!paths.contains(&"a.txt".into()));
    assert!(!paths.contains(&"untracked.txt".into()));
    assert!(paths.contains(&"moved.txt".into()));
    // A full detection agrees with the incremental one.
    env.store.reset_continuity();
    let full = env.prior();
    assert_eq!(env.paths(&full.snapshot_id), paths);
}

#[test]
fn only_snapshots_with_ref_and_complete_row_are_offered() {
    let env = Env::busy();
    let good = env.prior();
    let offered = |env: &Env| -> Vec<String> {
        env.oplog
            .lock()
            .unwrap()
            .offerable_snapshots(&SnapshotFilter::default(), &env.store)
            .unwrap()
            .into_iter()
            .map(|s| s.record.snapshot_id)
            .collect()
    };
    assert_eq!(offered(&env), std::slice::from_ref(&good.snapshot_id));

    // A row without ref (crash before the ref).
    let pending = env
        .oplog
        .lock()
        .unwrap()
        .begin_snapshot(
            &gitraptor_core::timemachine::oplog::NewSnapshot {
                level: SnapshotLevel::Observation,
                worktrees: vec!["main".into()],
                engine_mark: None,
                cause_operation: None,
                cause_event_seq: None,
            },
            2,
        )
        .unwrap();
    // A ref without row (written by someone else into the store).
    let commit = env
        .store
        .snapshot_commit(&good.snapshot_id)
        .unwrap()
        .unwrap();
    let orphan = "00000000-0000-4000-8000-000000000000";
    env.store_git(&[
        "update-ref",
        &format!("refs/tm/snap/{orphan}"),
        &commit.to_hex(),
    ]);
    assert_eq!(offered(&env), std::slice::from_ref(&good.snapshot_id));

    // A row `pending` with its ref (crash after the ref): not offered, and the recovery
    // discards it and deletes its ref; the ref the oplog does not know is kept.
    env.store_git(&[
        "update-ref",
        &format!("refs/tm/snap/{pending}"),
        &commit.to_hex(),
    ]);
    assert_eq!(offered(&env), std::slice::from_ref(&good.snapshot_id));
    let mut store = SnapshotStore::open_existing(&env.dirs, tm_common::REPO_ID)
        .unwrap()
        .unwrap();
    env.oplog
        .lock()
        .unwrap()
        .recover(
            &mut store,
            &RecoveryOptions {
                git_dir: &env.f.repo.join(".git"),
                deadline: std::time::Instant::now() + Duration::from_secs(1),
                poll: Duration::from_millis(10),
                probe: &SystemProbe,
            },
            3,
        )
        .unwrap();
    assert!(env.store.snapshot_commit(&pending).unwrap().is_none());
    assert!(env.store.snapshot_commit(orphan).unwrap().is_some());
    assert_eq!(offered(&env), [good.snapshot_id]);
}

#[test]
fn an_observation_gives_way_to_a_guaranteed_prior() {
    let env = Arc::new(Env::new(Fixture::with_commit(&git())));
    // Enough new content for the observation to still be writing when the prior arrives.
    let chunk = vec![7u8; 4 << 20];
    for i in 0..40 {
        let mut c = chunk.clone();
        c[0] = i;
        write(&env.f.repo, &format!("big/{i}.bin"), &c);
    }
    let obs_env = Arc::clone(&env);
    let observation = std::thread::spawn(move || {
        obs_env
            .store
            .capture(&obs_env.oplog, &obs_env.request(level_obs(), None))
    });
    std::thread::sleep(Duration::from_millis(30));
    let asked = std::time::Instant::now();
    let prior = env
        .store
        .capture(
            &env.oplog,
            &env.request(SnapshotLevel::GuaranteedPrior, None),
        )
        .unwrap();
    let waited = prior.timings.queue;
    let obs = observation.join().unwrap();
    match obs {
        Err(CaptureError::Yielded) => {
            // ~10 ms in release, and the bench (`tm_snapshot`) gates that budget. Unoptimized
            // hashing on a shared CI runner took 270–470 ms to reach the next yield point, so in
            // debug this only checks that the prior is not stuck behind the whole observation.
            let limit = if cfg!(debug_assertions) { 2_000 } else { 20 };
            assert!(
                waited < Duration::from_millis(limit),
                "prior waited {waited:?} for a yielding observation"
            );
        }
        Ok(_) => eprintln!(
            "observation finished before the prior arrived ({:?})",
            asked.elapsed()
        ),
        Err(e) => panic!("observation failed: {e}"),
    }
    // The prior has everything, big files included.
    assert_eq!(
        env.file(&prior.snapshot_id, "big/39.bin").unwrap().len(),
        4 << 20
    );
}

#[test]
fn capture_failure_leaves_no_snapshot_and_no_trace_in_the_repo() {
    // "Disk full" simulated by a store that refuses writes: the capture fails with its reason,
    // nothing is offered and the user's repository is unchanged.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let env = Env::busy();
        let before = env.f.fingerprint();
        let objects = env.store_path().join("objects");
        let set = |mode| {
            for e in std::fs::read_dir(&objects).unwrap() {
                let p = e.unwrap().path();
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
            }
            std::fs::set_permissions(&objects, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        set(0o500);
        let res = env.store.capture(
            &env.oplog,
            &env.request(SnapshotLevel::GuaranteedPrior, None),
        );
        set(0o700);
        assert!(res.is_err());
        let rows = env
            .oplog
            .lock()
            .unwrap()
            .offerable_snapshots(&SnapshotFilter::default(), &env.store)
            .unwrap();
        assert!(rows.is_empty());
        let after = env.f.fingerprint();
        let changes: Vec<_> = gitraptor_testkit::diff(&before, &after)
            .into_iter()
            .filter(|c| c.scope != "profile")
            .collect();
        assert!(changes.is_empty(), "repo changed: {changes:?}");
        // The next capture works.
        env.prior();
    }
}

#[test]
fn every_worktree_of_the_scope_is_captured() {
    let f = Fixture::busy(&git());
    let wt = f.add_worktree("feature", "feature");
    write(&wt, "c.txt", b"gamma in the worktree\n");
    let env = Env::new(f);
    let mut req = env.request(SnapshotLevel::GuaranteedPrior, None);
    req.worktrees
        .push(gitraptor_core::timemachine::store::WorktreeScope {
            key: "feature".into(),
            path: wt.clone(),
            hint: None,
        });
    let out = env.store.capture(&env.oplog, &req).unwrap();
    let wt_files = env.files(&out.snapshot_id, "feature");
    assert!(wt_files.contains(&("c.txt".into(), b"gamma in the worktree\n".to_vec())));
    let meta = env.store.meta(&out.snapshot_id).unwrap();
    assert_eq!(meta.scope, ["main", "feature"]);
    assert_eq!(meta.worktrees[1].head_branch.as_deref(), Some("feature"));
    assert_eq!(meta.registered.len(), 2);
    assert!(
        meta.registered
            .iter()
            .any(|w| w.branch.as_deref() == Some("feature"))
    );
    let _ = SystemTime::now();
}

#[test]
fn bad_requests_are_rejected_before_reading() {
    let env = Env::busy();
    let mut req = env.request(SnapshotLevel::Observation, None);
    req.worktrees[0].key = "../x".into();
    assert!(matches!(
        env.store.capture(&env.oplog, &req),
        Err(CaptureError::InvalidInput(_))
    ));
    let mut req = env.request(SnapshotLevel::Observation, None);
    req.worktrees[0].path = "relative".into();
    assert!(matches!(
        env.store.capture(&env.oplog, &req),
        Err(CaptureError::InvalidInput(_))
    ));
    // A hint with an escaping path breaks continuity; it is never followed.
    env.capture_at(level_obs(), 1, None);
    let out = env.capture_at(level_obs(), 2, Some(hint(1, 2, &["../../etc/passwd"])));
    assert_eq!(out.detection[0].1, Detection::Full("invalid-hint"));
}

#[test]
fn changing_the_credentials_option_forces_a_full_detection() {
    let env = Env::new(Fixture::with_commit(&git()));
    write(&env.f.repo, "deploy.pem", b"fake key material\n");
    let first = env.capture_at(SnapshotLevel::GuaranteedPrior, 1, None);
    assert!(env.file(&first.snapshot_id, "deploy.pem").is_none());
    // Same scope, a continuous hint that does not name the file: only the
    // option changed, and the capture must not trust its cache.
    let mut req = env.request(SnapshotLevel::GuaranteedPrior, Some(hint(1, 2, &[])));
    req.engine_mark = Some(2);
    req.include_credentials = true;
    let second = env.store.capture(&env.oplog, &req).unwrap();
    assert_eq!(
        second.detection,
        vec![("main".into(), Detection::Full("credential-option-changed"))]
    );
    assert_eq!(
        env.file(&second.snapshot_id, "deploy.pem").unwrap(),
        b"fake key material\n"
    );
    assert!(!second.exclusions.iter().any(|e| e.reason == "credential"));
}

/// US-TMC-004 (ADR-TMC-004 § 2, consistency): a capture whose guard says no at the validity point
/// leaves no row and no ref; the next one, with the guard holding, is a normal snapshot.
#[test]
fn a_capture_that_stopped_being_consistent_is_discarded_without_a_row() {
    use gitraptor_core::timemachine::store::ValidityGuard;
    let env = Env::new(Fixture::with_commit(&git()));
    write(&env.f.repo, "api.rs", b"fn api() {}\n");
    let mut req = env.request(level_obs(), None);
    req.still_valid = Some(ValidityGuard(Arc::new(|| false)));
    assert!(matches!(
        env.store.capture(&env.oplog, &req),
        Err(CaptureError::Discarded)
    ));
    let rows = env
        .oplog
        .lock()
        .unwrap()
        .snapshots(&SnapshotFilter::default())
        .unwrap();
    assert!(rows.is_empty(), "{rows:#?}");
    assert!(
        env.store_git(&["for-each-ref", "refs/tm/snap"])
            .trim()
            .is_empty()
    );
    req.still_valid = Some(ValidityGuard(Arc::new(|| true)));
    let out = env.store.capture(&env.oplog, &req).unwrap();
    assert_eq!(
        env.file(&out.snapshot_id, "api.rs").unwrap(),
        b"fn api() {}\n"
    );
}

/// NFR-01 (US-TMC-002 on macOS under load): a capture that read the new content and then stopped
/// at its validity point recorded no root, so the next one must not reuse the previous root as
/// "unchanged". Otherwise the snapshot after a `reset --hard` held the work from before it, and
/// the undo of the reset found nothing to write.
#[test]
fn a_capture_stopped_at_its_validity_point_never_lends_the_previous_root() {
    use gitraptor_core::timemachine::store::ValidityGuard;
    let env = Env::new(Fixture::with_commit(&git()));
    write(
        &env.f.repo,
        "api.rs",
        b"fn api() { trabajo_sin_commitear(); }\n",
    );
    let prior = env.capture_at(SnapshotLevel::GuaranteedPrior, 1, None);
    write(&env.f.repo, "api.rs", b"fn api() {}\n");
    let mut req = env.request(level_obs(), None);
    req.engine_mark = Some(2);
    req.still_valid = Some(ValidityGuard(Arc::new(|| false)));
    assert!(matches!(
        env.store.capture(&env.oplog, &req),
        Err(CaptureError::Discarded)
    ));
    req.still_valid = None;
    let after = env.store.capture(&env.oplog, &req).unwrap();
    assert!(!after.fast_path);
    assert_eq!(
        env.file(&after.snapshot_id, "api.rs").unwrap(),
        b"fn api() {}\n"
    );
    assert_eq!(
        env.file(&prior.snapshot_id, "api.rs").unwrap(),
        b"fn api() { trabajo_sin_commitear(); }\n"
    );
}

/// US-TMC-004 (D13): an observation gives way when asked, with no row, like it gives way to a
/// guaranteed prior; a prior never gives way.
#[test]
fn an_observation_gives_way_when_asked_and_a_prior_never_does() {
    use gitraptor_core::timemachine::store::ValidityGuard;
    let env = Env::new(Fixture::with_commit(&git()));
    write(&env.f.repo, "api.rs", b"fn api() {}\n");
    let mut req = env.request(level_obs(), None);
    req.give_way = Some(ValidityGuard(Arc::new(|| true)));
    assert!(matches!(
        env.store.capture(&env.oplog, &req),
        Err(CaptureError::Yielded)
    ));
    let rows = env
        .oplog
        .lock()
        .unwrap()
        .snapshots(&SnapshotFilter::default())
        .unwrap();
    assert!(rows.is_empty(), "{rows:#?}");
    let mut prior = env.request(SnapshotLevel::GuaranteedPrior, None);
    prior.give_way = Some(ValidityGuard(Arc::new(|| true)));
    let out = env.store.capture(&env.oplog, &prior).unwrap();
    assert_eq!(
        env.file(&out.snapshot_id, "api.rs").unwrap(),
        b"fn api() {}\n"
    );
}
