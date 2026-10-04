//! Hash chain of the oplog and its head kept outside it (SEC-TMC-09).
//!
//! Every appended row gets an entry in `chain` with the hash of the previous
//! entry and its own: SHA-256 over the previous hash, the encoding format,
//! the row's kind, sequence and batch, and the row's columns read back with
//! a fixed query. The first entry hangs from a genesis derived from the
//! repo id, so an oplog copied from another repo does not verify.
//!
//! The head (last sequence, batch and hash) is also written to a file next
//! to the oplog after every commit. A crash between the commit and the head
//! write leaves the head one batch behind, which is tolerated; anything else
//! is a break.
//!
//! This detects edits made with ordinary tools. The same user can recompute
//! the whole chain and the head: that residual risk is accepted (SEC-TMC-09
//! is Medium, and a key would live in the same profile).

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use super::model::{BreakCause, ChainBreak};
use crate::profile::{Result, create_private_file};

/// Encoding format of the hashed rows. A future change of encoding bumps
/// it, and rows keep verifying with the format they were written with.
pub(crate) const FORMAT: i64 = 1;

pub(crate) type Hash = [u8; 32];

/// Kind of a chained row: which table holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowKind {
    Snapshot,
    Operation,
    Journal,
    Notice,
}

impl RowKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Snapshot => "snapshot",
            Self::Operation => "operation",
            Self::Journal => "journal",
            Self::Notice => "notice",
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "snapshot" => Some(Self::Snapshot),
            "operation" => Some(Self::Operation),
            "journal" => Some(Self::Journal),
            "notice" => Some(Self::Notice),
            _ => None,
        }
    }

    /// The fixed query that reads a row back for hashing.
    fn select(self) -> &'static str {
        match self {
            Self::Snapshot => {
                "SELECT snapshot_id, seq, level, worktrees, store_ref, engine_mark,
                        cause_operation, cause_event_seq, recorded_ms
                 FROM snapshots WHERE seq = ?1"
            }
            Self::Operation => {
                "SELECT operation_id, seq, kind, subtype, scope, requester, requester_session,
                        channel, confirmed, target, warnings, engine_mark, recorded_ms
                 FROM operations WHERE seq = ?1"
            }
            Self::Journal => {
                "SELECT seq, entry, subject_id, state, step, related_id, path, inode, pid,
                        detail, recorded_ms
                 FROM journal WHERE seq = ?1"
            }
            Self::Notice => {
                "SELECT notice_id, seq, kind, worktree, operation_id, detail, recorded_ms
                 FROM notices WHERE seq = ?1"
            }
        }
    }
}

/// Hash every chain hangs from: tied to the repo id.
pub(crate) fn genesis(repo_id: &str) -> Hash {
    let mut h = Sha256::new();
    h.update(b"gitraptor-oplog-genesis\0");
    h.update(repo_id.as_bytes());
    h.finalize().into()
}

/// Reads the row of `kind` at `seq` with its fixed query.
fn load_row(conn: &Connection, kind: RowKind, seq: i64) -> Result<Option<Vec<Value>>> {
    let mut stmt = conn.prepare_cached(kind.select())?;
    let columns = stmt.column_count();
    Ok(stmt
        .query_row(params![seq], |row| {
            (0..columns).map(|i| row.get::<_, Value>(i)).collect()
        })
        .optional()?)
}

fn row_hash(prev: &[u8], format: i64, kind: RowKind, seq: i64, batch: i64, row: &[Value]) -> Hash {
    let mut h = Sha256::new();
    h.update(prev);
    h.update(format.to_be_bytes());
    put_bytes(&mut h, kind.as_str().as_bytes());
    h.update(seq.to_be_bytes());
    h.update(batch.to_be_bytes());
    h.update((row.len() as u64).to_be_bytes());
    for value in row {
        match value {
            Value::Null => h.update([0]),
            Value::Integer(i) => {
                h.update([1]);
                h.update(i.to_be_bytes());
            }
            Value::Real(r) => {
                h.update([2]);
                h.update(r.to_bits().to_be_bytes());
            }
            Value::Text(t) => {
                h.update([3]);
                put_bytes(&mut h, t.as_bytes());
            }
            Value::Blob(b) => {
                h.update([4]);
                put_bytes(&mut h, b);
            }
        }
    }
    h.finalize().into()
}

fn put_bytes(h: &mut Sha256, bytes: &[u8]) {
    h.update((bytes.len() as u64).to_be_bytes());
    h.update(bytes);
}

/// Last entry of the chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tip {
    pub seq: i64,
    pub batch: i64,
    pub hash: Hash,
}

pub(crate) fn tip(conn: &Connection) -> Result<Option<Tip>> {
    Ok(conn
        .query_row(
            "SELECT seq, batch, hash FROM chain ORDER BY seq DESC LIMIT 1",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            },
        )
        .optional()?
        .map(|(seq, batch, hash)| Tip {
            seq,
            batch,
            hash: to_hash(&hash),
        }))
}

/// Chains the row of `kind` already inserted at `seq`. Runs inside the
/// caller's transaction, right after the insert.
pub(crate) fn link(
    conn: &Connection,
    kind: RowKind,
    seq: i64,
    batch: i64,
    prev: &Hash,
) -> Result<Hash> {
    let row = load_row(conn, kind, seq)?
        .ok_or_else(|| crate::profile::ProfileError::InvalidWrite("chained row vanished".into()))?;
    let hash = row_hash(prev, FORMAT, kind, seq, batch, &row);
    conn.execute(
        "INSERT INTO chain (seq, kind, format, batch, prev_hash, hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![seq, kind.as_str(), FORMAT, batch, &prev[..], &hash[..]],
    )?;
    Ok(hash)
}

/// Walks the whole chain and returns every break, in sequence order. After
/// a break the walk continues from the stored hash, so one edited row is
/// reported once and does not hide the rest.
pub(crate) fn verify(conn: &Connection, repo_id: &str) -> Result<Vec<ChainBreak>> {
    let mut breaks = Vec::new();
    let mut prev = genesis(repo_id);
    let mut stmt =
        conn.prepare("SELECT seq, kind, format, batch, prev_hash, hash FROM chain ORDER BY seq")?;
    let entries = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, Vec<u8>>(4)?,
                row.get::<_, Vec<u8>>(5)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (seq, kind, format, batch, stored_prev, stored_hash) in entries {
        if stored_prev[..] != prev[..] {
            breaks.push(ChainBreak {
                seq,
                cause: BreakCause::LinkBroken,
            });
        }
        let intact = match (RowKind::parse(&kind), format) {
            (Some(kind), FORMAT) => match load_row(conn, kind, seq)? {
                Some(row) => {
                    row_hash(&stored_prev, format, kind, seq, batch, &row)[..] == stored_hash[..]
                }
                None => false,
            },
            _ => false,
        };
        if !intact {
            breaks.push(ChainBreak {
                seq,
                cause: BreakCause::RowAltered,
            });
        }
        prev = to_hash(&stored_hash);
    }
    let mut unchained = conn.prepare(
        "SELECT seq FROM snapshots WHERE seq NOT IN (SELECT seq FROM chain WHERE kind = 'snapshot')
         UNION ALL
         SELECT seq FROM operations WHERE seq NOT IN (SELECT seq FROM chain WHERE kind = 'operation')
         UNION ALL
         SELECT seq FROM journal WHERE seq NOT IN (SELECT seq FROM chain WHERE kind = 'journal')
         UNION ALL
         SELECT seq FROM notices WHERE seq NOT IN (SELECT seq FROM chain WHERE kind = 'notice')",
    )?;
    for seq in unchained.query_map([], |row| row.get::<_, i64>(0))? {
        breaks.push(ChainBreak {
            seq: seq?,
            cause: BreakCause::Unchained,
        });
    }
    breaks.sort_by_key(|b| b.seq);
    Ok(breaks)
}

fn to_hash(bytes: &[u8]) -> Hash {
    let mut hash = [0u8; 32];
    let n = bytes.len().min(32);
    hash[..n].copy_from_slice(&bytes[..n]);
    hash
}

const HEAD_MAGIC: &str = "gitraptor-oplog-head";

/// Writes the head file atomically: private temporary file, fsync, rename.
pub(crate) fn write_head(path: &Path, tip: &Tip) -> io::Result<()> {
    let tmp = path.with_extension("head.tmp");
    match fs::remove_file(&tmp) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    let mut file = create_private_file(&tmp)?;
    let line = [
        HEAD_MAGIC,
        " ",
        &FORMAT.to_string(),
        " ",
        &tip.seq.to_string(),
        " ",
        &tip.batch.to_string(),
        " ",
        &hex(&tip.hash),
        "\n",
    ]
    .concat();
    file.write_all(line.as_bytes())?;
    // A plain fsync, not `sync_all` (`F_FULLFSYNC` on macOS, ~4 ms each, three per snapshot
    // with the folder): the next oplog commit flushes the drive cache anyway, and a head left
    // one batch behind by a power cut is brought up to date on open (TS-TMC-001, ADR-TMC-006
    // § 2: ref + oplog ≤ 25 ms).
    plain_fsync(&file)?;
    drop(file);
    fs::rename(&tmp, path)?;
    #[cfg(unix)]
    if let Some(dir) = path.parent() {
        plain_fsync(&fs::File::open(dir)?)?;
    }
    Ok(())
}

fn plain_fsync(file: &fs::File) -> io::Result<()> {
    #[cfg(unix)]
    {
        rustix::fs::fsync(file)?;
    }
    #[cfg(not(unix))]
    {
        file.sync_all()?;
    }
    Ok(())
}

/// What the head file says.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Head {
    Missing,
    Unreadable,
    At { seq: i64, hash: Hash },
}

fn read_head(path: &Path) -> io::Result<Head> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Head::Missing),
        Err(err) if err.kind() == io::ErrorKind::InvalidData => return Ok(Head::Unreadable),
        Err(err) => return Err(err),
    };
    let parts: Vec<&str> = text.trim_end().split(' ').collect();
    let parsed = match parts.as_slice() {
        [magic, format, seq, _batch, hash] if *magic == HEAD_MAGIC && *format == "1" => {
            seq.parse::<i64>().ok().zip(unhex(hash))
        }
        _ => None,
    };
    Ok(match parsed {
        Some((seq, hash)) => Head::At { seq, hash },
        None => Head::Unreadable,
    })
}

/// Compares the head file with the chain. A head exactly one batch behind
/// (a crash between commit and head write) is not a break.
pub(crate) fn check_head(conn: &Connection, path: &Path) -> Result<Option<ChainBreak>> {
    let Some(last) = tip(conn)? else {
        return Ok(match read_head(path)? {
            Head::Missing => None,
            Head::At { seq, .. } => Some(ChainBreak {
                seq,
                cause: BreakCause::HeadAhead,
            }),
            Head::Unreadable => Some(ChainBreak {
                seq: 0,
                cause: BreakCause::HeadMismatch,
            }),
        });
    };
    let (head_seq, head_hash) = match read_head(path)? {
        Head::Missing => (0, None),
        Head::Unreadable => {
            return Ok(Some(ChainBreak {
                seq: last.seq,
                cause: BreakCause::HeadMismatch,
            }));
        }
        Head::At { seq, hash } => (seq, Some(hash)),
    };
    if head_seq > last.seq {
        return Ok(Some(ChainBreak {
            seq: head_seq,
            cause: BreakCause::HeadAhead,
        }));
    }
    if let Some(head_hash) = head_hash {
        let stored: Option<Vec<u8>> = conn
            .query_row(
                "SELECT hash FROM chain WHERE seq = ?1",
                params![head_seq],
                |row| row.get(0),
            )
            .optional()?;
        if stored.as_deref() != Some(&head_hash[..]) {
            return Ok(Some(ChainBreak {
                seq: head_seq,
                cause: BreakCause::HeadMismatch,
            }));
        }
    }
    if head_seq == last.seq {
        return Ok(None);
    }
    // Rows after the head: tolerated only if they are the last batch and
    // the head stops right before it.
    let first_after: i64 = conn.query_row(
        "SELECT MIN(batch) FROM chain WHERE seq > ?1",
        params![head_seq],
        |row| row.get(0),
    )?;
    if first_after == last.batch && last.batch == head_seq + 1 {
        return Ok(None);
    }
    Ok(Some(ChainBreak {
        seq: head_seq,
        cause: if head_hash.is_none() {
            BreakCause::HeadMissing
        } else {
            BreakCause::HeadBehind
        },
    }))
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    bytes
        .iter()
        .flat_map(|b| {
            [
                DIGITS[(b >> 4) as usize] as char,
                DIGITS[(b & 0x0f) as usize] as char,
            ]
        })
        .collect()
}

fn unhex(text: &str) -> Option<Hash> {
    if text.len() != 64 {
        return None;
    }
    let mut hash = [0u8; 32];
    for (i, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips() {
        let hash = genesis("repo");
        assert_eq!(unhex(&hex(&hash)), Some(hash));
        assert_eq!(unhex("zz"), None);
    }

    #[test]
    fn genesis_depends_on_the_repo() {
        assert_ne!(genesis("a"), genesis("b"));
    }

    #[test]
    fn every_column_changes_the_hash() {
        let base = vec![Value::Text("x".into()), Value::Integer(1), Value::Null];
        let h = row_hash(&[0; 32], FORMAT, RowKind::Journal, 1, 1, &base);
        let mut other = base.clone();
        other[2] = Value::Integer(0);
        assert_ne!(
            h,
            row_hash(&[0; 32], FORMAT, RowKind::Journal, 1, 1, &other)
        );
        assert_ne!(h, row_hash(&[0; 32], FORMAT, RowKind::Notice, 1, 1, &base));
        assert_ne!(h, row_hash(&[1; 32], FORMAT, RowKind::Journal, 1, 1, &base));
    }
}
