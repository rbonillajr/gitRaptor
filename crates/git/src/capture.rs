//! Reads of the user's repository for a Time Machine capture (ADR-TMC-001 § 2, TS-TMC-001).
//!
//! The index with its stat data, the untracked files without ignored ones, the conversion
//! attributes of a path and the stash. Everything here is read-only: the index is never refreshed
//! or written, no lock is taken, no program runs and the untracked cache is not used (gix walks
//! the directories itself).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use gix::bstr::{BStr, BString, ByteSlice};

use crate::{Oid, ReadError, RepoReader};

/// Stat data of a file in the working tree, compared between captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileStat {
    pub mtime_s: i64,
    pub mtime_ns: u32,
    pub ctime_s: i64,
    pub ctime_ns: u32,
    pub size: u64,
    pub ino: u64,
    pub kind: FileKind,
}

/// What a working tree path is, in Git's model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    File,
    Executable,
    Symlink,
    /// A directory, FIFO, socket or device: never captured as content.
    Other,
}

impl FileStat {
    /// From `symlink_metadata`: a symlink is described, never followed.
    pub fn of(meta: &std::fs::Metadata) -> Self {
        let ft = meta.file_type();
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let kind = if ft.is_symlink() {
                FileKind::Symlink
            } else if ft.is_file() {
                if meta.mode() & 0o111 != 0 {
                    FileKind::Executable
                } else {
                    FileKind::File
                }
            } else {
                FileKind::Other
            };
            Self {
                mtime_s: meta.mtime(),
                mtime_ns: u32::try_from(meta.mtime_nsec()).unwrap_or(0),
                ctime_s: meta.ctime(),
                ctime_ns: u32::try_from(meta.ctime_nsec()).unwrap_or(0),
                size: meta.len(),
                ino: meta.ino(),
                kind,
            }
        }
        #[cfg(not(unix))]
        {
            let split = |t: std::io::Result<std::time::SystemTime>| {
                t.ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or((0, 0), |d| {
                        (i64::try_from(d.as_secs()).unwrap_or(0), d.subsec_nanos())
                    })
            };
            let (mtime_s, mtime_ns) = split(meta.modified());
            let (ctime_s, ctime_ns) = split(meta.created());
            let kind = if ft.is_symlink() {
                FileKind::Symlink
            } else if ft.is_file() {
                FileKind::File
            } else {
                FileKind::Other
            };
            Self {
                mtime_s,
                mtime_ns,
                ctime_s,
                ctime_ns,
                size: meta.len(),
                ino: 0,
                kind,
            }
        }
    }

    /// Modified at or after `(secs, nanos)`: the stat cannot tell this file from a later write
    /// in the same tick ("racy"), so its content must be read.
    pub fn modified_since(&self, since: (i64, u32)) -> bool {
        (self.mtime_s, self.mtime_ns) >= since
    }
}

/// Kind of an index entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    Blob,
    Executable,
    Symlink,
    /// A submodule: the commit is recorded, its content is another repository.
    Gitlink,
}

impl EntryKind {
    /// The kind a working tree file is stored as, if it can be stored at all.
    pub fn of_file(kind: FileKind) -> Option<Self> {
        match kind {
            FileKind::File => Some(Self::Blob),
            FileKind::Executable => Some(Self::Executable),
            FileKind::Symlink => Some(Self::Symlink),
            FileKind::Other => None,
        }
    }
}

/// One entry of the user's index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    /// Worktree-relative path with `/` separators, as bytes.
    pub path: BString,
    pub kind: EntryKind,
    pub id: Oid,
    /// 0, or 1–3 for a conflict.
    pub stage: u8,
    pub intent_to_add: bool,
    pub skip_worktree: bool,
    stat: gix::index::entry::Stat,
}

impl IndexEntry {
    /// Whether the file on disk is the one Git staged, by Git's stat rules (mtime, size, inode,
    /// kind) and not racy against the index timestamp. Only then may its index blob stand for
    /// its content (and only if no conversion applies to it).
    pub fn matches_file(&self, file: &FileStat, index_mtime: Option<(i64, u32)>) -> bool {
        let s = &self.stat;
        // Git truncates seconds, size and inode to 32 bits on purpose.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let same = s.mtime.secs == file.mtime_s as u32
            && s.mtime.nsecs == file.mtime_ns
            && s.size == file.size as u32
            && (cfg!(not(unix)) || s.ino == file.ino as u32)
            && EntryKind::of_file(file.kind) == Some(self.kind);
        let racy = index_mtime.is_none_or(|ts| file.modified_since(ts));
        same && !racy
    }
}

/// The user's index as read now.
#[derive(Debug, Clone, Default)]
pub struct IndexView {
    pub entries: Vec<IndexEntry>,
    /// mtime of the index file: entries modified at or after it are racy.
    pub mtime: Option<(i64, u32)>,
    /// Stat of the index file.
    pub file: Option<FileStat>,
    /// Trailing checksum of the index file: two writes in the same tick differ here.
    pub checksum: Option<Oid>,
}

/// What an untracked, not ignored path is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UntrackedKind {
    File,
    Symlink,
    /// A directory with its own `.git`: never captured (TQ-15 → a).
    NestedRepo,
    /// FIFO, socket or device.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Untracked {
    pub path: BString,
    pub kind: UntrackedKind,
}

/// History the store cannot copy from this repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryGap {
    Shallow,
    PartialClone,
}

impl HistoryGap {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shallow => "shallow",
            Self::PartialClone => "partial-clone",
        }
    }
}

fn unavailable(what: &str) -> impl Fn(gix::Error) -> ReadError + '_ {
    move |e| ReadError::Unavailable(format!("{what}: {e}"))
}

impl RepoReader {
    /// Root of the working tree, if this is not a bare repository.
    pub fn workdir(&self) -> Option<PathBuf> {
        self.repo.workdir().map(ToOwned::to_owned)
    }

    /// Git directory of this worktree.
    pub fn git_dir(&self) -> &Path {
        self.repo.git_dir()
    }

    /// Common Git directory shared by every worktree.
    pub fn common_dir(&self) -> &Path {
        self.repo.common_dir()
    }

    /// The index with its stat data and marks. Never refreshed or written.
    pub fn index_view(&self) -> Result<IndexView, ReadError> {
        use gix::index::entry::{Flags, Mode};
        let index = self.repo.index_or_empty().map_err(unavailable("index"))?;
        let file = std::fs::symlink_metadata(self.repo.index_path())
            .ok()
            .map(|m| FileStat::of(&m));
        let mut entries = Vec::with_capacity(index.entries().len());
        for e in index.entries() {
            let kind = if e.mode == Mode::FILE {
                EntryKind::Blob
            } else if e.mode == Mode::FILE_EXECUTABLE {
                EntryKind::Executable
            } else if e.mode == Mode::SYMLINK {
                EntryKind::Symlink
            } else if e.mode == Mode::COMMIT {
                EntryKind::Gitlink
            } else {
                // A sparse directory entry: its files are not in the working tree.
                continue;
            };
            entries.push(IndexEntry {
                path: e.path(&index).to_owned(),
                kind,
                id: Oid(e.id),
                stage: u8::try_from(e.stage_raw()).unwrap_or(0),
                intent_to_add: e.flags.contains(Flags::INTENT_TO_ADD),
                skip_worktree: e.flags.contains(Flags::SKIP_WORKTREE),
                stat: e.stat,
            });
        }
        Ok(IndexView {
            entries,
            mtime: file.map(|f| (f.mtime_s, f.mtime_ns)),
            file,
            checksum: index.checksum().map(Oid),
        })
    }

    /// Stat and trailing checksum of the index file, without parsing it: enough to tell whether
    /// the index changed since it was last read (two writes in one tick differ in the checksum).
    pub fn index_signature(&self) -> (Option<FileStat>, Option<Oid>) {
        use std::io::{Read, Seek, SeekFrom};
        let path = self.repo.index_path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            return (None, None);
        };
        let stat = FileStat::of(&meta);
        let checksum = std::fs::File::open(&path).ok().and_then(|mut f| {
            let mut buf = [0u8; 20];
            f.seek(SeekFrom::End(-20)).ok()?;
            f.read_exact(&mut buf).ok()?;
            Some(Oid(gix::ObjectId::from_bytes_or_panic(&buf)))
        });
        (Some(stat), checksum)
    }

    /// Untracked files that are not ignored, plus nested repositories, walked by gix with the
    /// ignore rules of the repository and the user. Tracked paths are never emitted.
    pub fn untracked(&self) -> Result<Vec<Untracked>, ReadError> {
        use gix::dir::entry::{Kind, Status};
        use gix::dir::walk::EmissionMode;
        let index = self.repo.index_or_empty().map_err(unavailable("index"))?;
        // In memory only: every entry counts as tracked for the walk.
        let mut state: gix::index::State = (**index).clone().into();
        for e in state.entries_mut() {
            e.flags.insert(gix::index::entry::Flags::UPTODATE);
        }
        let options = self
            .repo
            .dirwalk_options()
            .map_err(unavailable("dirwalk"))?
            .emit_untracked(EmissionMode::Matching)
            .emit_tracked(false)
            .emit_ignored(None)
            .emit_pruned(false)
            .emit_empty_directories(false)
            .recurse_repositories(false)
            .classify_untracked_bare_repositories(true);
        let mut collect = gix::dir::walk::delegate::Collect::default();
        let interrupt = AtomicBool::new(false);
        self.repo
            .dirwalk(&state, None::<&BStr>, &interrupt, options, &mut collect)
            .map_err(unavailable("dirwalk"))?;
        let mut out = Vec::new();
        for (entry, _) in collect.into_entries_by_path() {
            if entry.status != Status::Untracked {
                continue;
            }
            let kind = match entry.disk_kind {
                Some(Kind::File) => UntrackedKind::File,
                Some(Kind::Symlink) => UntrackedKind::Symlink,
                Some(Kind::Repository) => UntrackedKind::NestedRepo,
                Some(Kind::Untrackable) => UntrackedKind::Other,
                // A bare repository inside the worktree is classified as a directory.
                Some(Kind::Directory) | None => UntrackedKind::NestedRepo,
            };
            out.push(Untracked {
                path: entry.rela_path,
                kind,
            });
        }
        Ok(out)
    }

    /// A reusable checker of conversion attributes (ADR-TMC-001 § 2).
    pub fn conversions(&self) -> Result<Conversions<'_>, ReadError> {
        let index = self.repo.index_or_empty().map_err(unavailable("index"))?;
        let stack = self
            .repo
            .attributes_only(
                &index,
                gix::worktree::stack::state::attributes::Source::WorktreeThenIdMapping,
            )
            .map_err(unavailable("attributes"))?;
        let outcome = stack.selected_attribute_matches(CONVERSION_ATTRIBUTES);
        let autocrlf = self
            .repo
            .config_snapshot()
            .string("core.autocrlf")
            .is_some_and(|v| {
                let v = v.to_str_lossy().to_ascii_lowercase();
                matches!(v.as_str(), "true" | "input" | "yes" | "on" | "1")
            });
        Ok(Conversions {
            stack,
            outcome,
            autocrlf,
        })
    }

    /// A reusable checker of the ignore rules, for paths reported by the engine.
    pub fn ignore_check(&self) -> Result<IgnoreCheck<'_>, ReadError> {
        let index = self.repo.index_or_empty().map_err(unavailable("index"))?;
        let excludes = self
            .repo
            .excludes(
                &index,
                None,
                gix::worktree::stack::state::ignore::Source::WorktreeThenIdMappingIfNotSkipped,
            )
            .map_err(unavailable("ignore rules"))?;
        Ok(IgnoreCheck { excludes })
    }

    /// Files whose change alters the ignore rules without an event in the worktree:
    /// `info/exclude` and `core.excludesFile`.
    pub fn ignore_sources(&self) -> Vec<PathBuf> {
        let mut out = vec![self.repo.common_dir().join("info").join("exclude")];
        if let Ok(Some(p)) = self
            .repo
            .config_snapshot()
            .trusted_path("core.excludesFile")
        {
            out.push(p);
        }
        out
    }

    /// Local branches and the object each one names, read from the refs alone: no object is
    /// looked up, so the pack indexes are not loaded (the capture's detection budget is 5 ms,
    /// ADR-TMC-006 § 2). A symbolic branch is followed.
    pub fn branch_tips(&self) -> Result<Vec<(String, Oid)>, ReadError> {
        let refs = self.repo.references().map_err(unavailable("refs"))?;
        let mut out = Vec::new();
        for r in refs.local_branches().map_err(unavailable("refs"))? {
            let mut r = r.map_err(|e| ReadError::Unavailable(format!("refs: {e}")))?;
            let name = r.name().shorten().to_str_lossy().into_owned();
            let id = match r.target().try_id() {
                Some(id) => id.to_owned(),
                None => r.peel_to_id().map_err(unavailable("refs"))?.detach(),
            };
            out.push((name, Oid(id)));
        }
        Ok(out)
    }

    /// Where `HEAD` points, read from the refs alone (no object lookup): branch name, the object
    /// it names, and whether it is detached.
    pub fn head_tip(&self) -> Result<(Option<String>, Option<Oid>, bool), ReadError> {
        use gix::head::Kind;
        let head = self.repo.head().map_err(unavailable("HEAD"))?;
        Ok(match &head.kind {
            Kind::Symbolic(r) => (
                Some(r.name.shorten().to_str_lossy().into_owned()),
                r.target.try_id().map(|id| Oid(id.to_owned())),
                false,
            ),
            Kind::Detached { target, .. } => (None, Some(Oid(*target)), true),
            Kind::Unborn(name) => (
                Some(name.shorten().to_str_lossy().into_owned()),
                None,
                false,
            ),
        })
    }

    /// Commit at the tip of `refs/stash`, if any.
    pub fn stash(&self) -> Result<Option<Oid>, ReadError> {
        let Some(mut r) = self
            .repo
            .try_find_reference("refs/stash")
            .map_err(unavailable("refs"))?
        else {
            return Ok(None);
        };
        Ok(Some(Oid(r
            .peel_to_id()
            .map_err(unavailable("refs"))?
            .detach())))
    }

    /// History the store cannot copy: a shallow or partial clone.
    pub fn history_gaps(&self) -> Vec<HistoryGap> {
        let mut gaps = Vec::new();
        if self.repo.is_shallow() {
            gaps.push(HistoryGap::Shallow);
        }
        let config = self.repo.config_snapshot();
        let promisor = config.string("extensions.partialClone").is_some()
            || config
                .sections_by_name("remote")
                .into_iter()
                .flatten()
                .any(|s| s.value("promisor").is_some());
        if promisor {
            gaps.push(HistoryGap::PartialClone);
        }
        gaps
    }
}

const CONVERSION_ATTRIBUTES: [&str; 6] = [
    "text",
    "eol",
    "crlf",
    "filter",
    "ident",
    "working-tree-encoding",
];

/// Answers whether Git would convert a path between the working tree and the index, so its
/// index blob is not its bytes on disk.
pub struct Conversions<'r> {
    stack: gix::AttributeStack<'r>,
    outcome: gix::attrs::search::Outcome,
    autocrlf: bool,
}

impl Conversions<'_> {
    pub fn converts(&mut self, rela_path: &BStr) -> Result<bool, ReadError> {
        use gix::attrs::StateRef;
        let platform = self
            .stack
            .at_entry(rela_path, Some(gix::index::entry::Mode::FILE))
            .map_err(|e| ReadError::Unavailable(format!("attributes: {e}")))?;
        platform.matching_attributes(&mut self.outcome);
        let mut converts = false;
        let mut text_unset = false;
        for m in self.outcome.iter_selected() {
            let name = m.assignment.name.as_str();
            match (name, m.assignment.state) {
                (_, StateRef::Unspecified) => {}
                ("text", StateRef::Unset) => text_unset = true,
                (_, StateRef::Unset) => {}
                (_, StateRef::Set | StateRef::Value(_)) => converts = true,
            }
        }
        Ok(converts || (self.autocrlf && !text_unset))
    }
}

/// The ignore rules of a worktree.
pub struct IgnoreCheck<'r> {
    excludes: gix::AttributeStack<'r>,
}

impl IgnoreCheck<'_> {
    pub fn is_ignored(&mut self, rela_path: &BStr, is_dir: bool) -> Result<bool, ReadError> {
        let mode = if is_dir {
            gix::index::entry::Mode::DIR
        } else {
            gix::index::entry::Mode::FILE
        };
        let platform = self
            .excludes
            .at_entry(rela_path, Some(mode))
            .map_err(|e| ReadError::Unavailable(format!("ignore rules: {e}")))?;
        Ok(platform.is_excluded())
    }
}
