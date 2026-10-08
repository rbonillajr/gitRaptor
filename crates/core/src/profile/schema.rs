//! Schema migrations, compiled into the binary (ADR-GRP-006 § 4).
//!
//! `MIGRATIONS[n]` takes a file from version `n` to `n + 1`; the version is
//! kept in `PRAGMA user_version`. Migrations are append-only: never edit a
//! published one, add a new entry instead.

/// Migrations of the global index (`data/index.sqlite`).
pub(crate) const INDEX_MIGRATIONS: &[&str] = &[
    r"
CREATE TABLE profile_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

CREATE TABLE repos (
    repo_id          TEXT PRIMARY KEY,
    key_path         TEXT NOT NULL UNIQUE,
    canonical_path   TEXT NOT NULL,
    state            TEXT NOT NULL CHECK (state IN ('observed', 'retired')),
    added_ms         INTEGER NOT NULL,
    retired_ms       INTEGER,
    root_commit_hint TEXT
) STRICT;
",
    r"
-- TS-GRP-004: append-only audit of reserved commands (ADR-GRP-013 § 1, SEC-03).
CREATE TABLE reserved_audit (
    id        INTEGER PRIMARY KEY,
    at_ms     INTEGER NOT NULL,
    operation TEXT NOT NULL,
    repo_id   TEXT,
    outcome   TEXT NOT NULL CHECK (outcome IN ('accepted', 'rejected', 'not-implemented')),
    reason    TEXT,
    client    TEXT NOT NULL,
    chain     TEXT NOT NULL
) STRICT;
CREATE TRIGGER reserved_audit_no_update BEFORE UPDATE ON reserved_audit
    BEGIN SELECT RAISE(ABORT, 'the audit is append-only'); END;
CREATE TRIGGER reserved_audit_no_delete BEFORE DELETE ON reserved_audit
    BEGIN SELECT RAISE(ABORT, 'the audit is append-only'); END;
",
    r"
-- US-MCP-002: the MCP allowlist is a mark of the observed repo (ADR-GRP-006,
-- Enmienda (2026-10-05, MCP)). NULL: not enabled.
ALTER TABLE repos ADD COLUMN mcp_enabled_ms INTEGER;
ALTER TABLE repos ADD COLUMN mcp_enabled_by TEXT;
",
    r"
-- US-GRP-020 and US-GRP-022: discovery roots, the repos found in them waiting
-- for the developer's decision and the dismissals, by path (ADR-GRP-010,
-- Enmienda 2026-10-07, N6). Profile state, never settings: only reserved
-- commands change the roots.
CREATE TABLE discovery_roots (
    path     TEXT PRIMARY KEY,
    broad    INTEGER NOT NULL CHECK (broad IN (0, 1)),
    added_ms INTEGER NOT NULL
) STRICT;
CREATE TABLE discovery_candidates (
    path     TEXT PRIMARY KEY,
    root     TEXT NOT NULL,
    key_path TEXT NOT NULL,
    found_ms INTEGER NOT NULL
) STRICT;
CREATE INDEX discovery_candidates_by_root ON discovery_candidates(root);
CREATE TABLE discovery_dismissed (
    path         TEXT PRIMARY KEY,
    key_path     TEXT NOT NULL,
    dismissed_ms INTEGER NOT NULL
) STRICT;
",
];

/// Migrations of each per-repo store (`data/repos/<repo_id>.sqlite`), with
/// the entities of ADR-GRP-013 § 1.
pub(crate) const STORE_MIGRATIONS: &[&str] = &[
    r"
CREATE TABLE store_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

CREATE TABLE worktrees (
    id             INTEGER PRIMARY KEY,
    canonical_path TEXT NOT NULL UNIQUE,
    admin_name     TEXT,
    first_seen_ms  INTEGER NOT NULL,
    gone_ms        INTEGER
) STRICT;

CREATE TABLE sessions (
    session_id     TEXT PRIMARY KEY,
    worktree_id    INTEGER NOT NULL REFERENCES worktrees(id),
    agent_kind     TEXT NOT NULL CHECK (agent_kind IN ('claude-code', 'other')),
    agent_name     TEXT,
    initial_origin TEXT NOT NULL CHECK (initial_origin IN ('detected', 'registered')),
    detection_key  TEXT,
    started_ms     INTEGER NOT NULL,
    ended_ms       INTEGER,
    end_cause      TEXT CHECK (end_cause IN
                       ('process-gone', 'ended-during-gap', 'registration-withdrawn'))
) STRICT;
CREATE INDEX sessions_by_worktree ON sessions(worktree_id);

CREATE TABLE attribution_records (
    id            INTEGER PRIMARY KEY,
    effective_seq INTEGER NOT NULL UNIQUE,
    session_id    TEXT NOT NULL REFERENCES sessions(session_id),
    kind          TEXT NOT NULL CHECK (kind IN ('register', 'confirm', 'correct',
                      'withdraw-correction', 'withdraw-registration')),
    agent_kind    TEXT NOT NULL CHECK (agent_kind IN ('claude-code', 'other')),
    agent_name    TEXT,
    author        TEXT NOT NULL CHECK (author IN ('developer', 'agent')),
    recorded_ms   INTEGER NOT NULL
) STRICT;
CREATE INDEX attribution_by_session ON attribution_records(session_id);
CREATE TRIGGER attribution_no_update BEFORE UPDATE ON attribution_records
    BEGIN SELECT RAISE(ABORT, 'attribution records are append-only'); END;
CREATE TRIGGER attribution_no_delete BEFORE DELETE ON attribution_records
    BEGIN SELECT RAISE(ABORT, 'attribution records are append-only'); END;

CREATE TABLE gaps (
    gap_id       TEXT PRIMARY KEY,
    started_ms   INTEGER NOT NULL,
    ended_ms     INTEGER,
    cause        TEXT NOT NULL CHECK (cause IN ('machine-off', 'daemon-down',
                     'daemon-down-during-session', 'daemon-stopped', 'repo-retired',
                     'git-unavailable', 'profile-lost', 'store-corrupt')),
    requested_by TEXT
) STRICT;

CREATE TABLE events (
    seq             INTEGER PRIMARY KEY,
    worktree_id     INTEGER NOT NULL REFERENCES worktrees(id),
    kind            TEXT NOT NULL,
    metadata        TEXT NOT NULL,
    observed_utc_ms INTEGER NOT NULL,
    utc_offset_s    INTEGER NOT NULL,
    session_id      TEXT REFERENCES sessions(session_id),
    evidence        TEXT,
    gap_id          TEXT REFERENCES gaps(gap_id)
) STRICT;
CREATE INDEX events_by_session ON events(session_id);
CREATE INDEX events_by_worktree ON events(worktree_id);
CREATE TRIGGER events_no_update BEFORE UPDATE ON events
    BEGIN SELECT RAISE(ABORT, 'events are append-only'); END;
CREATE TRIGGER events_no_delete BEFORE DELETE ON events
    BEGIN SELECT RAISE(ABORT, 'events are append-only'); END;

CREATE TABLE last_known_state (
    worktree_id       INTEGER PRIMARY KEY REFERENCES worktrees(id),
    head              TEXT,
    refs              TEXT NOT NULL,
    operation         TEXT,
    dirty_fingerprint TEXT,
    updated_ms        INTEGER NOT NULL
) STRICT;
",
    r"
-- US-GRP-002: causes of the observer's gaps (ADR-GRP-010 § 6, ADR-GRP-013 § 5).
-- SQLite cannot alter a CHECK: the table is rebuilt with the same rows. The
-- migration runs with foreign keys off and is checked before commit.
CREATE TABLE gaps_v2 (
    gap_id       TEXT PRIMARY KEY,
    started_ms   INTEGER NOT NULL,
    ended_ms     INTEGER,
    cause        TEXT NOT NULL CHECK (cause IN ('machine-off', 'daemon-down',
                     'daemon-down-during-session', 'daemon-stopped', 'repo-retired',
                     'git-unavailable', 'profile-lost', 'store-corrupt',
                     'watcher-overflow', 'stream-recreated', 'periodic-reconciliation')),
    requested_by TEXT
) STRICT;
INSERT INTO gaps_v2 SELECT gap_id, started_ms, ended_ms, cause, requested_by FROM gaps;
DROP TABLE gaps;
ALTER TABLE gaps_v2 RENAME TO gaps;
",
    r"
-- US-GRD-019: declared authorship of the commit an event created (amendment of
-- ADR-GRP-013): author, committer and co-authors as JSON, never the message.
ALTER TABLE events ADD COLUMN authorship TEXT;
",
    r"
-- US-GRD-005: the Guardrails decision log (ADR-GRD-006 § 1, Enmienda 2026-10-07). Not an
-- event table: occurrences aggregate (count, last_ms) and rows expire after 90 days. The
-- repo is the store's; nothing of a commit message, argv or oids is kept.
CREATE TABLE guardrails_decisions (
    id             INTEGER PRIMARY KEY,
    at_ms          INTEGER NOT NULL,
    utc_offset_s   INTEGER NOT NULL,
    last_ms        INTEGER NOT NULL,
    count          INTEGER NOT NULL CHECK (count >= 1),
    worktree       TEXT,
    branch         TEXT,
    actor          TEXT CHECK (actor IN ('claude-code', 'other')),
    operation      TEXT NOT NULL,
    kind           TEXT NOT NULL CHECK (kind IN ('denial', 'notice', 'request', 'exception',
                       'exception-rejected', 'exception-cancelled', 'protection-state')),
    detail         TEXT NOT NULL CHECK (detail IN ('full', 'rate-limited')),
    effect         TEXT NOT NULL,
    applied_effect TEXT NOT NULL,
    reasons        TEXT NOT NULL,
    layer          TEXT NOT NULL CHECK (layer IN ('hooks', 'mcp', 'guardrails', 'cockpit')),
    request_state  TEXT,
    decision_id    TEXT NOT NULL,
    origin         TEXT NOT NULL CHECK (origin IN ('daemon', 'spool-unverified')),
    authorship     TEXT,
    agg_key        TEXT NOT NULL
) STRICT;
CREATE INDEX guardrails_decisions_by_last ON guardrails_decisions(last_ms);
CREATE INDEX guardrails_decisions_by_key ON guardrails_decisions(agg_key, last_ms);
",
];
