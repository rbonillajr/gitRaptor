//! Errors of the profile store.

use std::fmt;
use std::io;
use std::path::PathBuf;

/// Result alias for the profile store.
pub type Result<T> = std::result::Result<T, ProfileError>;

/// Everything that can go wrong while opening or writing the profile.
#[derive(Debug)]
pub enum ProfileError {
    /// The standard OS folders could not be resolved (no home directory).
    NoHomeDir,
    /// A profile folder exists but is not private to the current user
    /// (wrong owner, mode other than 0700, or a symlink). The engine refuses
    /// to start instead of "fixing" it (SEC-06).
    InsecureDir {
        path: PathBuf,
        reason: String,
    },
    /// A path handed to the profile is not acceptable (relative, UNC,
    /// device or alternate data stream; SEC-02).
    InvalidPath {
        path: PathBuf,
        reason: String,
    },
    /// The file was written by a newer binary. It is left untouched and the
    /// repo is not observed.
    SchemaTooNew {
        path: PathBuf,
        found: i64,
        supported: i64,
    },
    /// No repo with this key exists in the global index.
    UnknownRepo(String),
    /// A write operation referenced something that does not exist
    /// (for example an event on an unknown worktree).
    InvalidWrite(String),
    Io(io::Error),
    Sqlite(rusqlite::Error),
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoHomeDir => write!(f, "could not resolve the user's home directory"),
            Self::InsecureDir { path, reason } => {
                write!(
                    f,
                    "profile folder {} is not private: {reason}",
                    path.display()
                )
            }
            Self::InvalidPath { path, reason } => {
                write!(f, "invalid path {}: {reason}", path.display())
            }
            Self::SchemaTooNew {
                path,
                found,
                supported,
            } => write!(
                f,
                "{} has schema version {found}, newer than the supported {supported}; \
                 upgrade GitRaptor to open it",
                path.display()
            ),
            Self::UnknownRepo(id) => write!(f, "unknown repo key {id}"),
            Self::InvalidWrite(msg) => write!(f, "invalid write: {msg}"),
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Sqlite(err) => write!(f, "SQLite error: {err}"),
        }
    }
}

impl std::error::Error for ProfileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Sqlite(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for ProfileError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<rusqlite::Error> for ProfileError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Sqlite(err)
    }
}
