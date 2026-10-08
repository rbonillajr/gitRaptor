//! Global index of the profile: instance id and observed repos
//! (ADR-GRP-006 § 2 and § 3).

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::error::{ProfileError, Result};
use super::repo_key::NormalizedPath;
use super::sqlite;

/// Whether a repo is currently observed. Retiring keeps all its data (Q25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoState {
    Observed,
    Retired,
}

impl RepoState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Retired => "retired",
        }
    }

    fn parse(text: &str) -> rusqlite::Result<Self> {
        match text {
            "observed" => Ok(Self::Observed),
            "retired" => Ok(Self::Retired),
            _ => Err(rusqlite::Error::InvalidColumnType(
                0,
                "state".into(),
                rusqlite::types::Type::Text,
            )),
        }
    }
}

/// One row of the global index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoEntry {
    /// Opaque repo key (UUID). Names the per-repo store file.
    pub repo_id: String,
    /// Canonical path of the Git common directory, as found on disk.
    pub canonical_path: PathBuf,
    pub state: RepoState,
    pub added_ms: i64,
    pub retired_ms: Option<i64>,
    /// Root commit, kept as a hint only (empty repo: `None`).
    pub root_commit_hint: Option<String>,
}

/// Last recorded run of the daemon, kept in `profile_meta` so a crash can
/// be told apart from an orderly stop on the next start (ADR-GRP-005 § 4,
/// SEC-13).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonRun {
    /// No daemon ever ran on this profile (or the index was recreated).
    Never,
    /// A daemon started at `started_ms` and never recorded an orderly stop:
    /// it crashed or was killed.
    Running { started_ms: i64 },
    /// The last daemon stopped in order.
    Stopped {
        stopped_ms: i64,
        /// Stable cause text written by the daemon.
        cause: String,
        /// Client that asked for the stop, when a command caused it.
        requested_by: Option<String>,
    },
}

/// What adding a repo did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddOutcome {
    /// First time this common directory is added: new key.
    New,
    /// The repo was already observed; nothing changed.
    AlreadyObserved,
    /// A retired repo came back with its previous key and data. The caller
    /// records `[retired_ms, now]` as a gap (US-GRP-006).
    Reactivated { retired_ms: i64 },
}

/// One row of the reserved-command audit (ADR-GRP-013 § 1). `client` and
/// `chain` are JSON documents written by the channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRow {
    pub at_ms: i64,
    pub operation: String,
    pub repo_id: Option<String>,
    /// `accepted`, `rejected` or `not-implemented`.
    pub outcome: String,
    pub reason: Option<String>,
    pub client: String,
    pub chain: String,
}

/// A declared discovery root (US-GRP-020).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryRoot {
    pub path: String,
    pub broad: bool,
    pub added_ms: i64,
}

/// A discovered repo waiting for the developer's decision (US-GRP-020).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryCandidate {
    pub path: String,
    pub root: String,
    pub key_path: String,
    pub found_ms: i64,
}

pub(crate) struct Index {
    conn: Connection,
    instance_id: String,
}

impl Index {
    pub(crate) fn from_conn(conn: Connection) -> Result<Self> {
        let existing: Option<String> = conn
            .query_row(
                "SELECT value FROM profile_meta WHERE key = 'instance_id'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        let instance_id = match existing {
            Some(id) => id,
            None => {
                let id = sqlite::new_uuid(&conn)?;
                conn.execute(
                    "INSERT INTO profile_meta (key, value) VALUES ('instance_id', ?1)",
                    params![id],
                )?;
                id
            }
        };
        Ok(Self { conn, instance_id })
    }

    pub(crate) fn instance_id(&self) -> &str {
        &self.instance_id
    }

    pub(crate) fn daemon_run(&self) -> Result<DaemonRun> {
        let get = |key: &str| -> Result<Option<String>> {
            Ok(self
                .conn
                .query_row(
                    "SELECT value FROM profile_meta WHERE key = ?1",
                    params![key],
                    |row| row.get(0),
                )
                .optional()?)
        };
        let ms = |key: &str| -> Result<i64> {
            get(key)?
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| ProfileError::InvalidWrite(["missing ", key].concat()))
        };
        Ok(match get("daemon_run")?.as_deref() {
            None => DaemonRun::Never,
            Some("running") => DaemonRun::Running {
                started_ms: ms("daemon_started_ms")?,
            },
            Some(_) => DaemonRun::Stopped {
                stopped_ms: ms("daemon_stopped_ms")?,
                cause: get("daemon_stop_cause")?.unwrap_or_default(),
                requested_by: get("daemon_stop_requested_by")?,
            },
        })
    }

    pub(crate) fn set_daemon_running(&mut self, started_ms: i64) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM profile_meta WHERE key IN ('daemon_stopped_ms', 'daemon_stop_cause',
                 'daemon_stop_requested_by')",
            [],
        )?;
        set_meta(&tx, "daemon_run", "running")?;
        set_meta(&tx, "daemon_started_ms", &started_ms.to_string())?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn append_audit(&mut self, row: &AuditRow) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO reserved_audit (at_ms, operation, repo_id, outcome, reason, client, chain)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                row.at_ms,
                row.operation,
                row.repo_id,
                row.outcome,
                row.reason,
                row.client,
                row.chain
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub(crate) fn audit(&self, after_id: i64, limit: u32) -> Result<Vec<(i64, AuditRow)>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, at_ms, operation, repo_id, outcome, reason, client, chain
             FROM reserved_audit WHERE id > ?1 ORDER BY id LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![after_id, limit], |row| {
            Ok((
                row.get(0)?,
                AuditRow {
                    at_ms: row.get(1)?,
                    operation: row.get(2)?,
                    repo_id: row.get(3)?,
                    outcome: row.get(4)?,
                    reason: row.get(5)?,
                    client: row.get(6)?,
                    chain: row.get(7)?,
                },
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn set_daemon_stopped(
        &mut self,
        stopped_ms: i64,
        cause: &str,
        requested_by: Option<&str>,
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        set_meta(&tx, "daemon_run", "stopped")?;
        set_meta(&tx, "daemon_stopped_ms", &stopped_ms.to_string())?;
        set_meta(&tx, "daemon_stop_cause", cause)?;
        match requested_by {
            Some(client) => set_meta(&tx, "daemon_stop_requested_by", client)?,
            None => {
                tx.execute(
                    "DELETE FROM profile_meta WHERE key = 'daemon_stop_requested_by'",
                    [],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn add(
        &mut self,
        path: &NormalizedPath,
        root_commit_hint: Option<&str>,
        now_ms: i64,
    ) -> Result<(RepoEntry, AddOutcome)> {
        let tx = self.conn.transaction()?;
        let outcome = match find_by_key(&tx, &path.key_path)? {
            Some(entry) if entry.state == RepoState::Observed => AddOutcome::AlreadyObserved,
            Some(entry) => {
                tx.execute(
                    "UPDATE repos SET state = 'observed', retired_ms = NULL,
                         mcp_enabled_ms = NULL, mcp_enabled_by = NULL,
                         root_commit_hint = COALESCE(?2, root_commit_hint)
                     WHERE repo_id = ?1",
                    params![entry.repo_id, root_commit_hint],
                )?;
                AddOutcome::Reactivated {
                    retired_ms: entry.retired_ms.unwrap_or(now_ms),
                }
            }
            None => {
                let repo_id = sqlite::new_uuid(&tx)?;
                tx.execute(
                    "INSERT INTO repos (repo_id, key_path, canonical_path, state, added_ms,
                         root_commit_hint)
                     VALUES (?1, ?2, ?3, 'observed', ?4, ?5)",
                    params![
                        repo_id,
                        path.key_path,
                        path_text(&path.canonical_path),
                        now_ms,
                        root_commit_hint
                    ],
                )?;
                AddOutcome::New
            }
        };
        let entry = find_by_key(&tx, &path.key_path)?
            .ok_or_else(|| ProfileError::InvalidWrite("repo vanished while adding".into()))?;
        tx.commit()?;
        Ok((entry, outcome))
    }

    pub(crate) fn retire(&mut self, repo_id: &str, now_ms: i64) -> Result<()> {
        let changed = self.conn.execute(
            // The MCP mark goes in the same statement (US-MCP-002 cascade).
            "UPDATE repos SET state = ?2, retired_ms = ?3, mcp_enabled_ms = NULL,
                 mcp_enabled_by = NULL
             WHERE repo_id = ?1 AND state = 'observed'",
            params![repo_id, RepoState::Retired.as_str(), now_ms],
        )?;
        if changed == 0 && self.get(repo_id)?.is_none() {
            return Err(ProfileError::UnknownRepo(repo_id.to_owned()));
        }
        Ok(())
    }

    /// Puts or takes the MCP mark of an observed repo (US-MCP-002). `None`
    /// when the repo is not observed: the allowlist stays a subset of the
    /// observed repos by construction. `Some(changed)` otherwise.
    pub(crate) fn set_mcp_enabled(
        &mut self,
        repo_id: &str,
        enabled: bool,
        by: &str,
        now_ms: i64,
    ) -> Result<Option<bool>> {
        let changed = if enabled {
            self.conn.execute(
                "UPDATE repos SET mcp_enabled_ms = ?2, mcp_enabled_by = ?3
                 WHERE repo_id = ?1 AND state = 'observed' AND mcp_enabled_ms IS NULL",
                params![repo_id, now_ms, by],
            )?
        } else {
            self.conn.execute(
                "UPDATE repos SET mcp_enabled_ms = NULL, mcp_enabled_by = NULL
                 WHERE repo_id = ?1 AND state = 'observed' AND mcp_enabled_ms IS NOT NULL",
                params![repo_id],
            )?
        };
        if changed > 0 {
            return Ok(Some(true));
        }
        let observed = self
            .get(repo_id)?
            .is_some_and(|e| e.state == RepoState::Observed);
        Ok(observed.then_some(false))
    }

    /// The observed repos with the MCP mark (US-MCP-002).
    pub(crate) fn mcp_enabled(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT repo_id FROM repos
             WHERE state = 'observed' AND mcp_enabled_ms IS NOT NULL ORDER BY repo_id",
        )?;
        let ids = stmt
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(ids)
    }

    pub(crate) fn get(&self, repo_id: &str) -> Result<Option<RepoEntry>> {
        Ok(self
            .conn
            .query_row(
                "SELECT repo_id, canonical_path, state, added_ms, retired_ms, root_commit_hint
                 FROM repos WHERE repo_id = ?1",
                params![repo_id],
                entry_from_row,
            )
            .optional()?)
    }

    pub(crate) fn by_key(&self, key_path: &str) -> Result<Option<RepoEntry>> {
        find_by_key(&self.conn, key_path)
    }

    pub(crate) fn all(&self) -> Result<Vec<RepoEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT repo_id, canonical_path, state, added_ms, retired_ms, root_commit_hint
             FROM repos ORDER BY added_ms, repo_id",
        )?;
        let rows = stmt.query_map([], entry_from_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub(crate) fn discovery_roots(&self) -> Result<Vec<DiscoveryRoot>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path, broad, added_ms FROM discovery_roots ORDER BY path")?;
        let rows = stmt.query_map([], |row| {
            Ok(DiscoveryRoot {
                path: row.get(0)?,
                broad: row.get::<_, i64>(1)? != 0,
                added_ms: row.get(2)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// `false`: already declared, nothing changed.
    pub(crate) fn add_discovery_root(&mut self, path: &str, broad: bool, now_ms: i64) -> Result<bool> {
        let added = self.conn.execute(
            "INSERT INTO discovery_roots (path, broad, added_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT (path) DO NOTHING",
            params![path, i64::from(broad), now_ms],
        )?;
        Ok(added == 1)
    }

    /// Removes a root and its pending candidates; `None` if it was not
    /// declared. Dismissals and observed repos stay.
    pub(crate) fn remove_discovery_root(&mut self, path: &str) -> Result<Option<u32>> {
        let tx = self.conn.transaction()?;
        if tx.execute("DELETE FROM discovery_roots WHERE path = ?1", [path])? == 0 {
            return Ok(None);
        }
        let removed = tx.execute("DELETE FROM discovery_candidates WHERE root = ?1", [path])?;
        tx.commit()?;
        Ok(Some(u32::try_from(removed).unwrap_or(u32::MAX)))
    }

    pub(crate) fn discovery_candidates(&self) -> Result<Vec<DiscoveryCandidate>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, root, key_path, found_ms FROM discovery_candidates ORDER BY path",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(DiscoveryCandidate {
                path: row.get(0)?,
                root: row.get(1)?,
                key_path: row.get(2)?,
                found_ms: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Replaces the candidates of `root` with the repos of a listing,
    /// `(path, key_path)`, leaving out the observed repos (by key), the
    /// dismissed paths and the repos already proposed by another root.
    /// Returns the new candidates. A root removed meanwhile changes nothing.
    pub(crate) fn sync_candidates(
        &mut self,
        root: &str,
        found: &[(String, String)],
        now_ms: i64,
    ) -> Result<Vec<DiscoveryCandidate>> {
        let tx = self.conn.transaction()?;
        let declared: Option<i64> = tx
            .query_row("SELECT 1 FROM discovery_roots WHERE path = ?1", [root], |r| {
                r.get(0)
            })
            .optional()?;
        if declared.is_none() {
            return Ok(Vec::new());
        }
        let current: Vec<(String, String)> = {
            let mut stmt =
                tx.prepare("SELECT path, key_path FROM discovery_candidates WHERE root = ?1")?;
            let rows = stmt.query_map([root], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        let observed = |key: &str| -> rusqlite::Result<bool> {
            tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM repos WHERE key_path = ?1 AND state = 'observed')",
                [key],
                |r| r.get(0),
            )
        };
        for (path, key) in &current {
            if !found.iter().any(|(p, k)| p == path && k == key) || observed(key)? {
                tx.execute("DELETE FROM discovery_candidates WHERE path = ?1", [path])?;
            }
        }
        let mut new = Vec::new();
        for (path, key) in found {
            let skip: bool = observed(key)?
                || tx.query_row(
                    "SELECT EXISTS (SELECT 1 FROM discovery_dismissed WHERE path = ?1)
                         OR EXISTS (SELECT 1 FROM discovery_candidates
                                    WHERE path = ?1 OR key_path = ?2)",
                    params![path, key],
                    |r| r.get(0),
                )?;
            if skip {
                continue;
            }
            tx.execute(
                "INSERT INTO discovery_candidates (path, root, key_path, found_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                params![path, root, key, now_ms],
            )?;
            new.push(DiscoveryCandidate {
                path: path.clone(),
                root: root.to_owned(),
                key_path: key.clone(),
                found_ms: now_ms,
            });
        }
        tx.commit()?;
        Ok(new)
    }

    /// Dismisses a candidate by its path: `false` if it is not one.
    pub(crate) fn dismiss_candidate(&mut self, path: &str, now_ms: i64) -> Result<bool> {
        let tx = self.conn.transaction()?;
        let key: Option<String> = tx
            .query_row(
                "SELECT key_path FROM discovery_candidates WHERE path = ?1",
                [path],
                |r| r.get(0),
            )
            .optional()?;
        let Some(key) = key else {
            return Ok(false);
        };
        tx.execute("DELETE FROM discovery_candidates WHERE path = ?1", [path])?;
        tx.execute(
            "INSERT INTO discovery_dismissed (path, key_path, dismissed_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT (path) DO UPDATE SET key_path = ?2, dismissed_ms = ?3",
            params![path, key, now_ms],
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// A repo was added by hand or accepted: it is no longer a candidate
    /// and no longer dismissed.
    pub(crate) fn forget_discovered_key(&mut self, key_path: &str) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM discovery_candidates WHERE key_path = ?1", [key_path])?;
        tx.execute("DELETE FROM discovery_dismissed WHERE key_path = ?1", [key_path])?;
        tx.commit()?;
        Ok(())
    }

    /// Removes the candidate at `path`; `false` if there was none.
    pub(crate) fn forget_candidate(&mut self, path: &str) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM discovery_candidates WHERE path = ?1", [path])?
            == 1)
    }
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO profile_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn find_by_key(conn: &Connection, key_path: &str) -> Result<Option<RepoEntry>> {
    Ok(conn
        .query_row(
            "SELECT repo_id, canonical_path, state, added_ms, retired_ms, root_commit_hint
             FROM repos WHERE key_path = ?1",
            params![key_path],
            entry_from_row,
        )
        .optional()?)
}

fn entry_from_row(row: &Row<'_>) -> rusqlite::Result<RepoEntry> {
    let canonical: String = row.get(1)?;
    let state: String = row.get(2)?;
    Ok(RepoEntry {
        repo_id: row.get(0)?,
        canonical_path: PathBuf::from(canonical),
        state: RepoState::parse(&state)?,
        added_ms: row.get(3)?,
        retired_ms: row.get(4)?,
        root_commit_hint: row.get(5)?,
    })
}

/// Canonical paths are validated as UTF-8 before reaching the index.
fn path_text(path: &Path) -> &str {
    path.to_str().unwrap_or_default()
}

/// The repos of the index, `(repo_id, canonical path)`, read without the
/// engine and without writing anything (US-GRP-017: the engine is stopped).
///
/// A read-only SQLite connection to a WAL database still creates `-wal` and
/// `-shm` files, so the index is opened `immutable` and only when no `-wal`
/// file exists: then every commit is in the main file and there is nothing
/// to recover. With a `-wal` (the engine runs or crashed), an index of an
/// unknown schema or any error, the answer is `None` and the caller shows
/// repo ids instead. Never created, migrated nor recovered.
pub fn read_only_repos(index: &Path) -> Option<Vec<(String, PathBuf)>> {
    use rusqlite::OpenFlags;
    if !index.is_file() {
        return None;
    }
    let mut wal = index.as_os_str().to_owned();
    wal.push("-wal");
    if Path::new(&wal).exists() {
        return None;
    }
    let conn = Connection::open_with_flags(
        immutable_uri(index)?,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    conn.pragma_update(None, "query_only", true).ok()?;
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .ok()?;
    let known = i64::try_from(super::schema::INDEX_MIGRATIONS.len()).ok()?;
    if version < 1 || version > known {
        return None;
    }
    let mut stmt = conn
        .prepare("SELECT repo_id, canonical_path FROM repos ORDER BY repo_id")
        .ok()?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .ok()?;
    rows.map(|r| r.ok().map(|(id, path)| (id, PathBuf::from(path))))
        .collect()
}

/// `file:` URI of `path` with `immutable=1`, percent-encoded.
fn immutable_uri(path: &Path) -> Option<String> {
    use std::fmt::Write as _;
    let text = path.to_str()?;
    let text = if cfg!(windows) {
        ["/", &text.replace('\\', "/")].concat()
    } else {
        text.to_owned()
    };
    let mut uri = String::from("file:");
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/:".contains(&b) {
            uri.push(char::from(b));
        } else {
            let _ = write!(uri, "%{b:02X}");
        }
    }
    uri.push_str("?immutable=1");
    Some(uri)
}
