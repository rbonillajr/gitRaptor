//! Working tree writes of the applier (ADR-TMC-002 § 3, step 6; SEC-TMC-04, SEC-TMC-11).
//!
//! Every open is relative to a descriptor of the worktree root and goes one component at a time
//! with `O_NOFOLLOW`, checking each folder is on the root's device: never "validate, then open".
//! A file is replaced by **atomic exchange** (`renameat2(RENAME_EXCHANGE)` on Linux,
//! `renamex_np(RENAME_SWAP)` on macOS): the new content is written to a temporary name next to
//! the path and swapped in; what was displaced is compared with the prior snapshot. If it differs
//! (an agent wrote there), the exchange is undone and the path is reported as overlap. If it is
//! equal, it is already in the store (it *is* the prior snapshot) and is deleted. A path the
//! prior snapshot says is absent is created with `RENAME_NOREPLACE`/`RENAME_EXCL`: if something is
//! there now, it is overlap and stays.
//!
//! Removing follows the same idea: rename to a temporary name, compare, delete or put back. On a
//! file system without exchange the path is reported as "not restorable with guarantee" and left
//! as it is.
//!
//! Windows has no atomic exchange (DS-TS-TMC-003, Enmienda 2026-10-08, W1–W5): the current entry
//! is renamed aside, compared, and the new content renamed in, every rename exclusive and never
//! following a link or junction. The folders on the way stay open without `FILE_SHARE_DELETE`
//! while a path is written, so none of them can be swapped for a junction meanwhile. A file
//! another program holds open is retried for a bounded time and then fails as
//! [`WriteError::Locked`], untouched.
//!
//! Temporary names start with [`TEMP_PREFIX`]. One left behind by a crash holds the target
//! content (in the store), the prior snapshot's content (in the store) or, before the comparison
//! ended, someone else's content, which is not in the store and is kept there, never deleted
//! (NFR-01). [`RootDir::temps`] lists them and [`RootDir::restore_temp`] puts one back at its
//! path, exclusively; the start of the daemon decides which (DS-TS-TMC-003, Enmienda T).

use crate::Oid;

use super::Result;
#[cfg(any(unix, windows))]
use super::WriteError;

/// Prefix of every temporary name the applier creates in a worktree.
pub const TEMP_PREFIX: &str = ".gitraptor-tm-";

/// Kind of a working tree entry, as the store keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    File,
    Executable,
    Symlink,
}

/// What the applier writes at a path.
#[derive(Debug, Clone, Copy)]
pub enum Content<'a> {
    File {
        bytes: &'a [u8],
        executable: bool,
    },
    /// The link target, written as is and never followed.
    Symlink(&'a [u8]),
}

impl Content<'_> {
    fn kind(&self) -> Kind {
        match self {
            Self::File {
                executable: true, ..
            } => Kind::Executable,
            Self::File { .. } => Kind::File,
            Self::Symlink(_) => Kind::Symlink,
        }
    }

    fn bytes(&self) -> &[u8] {
        match self {
            Self::File { bytes, .. } => bytes,
            Self::Symlink(target) => target,
        }
    }

    /// Kind and blob id of this content.
    pub fn identity(&self) -> (Kind, Oid) {
        (self.kind(), blob_id(self.bytes()))
    }
}

/// What the prior snapshot says is at a path now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expected {
    Absent,
    Present { kind: Kind, id: Oid },
}

/// What happened at one path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The target content is in place.
    Written,
    /// The path is gone, as the target says.
    Removed,
    /// The path already held the target.
    Unchanged,
    /// The path did not hold what the prior snapshot says: someone else wrote there. Their
    /// content was kept, in place or, if the path was taken again meanwhile, at `kept_at`.
    Overlap { kept_at: Option<std::path::PathBuf> },
    /// The file system has no atomic exchange: the path was not touched.
    NotGuaranteed,
    /// A folder on the way is a link, a file or on another device: nothing was written.
    Blocked(&'static str),
}

/// A temporary entry of the applier found in a folder: a file or a link, never followed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TempEntry {
    /// Its name in the folder (starts with [`TEMP_PREFIX`]).
    pub name: String,
    pub kind: Kind,
    pub id: Oid,
}

/// What putting a temporary entry back at its path did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Restore {
    /// The entry is at its path again, byte for byte.
    Restored,
    /// Something is at the path: nothing was touched.
    Occupied,
    /// The temporary entry is gone, no longer holds the expected content or is no longer the
    /// entry compared (another one took its name): nothing was touched.
    Mismatch,
    /// The entry renamed is not the one compared: another one took the temporary name between
    /// the last check and the rename. It is at the path now; nothing was overwritten or deleted.
    Swapped,
    /// Another program holds the temporary entry open: nothing was touched.
    Busy,
    /// A folder on the way is missing, a link or on another device: nothing was touched.
    Blocked,
    /// The file system has no exclusive rename: nothing was touched.
    NotGuaranteed,
}

/// Where a test makes another entry take the temporary name while it is put back (the race of
/// DS-TS-TMC-003, Enmienda T2): after the comparison, or after the last check.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapPoint {
    AfterCompare,
    BeforeRename,
}

/// Whether `name` is a temporary name of the applier, as found in one folder.
/// Only the exact form the applier writes counts (`.gitraptor-tm-<digits>`): a user's
/// `.gitraptor-tm-notes` is not one.
pub fn is_temp_name(name: &str) -> bool {
    name.strip_prefix(TEMP_PREFIX)
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The path of a probe entry inside `folder` (`""` for the root), to reach the folder itself
/// through the same checks as a path.
#[cfg(any(unix, windows))]
fn in_folder(folder: &[u8]) -> Vec<u8> {
    if folder.is_empty() {
        b"x".to_vec()
    } else {
        [folder, b"/x"].concat()
    }
}

/// How the target file system compares names, probed at the root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Folding {
    pub case_insensitive: bool,
    pub normalizing: bool,
}

/// Blob id of `bytes`.
pub fn blob_id(bytes: &[u8]) -> Oid {
    Oid(
        gix::objs::compute_hash(gix::hash::Kind::Sha1, gix::object::Kind::Blob, bytes)
            .expect("sha1 of a blob"),
    )
}

#[cfg(unix)]
pub use unix::RootDir;

#[cfg(windows)]
pub use windows::RootDir;

#[cfg(not(any(unix, windows)))]
pub use other::RootDir;

#[cfg(unix)]
mod unix {
    use std::io::{Read, Write};
    use std::os::fd::{AsFd, OwnedFd};
    use std::path::{Path, PathBuf};

    use rustix::fs::{AtFlags, Mode, OFlags, RenameFlags};
    use rustix::io::Errno;

    use super::*;

    /// A descriptor of a worktree root; every write happens beneath it.
    #[derive(Debug)]
    pub struct RootDir {
        fd: OwnedFd,
        dev: u64,
        path: PathBuf,
        no_exchange: bool,
        crash_between_moves: bool,
        swap_at: Option<SwapPoint>,
    }

    fn io(e: Errno) -> WriteError {
        WriteError::Io(e.into())
    }

    const DIR_FLAGS: OFlags = OFlags::RDONLY
        .union(OFlags::DIRECTORY)
        .union(OFlags::NOFOLLOW)
        .union(OFlags::CLOEXEC);

    fn temp_name() -> String {
        format!("{TEMP_PREFIX}{}", super::super::nanos())
    }

    impl RootDir {
        /// Opens the root, which must be a real folder (not a link).
        pub fn open(root: &Path) -> Result<Self> {
            let fd = rustix::fs::open(root, DIR_FLAGS, Mode::empty()).map_err(io)?;
            let dev = rustix::fs::fstat(&fd).map_err(io)?.st_dev as u64;
            Ok(Self {
                fd,
                dev,
                path: root.to_owned(),
                no_exchange: false,
                crash_between_moves: false,
                swap_at: None,
            })
        }

        /// Makes another entry, with the same content, take the temporary name at `at` while
        /// [`Self::restore_temp`] runs (tests of the race of DS-TS-TMC-003, Enmienda T2).
        #[doc(hidden)]
        pub fn simulating_swap_during_restore(mut self, at: SwapPoint) -> Self {
            self.swap_at = Some(at);
            self
        }

        fn swap_if(&self, at: SwapPoint, dir: &OwnedFd, temp: &str) -> Result<()> {
            if self.swap_at != Some(at) {
                return Ok(());
            }
            let bytes = {
                let fd =
                    rustix::fs::openat(dir, temp, OFlags::RDONLY | OFlags::NOFOLLOW, Mode::empty())
                        .map_err(io)?;
                let mut bytes = Vec::new();
                std::fs::File::from(fd).read_to_end(&mut bytes)?;
                bytes
            };
            let other = format!("{temp}-swap");
            let fd = rustix::fs::openat(
                dir,
                other.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o644),
            )
            .map_err(io)?;
            std::fs::File::from(fd).write_all(&bytes)?;
            rustix::fs::renameat(dir, other.as_str(), dir, temp).map_err(io)?;
            Ok(())
        }

        /// `(device, inode)` of what is at `name` in `dir`, never following a link.
        fn identity(dir: &OwnedFd, name: &[u8]) -> Result<Option<(u64, u64)>> {
            match rustix::fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW) {
                // `st_dev` is `i32` on macOS and `u64` on Linux: the cast is needed on one of them.
                #[allow(clippy::unnecessary_cast)]
                Ok(s) => Ok(Some((s.st_dev as u64, s.st_ino))),
                Err(Errno::NOENT) => Ok(None),
                Err(e) => Err(io(e)),
            }
        }

        /// Stops `remove` right after the current entry went aside, as if the process died there
        /// (tests of the sweep after a crash, DS-TS-TMC-003 Enmienda T).
        #[doc(hidden)]
        pub fn simulating_crash_between_moves(mut self) -> Self {
            self.crash_between_moves = true;
            self
        }

        /// Behaves as a file system without atomic exchange or exclusive rename: every such
        /// rename fails with `EINVAL`, as on one that lacks them (tests of SEC-TMC-11).
        #[doc(hidden)]
        pub fn simulating_no_exchange(mut self) -> Self {
            self.no_exchange = true;
            self
        }

        fn rename(
            &self,
            dir: &OwnedFd,
            from: &[u8],
            to: &[u8],
            flags: RenameFlags,
        ) -> std::result::Result<(), Errno> {
            if self.no_exchange && !flags.is_empty() {
                return Err(Errno::INVAL);
            }
            rustix::fs::renameat_with(dir, from, dir, to, flags)
        }

        pub fn path(&self) -> &Path {
            &self.path
        }

        /// Probes how the file system compares names, with a temporary file at the root.
        pub fn probe_folding(&self) -> Result<Folding> {
            let base = temp_name();
            let name = format!("{base}-A\u{e9}");
            let fd = rustix::fs::openat(
                &self.fd,
                name.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )
            .map_err(io)?;
            drop(fd);
            let exists =
                |n: &str| rustix::fs::statat(&self.fd, n, AtFlags::SYMLINK_NOFOLLOW).is_ok();
            let folding = Folding {
                case_insensitive: exists(&format!("{base}-a\u{e9}")),
                normalizing: exists(&format!("{base}-Ae\u{301}")),
            };
            rustix::fs::unlinkat(&self.fd, name.as_str(), AtFlags::empty()).map_err(io)?;
            Ok(folding)
        }

        /// Opens the folder that holds `rel` and returns it with the last component. With
        /// `create`, missing folders are created (mode from the umask).
        fn parent(&self, rel: &[u8], create: bool) -> Result<Result<(OwnedFd, Vec<u8>), Outcome>> {
            super::super::tree_path::check(rel)
                .map_err(|r| WriteError::InvalidInput(r.to_string()))?;
            let mut parts: Vec<&[u8]> = rel.split(|b| *b == b'/').collect();
            let name = parts
                .pop()
                .expect("a checked path has a component")
                .to_vec();
            let mut current = rustix::io::dup(&self.fd).map_err(io)?;
            for part in parts {
                let next = match rustix::fs::openat(&current, part, DIR_FLAGS, Mode::empty()) {
                    Ok(fd) => fd,
                    Err(Errno::NOENT) if create => {
                        match rustix::fs::mkdirat(&current, part, Mode::from_raw_mode(0o777)) {
                            Ok(()) | Err(Errno::EXIST) => {}
                            Err(e) => return Err(io(e)),
                        }
                        match rustix::fs::openat(&current, part, DIR_FLAGS, Mode::empty()) {
                            Ok(fd) => fd,
                            Err(Errno::LOOP | Errno::NOTDIR) => {
                                return Ok(Err(Outcome::Blocked(
                                    "a folder on the way is a link or a file",
                                )));
                            }
                            Err(e) => return Err(io(e)),
                        }
                    }
                    Err(Errno::NOENT) => return Ok(Err(Outcome::Unchanged)),
                    Err(Errno::LOOP | Errno::NOTDIR) => {
                        return Ok(Err(Outcome::Blocked(
                            "a folder on the way is a link or a file",
                        )));
                    }
                    Err(e) => return Err(io(e)),
                };
                if rustix::fs::fstat(&next).map_err(io)?.st_dev as u64 != self.dev {
                    return Ok(Err(Outcome::Blocked(
                        "a folder on the way is on another device",
                    )));
                }
                current = next;
            }
            Ok(Ok((current, name)))
        }

        /// Kind and blob id of what is at `name` in `dir`, never following a link. `Ok(None)` if
        /// nothing is there; `Err(())` inside for a folder or a special file.
        fn observe(
            dir: &OwnedFd,
            name: &[u8],
        ) -> Result<Option<std::result::Result<(Kind, Oid), ()>>> {
            let stat = match rustix::fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(s) => s,
                Err(Errno::NOENT) => return Ok(None),
                Err(e) => return Err(io(e)),
            };
            let file_type = rustix::fs::FileType::from_raw_mode(stat.st_mode as _);
            match file_type {
                rustix::fs::FileType::Symlink => {
                    let target = rustix::fs::readlinkat(dir, name, Vec::new()).map_err(io)?;
                    Ok(Some(Ok((Kind::Symlink, blob_id(target.as_bytes())))))
                }
                rustix::fs::FileType::RegularFile => {
                    let fd = rustix::fs::openat(
                        dir,
                        name,
                        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(io)?;
                    let mut file = std::fs::File::from(fd);
                    let mut bytes = Vec::new();
                    file.read_to_end(&mut bytes)?;
                    let kind = if stat.st_mode as u32 & 0o100 != 0 {
                        Kind::Executable
                    } else {
                        Kind::File
                    };
                    Ok(Some(Ok((kind, blob_id(&bytes)))))
                }
                _ => Ok(Some(Err(()))),
            }
        }

        /// Writes `content` under a temporary name in `dir`.
        fn write_temp(dir: &OwnedFd, content: &Content<'_>) -> Result<String> {
            let tmp = temp_name();
            match content {
                Content::File { bytes, executable } => {
                    let mode = if *executable { 0o777 } else { 0o666 };
                    let fd = rustix::fs::openat(
                        dir,
                        tmp.as_str(),
                        OFlags::WRONLY
                            | OFlags::CREATE
                            | OFlags::EXCL
                            | OFlags::NOFOLLOW
                            | OFlags::CLOEXEC,
                        Mode::from_raw_mode(mode),
                    )
                    .map_err(io)?;
                    let mut file = std::fs::File::from(fd);
                    file.write_all(bytes)?;
                    rustix::fs::fsync(&file).map_err(io)?;
                }
                Content::Symlink(target) => {
                    rustix::fs::symlinkat(*target, dir, tmp.as_str()).map_err(io)?;
                }
            }
            Ok(tmp)
        }

        fn unlink(dir: &OwnedFd, name: &str) -> Result<()> {
            match rustix::fs::unlinkat(dir, name, AtFlags::empty()) {
                Ok(()) | Err(Errno::NOENT) => Ok(()),
                Err(e) => Err(io(e)),
            }
        }

        fn kept_at(&self, rel: &[u8], tmp: &str) -> PathBuf {
            let rel = String::from_utf8_lossy(rel);
            let parent = rel.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
            self.path.join(parent).join(tmp)
        }

        /// Puts `content` at `rel`, which the prior snapshot says holds `expected`.
        /// `before_exchange` runs right before the exchange (test hook for SEC-TMC-11).
        pub fn replace(
            &self,
            rel: &[u8],
            content: &Content<'_>,
            expected: &Expected,
            before_exchange: &dyn Fn(&[u8]),
        ) -> Result<Outcome> {
            let (dir, name) = match self.parent(rel, true)? {
                Ok(found) => found,
                Err(outcome) => return Ok(outcome),
            };
            let target = content.identity();
            if let Some(Ok(current)) = Self::observe(&dir, &name)?
                && current == target
            {
                return Ok(Outcome::Unchanged);
            }
            let tmp = Self::write_temp(&dir, content)?;
            before_exchange(rel);
            let flags = match expected {
                Expected::Absent => RenameFlags::NOREPLACE,
                Expected::Present { .. } => RenameFlags::EXCHANGE,
            };
            match self.rename(&dir, tmp.as_bytes(), &name, flags) {
                Ok(()) => {}
                // Something is there although the prior snapshot says it is absent, or nothing is
                // there although it says present: someone else changed it. Keep theirs.
                Err(Errno::EXIST | Errno::NOENT | Errno::NOTEMPTY | Errno::ISDIR) => {
                    Self::unlink(&dir, &tmp)?;
                    return Ok(Outcome::Overlap { kept_at: None });
                }
                Err(Errno::INVAL | Errno::NOTSUP | Errno::NOSYS) => {
                    Self::unlink(&dir, &tmp)?;
                    return Ok(Outcome::NotGuaranteed);
                }
                Err(e) => {
                    Self::unlink(&dir, &tmp)?;
                    return Err(io(e));
                }
            }
            if let Expected::Present { kind, id } = expected {
                let displaced = Self::observe(&dir, tmp.as_bytes())?;
                if displaced != Some(Ok((*kind, *id))) {
                    // Undo the exchange: theirs goes back, ours is the temporary again.
                    if self
                        .rename(&dir, tmp.as_bytes(), &name, RenameFlags::EXCHANGE)
                        .is_err()
                    {
                        return Ok(Outcome::Overlap {
                            kept_at: Some(self.kept_at(rel, &tmp)),
                        });
                    }
                    Self::remove_temp(&dir, &tmp)?;
                    let _ = rustix::fs::fsync(&dir);
                    return Ok(Outcome::Overlap { kept_at: None });
                }
                Self::unlink(&dir, &tmp)?;
            }
            let _ = rustix::fs::fsync(&dir);
            Ok(Outcome::Written)
        }

        /// Removes our own temporary entry: a file or link we wrote.
        fn remove_temp(dir: &OwnedFd, tmp: &str) -> Result<()> {
            match Self::observe(dir, tmp.as_bytes())? {
                Some(Ok(_)) | None => Self::unlink(dir, tmp),
                // Not ours any more: leave it.
                Some(Err(())) => Ok(()),
            }
        }

        /// Removes `rel`, which the prior snapshot says holds `kind`/`id`.
        pub fn remove(&self, rel: &[u8], kind: Kind, id: Oid) -> Result<Outcome> {
            let (dir, name) = match self.parent(rel, false)? {
                Ok(found) => found,
                Err(outcome) => return Ok(outcome),
            };
            let tmp = temp_name();
            match self.rename(&dir, &name, tmp.as_bytes(), RenameFlags::NOREPLACE) {
                Ok(()) => {}
                Err(Errno::NOENT) => return Ok(Outcome::Unchanged),
                Err(Errno::INVAL | Errno::NOTSUP | Errno::NOSYS) => {
                    return Ok(Outcome::NotGuaranteed);
                }
                Err(e) => return Err(io(e)),
            }
            if self.crash_between_moves {
                return Err(WriteError::Io(std::io::Error::other(
                    "simulated crash between the two renames",
                )));
            }
            if Self::observe(&dir, tmp.as_bytes())? == Some(Ok((kind, id))) {
                Self::unlink(&dir, &tmp)?;
                let _ = rustix::fs::fsync(&dir);
                return Ok(Outcome::Removed);
            }
            let back = self.rename(&dir, tmp.as_bytes(), &name, RenameFlags::NOREPLACE);
            let _ = rustix::fs::fsync(&dir);
            Ok(Outcome::Overlap {
                kept_at: back.is_err().then(|| self.kept_at(rel, &tmp)),
            })
        }

        /// Removes the folder `rel` if it is empty; a folder with anything left (an ignored
        /// file, say) stays. Never follows a link.
        pub fn remove_dir_if_empty(&self, rel: &[u8]) -> Result<bool> {
            let (dir, name) = match self.parent(rel, false)? {
                Ok(found) => found,
                Err(_) => return Ok(false),
            };
            match rustix::fs::unlinkat(&dir, name.as_slice(), AtFlags::REMOVEDIR) {
                Ok(()) => {
                    let _ = rustix::fs::fsync(dir.as_fd());
                    Ok(true)
                }
                Err(_) => Ok(false),
            }
        }

        /// Kind and blob id of what is at `rel` now, for checks before applying.
        pub fn current(&self, rel: &[u8]) -> Result<Option<(Kind, Oid)>> {
            let (dir, name) = match self.parent(rel, false)? {
                Ok(found) => found,
                Err(_) => return Ok(None),
            };
            Ok(Self::observe(&dir, &name)?.and_then(|r| r.ok()))
        }

        /// Temporary entries of the applier in `folder` (`""` for the root): files and links,
        /// never followed. Folders and special files are left out; a folder on the way that is a
        /// link or on another device gives none.
        pub fn temps(&self, folder: &[u8]) -> Result<Vec<TempEntry>> {
            let dir = match self.parent(&in_folder(folder), false)? {
                Ok((dir, _)) => dir,
                Err(_) => return Ok(Vec::new()),
            };
            let mut found = Vec::new();
            for entry in rustix::fs::Dir::read_from(&dir).map_err(io)? {
                let entry = entry.map_err(io)?;
                let Ok(name) = std::str::from_utf8(entry.file_name().to_bytes()) else {
                    continue;
                };
                if !is_temp_name(name) {
                    continue;
                }
                if let Some(Ok((kind, id))) = Self::observe(&dir, name.as_bytes())? {
                    found.push(TempEntry {
                        name: name.to_owned(),
                        kind,
                        id,
                    });
                }
            }
            found.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(found)
        }

        /// Puts the temporary entry `temp`, in the folder of `rel`, back at `rel` if it still
        /// holds `expected` and nothing is at `rel`: one exclusive rename, never following a
        /// link. Anything else leaves both untouched. The entry renamed must be the entry
        /// compared, by `(device, inode)`: checked again right before the rename (another entry
        /// there: [`Restore::Mismatch`], untouched) and at the path after it
        /// ([`Restore::Swapped`] if another one slipped into the window left between the two).
        pub fn restore_temp(
            &self,
            rel: &[u8],
            temp: &str,
            expected: (Kind, Oid),
        ) -> Result<Restore> {
            if !is_temp_name(temp) {
                return Err(WriteError::InvalidInput("not a temporary name".into()));
            }
            let (dir, name) = match self.parent(rel, false)? {
                Ok(found) => found,
                Err(_) => return Ok(Restore::Blocked),
            };
            // The entry compared is the one renamed (Enmienda T2): its identity before the
            // comparison, again right before the rename, and at the path after it.
            let Some(compared) = Self::identity(&dir, temp.as_bytes())? else {
                return Ok(Restore::Mismatch);
            };
            if Self::observe(&dir, temp.as_bytes())? != Some(Ok(expected)) {
                return Ok(Restore::Mismatch);
            }
            self.swap_if(SwapPoint::AfterCompare, &dir, temp)?;
            if Self::identity(&dir, temp.as_bytes())? != Some(compared) {
                return Ok(Restore::Mismatch);
            }
            self.swap_if(SwapPoint::BeforeRename, &dir, temp)?;
            match self.rename(&dir, temp.as_bytes(), &name, RenameFlags::NOREPLACE) {
                Ok(()) => {
                    let _ = rustix::fs::fsync(&dir);
                    // Another entry took the name between the last check and the rename: there
                    // is no rename by descriptor on Unix. It stays at the path, never moved again:
                    // whose it is cannot be told, and nothing was overwritten or deleted.
                    if Self::identity(&dir, &name)? != Some(compared) {
                        return Ok(Restore::Swapped);
                    }
                    Ok(Restore::Restored)
                }
                Err(Errno::EXIST | Errno::NOTEMPTY | Errno::ISDIR) => Ok(Restore::Occupied),
                Err(Errno::NOENT) => Ok(Restore::Mismatch),
                Err(Errno::INVAL | Errno::NOTSUP | Errno::NOSYS) => Ok(Restore::NotGuaranteed),
                Err(e) => Err(io(e)),
            }
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;

        fn temp_with(dir: &Path, bytes: &[u8]) -> (String, (Kind, Oid)) {
            let name = format!("{TEMP_PREFIX}123");
            std::fs::write(dir.join(&name), bytes).unwrap();
            (name, (Kind::File, blob_id(bytes)))
        }

        fn inode(path: &Path) -> u64 {
            use std::os::unix::fs::MetadataExt;
            std::fs::symlink_metadata(path).unwrap().ino()
        }

        #[test]
        fn the_entry_compared_goes_back() {
            let tmp = tempfile::tempdir().unwrap();
            let (temp, expected) = temp_with(tmp.path(), b"prior");
            let before = inode(&tmp.path().join(&temp));
            let root = RootDir::open(tmp.path()).unwrap();
            assert_eq!(
                root.restore_temp(b"a", &temp, expected).unwrap(),
                Restore::Restored
            );
            assert_eq!(std::fs::read(tmp.path().join("a")).unwrap(), b"prior");
            assert_eq!(inode(&tmp.path().join("a")), before);
            assert!(!tmp.path().join(&temp).exists());
        }

        #[test]
        fn another_entry_after_the_comparison_is_never_moved() {
            let tmp = tempfile::tempdir().unwrap();
            let (temp, expected) = temp_with(tmp.path(), b"prior");
            let root = RootDir::open(tmp.path())
                .unwrap()
                .simulating_swap_during_restore(SwapPoint::AfterCompare);
            assert_eq!(
                root.restore_temp(b"a", &temp, expected).unwrap(),
                Restore::Mismatch
            );
            // Same content, another entry: it stays under its name and the path stays free.
            assert!(!tmp.path().join("a").exists());
            assert_eq!(std::fs::read(tmp.path().join(&temp)).unwrap(), b"prior");
        }

        #[test]
        fn another_entry_right_before_the_rename_is_reported_and_left_at_the_path() {
            let tmp = tempfile::tempdir().unwrap();
            let (temp, expected) = temp_with(tmp.path(), b"prior");
            let root = RootDir::open(tmp.path())
                .unwrap()
                .simulating_swap_during_restore(SwapPoint::BeforeRename);
            assert_eq!(
                root.restore_temp(b"a", &temp, expected).unwrap(),
                Restore::Swapped
            );
            // Nothing overwritten or deleted by the restore: the entry that slipped in is at the
            // path, byte for byte, and is not moved again.
            assert_eq!(std::fs::read(tmp.path().join("a")).unwrap(), b"prior");
            assert!(!tmp.path().join(&temp).exists());
        }
    }
}

#[cfg(windows)]
mod windows;

#[cfg(not(any(unix, windows)))]
mod other {
    use std::path::Path;

    use super::super::WriteError;
    use super::*;

    /// Not supported yet on this OS (Pendiente: etapa de validación multiplataforma).
    #[derive(Debug)]
    pub struct RootDir;

    impl RootDir {
        pub fn open(_root: &Path) -> Result<Self> {
            Err(WriteError::Unsupported(
                "atomic exchange in the working tree",
            ))
        }

        pub fn path(&self) -> &Path {
            Path::new("")
        }

        #[doc(hidden)]
        pub fn simulating_no_exchange(self) -> Self {
            self
        }

        #[doc(hidden)]
        pub fn simulating_crash_between_moves(self) -> Self {
            self
        }

        #[doc(hidden)]
        pub fn simulating_swap_during_restore(self, _at: SwapPoint) -> Self {
            self
        }

        pub fn probe_folding(&self) -> Result<Folding> {
            Err(WriteError::Unsupported(
                "atomic exchange in the working tree",
            ))
        }

        pub fn replace(
            &self,
            _rel: &[u8],
            _content: &Content<'_>,
            _expected: &Expected,
            _before_exchange: &dyn Fn(&[u8]),
        ) -> Result<Outcome> {
            Ok(Outcome::NotGuaranteed)
        }

        pub fn remove(&self, _rel: &[u8], _kind: Kind, _id: Oid) -> Result<Outcome> {
            Ok(Outcome::NotGuaranteed)
        }

        pub fn remove_dir_if_empty(&self, _rel: &[u8]) -> Result<bool> {
            Ok(false)
        }

        pub fn current(&self, _rel: &[u8]) -> Result<Option<(Kind, Oid)>> {
            Ok(None)
        }

        pub fn temps(&self, _folder: &[u8]) -> Result<Vec<TempEntry>> {
            Ok(Vec::new())
        }

        pub fn restore_temp(
            &self,
            _rel: &[u8],
            _temp: &str,
            _expected: (Kind, Oid),
        ) -> Result<Restore> {
            Ok(Restore::NotGuaranteed)
        }
    }
}
