//! Working tree writes on Windows (DS-TS-TMC-003, Enmienda 2026-10-08, W1–W5).
//!
//! NTFS has no atomic exchange, so a replacement is two exclusive renames (`MoveFileExW` without
//! `MOVEFILE_REPLACE_EXISTING`, which renames a link or junction itself and never follows it):
//! the current entry goes aside under a temporary name, it is compared with the prior snapshot,
//! and only then the new content takes the path. Between the two renames the path is absent for
//! an instant; a crash there leaves the displaced entry under its temporary name (the prior
//! snapshot, in the store, or someone else's content, kept): nothing is lost (NFR-01).
//!
//! Every folder on the way, the root included, is opened with `FILE_FLAG_OPEN_REPARSE_POINT` and
//! without `FILE_SHARE_DELETE`, and stays open while one path is written: none of them can be
//! renamed, removed or swapped for a junction meanwhile. A folder that is a link or a junction,
//! or on another volume, blocks the path.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use gitraptor_winsys::fs::{is_in_use, rename_no_replace};

use super::*;

const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_READ_ATTRIBUTES: u32 = 0x80;
const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_WRITE: u32 = 0x2;
const FILE_SHARE_DELETE: u32 = 0x4;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// Waits between attempts on a file another program holds open: about 1.5 s in total (W5).
const RETRY_WAITS_MS: [u64; 5] = [50, 100, 200, 400, 800];
/// Total waiting allowed for one root (one application), however many paths are locked.
const WAIT_BUDGET_MS: u64 = 5_000;

/// A worktree root, held open; every write happens beneath it.
#[derive(Debug)]
pub struct RootDir {
    _handle: File,
    volume: u32,
    path: PathBuf,
    no_exchange: bool,
    crash_between_moves: bool,
    wait_left_ms: AtomicU64,
}

fn temp_name() -> String {
    format!("{TEMP_PREFIX}{}", super::super::nanos())
}

/// What one open handle says about the entry.
struct Entry {
    attributes: u32,
    /// A link or a junction (a name surrogate): never followed, never read.
    surrogate: bool,
}

impl Entry {
    fn of(file: &File) -> std::io::Result<Self> {
        let meta = file.metadata()?;
        Ok(Self {
            attributes: meta.file_attributes(),
            surrogate: meta.file_type().is_symlink(),
        })
    }

    fn is_dir(&self) -> bool {
        self.attributes & FILE_ATTRIBUTE_DIRECTORY != 0
    }

    fn is_reparse(&self) -> bool {
        self.attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
}

/// Opens a folder without following it and without letting anyone rename or delete it while
/// the handle lives.
fn pin_dir(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

/// A name Windows would read as something else (`CON`, `a.`, `a `, an 8.3 alias like `AB~1`):
/// the open and the rename could then reach different entries.
fn reinterpreted(name: &str) -> bool {
    if name.ends_with('.') || name.ends_with(' ') {
        return true;
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let device = matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ((stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.len() == 4
        && stem.as_bytes()[3].is_ascii_digit());
    let short_alias = name
        .split_once('~')
        .is_some_and(|(_, rest)| rest.starts_with(|c: char| c.is_ascii_digit()));
    device || short_alias
}

fn locked(path: &Path) -> WriteError {
    WriteError::Locked(path.to_owned())
}

impl RootDir {
    /// Opens the root, which must be a real folder (not a link or junction), and keeps it open.
    pub fn open(root: &Path) -> Result<Self> {
        let handle = pin_dir(root)?;
        let entry = Entry::of(&handle)?;
        if !entry.is_dir() || entry.is_reparse() {
            return Err(WriteError::Untrusted(format!(
                "{} is not a real folder",
                root.display()
            )));
        }
        let (volume, _) = gitraptor_winsys::file_id::of_file(&handle)?;
        Ok(Self {
            _handle: handle,
            volume,
            path: root.to_owned(),
            no_exchange: false,
            crash_between_moves: false,
            wait_left_ms: AtomicU64::new(WAIT_BUDGET_MS),
        })
    }

    /// Behaves as a file system without exclusive rename (tests of SEC-TMC-11).
    #[doc(hidden)]
    pub fn simulating_no_exchange(mut self) -> Self {
        self.no_exchange = true;
        self
    }

    /// Stops `replace` right after the current entry went aside, as if the process died there
    /// (test of the window between the two renames, W1).
    #[doc(hidden)]
    pub fn simulating_crash_between_moves(mut self) -> Self {
        self.crash_between_moves = true;
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Runs `op` again while another program holds the file, within the root's budget.
    fn retry<T>(&self, path: &Path, mut op: impl FnMut() -> std::io::Result<T>) -> Result<T> {
        let mut waits = RETRY_WAITS_MS.iter();
        loop {
            match op() {
                Err(e) if is_in_use(&e) => {
                    let Some(&wait) = waits.next() else {
                        return Err(locked(path));
                    };
                    let left = self.wait_left_ms.load(Ordering::Relaxed);
                    if left < wait {
                        return Err(locked(path));
                    }
                    self.wait_left_ms.store(left - wait, Ordering::Relaxed);
                    std::thread::sleep(Duration::from_millis(wait));
                }
                other => return Ok(other?),
            }
        }
    }

    /// Probes how the file system compares names, with a temporary file at the root.
    pub fn probe_folding(&self) -> Result<Folding> {
        let base = temp_name();
        let name = self.path.join(format!("{base}-A\u{e9}"));
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&name)?;
        let exists = |n: String| self.path.join(n).symlink_metadata().is_ok();
        let folding = Folding {
            case_insensitive: exists(format!("{base}-a\u{e9}")),
            normalizing: exists(format!("{base}-Ae\u{301}")),
        };
        std::fs::remove_file(&name)?;
        Ok(folding)
    }

    /// Pins every folder that holds `rel` and returns them with the folder path and the last
    /// component. With `create`, missing folders are created.
    #[allow(clippy::type_complexity)]
    fn parent(
        &self,
        rel: &[u8],
        create: bool,
    ) -> Result<Result<(Vec<File>, PathBuf, String), Outcome>> {
        super::super::tree_path::check(rel).map_err(|r| WriteError::InvalidInput(r.to_string()))?;
        let rel = std::str::from_utf8(rel)
            .map_err(|_| WriteError::InvalidInput("path is not UTF-8".into()))?;
        let mut parts: Vec<&str> = rel.split('/').collect();
        let name = parts
            .pop()
            .expect("a checked path has a component")
            .to_owned();
        if parts.iter().chain([&name.as_str()]).any(|p| {
            p.is_empty() || *p == "." || *p == ".." || p.contains(['\\', ':']) || reinterpreted(p)
        }) {
            return Ok(Err(Outcome::Blocked("a name Windows would reinterpret")));
        }
        let mut pins = Vec::with_capacity(parts.len());
        let mut dir = self.path.clone();
        for part in parts {
            dir.push(part);
            let pinned = match pin_dir(&dir) {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => {
                    match std::fs::create_dir(&dir) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Err(e) => return Err(e.into()),
                    }
                    self.retry(&dir, || pin_dir(&dir))?
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(Err(Outcome::Unchanged));
                }
                Err(e) if is_in_use(&e) => self.retry(&dir, || pin_dir(&dir))?,
                Err(e) => return Err(e.into()),
            };
            let entry = Entry::of(&pinned)?;
            if !entry.is_dir() || entry.is_reparse() {
                return Ok(Err(Outcome::Blocked(
                    "a folder on the way is a link or a file",
                )));
            }
            if gitraptor_winsys::file_id::of_file(&pinned)?.0 != self.volume {
                return Ok(Err(Outcome::Blocked(
                    "a folder on the way is on another volume",
                )));
            }
            pins.push(pinned);
        }
        Ok(Ok((pins, dir, name)))
    }

    /// Kind and blob id of what is at `path`, never following a link or junction. `Ok(None)` if
    /// nothing is there; `Err(())` inside for a folder, a link or a junction. NTFS keeps no
    /// executable bit, so a file reads as [`Kind::File`].
    fn observe(&self, path: &Path) -> Result<Option<std::result::Result<(Kind, Oid), ()>>> {
        let open = |flags: u32| {
            OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | flags)
                .open(path)
        };
        let file = match self.retry(path, || open(FILE_FLAG_OPEN_REPARSE_POINT)) {
            Ok(f) => f,
            Err(WriteError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let entry = Entry::of(&file)?;
        if entry.is_dir() || entry.surrogate {
            return Ok(Some(Err(())));
        }
        // Another kind of reparse point (compressed by `compact.exe`, deduplicated) is plain
        // data that only reads right when opened normally; it redirects no name.
        let mut file = if entry.is_reparse() {
            let normal = self.retry(path, || open(0))?;
            if Entry::of(&normal)?.surrogate {
                return Ok(Some(Err(())));
            }
            normal
        } else {
            file
        };
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(Some(Ok((Kind::File, blob_id(&bytes)))))
    }

    /// Whether an observation is the content `(kind, id)` of the store. The executable bit does
    /// not exist on NTFS, and Git for Windows checks a link out as a file holding its target.
    fn same(observed: Option<std::result::Result<(Kind, Oid), ()>>, id: Oid) -> bool {
        matches!(observed, Some(Ok((_, found))) if found == id)
    }

    /// Writes `bytes` under a new temporary name in `dir`, flushed to the disk.
    fn write_temp(dir: &Path, bytes: &[u8]) -> Result<PathBuf> {
        let tmp = dir.join(temp_name());
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(0)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(tmp)
    }

    /// Removes our own temporary file, if it still is a file.
    fn remove_temp(&self, path: &Path) -> Result<()> {
        match std::fs::symlink_metadata(path) {
            Ok(m) if m.is_file() => match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.into()),
            },
            _ => Ok(()),
        }
    }

    /// Puts `content` at `rel`, which the prior snapshot says holds `expected`. A link is
    /// written as a file holding its target, as Git for Windows does with `core.symlinks=false`.
    /// `before_exchange` runs right before the first rename (test hook for SEC-TMC-11).
    pub fn replace(
        &self,
        rel: &[u8],
        content: &Content<'_>,
        expected: &Expected,
        before_exchange: &dyn Fn(&[u8]),
    ) -> Result<Outcome> {
        let (_pins, dir, name) = match self.parent(rel, true)? {
            Ok(found) => found,
            Err(outcome) => return Ok(outcome),
        };
        let path = dir.join(&name);
        let (_, target) = content.identity();
        if Self::same(self.observe(&path)?, target) {
            return Ok(Outcome::Unchanged);
        }
        let tmp = Self::write_temp(&dir, content.bytes())?;
        before_exchange(rel);
        if self.no_exchange {
            self.remove_temp(&tmp)?;
            return Ok(Outcome::NotGuaranteed);
        }
        let prior = match expected {
            Expected::Absent => {
                return match rename_no_replace(&tmp, &path) {
                    Ok(()) => Ok(Outcome::Written),
                    // Something is there although the prior snapshot says absent: keep it.
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                        self.remove_temp(&tmp)?;
                        Ok(Outcome::Overlap { kept_at: None })
                    }
                    Err(e) => {
                        self.remove_temp(&tmp)?;
                        Err(e.into())
                    }
                };
            }
            Expected::Present { id, .. } => *id,
        };
        let aside = dir.join(temp_name());
        match self.retry(&path, || rename_no_replace(&path, &aside)) {
            Ok(()) => {}
            // Nothing there although the prior snapshot says present: someone removed it.
            Err(WriteError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                self.remove_temp(&tmp)?;
                return Ok(Outcome::Overlap { kept_at: None });
            }
            Err(e) => {
                self.remove_temp(&tmp)?;
                return Err(e);
            }
        }
        if self.crash_between_moves {
            return Err(WriteError::Io(std::io::Error::other(
                "simulated crash between the two renames",
            )));
        }
        if !Self::same(self.observe(&aside)?, prior) {
            // Someone else's content: it goes back, ours is dropped.
            self.remove_temp(&tmp)?;
            return Ok(match rename_no_replace(&aside, &path) {
                Ok(()) => Outcome::Overlap { kept_at: None },
                Err(_) => Outcome::Overlap {
                    kept_at: Some(aside),
                },
            });
        }
        match rename_no_replace(&tmp, &path) {
            Ok(()) => {
                // The displaced entry is the prior snapshot: it is in the store.
                self.remove_temp(&aside)?;
                Ok(Outcome::Written)
            }
            // Someone took the path between the two renames: theirs stays.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                self.remove_temp(&tmp)?;
                self.remove_temp(&aside)?;
                Ok(Outcome::Overlap { kept_at: None })
            }
            Err(e) => {
                let _ = rename_no_replace(&aside, &path);
                self.remove_temp(&tmp)?;
                Err(e.into())
            }
        }
    }

    /// Removes `rel`, which the prior snapshot says holds `kind`/`id`.
    pub fn remove(&self, rel: &[u8], _kind: Kind, id: Oid) -> Result<Outcome> {
        let (_pins, dir, name) = match self.parent(rel, false)? {
            Ok(found) => found,
            Err(outcome) => return Ok(outcome),
        };
        if self.no_exchange {
            return Ok(Outcome::NotGuaranteed);
        }
        let path = dir.join(&name);
        let aside = dir.join(temp_name());
        match self.retry(&path, || rename_no_replace(&path, &aside)) {
            Ok(()) => {}
            Err(WriteError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Outcome::Unchanged);
            }
            Err(e) => return Err(e),
        }
        if Self::same(self.observe(&aside)?, id) {
            self.remove_temp(&aside)?;
            return Ok(Outcome::Removed);
        }
        let back = rename_no_replace(&aside, &path);
        Ok(Outcome::Overlap {
            kept_at: back.is_err().then_some(aside),
        })
    }

    /// Removes the folder `rel` if it is empty and real; a link, a junction or a folder with
    /// anything left stays.
    pub fn remove_dir_if_empty(&self, rel: &[u8]) -> Result<bool> {
        let (_pins, dir, name) = match self.parent(rel, false)? {
            Ok(found) => found,
            Err(_) => return Ok(false),
        };
        let path = dir.join(&name);
        let Ok(handle) = pin_dir(&path) else {
            return Ok(false);
        };
        let entry = Entry::of(&handle)?;
        drop(handle);
        if !entry.is_dir() || entry.is_reparse() {
            return Ok(false);
        }
        Ok(std::fs::remove_dir(&path).is_ok())
    }

    /// Kind and blob id of what is at `rel` now, for checks before applying.
    pub fn current(&self, rel: &[u8]) -> Result<Option<(Kind, Oid)>> {
        let (_pins, dir, name) = match self.parent(rel, false)? {
            Ok(found) => found,
            Err(_) => return Ok(None),
        };
        Ok(self.observe(&dir.join(&name))?.and_then(|r| r.ok()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content(bytes: &[u8]) -> Content<'_> {
        Content::File {
            bytes,
            executable: false,
        }
    }

    fn present(bytes: &[u8]) -> Expected {
        Expected::Present {
            kind: Kind::File,
            id: blob_id(bytes),
        }
    }

    fn temps(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(TEMP_PREFIX)
            })
            .collect()
    }

    #[test]
    fn replaces_and_removes_what_the_prior_snapshot_says() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("d")).unwrap();
        std::fs::write(tmp.path().join("d/a"), b"old").unwrap();
        let root = RootDir::open(tmp.path()).unwrap();
        let out = root
            .replace(b"d/a", &content(b"new"), &present(b"old"), &|_| {})
            .unwrap();
        assert_eq!(out, Outcome::Written);
        assert_eq!(std::fs::read(tmp.path().join("d/a")).unwrap(), b"new");
        let out = root
            .replace(b"e/b", &content(b"b"), &Expected::Absent, &|_| {})
            .unwrap();
        assert_eq!(out, Outcome::Written);
        assert_eq!(
            root.remove(b"d/a", Kind::File, blob_id(b"new")).unwrap(),
            Outcome::Removed
        );
        assert!(root.remove_dir_if_empty(b"d").unwrap());
        assert!(temps(tmp.path()).is_empty());
    }

    #[test]
    fn a_file_open_in_an_editor_is_locked_and_untouched_then_written_once_closed() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a");
        std::fs::write(&path, b"old").unwrap();
        let root = RootDir::open(tmp.path()).unwrap();
        // An editor that does not share delete keeps the file open.
        let editor = OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&path)
            .unwrap();
        let err = root
            .replace(b"a", &content(b"new"), &present(b"old"), &|_| {})
            .unwrap_err();
        assert!(
            matches!(&err, WriteError::Locked(p) if p == &path),
            "{err:?}"
        );
        let err = root.remove(b"a", Kind::File, blob_id(b"old")).unwrap_err();
        assert!(matches!(err, WriteError::Locked(_)), "{err:?}");
        drop(editor);
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        assert!(temps(tmp.path()).is_empty(), "no temporary left behind");
        let out = root
            .replace(b"a", &content(b"new"), &present(b"old"), &|_| {})
            .unwrap();
        assert_eq!(out, Outcome::Written);
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }

    #[test]
    fn someone_elses_content_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a");
        std::fs::write(&path, b"agent").unwrap();
        let root = RootDir::open(tmp.path()).unwrap();
        let out = root
            .replace(b"a", &content(b"new"), &present(b"old"), &|_| {})
            .unwrap();
        assert_eq!(out, Outcome::Overlap { kept_at: None });
        assert_eq!(std::fs::read(&path).unwrap(), b"agent");
        // Present although the prior snapshot says absent.
        let out = root
            .replace(b"a", &content(b"new"), &Expected::Absent, &|_| {})
            .unwrap();
        assert_eq!(out, Outcome::Overlap { kept_at: None });
        assert_eq!(std::fs::read(&path).unwrap(), b"agent");
        // A write between the comparison and the first rename (SEC-TMC-11).
        std::fs::write(&path, b"old").unwrap();
        let out = root
            .replace(b"a", &content(b"new"), &present(b"old"), &|_| {
                std::fs::write(&path, b"late agent").unwrap();
            })
            .unwrap();
        assert_eq!(out, Outcome::Overlap { kept_at: None });
        assert_eq!(std::fs::read(&path).unwrap(), b"late agent");
        assert!(temps(tmp.path()).is_empty());
    }

    #[test]
    fn a_crash_between_the_two_renames_keeps_the_prior_content() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("a");
        std::fs::write(&path, b"agent work").unwrap();
        let root = RootDir::open(tmp.path())
            .unwrap()
            .simulating_crash_between_moves();
        assert!(
            root.replace(b"a", &content(b"new"), &present(b"agent work"), &|_| {})
                .is_err()
        );
        // The path is absent, and both contents survive byte for byte under temporary names.
        assert!(!path.exists());
        let mut kept: Vec<Vec<u8>> = temps(tmp.path())
            .iter()
            .map(|p| std::fs::read(p).unwrap())
            .collect();
        kept.sort();
        assert_eq!(kept, vec![b"agent work".to_vec(), b"new".to_vec()]);
        // A later application sees the path absent and writes it.
        let root = RootDir::open(tmp.path()).unwrap();
        let out = root
            .replace(b"a", &content(b"agent work"), &Expected::Absent, &|_| {})
            .unwrap();
        assert_eq!(out, Outcome::Written);
    }

    #[test]
    fn links_and_junctions_are_never_followed() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("a"), b"outside").unwrap();
        // A junction needs no privilege, unlike a symbolic link.
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(tmp.path().join("j"))
            .arg(outside.path())
            .output()
            .unwrap();
        assert!(status.status.success(), "{status:?}");
        let root = RootDir::open(tmp.path()).unwrap();
        // On the way: blocked, nothing written outside.
        let out = root
            .replace(b"j/a", &content(b"new"), &present(b"outside"), &|_| {})
            .unwrap();
        assert!(matches!(out, Outcome::Blocked(_)), "{out:?}");
        assert!(matches!(
            root.remove(b"j/a", Kind::File, blob_id(b"outside"))
                .unwrap(),
            Outcome::Blocked(_)
        ));
        // At the path itself: someone else's entry, kept as it is.
        let out = root
            .replace(b"j", &content(b"new"), &present(b"x"), &|_| {})
            .unwrap();
        assert_eq!(out, Outcome::Overlap { kept_at: None });
        assert!(!root.remove_dir_if_empty(b"j").unwrap());
        assert_eq!(root.current(b"j").unwrap(), None);
        assert_eq!(std::fs::read(outside.path().join("a")).unwrap(), b"outside");
        assert!(tmp.path().join("j").symlink_metadata().is_ok());
    }

    #[test]
    fn names_windows_reinterprets_are_blocked() {
        for name in ["CON", "nul.txt", "a.", "a ", "PROGRA~1", "com1"] {
            assert!(reinterpreted(name), "{name}");
        }
        for name in ["CONTRIBUTING.md", "a.b", "~tmp", "com10", "a~b"] {
            assert!(!reinterpreted(name), "{name}");
        }
    }

    #[test]
    fn the_executable_bit_and_links_checked_out_as_files_compare_by_content() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("run.sh"), b"#!/bin/sh").unwrap();
        std::fs::write(tmp.path().join("link"), b"target").unwrap();
        let root = RootDir::open(tmp.path()).unwrap();
        let exec = Content::File {
            bytes: b"#!/bin/sh",
            executable: true,
        };
        assert_eq!(
            root.replace(b"run.sh", &exec, &Expected::Absent, &|_| {})
                .unwrap(),
            Outcome::Unchanged
        );
        assert_eq!(
            root.replace(
                b"link",
                &Content::Symlink(b"target"),
                &Expected::Absent,
                &|_| {}
            )
            .unwrap(),
            Outcome::Unchanged
        );
        let out = root
            .replace(
                b"link2",
                &Content::Symlink(b"target"),
                &Expected::Absent,
                &|_| {},
            )
            .unwrap();
        assert_eq!(out, Outcome::Written);
        assert_eq!(std::fs::read(tmp.path().join("link2")).unwrap(), b"target");
    }
}
