//! Fingerprint of a set of directories (ADR-GRP-009, Validación 1 and 2): path, kind, size,
//! content hash and mtime of every file **and directory**, never `atime`. A directory mtime
//! catches a lock that was created and deleted between two snapshots.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::hash::Hasher;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::guard;

/// A named root to fingerprint. Two runs of the same scenario use the same labels, so their
/// changes can be compared even though the absolute paths differ (control run).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub label: String,
    pub root: PathBuf,
    /// A machine-level path read in place (the system Git config): exempt from the
    /// testkit-root guard, never written by the harness.
    pub system: bool,
}

impl Scope {
    /// A scope that must live inside a temporary root created by the testkit.
    pub fn new(label: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        Self {
            label: label.into(),
            root: root.into(),
            system: false,
        }
    }

    /// A machine-level path, fingerprinted read-only (ADR-GRP-009, Validación 2).
    pub fn system(label: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        Self {
            system: true,
            ..Self::new(label, root)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Dir,
    File,
    Symlink,
    /// FIFO, socket or device: recorded, never opened.
    Other,
}

/// One fingerprinted path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: Kind,
    pub size: u64,
    /// Content hash for files, target hash for symlinks, 0 otherwise.
    pub hash: u64,
    pub mtime: Option<SystemTime>,
    /// Permission bits (unix mode; on Windows, the read-only flag). A `chmod` is a write.
    pub mode: u32,
    /// Inode and ctime (unix only; 0 and `None` elsewhere): a rewrite in place or a
    /// rename over the file changes them even when content and mtime are restored.
    pub ino: u64,
    pub ctime: Option<(i64, i64)>,
    /// Full content, only for the paths an exception needs to compare semantically.
    pub content: Option<Vec<u8>>,
}

/// Key of an entry: scope label and path relative to the scope root (empty for the root).
pub type Key = (String, PathBuf);

/// Snapshot of every scope at one point in time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    pub entries: BTreeMap<Key, Entry>,
}

impl Snapshot {
    /// Fingerprint `scopes`, keeping the content of the `keep` paths.
    ///
    /// Panics if a scope fails the [`guard`]: outside a testkit root, inside the GitRaptor repo
    /// or covering the real home directory.
    pub fn take(scopes: &[Scope], keep: &BTreeSet<Key>) -> Self {
        let mut snap = Self::default();
        for scope in scopes {
            let checked = if scope.system {
                guard::check_not_forbidden(&scope.root)
            } else {
                guard::check(&scope.root)
            };
            if let Err(e) = checked {
                panic!("harness guard: {e}");
            }
            if std::fs::symlink_metadata(&scope.root).is_ok() {
                walk(scope, &scope.root, keep, &mut snap.entries);
            }
        }
        snap
    }

    /// Fingerprint one directory as the scope `"root"`.
    pub fn of_dir(root: &Path) -> Self {
        Self::take(&[Scope::new("root", root)], &BTreeSet::new())
    }

    pub fn get(&self, scope: &str, path: &Path) -> Option<&Entry> {
        self.entries.get(&(scope.to_owned(), path.to_owned()))
    }
}

fn walk(scope: &Scope, path: &Path, keep: &BTreeSet<Key>, out: &mut BTreeMap<Key, Entry>) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return; // Removed while walking: the next snapshot reports it.
    };
    let rel = path.strip_prefix(&scope.root).unwrap().to_owned();
    let key = (scope.label.clone(), rel);
    let ft = meta.file_type();
    let (mode, ino, ctime) = platform_meta(&meta);
    let base = Entry {
        kind: Kind::Other,
        size: 0,
        hash: 0,
        mtime: meta.modified().ok(),
        mode,
        ino,
        ctime,
        content: None,
    };
    let entry = if ft.is_dir() {
        Entry {
            kind: Kind::Dir,
            ..base
        }
    } else if ft.is_symlink() {
        let target = std::fs::read_link(path).unwrap_or_default();
        Entry {
            kind: Kind::Symlink,
            hash: hash_bytes(target.as_os_str().as_encoded_bytes()),
            mtime: None,
            ..base
        }
    } else if ft.is_file() {
        let content = if keep.contains(&key) {
            std::fs::read(path).ok()
        } else {
            None
        };
        Entry {
            kind: Kind::File,
            size: meta.len(),
            hash: hash_file(path),
            content,
            ..base
        }
    } else {
        base
    };
    let is_dir = entry.kind == Kind::Dir;
    out.insert(key, entry);
    if is_dir && let Ok(children) = std::fs::read_dir(path) {
        for child in children.flatten() {
            walk(scope, &child.path(), keep, out);
        }
    }
}

#[cfg(unix)]
fn platform_meta(meta: &std::fs::Metadata) -> (u32, u64, Option<(i64, i64)>) {
    use std::os::unix::fs::MetadataExt;
    (
        meta.mode(),
        meta.ino(),
        Some((meta.ctime(), meta.ctime_nsec())),
    )
}

#[cfg(not(unix))]
fn platform_meta(meta: &std::fs::Metadata) -> (u32, u64, Option<(i64, i64)>) {
    (u32::from(meta.permissions().readonly()), 0, None)
}

fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = std::hash::DefaultHasher::new();
    h.write(bytes);
    h.finish()
}

/// Streamed content hash; an unreadable file hashes to a fixed sentinel.
fn hash_file(path: &Path) -> u64 {
    let Ok(mut file) = std::fs::File::open(path) else {
        return u64::MAX;
    };
    let mut h = std::hash::DefaultHasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => h.write(&buf[..n]),
            Err(_) => return u64::MAX - 1,
        }
    }
    h.finish()
}

/// What changed in one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Field {
    Kind,
    Size,
    Content,
    Mtime,
    Mode,
    Inode,
    Ctime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    Created,
    Removed,
    Modified(Vec<Field>),
}

impl ChangeKind {
    fn tag(&self) -> u8 {
        match self {
            Self::Created => 0,
            Self::Removed => 1,
            Self::Modified(_) => 2,
        }
    }

    /// Same kind of change, ignoring which fields of a modification differ.
    pub fn same_kind(&self, other: &Self) -> bool {
        self.tag() == other.tag()
    }
}

/// One difference between two snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub scope: String,
    pub path: PathBuf,
    pub kind: ChangeKind,
}

impl fmt::Display for Change {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = if self.path.as_os_str().is_empty() {
            ".".into()
        } else {
            self.path.display().to_string()
        };
        match &self.kind {
            ChangeKind::Created => write!(f, "[{}] {path}: created", self.scope),
            ChangeKind::Removed => write!(f, "[{}] {path}: removed", self.scope),
            ChangeKind::Modified(fields) => {
                write!(f, "[{}] {path}: modified ({fields:?})", self.scope)
            }
        }
    }
}

/// Wait until the file-system clock has moved past every timestamp written so far.
///
/// Kernels stamp mtime and ctime from a coarse clock (one tick: 1 to 10 ms on Linux, about
/// 15.6 ms on Windows). A lock created and deleted in the same tick as the last change before a
/// snapshot leaves its directory's mtime and ctime as they were, so the write would go unseen.
/// Call it right after the "before" snapshot: every later write then gets a newer timestamp.
pub fn wait_for_timestamp_tick() {
    let probe = tempfile::NamedTempFile::new().expect("timestamp probe");
    let stamp = |byte: u8| {
        std::fs::write(probe.path(), [byte]).expect("write timestamp probe");
        std::fs::metadata(probe.path())
            .and_then(|m| m.modified())
            .expect("timestamp probe mtime")
    };
    let first = stamp(0);
    for i in 1..=1000u32 {
        std::thread::sleep(std::time::Duration::from_millis(1));
        if stamp(i as u8) > first {
            return;
        }
    }
    panic!("file-system timestamps did not advance in one second");
}

/// Every difference between `before` and `after`, in path order.
pub fn diff(before: &Snapshot, after: &Snapshot) -> Vec<Change> {
    let mut out = Vec::new();
    for (key, b) in &before.entries {
        match after.entries.get(key) {
            None => out.push(change(key, ChangeKind::Removed)),
            Some(a) => {
                let mut fields = Vec::new();
                if a.kind != b.kind {
                    fields.push(Field::Kind);
                }
                if a.size != b.size {
                    fields.push(Field::Size);
                }
                if a.hash != b.hash {
                    fields.push(Field::Content);
                }
                if a.mtime != b.mtime {
                    fields.push(Field::Mtime);
                }
                if a.mode != b.mode {
                    fields.push(Field::Mode);
                }
                if a.ino != b.ino {
                    fields.push(Field::Inode);
                }
                if a.ctime != b.ctime {
                    fields.push(Field::Ctime);
                }
                if !fields.is_empty() {
                    out.push(change(key, ChangeKind::Modified(fields)));
                }
            }
        }
    }
    for key in after.entries.keys() {
        if !before.entries.contains_key(key) {
            out.push(change(key, ChangeKind::Created));
        }
    }
    out.sort_by(|a, b| (&a.scope, &a.path).cmp(&(&b.scope, &b.path)));
    out
}

fn change(key: &Key, kind: ChangeKind) -> Change {
    Change {
        scope: key.0.clone(),
        path: key.1.clone(),
        kind,
    }
}
