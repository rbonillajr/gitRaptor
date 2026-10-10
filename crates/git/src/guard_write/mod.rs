//! The Guardrails write layer (ADR-GRD-001 § 4 and § 7): a closed list of typed operations, apart
//! from the read layer (ADR-GRP-009) and from the Time Machine write layer (ADR-TMC-002).
//!
//! - The activation key `core.hooksPath`, only in the common `config`, through the Git CLI with
//!   a fixed argv ([`crate::invoke::GuardSubcommand`]).
//! - Files only inside `<common>/gitraptor/`: relative to a descriptor of the common directory,
//!   never following links, a temporary folder with a random name created exclusively, `fsync`,
//!   an atomic rename, and removal of listed files only (never recursive).
//!
//! Only the `guardrails` module of `crates/core` reaches it (checked by the static checks of
//! ADR-GRP-009 Validación 5).

use std::path::{Path, PathBuf};

use crate::invoke::{GuardRead, GuardSubcommand, Invoker};
use crate::{ReadError, SystemGit};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix as fs;
#[cfg(not(unix))]
mod portable;
#[cfg(not(unix))]
use portable as fs;

/// The Guardrails folder inside the common directory.
pub const FOLDER: &str = "gitraptor";

/// Why a Guardrails write did not happen.
#[derive(Debug)]
pub enum GuardWriteError {
    /// The input was rejected before touching anything.
    InvalidInput(String),
    /// `gitraptor/` already exists.
    Exists,
    /// A path is a link or not what the journal recorded: nothing was written there.
    Changed(&'static str),
    Io(std::io::Error),
    Git(String),
}

impl std::fmt::Display for GuardWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(why) => write!(f, "invalid input: {why}"),
            Self::Exists => f.write_str("the guardrails folder already exists"),
            Self::Changed(what) => write!(f, "{what} changed or is a link"),
            Self::Io(e) => write!(f, "io: {e}"),
            Self::Git(why) => write!(f, "git: {why}"),
        }
    }
}

impl std::error::Error for GuardWriteError {}

impl From<std::io::Error> for GuardWriteError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<ReadError> for GuardWriteError {
    fn from(e: ReadError) -> Self {
        Self::Git(format!("{e:?}"))
    }
}

pub type Result<T> = std::result::Result<T, GuardWriteError>;

/// `(device, inode)` of a file or folder, recorded in the journal and checked before any later
/// write or removal (M-03). `None` where the platform has no such identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileId {
    pub dev: u64,
    pub ino: u64,
}

/// One file of the folder, by its path relative to `gitraptor/` (`hooks/pre-push`).
#[derive(Debug, Clone, Copy)]
pub struct NewFile<'a> {
    pub path: &'a str,
    pub bytes: &'a [u8],
    pub executable: bool,
}

fn check_relative(path: &str) -> Result<()> {
    let ok = !path.is_empty()
        && path.split('/').count() <= 2
        && path.split('/').all(|c| {
            !c.is_empty()
                && c != "."
                && c != ".."
                && c.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        });
    if ok {
        Ok(())
    } else {
        Err(GuardWriteError::InvalidInput(format!("path {path:?}")))
    }
}

/// One definition of `core.hooksPath` as Git sees it from a worktree (ADR-GRD-001 § 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HooksPathEntry {
    /// `system`, `global`, `local`, `worktree` or `command`.
    pub scope: String,
    /// The file that defines it, resolved against the worktree (`None` if not a file).
    pub origin: Option<PathBuf>,
    pub value: String,
}

/// The typed operations of the layer.
pub struct GuardWriter<'a> {
    git: &'a SystemGit,
    invoker: &'a Invoker,
}

impl<'a> GuardWriter<'a> {
    pub fn new(git: &'a SystemGit, invoker: &'a Invoker) -> Self {
        Self { git, invoker }
    }

    fn config(common: &Path) -> Result<PathBuf> {
        if !common.is_absolute() {
            return Err(GuardWriteError::InvalidInput(
                "common dir must be absolute".into(),
            ));
        }
        Ok(common.join("config"))
    }

    fn key(&self, common: &Path, sub: GuardSubcommand, value: Option<&Path>) -> Result<()> {
        let config = Self::config(common)?;
        let out = self
            .invoker
            .run_guard(&self.git.path, &config, sub, value)?;
        // `--unset` of a missing key exits 5: nothing to undo.
        if out.success || (sub == GuardSubcommand::Unset && out.code == Some(5)) {
            return Ok(());
        }
        Err(GuardWriteError::Git(format!(
            "config failed with code {:?}: {}",
            out.code,
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .next()
                .unwrap_or("")
        )))
    }

    /// Writes the absolute path of the dispatchers folder: the commit point of the install.
    /// Git writes the `config` with its lock and an atomic rename.
    pub fn set_hooks_path(&self, common: &Path, hooks_dir: &Path) -> Result<()> {
        if !hooks_dir.starts_with(common.join(FOLDER)) {
            return Err(GuardWriteError::InvalidInput(
                "hooks dir outside the guardrails folder".into(),
            ));
        }
        self.key(common, GuardSubcommand::Set, Some(hooks_dir))
    }

    /// Writes back the `core.hooksPath` the repo had at local level before the install
    /// (uninstall, ADR-GRD-001 § 4): the value recorded in the journal, as is. Git rewrites the
    /// line in place, with its lock and an atomic rename.
    pub fn restore_hooks_path(&self, common: &Path, value: &str) -> Result<()> {
        if value.is_empty() || value.starts_with('-') || value.chars().any(char::is_control) {
            return Err(GuardWriteError::InvalidInput(
                "prior hooks path is not restorable".into(),
            ));
        }
        self.key(common, GuardSubcommand::Restore, Some(Path::new(value)))
    }

    /// Removes the key from the common `config`.
    pub fn unset_hooks_path(&self, common: &Path) -> Result<()> {
        self.key(common, GuardSubcommand::Unset, None)
    }

    /// The value of the key in the common `config` itself (no includes, no other level).
    pub fn local_hooks_path(&self, common: &Path) -> Result<Option<String>> {
        let config = Self::config(common)?;
        let out = self
            .invoker
            .run_guard(&self.git.path, &config, GuardSubcommand::Get, None)?;
        if out.code == Some(1) {
            return Ok(None);
        }
        if !out.success {
            return Err(GuardWriteError::Git(format!(
                "config --get failed with code {:?}",
                out.code
            )));
        }
        Ok(Some(
            String::from_utf8_lossy(&out.stdout)
                .trim_end_matches(['\n', '\r'])
                .to_owned(),
        ))
    }

    /// Every definition of `core.hooksPath` visible from `worktree`, in Git's order (the last
    /// one wins), with its scope and file (`--show-scope --show-origin`, SPIKE-GRD-001 § 5.2).
    pub fn hooks_path_entries(&self, worktree: &Path) -> Result<Vec<HooksPathEntry>> {
        let out = self
            .invoker
            .run_guard_read(&self.git.path, worktree, GuardRead::HooksPathAll)?;
        if out.code == Some(1) {
            return Ok(Vec::new());
        }
        if !out.success {
            return Err(GuardWriteError::Git(format!(
                "config --get-all failed with code {:?}",
                out.code
            )));
        }
        let fields: Vec<&[u8]> = out.stdout.split(|b| *b == 0).collect();
        let mut entries = Vec::new();
        for chunk in fields.chunks(3) {
            let [scope, origin, value] = chunk else {
                break;
            };
            let origin = std::str::from_utf8(origin).ok().and_then(|o| {
                o.strip_prefix("file:").map(|f| {
                    let f = Path::new(f);
                    if f.is_absolute() {
                        f.to_path_buf()
                    } else {
                        worktree.join(f)
                    }
                })
            });
            entries.push(HooksPathEntry {
                scope: String::from_utf8_lossy(scope).into_owned(),
                origin,
                value: String::from_utf8_lossy(value).into_owned(),
            });
        }
        Ok(entries)
    }

    /// Whether an `includeIf "onbranch:…"` is defined at local or worktree level: it can
    /// redefine the key on some branch only (ADR-GRD-001 § 5).
    pub fn has_onbranch_include(&self, worktree: &Path) -> Result<bool> {
        let out =
            self.invoker
                .run_guard_read(&self.git.path, worktree, GuardRead::OnbranchIncludes)?;
        if out.code == Some(1) {
            return Ok(false);
        }
        if !out.success {
            return Err(GuardWriteError::Git(format!(
                "config --get-regexp failed with code {:?}",
                out.code
            )));
        }
        let fields: Vec<&[u8]> = out.stdout.split(|b| *b == 0).collect();
        Ok(fields
            .chunks(2)
            .any(|c| matches!(c, [scope, _] if *scope == b"local" || *scope == b"worktree")))
    }

    /// Identity of the common `config`, only if it is a regular file (not a link).
    pub fn config_id(&self, common: &Path) -> Result<Option<FileId>> {
        fs::entry_id(common, "config", fs::Kind::File)
    }

    /// Identity of `gitraptor/`, only if it is a folder (not a link). `None` if absent.
    pub fn folder_id(&self, common: &Path) -> Result<Option<FileId>> {
        fs::entry_id(common, FOLDER, fs::Kind::Dir)
    }

    /// Writes `files` into a temporary folder with a random name, syncs it and renames it to
    /// `gitraptor/` atomically. Fails without touching anything if `gitraptor/` exists.
    pub fn write_folder(&self, common: &Path, files: &[NewFile<'_>]) -> Result<FileId> {
        for f in files {
            check_relative(f.path)?;
        }
        fs::write_folder(common, files)
    }

    /// Replaces (or adds) `files` inside the existing `gitraptor/`, each one written to a
    /// temporary file next to it, synced and renamed over it atomically: the folder is never
    /// without a working dispatcher (template upgrade, ADR-GRD-001 § 8). The folder must still be
    /// the one the journal recorded (`expected`).
    pub fn replace_files(
        &self,
        common: &Path,
        expected: FileId,
        files: &[NewFile<'_>],
    ) -> Result<()> {
        for f in files {
            check_relative(f.path)?;
        }
        fs::replace_files(common, expected, files)
    }

    /// Removes the listed files of `gitraptor/`, then its sub-folders and the folder if they are
    /// empty. Never recursive: a foreign file is left in place. The folder must still be the one
    /// the journal recorded (`expected`).
    pub fn remove_folder(&self, common: &Path, listed: &[&str], expected: FileId) -> Result<()> {
        for path in listed {
            check_relative(path)?;
        }
        fs::remove_folder(common, FOLDER, listed, Some(expected))
    }

    /// Removes what `replace_files` leaves when it is killed between writing a temporary and
    /// renaming it: `<file>.gitraptor.tmp-<16 hex>` next to a listed file, regular files only
    /// (never through a link), inside the folder the journal recorded (`expected`). Any other
    /// name is left in place.
    ///
    /// # Errors
    /// `InvalidInput` for a listed path that is not a plain relative one; `Changed` when the
    /// folder is not the recorded one or a link; `Io` otherwise. An absent folder is `Ok`.
    pub fn remove_file_temporaries(
        &self,
        common: &Path,
        expected: FileId,
        listed: &[&str],
    ) -> Result<()> {
        for path in listed {
            check_relative(path)?;
        }
        fs::remove_file_temporaries(common, expected, listed)
    }

    /// Removes the listed files of every leftover temporary folder of an interrupted install.
    pub fn remove_temporaries(&self, common: &Path, listed: &[&str]) -> Result<()> {
        for path in listed {
            check_relative(path)?;
        }
        for name in fs::temporaries(common)? {
            fs::remove_folder(common, &name, listed, None)?;
        }
        Ok(())
    }
}

/// Name of a temporary folder: `gitraptor.tmp-<random>`.
fn temporary_name() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    // RandomState is seeded from the OS's randomness; exclusive creation is what makes the
    // folder ours.
    let mut h = RandomState::new().build_hasher();
    h.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos()),
    );
    format!("{FOLDER}.tmp-{:016x}", h.finish())
}

fn is_temporary(name: &str) -> bool {
    name.strip_prefix(FOLDER)
        .and_then(|r| r.strip_prefix(".tmp-"))
        .is_some_and(|r| r.len() == 16 && r.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Whether `candidate` is `<file_name>.gitraptor.tmp-<16 hex>`: the exact name `replace_files`
/// gives the temporary of `file_name`.
fn is_file_temporary(file_name: &str, candidate: &str) -> bool {
    candidate
        .strip_prefix(file_name)
        .and_then(|r| r.strip_prefix('.'))
        .is_some_and(is_temporary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_relative_paths() {
        for ok in ["manifest.json", "hooks/pre-push", "dispatch.conf"] {
            assert!(check_relative(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "../x",
            "hooks/../x",
            "/abs",
            "a/b/c",
            "hooks//x",
            "a b",
            "./x",
        ] {
            assert!(check_relative(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn temporary_names_are_recognized() {
        let name = temporary_name();
        assert!(is_temporary(&name), "{name}");
        assert_ne!(name, temporary_name());
        assert!(!is_temporary("gitraptor"));
        assert!(!is_temporary("gitraptor.tmp-x"));
    }

    #[test]
    fn file_temporaries_have_the_exact_name() {
        let t = "gitraptor.tmp-0123456789abcdef";
        assert!(is_file_temporary("pre-push", &format!("pre-push.{t}")));
        assert!(!is_file_temporary("pre-push", &format!("pre-commit.{t}")));
        assert!(!is_file_temporary("pre-push", t));
        assert!(!is_file_temporary("pre-push", &format!("pre-push{t}")));
        assert!(!is_file_temporary("pre-push", &format!("pre-push.{t}0")));
        assert!(!is_file_temporary("pre-push", "pre-push.gitraptor.tmp-xyz"));
    }
}
