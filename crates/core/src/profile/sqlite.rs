//! Opening SQLite files of the profile: private creation, schema version,
//! integrity check and quarantine (ADR-GRP-006 § 4).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, ErrorCode, OpenFlags, OptionalExtension};

use super::error::{ProfileError, Result};
use super::fsperm;

/// Companion files SQLite may keep next to a database.
const COMPANIONS: [&str; 3] = ["-wal", "-shm", "-journal"];

/// An open, migrated database.
pub(crate) struct OpenedDb {
    pub conn: Connection,
    /// `true` when the file did not exist (or was replaced) and is new.
    pub fresh: bool,
    /// Where the previous, corrupt file was moved, if it was.
    pub quarantined: Option<PathBuf>,
}

/// Opens `path`, creating it private if missing. A corrupt file is moved to
/// `quarantine_dir` and replaced by an empty one; a file with a newer schema
/// is left untouched and reported as [`ProfileError::SchemaTooNew`].
pub(crate) fn open_db(path: &Path, migrations: &[&str], quarantine_dir: &Path) -> Result<OpenedDb> {
    let supported = migrations.len() as i64;
    let mut quarantined = None;

    let fresh = if path.exists() {
        match inspect(path)? {
            Health::Ok(version) if version > supported => {
                return Err(ProfileError::SchemaTooNew {
                    path: path.to_path_buf(),
                    found: version,
                    supported,
                });
            }
            Health::Ok(version) => version == 0,
            Health::Corrupt => {
                quarantined = Some(quarantine(path, quarantine_dir)?);
                fsperm::create_private_file(path)?;
                true
            }
        }
    } else {
        // Created empty and 0600 before SQLite sees it, so the database and
        // the -wal/-shm files SQLite derives from it are private.
        fsperm::create_private_file(path)?;
        true
    };

    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
    if !mode.eq_ignore_ascii_case("wal") {
        return Err(ProfileError::Sqlite(rusqlite::Error::InvalidQuery));
    }
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    migrate(&conn, migrations)?;
    Ok(OpenedDb {
        conn,
        fresh,
        quarantined,
    })
}

enum Health {
    Ok(i64),
    Corrupt,
}

/// Reads the schema version and runs the quick integrity check through a
/// read-only connection, so a file that will be rejected is never written.
fn inspect(path: &Path) -> Result<Health> {
    let conn = match Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) {
        Ok(conn) => conn,
        Err(err) if is_corruption(&err) => return Ok(Health::Corrupt),
        Err(err) => return Err(err.into()),
    };
    let version: i64 = match conn.query_row("PRAGMA user_version", [], |row| row.get(0)) {
        Ok(v) => v,
        Err(err) if is_corruption(&err) => return Ok(Health::Corrupt),
        Err(err) => return Err(err.into()),
    };
    let check: std::result::Result<String, _> =
        conn.query_row("PRAGMA quick_check", [], |row| row.get(0));
    match check {
        Ok(result) if result == "ok" => Ok(Health::Ok(version)),
        Ok(_) => Ok(Health::Corrupt),
        Err(err) if is_corruption(&err) => Ok(Health::Corrupt),
        Err(err) => Err(err.into()),
    }
}

fn is_corruption(err: &rusqlite::Error) -> bool {
    matches!(
        err.sqlite_error_code(),
        Some(ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase)
    )
}

/// Applies the pending migrations in one transaction.
fn migrate(conn: &Connection, migrations: &[&str]) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    let pending = migrations.iter().skip(current as usize);
    if pending.len() == 0 {
        return Ok(());
    }
    // A migration may rebuild a referenced table, which SQLite only allows
    // with foreign keys off; they can only change outside a transaction, and
    // are checked before the commit.
    let foreign_keys: bool = conn.query_row("PRAGMA foreign_keys", [], |row| row.get(0))?;
    conn.pragma_update(None, "foreign_keys", false)?;
    conn.execute_batch("BEGIN IMMEDIATE")?;
    let result = (|| -> Result<()> {
        for migration in pending {
            conn.execute_batch(migration)?;
        }
        let violation: Option<String> = conn
            .query_row("PRAGMA foreign_key_check", [], |row| row.get(0))
            .optional()?;
        if violation.is_some() {
            return Err(ProfileError::Sqlite(rusqlite::Error::InvalidQuery));
        }
        conn.pragma_update(None, "user_version", migrations.len() as i64)?;
        Ok(())
    })();
    match result {
        Ok(()) => conn.execute_batch("COMMIT")?,
        Err(_) => conn.execute_batch("ROLLBACK")?,
    }
    conn.pragma_update(None, "foreign_keys", foreign_keys)?;
    result
}

/// Moves `path` and its companions into `quarantine_dir` with a timestamp.
/// Nothing is deleted.
fn quarantine(path: &Path, quarantine_dir: &Path) -> Result<PathBuf> {
    fsperm::ensure_private_dir(quarantine_dir)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "unnamed database file"))?;
    let target = free_name(quarantine_dir, name, stamp);
    move_private(path, &target)?;
    for suffix in COMPANIONS {
        let companion = path.with_file_name([name, suffix].concat());
        if companion.exists() {
            let companion_target = target.with_file_name(
                [
                    target.file_name().and_then(|n| n.to_str()).unwrap_or(name),
                    suffix,
                ]
                .concat(),
            );
            move_private(&companion, &companion_target)?;
        }
    }
    Ok(target)
}

fn free_name(dir: &Path, name: &str, stamp: u128) -> PathBuf {
    let base = [name, ".corrupt-", &stamp.to_string()].concat();
    let mut candidate = dir.join(&base);
    let mut n = 1;
    while candidate.exists() {
        candidate = dir.join([base.as_str(), "-", &n.to_string()].concat());
        n += 1;
    }
    candidate
}

fn move_private(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)?;
    fsperm::set_private_file_mode(to)
}

/// A random opaque identifier formatted as a UUID v4, from SQLite's
/// `randomblob` (seeded by the OS). Not a secret.
pub(crate) fn new_uuid(conn: &Connection) -> Result<String> {
    let mut bytes: Vec<u8> = conn.query_row("SELECT randomblob(16)", [], |row| row.get(0))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| char_pair(*b)).collect();
    Ok([
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32],
    ]
    .join("-"))
}

fn char_pair(byte: u8) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    [
        HEX[(byte >> 4) as usize] as char,
        HEX[(byte & 0x0f) as usize] as char,
    ]
    .iter()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uuid_has_v4_shape() {
        let conn = Connection::open_in_memory().unwrap();
        let id = new_uuid(&conn).unwrap();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert!(matches!(&id[19..20], "8" | "9" | "a" | "b"));
        assert_ne!(id, new_uuid(&conn).unwrap());
    }

    #[test]
    fn migrations_are_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("x.sqlite");
        let q = tmp.path().join("q");
        let migrations = ["CREATE TABLE t (a INTEGER) STRICT;"];
        assert!(open_db(&path, &migrations, &q).unwrap().fresh);
        let again = open_db(&path, &migrations, &q).unwrap();
        assert!(!again.fresh);
        let version: i64 = again
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 1);
    }
}
