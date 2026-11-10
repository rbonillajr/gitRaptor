//! The diff of a worktree for the cockpit: what the branch would integrate and what the worktree
//! has not committed (ADR-GRP-009 § 1).
//!
//! Everything is read with gitoxide, on bytes, with no filter, `textconv`, external diff or
//! child `git`. The working tree is opened component by component without following links, and
//! a link is reported as a link: its target is never read. Nothing here writes to the repository.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::Path;
use std::time::Instant;

use gix::diff::blob::{Algorithm, Diff, InternedInput};

use crate::{ChangeKind, CommittedFile, NotRegular, ReadError, RefName, RepoReader};

/// Context lines around a hunk (Git's default).
const CONTEXT: u32 = 3;
/// Bytes looked at for a NUL, to call a file binary (Git's heuristic).
const BINARY_PROBE: usize = 8_000;
/// Rough serialized cost of a file entry before its lines, and of each line (budget only; the
/// caller measures the real serialization).
const FILE_OVERHEAD: usize = 96;
const LINE_OVERHEAD: usize = 48;

/// What to compare the branch against.
#[derive(Debug, Clone, Copy)]
pub struct DiffRequest<'a> {
    /// Full ref of the base branch; `None` when the repo has no base.
    pub base: Option<&'a RefName>,
    /// The developer confirmed the base. Without it the committed section is not computed.
    pub base_confirmed: bool,
}

/// Caps of one query.
#[derive(Debug, Clone, Copy)]
pub struct DiffLimits {
    /// Files listed per section.
    pub max_files: usize,
    /// Diff lines kept per file.
    pub max_lines_per_file: usize,
    /// Largest blob or file whose content is read.
    pub max_blob_bytes: u64,
    /// Rough size of the content of the whole answer.
    pub max_bytes: usize,
    /// When the query runs out of time.
    pub deadline: Instant,
}

/// Why the committed section was not computed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommittedGapKind {
    BaseUnconfirmed,
    NoBase,
    BaseMissing,
    NoMergeBase,
    /// Listing the changed paths went past the deadline.
    TooSlow,
}

/// Which cap cut a section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutKind {
    Files,
    Bytes,
    Time,
}

/// How a file changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitChange {
    Added,
    Modified,
    Deleted,
    TypeChanged,
}

/// Kind of a diff line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitLineKind {
    Context,
    Added,
    Removed,
}

/// One line of a hunk: the bytes as the file has them, lossily read as text, without the final `\n`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitLine {
    pub kind: GitLineKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHunk {
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    pub lines: Vec<GitLine>,
}

/// What a file shows: only `Text` carries content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitBody {
    Text {
        hunks: Vec<GitHunk>,
        truncated: bool,
    },
    Binary,
    TooLarge,
    Submodule,
    Symlink,
    /// A cap of the section was reached before this file.
    Omitted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFileDiff {
    /// Relative to the worktree root, `/`-separated, lossily read as text.
    pub path: String,
    pub change: GitChange,
    pub added: u32,
    pub removed: u32,
    pub body: GitBody,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitSection {
    /// In byte order of the path.
    pub files: Vec<GitFileDiff>,
    /// Every changed file, also the ones that did not fit.
    pub files_total: usize,
    pub cut: Option<CutKind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitWorktreeDiff {
    pub committed: Result<GitSection, CommittedGapKind>,
    pub uncommitted: GitSection,
}

/// One side of a file.
enum Side {
    Absent,
    /// A symbolic link, in the tree or in the working tree. Its target is never read.
    Link,
    /// A submodule, or a folder where the worktree had a file.
    Submodule,
    TooLarge,
    /// A file that could not be opened or read now.
    Unreadable,
    Blob(Vec<u8>),
}

impl Side {
    fn from_committed(file: CommittedFile) -> Self {
        match file {
            CommittedFile::Absent | CommittedFile::NotRegular(NotRegular::WrongKind) => {
                Self::Absent
            }
            CommittedFile::NotRegular(NotRegular::Symlink) => Self::Link,
            CommittedFile::NotRegular(NotRegular::Submodule) => Self::Submodule,
            CommittedFile::TooLarge { .. } => Self::TooLarge,
            CommittedFile::Blob { bytes, .. } => Self::Blob(bytes),
        }
    }

    fn class(&self) -> u8 {
        match self {
            Self::Absent => 0,
            Self::Link => 1,
            Self::Submodule => 2,
            Self::TooLarge | Self::Unreadable | Self::Blob(_) => 3,
        }
    }
}

impl RepoReader {
    /// The two sections of the diff of the worktree this reader was opened on.
    ///
    /// *Committed*: `HEAD` against its first merge-base with the confirmed base. *Uncommitted*:
    /// `HEAD` against the working tree, staged and unstaged together, untracked files that are
    /// not ignored included as added. There is no rename detection: a rename is a deletion and
    /// an addition.
    ///
    /// `stop` is asked between files; when it says `true` the rest of the files come out as
    /// omitted and the caller is expected to drop the result. A section that passes the
    /// deadline, the file cap or the byte budget says so in `cut`: a cap is never an error.
    ///
    /// # Errors
    /// [`ReadError`] when the repository, `HEAD` or the status cannot be read, or when the reader
    /// has no working tree.
    pub fn worktree_diff(
        &self,
        req: &DiffRequest<'_>,
        limits: &DiffLimits,
        stop: &dyn Fn() -> bool,
    ) -> Result<GitWorktreeDiff, ReadError> {
        let workdir =
            self.repo.workdir().map(Path::to_path_buf).ok_or_else(|| {
                ReadError::InvalidInput("the repository has no working tree".into())
            })?;
        let head = self.head()?.commit;
        // Each section gets its share of the budget; what the committed one leaves, the
        // uncommitted one keeps.
        let mut budget = limits.max_bytes / 2;
        let committed = self.committed_section(req, limits, stop, head.as_deref(), &mut budget)?;
        let mut budget = limits
            .max_bytes
            .saturating_sub(limits.max_bytes / 2 - budget);
        let uncommitted =
            self.uncommitted_section(&workdir, limits, stop, head.as_deref(), &mut budget)?;
        Ok(GitWorktreeDiff {
            committed,
            uncommitted,
        })
    }

    fn committed_section(
        &self,
        req: &DiffRequest<'_>,
        limits: &DiffLimits,
        stop: &dyn Fn() -> bool,
        head: Option<&str>,
        budget: &mut usize,
    ) -> Result<Result<GitSection, CommittedGapKind>, ReadError> {
        use CommittedGapKind as Gap;
        let Some(base) = req.base else {
            return Ok(Err(Gap::NoBase));
        };
        if !req.base_confirmed {
            return Ok(Err(Gap::BaseUnconfirmed));
        }
        let Some(base_tip) = self.resolve_ref(base)? else {
            return Ok(Err(Gap::BaseMissing));
        };
        let Some(head) = head else {
            return Ok(Err(Gap::NoMergeBase));
        };
        let Some(merge_base) = self.merge_bases(&base_tip, head)?.into_iter().next() else {
            return Ok(Err(Gap::NoMergeBase));
        };
        let listed =
            match self.changed_paths(Some(&merge_base), head, limits.max_files, limits.deadline) {
                Ok(listed) => listed,
                // The only wait that ends in this variant is the deadline: never an empty list.
                Err(ReadError::TemporarilyUnavailable(_)) => return Ok(Err(Gap::TooSlow)),
                Err(e) => return Err(e),
            };
        let total = listed.total as usize;
        let cut = (total > listed.paths.len()).then_some(CutKind::Files);
        let max = limits.max_blob_bytes;
        let section = build_section(
            &listed.paths,
            total,
            cut,
            limits,
            stop,
            budget,
            |path| {
                let comps = components(path)?;
                let old = self.committed_file(&merge_base, &comps, max)?;
                let new = self.committed_file(head, &comps, max)?;
                Ok((Side::from_committed(old), Side::from_committed(new)))
            },
            |path| {
                // Presence only: a size limit of zero never loads a blob.
                let probe = |commit: &str| {
                    components(path)
                        .and_then(|c| self.committed_file(commit, &c, 0))
                        .map_or(Side::Absent, Side::from_committed)
                };
                change_of(&probe(&merge_base), &probe(head)).unwrap_or(GitChange::Modified)
            },
        )?;
        Ok(Ok(section))
    }

    fn uncommitted_section(
        &self,
        workdir: &Path,
        limits: &DiffLimits,
        stop: &dyn Fn() -> bool,
        head: Option<&str>,
        budget: &mut usize,
    ) -> Result<GitSection, ReadError> {
        let status = self.status()?;
        let mut hints: BTreeMap<String, GitChange> = BTreeMap::new();
        for c in &status.staged {
            hints.insert(c.path.clone(), change_hint(c.kind));
        }
        for c in &status.unstaged {
            let hint = change_hint(c.kind);
            hints
                .entry(c.path.clone())
                .and_modify(|h| {
                    if *h != GitChange::Added {
                        *h = hint;
                    }
                })
                .or_insert(hint);
        }
        for path in &status.untracked {
            hints.insert(path.clone(), GitChange::Added);
        }
        let total = hints.len();
        let paths: Vec<String> = hints.keys().take(limits.max_files).cloned().collect();
        let cut = (total > paths.len()).then_some(CutKind::Files);
        let max = limits.max_blob_bytes;
        build_section(
            &paths,
            total,
            cut,
            limits,
            stop,
            budget,
            |path| {
                let comps = components(path)?;
                let old = match head {
                    Some(head) => Side::from_committed(self.committed_file(head, &comps, max)?),
                    None => Side::Absent,
                };
                Ok((old, read_worktree(workdir, &comps, max)))
            },
            |path| hints.get(path).copied().unwrap_or(GitChange::Modified),
        )
    }
}

fn change_hint(kind: ChangeKind) -> GitChange {
    match kind {
        ChangeKind::Added => GitChange::Added,
        ChangeKind::Deleted => GitChange::Deleted,
        ChangeKind::TypeChanged => GitChange::TypeChanged,
        ChangeKind::Modified | ChangeKind::Conflicted => GitChange::Modified,
    }
}

/// The components of a `/`-separated path, refused when one cannot be opened safely.
fn components(path: &str) -> Result<Vec<&str>, ReadError> {
    let comps: Vec<&str> = path.split('/').collect();
    if comps
        .iter()
        .any(|c| c.is_empty() || *c == "." || *c == ".." || c.contains('\\'))
    {
        return Err(ReadError::InvalidInput("invalid path".into()));
    }
    Ok(comps)
}

fn change_of(old: &Side, new: &Side) -> Option<GitChange> {
    match (old, new) {
        (Side::Absent, Side::Absent) => None,
        (Side::Absent, _) => Some(GitChange::Added),
        (_, Side::Absent) => Some(GitChange::Deleted),
        (o, n) if o.class() == n.class() => Some(GitChange::Modified),
        _ => Some(GitChange::TypeChanged),
    }
}

fn omitted(path: &str, change: GitChange) -> GitFileDiff {
    GitFileDiff {
        path: path.to_owned(),
        change,
        added: 0,
        removed: 0,
        body: GitBody::Omitted,
    }
}

fn cost(file: &GitFileDiff) -> usize {
    let GitBody::Text { hunks, .. } = &file.body else {
        return file.path.len() + FILE_OVERHEAD;
    };
    file.path.len()
        + FILE_OVERHEAD
        + hunks
            .iter()
            .flat_map(|h| &h.lines)
            .map(|l| l.text.len() + LINE_OVERHEAD)
            .sum::<usize>()
}

/// Builds a section from its paths (sorted, at most `max_files`). `load` reads both sides of a
/// path; `hint` says how an omitted file changed. Past the deadline, the stop signal or the
/// byte budget, the paths left come out as omitted.
#[allow(clippy::too_many_arguments)]
fn build_section(
    paths: &[String],
    total: usize,
    mut cut: Option<CutKind>,
    limits: &DiffLimits,
    stop: &dyn Fn() -> bool,
    budget: &mut usize,
    mut load: impl FnMut(&str) -> Result<(Side, Side), ReadError>,
    hint: impl Fn(&str) -> GitChange,
) -> Result<GitSection, ReadError> {
    let mut files = Vec::with_capacity(paths.len());
    let mut identical = 0usize;
    let mut omit = false;
    for path in paths {
        if !omit && (stop() || Instant::now() >= limits.deadline) {
            omit = true;
            cut = Some(CutKind::Time);
        }
        if omit {
            files.push(omitted(path, hint(path)));
            continue;
        }
        let (old, new) = match load(path) {
            Ok(sides) => sides,
            Err(ReadError::InvalidInput(_)) => {
                files.push(omitted(path, hint(path)));
                continue;
            }
            Err(e) => return Err(e),
        };
        let Some(file) = file_diff(path, &old, &new, limits) else {
            identical += 1;
            continue;
        };
        let size = cost(&file);
        if size > *budget {
            omit = true;
            cut = Some(CutKind::Bytes);
            files.push(omitted(path, file.change));
        } else {
            *budget -= size;
            files.push(file);
        }
    }
    Ok(GitSection {
        files,
        files_total: total.saturating_sub(identical),
        cut,
    })
}

/// The diff of one file, or `None` when both sides are the same or absent.
fn file_diff(path: &str, old: &Side, new: &Side, limits: &DiffLimits) -> Option<GitFileDiff> {
    let change = change_of(old, new)?;
    let body = |body| GitFileDiff {
        path: path.to_owned(),
        change,
        added: 0,
        removed: 0,
        body,
    };
    if matches!(old, Side::Link) || matches!(new, Side::Link) {
        return Some(body(GitBody::Symlink));
    }
    if matches!(old, Side::Submodule) || matches!(new, Side::Submodule) {
        return Some(body(GitBody::Submodule));
    }
    if matches!(old, Side::Unreadable) || matches!(new, Side::Unreadable) {
        return Some(body(GitBody::Omitted));
    }
    if matches!(old, Side::TooLarge) || matches!(new, Side::TooLarge) {
        return Some(body(GitBody::TooLarge));
    }
    fn bytes(side: &Side) -> &[u8] {
        match side {
            Side::Blob(b) => b.as_slice(),
            _ => &[],
        }
    }
    let (old, new) = (bytes(old), bytes(new));
    if matches!(change, GitChange::Modified) && old == new {
        return None;
    }
    if is_binary(old) || is_binary(new) {
        return Some(body(GitBody::Binary));
    }
    let (hunks, added, removed, truncated) = text_diff(old, new, limits.max_lines_per_file);
    Some(GitFileDiff {
        path: path.to_owned(),
        change,
        added,
        removed,
        body: GitBody::Text { hunks, truncated },
    })
}

/// A NUL in the first 8.000 bytes (Git's heuristic).
fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(BINARY_PROBE)].contains(&0)
}

fn split_lines(bytes: &[u8]) -> Vec<&[u8]> {
    bytes.split_inclusive(|b| *b == b'\n').collect()
}

fn line_text(line: &[u8]) -> String {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    String::from_utf8_lossy(line).into_owned()
}

/// Hunks with their context, how many lines were added and removed in all, and whether the line
/// cap cut the file.
fn text_diff(old: &[u8], new: &[u8], max_lines: usize) -> (Vec<GitHunk>, u32, u32, bool) {
    let (old_lines, new_lines) = (split_lines(old), split_lines(new));
    let to_u32 = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    let raw: Vec<(Range<u32>, Range<u32>)> = if old_lines.is_empty() && new_lines.is_empty() {
        Vec::new()
    } else if old_lines.is_empty() {
        vec![(0..0, 0..to_u32(new_lines.len()))]
    } else if new_lines.is_empty() {
        vec![(0..to_u32(old_lines.len()), 0..0)]
    } else {
        let input = InternedInput::new(old, new);
        Diff::compute(Algorithm::Histogram, &input)
            .hunks()
            .map(|h| (h.before, h.after))
            .collect()
    };
    let removed = raw.iter().map(|(b, _)| b.end - b.start).sum();
    let added = raw.iter().map(|(_, a)| a.end - a.start).sum();
    let (hunks, truncated) = build_hunks(&old_lines, &new_lines, &raw, max_lines);
    (hunks, added, removed, truncated)
}

fn build_hunks(
    old: &[&[u8]],
    new: &[&[u8]],
    raw: &[(Range<u32>, Range<u32>)],
    max_lines: usize,
) -> (Vec<GitHunk>, bool) {
    let mut hunks = Vec::new();
    let mut emitted = 0usize;
    let mut i = 0;
    while i < raw.len() {
        let mut j = i;
        while j + 1 < raw.len() && raw[j + 1].0.start - raw[j].0.end <= 2 * CONTEXT {
            j += 1;
        }
        let first = &raw[i];
        let from = first.0.start.saturating_sub(CONTEXT);
        let new_from = first.1.start.saturating_sub(first.0.start - from);
        let mut lines: Vec<GitLine> = Vec::new();
        let mut full = false;
        let mut push = |kind, line: &[u8], lines: &mut Vec<GitLine>| {
            if emitted >= max_lines {
                full = true;
                return;
            }
            emitted += 1;
            lines.push(GitLine {
                kind,
                text: line_text(line),
            });
        };
        let mut at = from;
        for (before, after) in &raw[i..=j] {
            for l in at..before.start {
                push(GitLineKind::Context, old[l as usize], &mut lines);
            }
            for l in before.clone() {
                push(GitLineKind::Removed, old[l as usize], &mut lines);
            }
            for l in after.clone() {
                push(GitLineKind::Added, new[l as usize], &mut lines);
            }
            at = before.end;
        }
        let end = (raw[j].0.end.saturating_add(CONTEXT) as usize).min(old.len());
        for line in old.iter().take(end).skip(at as usize) {
            push(GitLineKind::Context, line, &mut lines);
        }
        let count = |f: fn(GitLineKind) -> bool| {
            u32::try_from(lines.iter().filter(|l| f(l.kind)).count()).unwrap_or(u32::MAX)
        };
        let old_len = count(|k| k != GitLineKind::Added);
        let new_len = count(|k| k != GitLineKind::Removed);
        if !lines.is_empty() {
            hunks.push(GitHunk {
                old_start: if old_len == 0 { from } else { from + 1 },
                old_len,
                new_start: if new_len == 0 { new_from } else { new_from + 1 },
                new_len,
                lines,
            });
        }
        if full {
            return (hunks, true);
        }
        i = j + 1;
    }
    (hunks, false)
}

/// The file at `comps` under `root`, opened component by component without following links.
#[cfg(unix)]
fn read_worktree(root: &Path, comps: &[&str], max: u64) -> Side {
    use std::io::Read;

    use rustix::fs::{FileType, Mode, OFlags, fstat, open, openat};
    use rustix::io::Errno;

    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
    let Ok(mut dir) = open(
        root,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    ) else {
        return Side::Unreadable;
    };
    for (n, comp) in comps.iter().enumerate() {
        let fd = match openat(&dir, *comp, flags, Mode::empty()) {
            Ok(fd) => fd,
            Err(Errno::LOOP) => return Side::Link,
            Err(Errno::NOENT | Errno::NOTDIR) => return Side::Absent,
            Err(_) => return Side::Unreadable,
        };
        let Ok(stat) = fstat(&fd) else {
            return Side::Unreadable;
        };
        let kind = FileType::from_raw_mode(stat.st_mode);
        if n + 1 < comps.len() {
            if kind != FileType::Directory {
                return Side::Absent;
            }
            dir = fd;
            continue;
        }
        return match kind {
            FileType::RegularFile => {
                // The size comes from the descriptor that is read, before reading.
                if u64::try_from(stat.st_size).map_or(true, |size| size > max) {
                    return Side::TooLarge;
                }
                let mut bytes = Vec::new();
                match std::fs::File::from(fd)
                    .take(max.saturating_add(1))
                    .read_to_end(&mut bytes)
                {
                    Ok(_) if bytes.len() as u64 > max => Side::TooLarge,
                    Ok(_) => Side::Blob(bytes),
                    Err(_) => Side::Unreadable,
                }
            }
            FileType::Directory => Side::Submodule,
            _ => Side::Absent,
        };
    }
    Side::Absent
}

/// Outside Unix: `symlink_metadata` of each component, then open. The window between the check
/// and the open is a known residue of this platform.
#[cfg(not(unix))]
fn read_worktree(root: &Path, comps: &[&str], max: u64) -> Side {
    use std::io::Read;

    let mut path = root.to_path_buf();
    for (n, comp) in comps.iter().enumerate() {
        path.push(comp);
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Side::Absent,
            Err(_) => return Side::Unreadable,
        };
        if meta.file_type().is_symlink() {
            return Side::Link;
        }
        if n + 1 < comps.len() {
            if !meta.is_dir() {
                return Side::Absent;
            }
            continue;
        }
        if meta.is_dir() {
            return Side::Submodule;
        }
        if !meta.is_file() {
            return Side::Absent;
        }
    }
    let Ok(file) = std::fs::File::open(&path) else {
        return Side::Unreadable;
    };
    match file.metadata() {
        Ok(meta) if meta.len() > max => return Side::TooLarge,
        Ok(_) => {}
        Err(_) => return Side::Unreadable,
    }
    let mut bytes = Vec::new();
    match file.take(max.saturating_add(1)).read_to_end(&mut bytes) {
        Ok(_) if bytes.len() as u64 > max => Side::TooLarge,
        Ok(_) => Side::Blob(bytes),
        Err(_) => Side::Unreadable,
    }
}

#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;
