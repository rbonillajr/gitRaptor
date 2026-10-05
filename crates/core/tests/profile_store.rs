//! Per-repo store: batches, append-only, sequence, corruption isolation,
//! newer schema, privacy and instance id (ADR-GRP-006 § 4, ADR-GRP-013).

mod common;

use std::fs;

use common::*;
use gitraptor_core::profile::{
    Author, EndCause, GapCause, KnownState, Profile, ProfileError, RecordKind, StoreOpen, WriteOp,
};

#[test]
fn batch_round_trips_every_entity() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "r", true);
    let mut profile = tp.open();
    let (entry, _) = profile.add_repo(&common_dir(&repo), None, 1).unwrap();
    let (mut store, status) = profile.open_store(&entry.repo_id).unwrap();
    assert_eq!(status, StoreOpen::Created);

    let mut ops = sample_batch(&repo, "s1", 2);
    ops.extend([
        WriteOp::AppendAttribution {
            session_id: "s1".into(),
            kind: RecordKind::Correct,
            agent: agent(),
            author: Author::Developer,
            recorded_ms: 5,
        },
        WriteOp::OpenGap {
            gap_id: "g1".into(),
            started_ms: 6,
            cause: GapCause::DaemonStopped,
            requested_by: Some("raptor stop".into()),
        },
        WriteOp::CloseGap {
            gap_id: "g1".into(),
            ended_ms: 7,
        },
        WriteOp::SetLastKnownState {
            worktree: repo.clone(),
            state: KnownState {
                head: Some("abc".into()),
                refs: "{}".into(),
                operation: None,
                dirty_fingerprint: Some("f".into()),
                updated_ms: 8,
            },
        },
        WriteOp::EndSession {
            session_id: "s1".into(),
            ended_ms: Some(9),
            cause: EndCause::ProcessGone,
        },
        WriteOp::SetObservedUntil { ms: 10 },
    ]);
    let result = store.write_batch(&ops).unwrap();
    assert_eq!(result.seqs, vec![1, 2, 3], "two events and one record");

    let session = store.session("s1").unwrap().unwrap();
    assert_eq!(session.end_cause, Some(EndCause::ProcessGone));
    assert_eq!(store.sessions_for_worktree(&repo).unwrap().len(), 1);
    assert_eq!(store.events_for_worktree(&repo).unwrap().len(), 2);
    assert_eq!(store.events_in_range(2, 3).unwrap().len(), 1);
    assert_eq!(store.attribution_records("s1").unwrap()[0].effective_seq, 3);
    assert_eq!(store.gaps().unwrap()[0].ended_ms, Some(7));
    assert_eq!(
        store
            .last_known_state(&repo)
            .unwrap()
            .unwrap()
            .head
            .as_deref(),
        Some("abc")
    );
    assert_eq!(store.observed_until().unwrap(), Some(10));
    assert_eq!(store.worktrees().unwrap().len(), 1);

    // An ended session is never reopened.
    let err = store
        .write_batch(&[WriteOp::EndSession {
            session_id: "s1".into(),
            ended_ms: Some(11),
            cause: EndCause::ProcessGone,
        }])
        .unwrap_err();
    assert!(matches!(err, ProfileError::InvalidWrite(_)));
}

#[test]
fn batch_is_atomic() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "r", true);
    let mut profile = tp.open();
    let (entry, _) = profile.add_repo(&common_dir(&repo), None, 1).unwrap();
    let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();

    let mut ops = sample_batch(&repo, "s1", 3);
    ops.push(event(&repos.path().join("unknown-worktree"), None, "{}"));
    assert!(store.write_batch(&ops).is_err());
    assert!(
        store.worktrees().unwrap().is_empty(),
        "nothing of the failed batch persists"
    );
    assert_eq!(store.next_seq(), 1, "no sequence consumed");

    let ok = store.write_batch(&sample_batch(&repo, "s1", 1)).unwrap();
    assert_eq!(ok.seqs, vec![1]);
}

#[test]
fn events_and_records_are_append_only() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "r", true);
    let mut profile = tp.open();
    let (entry, _) = profile.add_repo(&common_dir(&repo), None, 1).unwrap();
    let path = profile.store_path(&entry.repo_id);
    {
        let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
        let mut ops = sample_batch(&repo, "s1", 1);
        ops.push(WriteOp::AppendAttribution {
            session_id: "s1".into(),
            kind: RecordKind::Confirm,
            agent: agent(),
            author: Author::Agent,
            recorded_ms: 3,
        });
        store.write_batch(&ops).unwrap();
    }
    let conn = rusqlite::Connection::open(&path).unwrap();
    for sql in [
        "UPDATE events SET kind = 'x'",
        "DELETE FROM events",
        "UPDATE attribution_records SET author = 'developer'",
        "DELETE FROM attribution_records",
    ] {
        let err = conn.execute(sql, []).unwrap_err();
        assert!(err.to_string().contains("append-only"), "{sql}: {err}");
    }
}

#[test]
fn seq_is_monotonic_across_reopen() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "r", true);
    let mut profile = tp.open();
    let (entry, _) = profile.add_repo(&common_dir(&repo), None, 1).unwrap();
    {
        let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
        assert_eq!(
            store
                .write_batch(&sample_batch(&repo, "s1", 2))
                .unwrap()
                .seqs,
            vec![1, 2]
        );
    }
    drop(profile);
    let profile = tp.open();
    let (mut store, status) = profile.open_store(&entry.repo_id).unwrap();
    assert_eq!(status, StoreOpen::Existing);
    let seqs = store
        .write_batch(&[event(&repo, Some("s1"), "{}")])
        .unwrap()
        .seqs;
    assert_eq!(seqs, vec![3]);
}

#[test]
fn corrupt_store_is_quarantined_others_untouched() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let bad_repo = init_repo(repos.path(), "bad", true);
    let good_repo = init_repo(repos.path(), "good", true);
    let mut profile = tp.open();
    let (bad, _) = profile.add_repo(&common_dir(&bad_repo), None, 1).unwrap();
    let (good, _) = profile.add_repo(&common_dir(&good_repo), None, 1).unwrap();
    for (entry, repo) in [(&bad, &bad_repo), (&good, &good_repo)] {
        let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
        store.write_batch(&sample_batch(repo, "s1", 2)).unwrap();
    }
    drop(profile);

    let bad_path = tp
        .dirs()
        .repos_dir()
        .join(format!("{}.sqlite", bad.repo_id));
    let good_path = tp
        .dirs()
        .repos_dir()
        .join(format!("{}.sqlite", good.repo_id));
    let garbage = vec![0xA5u8; 8192];
    fs::write(&bad_path, &garbage).unwrap();
    let good_before = fs::read(&good_path).unwrap();

    let profile = tp.open();
    let (store, status) = profile.open_store(&bad.repo_id).unwrap();
    let StoreOpen::Recovered { quarantined } = status else {
        panic!("expected recovery, got {status:?}");
    };
    assert!(quarantined.starts_with(tp.dirs().quarantine_dir()));
    let name = quarantined
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        name.starts_with(&format!("{}.sqlite.corrupt-", bad.repo_id)),
        "{name}"
    );
    assert_eq!(
        fs::read(&quarantined).unwrap(),
        garbage,
        "set aside, not deleted"
    );
    assert!(
        store.worktrees().unwrap().is_empty(),
        "the repo starts empty"
    );

    assert_eq!(
        fs::read(&good_path).unwrap(),
        good_before,
        "other repos unchanged"
    );
    let (good_store, good_status) = profile.open_store(&good.repo_id).unwrap();
    assert_eq!(good_status, StoreOpen::Existing);
    assert_eq!(good_store.events_for_session("s1").unwrap().len(), 2);
}

#[test]
fn corrupt_index_is_quarantined() {
    let tp = TempProfile::new();
    let first_id = tp.open().instance_id().to_owned();
    let index = tp.dirs().data.join("index.sqlite");
    fs::write(&index, b"garbage garbage garbage").unwrap();

    let (profile, report) = Profile::open(tp.dirs()).unwrap();
    assert!(report.quarantined_index.is_some());
    assert!(report.created);
    assert!(profile.repos().unwrap().is_empty());
    assert_ne!(
        profile.instance_id(),
        first_id,
        "a recreated profile has a new id"
    );
}

#[test]
fn newer_schema_is_rejected_and_file_untouched() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "r", true);
    let mut profile = tp.open();
    let (entry, _) = profile.add_repo(&common_dir(&repo), None, 1).unwrap();
    let path = profile.store_path(&entry.repo_id);
    drop(profile.open_store(&entry.repo_id).unwrap());
    {
        // Simulate a store written by a future binary, checkpointed so the
        // main file holds everything.
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
            .unwrap();
    }
    let before = fs::read(&path).unwrap();

    let err = profile
        .open_store(&entry.repo_id)
        .err()
        .expect("must not open");
    match &err {
        ProfileError::SchemaTooNew {
            found, supported, ..
        } => {
            assert_eq!(*found, 99);
            assert_eq!(*supported, 2);
        }
        other => panic!("unexpected {other}"),
    }
    assert!(err.to_string().contains("upgrade"), "diagnostic: {err}");
    assert_eq!(fs::read(&path).unwrap(), before, "file left intact");
    assert!(files_under(&tp.dirs().quarantine_dir()).is_empty());
}

#[test]
fn store_never_contains_file_content() {
    const MARKER: &str = "TOP-SECRET-CONTENT-7f3a";
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "r", true);
    fs::write(repo.join("secret.txt"), format!("{MARKER}\n")).unwrap();
    git(&repo, &["add", "secret.txt"]);
    git(&repo, &["commit", "-q", "-m", "add secret"]);
    let head = git(&repo, &["rev-parse", "HEAD"]);

    let mut profile = tp.open();
    let (entry, _) = profile.add_repo(&common_dir(&repo), None, 1).unwrap();
    let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
    // What the engine records for that commit: metadata only.
    let mut ops = sample_batch(&repo, "s1", 0);
    ops.push(event(
        &repo,
        Some("s1"),
        &format!("{{\"commit\":\"{head}\",\"paths\":[\"secret.txt\"]}}"),
    ));
    store.write_batch(&ops).unwrap();
    drop(store);
    drop(profile);

    for file in files_under(&tp.dirs().data) {
        let bytes = fs::read(&file).unwrap();
        let found = bytes.windows(MARKER.len()).any(|w| w == MARKER.as_bytes());
        assert!(!found, "{} contains file content", file.display());
    }
}

#[test]
fn instance_id_stable_across_reopen_and_new_on_recreate() {
    let tp = TempProfile::new();
    let (profile, report) = Profile::open(tp.dirs()).unwrap();
    assert!(report.created);
    let id = profile.instance_id().to_owned();
    drop(profile);

    let (profile, report) = Profile::open(tp.dirs()).unwrap();
    assert!(!report.created);
    assert_eq!(profile.instance_id(), id);
    drop(profile);

    fs::remove_dir_all(tp.dirs().owned_dirs()[0].as_path()).unwrap();
    assert_ne!(tp.open().instance_id(), id);
}

#[test]
fn damaged_pages_fail_the_integrity_check() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "r", true);
    let mut profile = tp.open();
    let (entry, _) = profile.add_repo(&common_dir(&repo), None, 1).unwrap();
    let path = profile.store_path(&entry.repo_id);
    {
        let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
        store.write_batch(&sample_batch(&repo, "s1", 50)).unwrap();
    }
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))
            .unwrap();
    }
    // Keep the header valid (the file still opens) but smash the b-tree
    // pages after the first one.
    let mut bytes = fs::read(&path).unwrap();
    assert!(bytes.len() > 8192);
    for b in &mut bytes[4096..] {
        *b = 0x5A;
    }
    fs::write(&path, &bytes).unwrap();

    let (_, status) = profile.open_store(&entry.repo_id).unwrap();
    assert!(matches!(status, StoreOpen::Recovered { .. }), "{status:?}");
}

/// US-GRP-002: a store of schema 1 with events linked to a gap migrates to
/// the gap causes of the observer, rows and links intact.
#[test]
fn a_v1_store_with_linked_events_migrates_to_the_observer_gap_causes() {
    let tp = TempProfile::new();
    let repos = tempfile::tempdir().unwrap();
    let repo = init_repo(repos.path(), "r", true);
    let mut profile = tp.open();
    let (entry, _) = profile.add_repo(&common_dir(&repo), None, 1).unwrap();
    let path = profile.store_path(&entry.repo_id);
    {
        let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
        let mut ops = vec![
            WriteOp::UpsertWorktree {
                path: repo.clone(),
                admin_name: None,
                seen_ms: 1,
            },
            WriteOp::OpenGap {
                gap_id: "g1".into(),
                started_ms: 2,
                cause: GapCause::DaemonDown,
                requested_by: None,
            },
        ];
        if let WriteOp::AppendEvent(mut e) = event(&repo, None, "{}") {
            e.gap_id = Some("g1".into());
            e.evidence = None;
            ops.push(WriteOp::AppendEvent(e));
        }
        store.write_batch(&ops).unwrap();
    }
    {
        // Back to schema 1: the gaps table with the old CHECK.
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "PRAGMA foreign_keys = OFF;
             BEGIN;
             CREATE TABLE gaps_v1 (
                 gap_id TEXT PRIMARY KEY, started_ms INTEGER NOT NULL, ended_ms INTEGER,
                 cause TEXT NOT NULL CHECK (cause IN ('machine-off', 'daemon-down',
                     'daemon-down-during-session', 'daemon-stopped', 'repo-retired',
                     'git-unavailable', 'profile-lost', 'store-corrupt')),
                 requested_by TEXT) STRICT;
             INSERT INTO gaps_v1 SELECT * FROM gaps;
             DROP TABLE gaps;
             ALTER TABLE gaps_v1 RENAME TO gaps;
             PRAGMA user_version = 1;
             COMMIT;",
        )
        .unwrap();
    }

    let (mut store, _) = profile.open_store(&entry.repo_id).unwrap();
    let events = store.events_for_worktree(&repo).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].gap_id.as_deref(), Some("g1"));
    assert_eq!(store.gaps().unwrap()[0].cause, GapCause::DaemonDown);
    store
        .write_batch(&[WriteOp::OpenGap {
            gap_id: "g2".into(),
            started_ms: 3,
            cause: GapCause::PeriodicReconciliation,
            requested_by: None,
        }])
        .unwrap();
    assert_eq!(store.gaps().unwrap().len(), 2);
    // Foreign keys are on again after the migration.
    let err = store.write_batch(&[WriteOp::AppendEvent(gitraptor_core::profile::NewEvent {
        worktree: repo.clone(),
        kind: "commit".into(),
        metadata: "{}".into(),
        observed: ts(4),
        session_id: None,
        evidence: None,
        gap_id: Some("missing".into()),
    })]);
    assert!(err.is_err());
}
