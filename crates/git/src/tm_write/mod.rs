//! Write layer of the Time Machine (ADR-TMC-002 § 1), separate from the read layer.
//!
//! Only `crates/core::timemachine` may use it (checked by `crates/core/tests/tm_boundary.rs`).
//! It holds two writers:
//!
//! - [`store`]: the snapshot store, written with gitoxide in process.
//! - The low-level operations the applier of undo, redo and restore uses on the user's
//!   repository (TS-TMC-003): [`worktree`] (validated roots and the preconditions of
//!   ADR-TMC-002 § 3, step 1), [`lock`] (Git's lock protocol), [`refs`] (one ref transaction with
//!   expected old values), [`files`] (writes relative to the worktree root, by atomic exchange),
//!   [`index`] (the target index, installed through the own `index.lock`) and [`objects`]
//!   (objects from the store into the repository as a pack) and [`recreate`] (a deleted linked
//!   worktree, without checkout).
//!
//! Every Git invocation of this layer goes through the write profile of `invoke.rs`: a closed
//! list of plumbing commands, no hooks, filters, signing, protocols or system and global
//! configuration, and explicit `--git-dir`/`--work-tree` (SEC-TMC-02). Nothing here talks to a
//! remote (SEC-TMC-05).

mod cli;
pub mod files;
pub mod index;
pub mod lock;
pub mod objects;
pub mod recreate;
pub mod refs;
pub mod store;
pub mod tree_path;
pub mod worktree;

use std::io;
use std::path::{Path, PathBuf};

use crate::{Invoker, ReadError, SystemGit};

/// Empty file of the profile used as `GIT_CONFIG_GLOBAL` (SEC-TMC-02).
pub const EMPTY_CONFIG_FILE: &str = "empty.gitconfig";
/// Scratch folder of the profile for temporary indexes and packs.
pub const SCRATCH_DIR: &str = "tmp";

/// Why a write could not be done. Every variant leaves what it found in place.
#[derive(Debug)]
pub enum WriteError {
    /// Rejected before touching anything.
    InvalidInput(String),
    /// A Git lock that is not ours is present ("Git ocupado"). It is never removed.
    Busy(PathBuf),
    /// A Git operation is in progress in a worktree (BR-TMC-EDGE-004).
    InProgress {
        worktree: PathBuf,
        marker: &'static str,
    },
    /// The repository or a folder of the profile cannot be trusted.
    Untrusted(String),
    /// A ref is not where the plan expected it: the transaction failed whole.
    RefMoved(String),
    /// Not available on this OS yet (Pendiente: etapa de validación multiplataforma).
    Unsupported(&'static str),
    /// A Git command did not finish in time.
    TimedOut(String),
    Io(io::Error),
    Git(String),
}

impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(m) => write!(f, "invalid input: {m}"),
            Self::Busy(p) => write!(f, "git busy: {} exists", p.display()),
            Self::InProgress { worktree, marker } => {
                write!(
                    f,
                    "git operation in progress in {}: {marker}",
                    worktree.display()
                )
            }
            Self::Untrusted(m) => write!(f, "not trusted: {m}"),
            Self::RefMoved(m) => write!(f, "ref moved since planning: {m}"),
            Self::Unsupported(m) => write!(f, "not supported on this OS: {m}"),
            Self::TimedOut(m) => write!(f, "timed out: {m}"),
            Self::Io(e) => write!(f, "i/o: {e}"),
            Self::Git(m) => write!(f, "git: {m}"),
        }
    }
}

impl std::error::Error for WriteError {}

impl From<io::Error> for WriteError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<ReadError> for WriteError {
    fn from(e: ReadError) -> Self {
        match e {
            ReadError::InvalidInput(m) => Self::InvalidInput(m),
            ReadError::Untrusted(m) => Self::Untrusted(m),
            ReadError::TemporarilyUnavailable(m) => Self::TimedOut(m),
            ReadError::Unavailable(m) => Self::Git(m),
        }
    }
}

pub type Result<T, E = WriteError> = std::result::Result<T, E>;

/// What every write-layer invocation needs: the Git the daemon resolved once (ADR-TMC-002 § 2,
/// Enmienda E6), the invoker and three private items of `<tm>/<repo-id>/` in the profile.
#[derive(Debug, Clone)]
pub struct WriteContext {
    git: SystemGit,
    invoker: Invoker,
    hooks_dir: PathBuf,
    global_config: PathBuf,
    scratch: PathBuf,
}

impl WriteContext {
    /// Binds the layer to the Time Machine folder of one repo (`<tm>/<repo-id>`), creating the
    /// empty hooks folder, the empty global configuration and the scratch folder if missing.
    /// Each must be private to the current user, not a link, and the first two empty.
    pub fn new(git: SystemGit, invoker: Invoker, repo_tm_dir: &Path) -> Result<Self> {
        if !repo_tm_dir.is_absolute() {
            return Err(WriteError::InvalidInput(
                "profile folder must be absolute".into(),
            ));
        }
        let hooks_dir = repo_tm_dir.join(store::NOHOOKS_DIR);
        let global_config = repo_tm_dir.join(EMPTY_CONFIG_FILE);
        let scratch = repo_tm_dir.join(SCRATCH_DIR);
        private::check_dir(repo_tm_dir)?;
        private::ensure_dir(&hooks_dir)?;
        private::ensure_dir(&scratch)?;
        private::ensure_empty_file(&global_config)?;
        if std::fs::read_dir(&hooks_dir)?.next().is_some() {
            return Err(WriteError::Untrusted("hooks folder is not empty".into()));
        }
        Ok(Self {
            git,
            invoker,
            hooks_dir,
            global_config,
            scratch,
        })
    }

    pub fn git(&self) -> &SystemGit {
        &self.git
    }

    pub(crate) fn target<'a>(
        &'a self,
        git_dir: &'a Path,
        work_tree: Option<&'a Path>,
        index_file: Option<&'a Path>,
    ) -> crate::invoke::WriteTarget<'a> {
        crate::invoke::WriteTarget {
            git_dir,
            work_tree,
            hooks_dir: &self.hooks_dir,
            global_config: &self.global_config,
            index_file,
        }
    }

    pub(crate) fn invoker(&self) -> &Invoker {
        &self.invoker
    }

    /// A fresh path in the scratch folder.
    pub(crate) fn scratch_path(&self, what: &str) -> PathBuf {
        self.scratch.join(format!("{what}-{}", nanos()))
    }
}

/// Nanoseconds of the wall clock plus a counter, to name temporary files.
pub(crate) fn nanos() -> u128 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    now + u128::from(COUNTER.fetch_add(1, Ordering::Relaxed))
}

/// Private folders and files of the profile (SEC-TMC-01): owned by the current user, no access
/// for others, never a link.
mod private {
    use super::{Result, WriteError};
    use std::path::Path;

    pub(super) fn check_dir(path: &Path) -> Result<()> {
        let meta = path.symlink_metadata()?;
        if !meta.is_dir() {
            return Err(WriteError::Untrusted(format!(
                "{} is not a real folder",
                path.display()
            )));
        }
        check_owner(&meta, path)
    }

    pub(super) fn ensure_dir(path: &Path) -> Result<()> {
        if path.symlink_metadata().is_err() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                std::fs::DirBuilder::new().mode(0o700).create(path)?;
            }
            #[cfg(not(unix))]
            std::fs::create_dir(path)?;
        }
        check_dir(path)
    }

    pub(super) fn ensure_empty_file(path: &Path) -> Result<()> {
        if path.symlink_metadata().is_err() {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(path)?;
        }
        let meta = path.symlink_metadata()?;
        if !meta.is_file() || meta.len() != 0 {
            return Err(WriteError::Untrusted(format!(
                "{} is not an empty file",
                path.display()
            )));
        }
        check_owner(&meta, path)
    }

    #[cfg(unix)]
    fn check_owner(meta: &std::fs::Metadata, path: &Path) -> Result<()> {
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != rustix::process::geteuid().as_raw() || meta.mode() & 0o077 != 0 {
            return Err(WriteError::Untrusted(format!(
                "{} is not private to the current user",
                path.display()
            )));
        }
        Ok(())
    }

    #[cfg(not(unix))]
    fn check_owner(_meta: &std::fs::Metadata, _path: &Path) -> Result<()> {
        Ok(())
    }
}
