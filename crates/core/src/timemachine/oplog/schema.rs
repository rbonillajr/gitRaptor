//! Schema of the Time Machine oplog, compiled into the binary (ADR-TMC-003
//! § 1). Same rules as the profile stores (ADR-GRP-006 § 4): the version is
//! kept in `PRAGMA user_version` and migrations are append-only, but the list
//! is its own because the oplog has its own owner and file.

/// Migrations of each oplog (`data/tm/<repo_id>/oplog.db`).
///
/// Every row of `snapshots`, `operations`, `journal` and `notices` takes a
/// global `seq` and is chained in `chain` by hash (SEC-TMC-09). All five
/// tables are append-only: the triggers reject `UPDATE` and `DELETE`.
pub(crate) const OPLOG_MIGRATIONS: &[&str] = &[
    r"
CREATE TABLE chain (
    seq       INTEGER PRIMARY KEY,
    kind      TEXT NOT NULL CHECK (kind IN ('snapshot', 'operation', 'journal', 'notice')),
    format    INTEGER NOT NULL,
    batch     INTEGER NOT NULL,
    prev_hash BLOB NOT NULL,
    hash      BLOB NOT NULL
) STRICT;

CREATE TABLE snapshots (
    snapshot_id     TEXT PRIMARY KEY,
    seq             INTEGER NOT NULL UNIQUE,
    level           TEXT NOT NULL CHECK (level IN ('guaranteed-prior', 'observation', 'hook-prior')),
    worktrees       TEXT NOT NULL,
    store_ref       TEXT NOT NULL,
    engine_mark     INTEGER,
    cause_operation TEXT,
    cause_event_seq INTEGER,
    recorded_ms     INTEGER NOT NULL
) STRICT;

CREATE TABLE operations (
    operation_id      TEXT PRIMARY KEY,
    seq               INTEGER NOT NULL UNIQUE,
    kind              TEXT NOT NULL CHECK (kind IN ('protected', 'undo', 'redo', 'restore')),
    subtype           TEXT,
    scope             TEXT NOT NULL,
    requester         TEXT NOT NULL,
    requester_session TEXT,
    channel           TEXT NOT NULL CHECK (channel IN ('cli', 'tui', 'mcp', 'hook')),
    confirmed         INTEGER NOT NULL CHECK (confirmed IN (0, 1)),
    target            TEXT NOT NULL,
    warnings          TEXT NOT NULL,
    engine_mark       INTEGER NOT NULL,
    recorded_ms       INTEGER NOT NULL
) STRICT;
CREATE INDEX operations_by_session ON operations(requester_session);

CREATE TABLE journal (
    seq         INTEGER PRIMARY KEY,
    entry       TEXT NOT NULL CHECK (entry IN ('snapshot-state', 'operation-state',
                    'lock-taken', 'lock-released', 'child-started', 'child-ended',
                    'notice-delivered', 'chain-break')),
    subject_id  TEXT,
    state       TEXT,
    step        INTEGER,
    related_id  TEXT,
    path        TEXT,
    inode       INTEGER,
    pid         INTEGER,
    detail      TEXT,
    recorded_ms INTEGER NOT NULL
) STRICT;
CREATE INDEX journal_by_subject ON journal(subject_id, seq);

CREATE TABLE notices (
    notice_id    TEXT PRIMARY KEY,
    seq          INTEGER NOT NULL UNIQUE,
    kind         TEXT NOT NULL CHECK (kind IN ('interruption', 'purge')),
    worktree     TEXT,
    operation_id TEXT,
    detail       TEXT NOT NULL,
    recorded_ms  INTEGER NOT NULL
) STRICT;

CREATE TRIGGER chain_no_update BEFORE UPDATE ON chain
    BEGIN SELECT RAISE(ABORT, 'the oplog is append-only'); END;
CREATE TRIGGER chain_no_delete BEFORE DELETE ON chain
    BEGIN SELECT RAISE(ABORT, 'the oplog is append-only'); END;
CREATE TRIGGER snapshots_no_update BEFORE UPDATE ON snapshots
    BEGIN SELECT RAISE(ABORT, 'the oplog is append-only'); END;
CREATE TRIGGER snapshots_no_delete BEFORE DELETE ON snapshots
    BEGIN SELECT RAISE(ABORT, 'the oplog is append-only'); END;
CREATE TRIGGER operations_no_update BEFORE UPDATE ON operations
    BEGIN SELECT RAISE(ABORT, 'the oplog is append-only'); END;
CREATE TRIGGER operations_no_delete BEFORE DELETE ON operations
    BEGIN SELECT RAISE(ABORT, 'the oplog is append-only'); END;
CREATE TRIGGER journal_no_update BEFORE UPDATE ON journal
    BEGIN SELECT RAISE(ABORT, 'the oplog is append-only'); END;
CREATE TRIGGER journal_no_delete BEFORE DELETE ON journal
    BEGIN SELECT RAISE(ABORT, 'the oplog is append-only'); END;
CREATE TRIGGER notices_no_update BEFORE UPDATE ON notices
    BEGIN SELECT RAISE(ABORT, 'the oplog is append-only'); END;
CREATE TRIGGER notices_no_delete BEFORE DELETE ON notices
    BEGIN SELECT RAISE(ABORT, 'the oplog is append-only'); END;
",
    r"
-- Birth time of an annotated lock, in ns since the epoch: with the inode, the
-- identity of the file (ADR-TMC-003 § 4). Hashed from chain format 2.
ALTER TABLE journal ADD COLUMN birth_ns INTEGER;
",
    r"
-- The `manual` level and the columns of a manual snapshot, at the end of the table: the old
-- columns are copied as they are, so the hash of every row written before verifies with its own
-- format. The append-only triggers are dropped first (the table is rebuilt) and recreated with
-- the text of migration 1.
DROP TRIGGER snapshots_no_update;
DROP TRIGGER snapshots_no_delete;
CREATE TABLE snapshots_v3 (
    snapshot_id       TEXT PRIMARY KEY,
    seq               INTEGER NOT NULL UNIQUE,
    level             TEXT NOT NULL CHECK (level IN ('guaranteed-prior', 'observation',
                          'hook-prior', 'manual')),
    worktrees         TEXT NOT NULL,
    store_ref         TEXT NOT NULL,
    engine_mark       INTEGER,
    cause_operation   TEXT,
    cause_event_seq   INTEGER,
    recorded_ms       INTEGER NOT NULL,
    label             TEXT,
    requester         TEXT,
    requester_session TEXT,
    worktree_key      TEXT,
    channel           TEXT CHECK (channel IS NULL OR channel IN ('cli', 'tui', 'mcp', 'hook')),
    CHECK ((level = 'manual') = (label IS NOT NULL AND requester IS NOT NULL
        AND requester_session IS NOT NULL AND worktree_key IS NOT NULL AND channel IS NOT NULL))
) STRICT;
INSERT INTO snapshots_v3 (snapshot_id, seq, level, worktrees, store_ref, engine_mark,
        cause_operation, cause_event_seq, recorded_ms)
    SELECT snapshot_id, seq, level, worktrees, store_ref, engine_mark,
        cause_operation, cause_event_seq, recorded_ms
    FROM snapshots;
DROP TABLE snapshots;
ALTER TABLE snapshots_v3 RENAME TO snapshots;
CREATE INDEX snapshots_manual ON snapshots(requester_session, recorded_ms)
    WHERE level = 'manual';
CREATE INDEX snapshots_manual_worktree ON snapshots(worktree_key, recorded_ms)
    WHERE level = 'manual';
CREATE INDEX snapshots_manual_time ON snapshots(recorded_ms) WHERE level = 'manual';
CREATE TRIGGER snapshots_no_update BEFORE UPDATE ON snapshots
    BEGIN SELECT RAISE(ABORT, 'the oplog is append-only'); END;
CREATE TRIGGER snapshots_no_delete BEFORE DELETE ON snapshots
    BEGIN SELECT RAISE(ABORT, 'the oplog is append-only'); END;
",
];
