//! The diff of the index of a worktree, for `diff.index`: staged (`HEAD` against the index),
//! unstaged (the index against the working tree, untracked included), the content id of each
//! side and the conflicted paths (DS-TS-CKP-006 D7, D8).
//!
//! Everything is read with gitoxide on raw bytes: the working tree id is the Git blob hash of the
//! bytes on disk, with no `clean` filter or end-of-line conversion. The index is never written.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use gix::bstr::ByteSlice;

use super::{
    CutKind, DiffLimits, GitChange, GitSection, Side, build_section, change_hint, components,
    read_worktree,
};
use crate::{BlobRead, CommittedFile, ReadError, RepoReader, Status};

/// The content id of each side of a file: a lowercase hex blob oid, or `None` when the side is
/// absent, too large, a link, a submodule or unreadable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitContentIds {
    pub old: Option<String>,
    pub new: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIndexSection {
    pub section: GitSection,
    /// By path, from the same read of the index as the section (L-02). Only the files listed in
    /// `section` have an entry.
    pub ids: BTreeMap<String, GitContentIds>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitIndexDiff {
    pub staged: GitIndexSection,
    pub unstaged: GitIndexSection,
    /// Paths with entries of stages 1 to 3, sorted, at most `max_files`; out of both sections.
    pub conflicts: Vec<String>,
    pub conflicts_total: usize,
}

/// The stage 0 entry of a path.
#[derive(Clone, Copy)]
enum IndexEntry {
    Blob(gix::ObjectId),
    Link,
    Submodule,
}

/// One read of the index: what the sections and the ids are built from.
struct IndexRead {
    checksum: Option<gix::ObjectId>,
    entries: BTreeMap<String, IndexEntry>,
    conflicts: BTreeSet<String>,
}

/// One side read with its content id.
struct Read {
    side: Side,
    id: Option<String>,
}

impl Read {
    fn committed(file: CommittedFile) -> Self {
        let id = match &file {
            CommittedFile::Blob { id, .. } => Some(id.clone()),
            _ => None,
        };
        Self {
            side: Side::from_committed(file),
            id,
        }
    }

    fn absent() -> Self {
        Self {
            side: Side::Absent,
            id: None,
        }
    }
}

impl RepoReader {
    /// The index of this worktree against `HEAD` and against the working tree.
    ///
    /// # Errors
    ///
    /// [`ReadError::TemporarilyUnavailable`] when the index keeps changing while it is read;
    /// [`ReadError::InvalidInput`] when the reader has no working tree; [`ReadError`] when the
    /// repository cannot be read.
    pub fn index_diff(
        &self,
        limits: &DiffLimits,
        stop: &dyn Fn() -> bool,
    ) -> Result<GitIndexDiff, ReadError> {
        let workdir =
            self.repo.workdir().map(Path::to_path_buf).ok_or_else(|| {
                ReadError::InvalidInput("the repository has no working tree".into())
            })?;
        // L-02: the ids and the membership of each section come from the same index. The
        // checksum is read again after the status; if it changed, both are read once more, and a
        // second change is TemporarilyUnavailable: never a mix of two indexes.
        let mut attempt = 0;
        let (index, status) = loop {
            let index = self.read_index()?;
            let status = self.status()?;
            if self.read_index()?.checksum == index.checksum {
                break (index, status);
            }
            attempt += 1;
            if attempt == 2 {
                return Err(ReadError::TemporarilyUnavailable(
                    "the index changed while it was read".into(),
                ));
            }
        };
        let head = self.head()?.commit;
        let max = limits.max_blob_bytes;
        let index_side = |path: &str| -> Result<Read, ReadError> {
            match index.entries.get(path) {
                None => Ok(Read::absent()),
                Some(IndexEntry::Link) => Ok(Read {
                    side: Side::Link,
                    id: None,
                }),
                Some(IndexEntry::Submodule) => Ok(Read {
                    side: Side::Submodule,
                    id: None,
                }),
                Some(IndexEntry::Blob(id)) => {
                    let hex = id.to_string();
                    Ok(match self.blob_by_id(&hex, max)? {
                        BlobRead::Blob { bytes } => Read {
                            side: Side::Blob(bytes),
                            id: Some(hex),
                        },
                        BlobRead::TooLarge { .. } => Read {
                            side: Side::TooLarge,
                            id: None,
                        },
                        BlobRead::Missing | BlobRead::NotABlob => Read {
                            side: Side::Unreadable,
                            id: None,
                        },
                    })
                }
            }
        };
        let hash_kind = self.repo.object_hash();
        let worktree_side = |comps: &[&str]| -> Read {
            match read_worktree(&workdir, comps, max) {
                // ⛔4.1: the hash of the raw bytes, with no filter: never `git hash-object`.
                Side::Blob(bytes) => {
                    let id = gix::objs::compute_hash(hash_kind, gix::objs::Kind::Blob, &bytes)
                        .ok()
                        .map(|id| id.to_string());
                    Read {
                        side: Side::Blob(bytes),
                        id,
                    }
                }
                side => Read { side, id: None },
            }
        };

        let (staged_hints, unstaged_hints) = split(&status, &index.conflicts);
        let mut budget = limits.max_bytes / 2;
        let staged = section(&staged_hints, limits, stop, &mut budget, |path| {
            let comps = components(path)?;
            let old = match head.as_deref() {
                Some(head) => Read::committed(self.committed_file(head, &comps, max)?),
                None => Read::absent(),
            };
            Ok((old, index_side(path)?))
        })?;
        let mut budget = limits
            .max_bytes
            .saturating_sub(limits.max_bytes / 2 - budget);
        let unstaged = section(&unstaged_hints, limits, stop, &mut budget, |path| {
            let comps = components(path)?;
            Ok((index_side(path)?, worktree_side(&comps)))
        })?;
        let conflicts_total = index.conflicts.len();
        let conflicts = index.conflicts.into_iter().take(limits.max_files).collect();
        Ok(GitIndexDiff {
            staged,
            unstaged,
            conflicts,
            conflicts_total,
        })
    }

    fn read_index(&self) -> Result<IndexRead, ReadError> {
        let index = self
            .repo
            .index_or_empty()
            .map_err(|e| ReadError::Unavailable(format!("index: {e}")))?;
        let mut entries = BTreeMap::new();
        let mut conflicts = BTreeSet::new();
        for entry in index.entries() {
            let path = entry.path(&index).to_str_lossy().into_owned();
            if entry.stage() != gix::index::entry::Stage::Unconflicted {
                conflicts.insert(path);
                continue;
            }
            let kind = if entry.mode.is_submodule() {
                IndexEntry::Submodule
            } else if entry.mode == gix::index::entry::Mode::SYMLINK {
                IndexEntry::Link
            } else {
                IndexEntry::Blob(entry.id)
            };
            entries.insert(path, kind);
        }
        for path in &conflicts {
            entries.remove(path);
        }
        Ok(IndexRead {
            checksum: index.checksum(),
            entries,
            conflicts,
        })
    }
}

/// The status split in the two sections, conflicted paths out of both (US-DSK-014).
fn split(
    status: &Status,
    conflicts: &BTreeSet<String>,
) -> (BTreeMap<String, GitChange>, BTreeMap<String, GitChange>) {
    let mut staged = BTreeMap::new();
    let mut unstaged = BTreeMap::new();
    for c in status.staged.iter().filter(|c| !conflicts.contains(&c.path)) {
        staged.insert(c.path.clone(), change_hint(c.kind));
    }
    for c in status.unstaged.iter().filter(|c| !conflicts.contains(&c.path)) {
        unstaged.insert(c.path.clone(), change_hint(c.kind));
    }
    for path in status.untracked.iter().filter(|p| !conflicts.contains(*p)) {
        unstaged.insert(path.clone(), GitChange::Added);
    }
    (staged, unstaged)
}

/// One section and the content ids of the files it lists.
fn section(
    hints: &BTreeMap<String, GitChange>,
    limits: &DiffLimits,
    stop: &dyn Fn() -> bool,
    budget: &mut usize,
    mut load: impl FnMut(&str) -> Result<(Read, Read), ReadError>,
) -> Result<GitIndexSection, ReadError> {
    let total = hints.len();
    let paths: Vec<String> = hints.keys().take(limits.max_files).cloned().collect();
    let cut = (total > paths.len()).then_some(CutKind::Files);
    let mut ids = BTreeMap::new();
    let section = build_section(
        &paths,
        total,
        cut,
        limits,
        stop,
        budget,
        |path| {
            let (old, new) = load(path)?;
            ids.insert(
                path.to_owned(),
                GitContentIds {
                    old: old.id,
                    new: new.id,
                },
            );
            Ok((old.side, new.side))
        },
        |path| hints.get(path).copied().unwrap_or(GitChange::Modified),
    )?;
    ids.retain(|path, _| section.files.iter().any(|f| &f.path == path));
    Ok(GitIndexSection { section, ids })
}
