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
            "UPDATE repos SET state = ?2, retired_ms = ?3 WHERE repo_id = ?1 AND state = 'observed'",
            params![repo_id, RepoState::Retired.as_str(), now_ms],
        )?;
        if changed == 0 && self.get(repo_id)?.is_none() {
            return Err(ProfileError::UnknownRepo(repo_id.to_owned()));
        }
        Ok(())
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
