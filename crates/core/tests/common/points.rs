//! The Time Machine points of a repo as a test reads them while the daemon runs: from the
//! snapshot store and, read-only, from the oplog (never this repo nor the real profile, NFR-01).

use std::collections::BTreeSet;
use std::time::Duration;

use gitraptor_core::profile::ProfileDirs;
use gitraptor_core::timemachine::oplog;
use gitraptor_core::timemachine::store::{SnapshotStore, snapshot_refs};
use gitraptor_git::tm_write::store::TreeEntryKind;

/// Every snapshot of worktree `key` holding `path` with `content`.
pub fn snapshots_with(
    dirs: &ProfileDirs,
    repo_id: &str,
    key: &str,
    path: &str,
    content: &[u8],
) -> BTreeSet<String> {
    let Some(store) = SnapshotStore::open_existing(dirs, repo_id).ok().flatten() else {
        return BTreeSet::new();
    };
    snapshot_refs(&store)
        .unwrap_or_default()
        .into_keys()
        .filter(|id| {
            store
                .files(id, key)
                .unwrap_or_default()
                .into_iter()
                .filter(|(_, k, _)| *k != TreeEntryKind::Gitlink)
                .any(|(p, _, oid)| p == path && store.read_blob(oid).unwrap() == content)
        })
        .collect()
}

/// Of `ids`, the point an undo recorded before `prior` returns to: the newest by engine mark,
/// then by oplog order, as the undo picks it (`prior_point::before_event`). `None` if none of
/// them was recorded before `prior`. The oplog is read read-only while the daemon runs.
pub fn newest_before(
    dirs: &ProfileDirs,
    repo_id: &str,
    ids: &BTreeSet<String>,
    prior: &str,
) -> Option<String> {
    let path = oplog::repo_dir(dirs, repo_id)
        .unwrap()
        .join(oplog::OPLOG_FILE);
    let conn =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    conn.busy_timeout(Duration::from_secs(5)).unwrap();
    let prior_seq: i64 = conn
        .query_row(
            "SELECT seq FROM snapshots WHERE snapshot_id = ?1",
            [prior],
            |r| r.get(0),
        )
        .unwrap();
    let mut stmt = conn
        .prepare("SELECT snapshot_id, engine_mark, seq FROM snapshots WHERE seq < ?1")
        .unwrap();
    stmt.query_map([prior_seq], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<i64>>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })
    .unwrap()
    .map(Result::unwrap)
    .filter(|(id, _, _)| ids.contains(id))
    .max_by_key(|(_, mark, seq)| (*mark, *seq))
    .map(|(id, _, _)| id)
}
