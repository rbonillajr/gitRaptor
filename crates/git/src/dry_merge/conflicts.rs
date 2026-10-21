//! The list of conflicted files of a tree merge, as Git reports them.
//!
//! The paths come from an index built in memory from the merged tree with the conflict stages
//! applied (never the repository's index), plus the conflicts that leave no stage there: a
//! directory that took a file's place, and a file moved by a directory rename (a conflict for
//! Git, whose default is `merge.directoryRenames=conflict`).

use std::collections::{BTreeMap, BTreeSet};

use gix::bstr::{BStr, ByteSlice};
use gix::merge::tree::{Conflict, Resolution, ResolutionFailure, TreatAsUnresolved};

use super::ConflictKind;

/// The blobs of a content merge that ended with conflict markers.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Content {
    pub merged: gix::ObjectId,
    pub ours: gix::ObjectId,
    pub theirs: gix::ObjectId,
    /// A side is a symbolic link: its "content" is a target, never lines.
    pub link: bool,
}

/// A conflicted file before its ranges and merge-rule mark are added.
#[derive(Debug, Clone)]
pub(crate) struct Listed {
    pub path: String,
    pub kind: ConflictKind,
    pub content: Option<Content>,
}

/// Which conflicts count as unresolved: Git's view.
pub(crate) fn how() -> TreatAsUnresolved {
    TreatAsUnresolved::git()
}

fn text(p: &BStr) -> String {
    p.to_str_lossy().into_owned()
}

/// The conflicted files of `conflicts`, given `index` (the merged tree with the conflicts
/// applied), in byte order of their paths.
pub(crate) fn extract(index: &gix::index::State, conflicts: &[Conflict]) -> Vec<Listed> {
    let how = how();
    let mut kinds: BTreeMap<String, ConflictKind> = BTreeMap::new();
    let mut contents: BTreeMap<String, Content> = BTreeMap::new();
    for c in conflicts.iter().filter(|c| c.is_unresolved(how)) {
        let kind = kind_of(c);
        let content = content_of(c);
        for loc in [c.ours.location(), c.theirs.location()] {
            kinds.entry(text(loc)).or_insert(kind);
            if let Some(content) = content {
                contents.entry(text(loc)).or_insert(content);
            }
        }
    }

    let mut seen = BTreeSet::new();
    for e in index.entries() {
        if e.stage_raw() != 0 {
            seen.insert(text(e.path(index)));
        }
    }
    let mut extra: BTreeMap<String, ConflictKind> = BTreeMap::new();
    for c in conflicts {
        if let Ok(Resolution::SourceLocationAffectedByRename { final_location }) = &c.resolution {
            let loc = text(final_location.as_ref());
            if !seen.contains(&loc) {
                extra.insert(loc, ConflictKind::Rename);
            }
        } else if c.is_unresolved(how) {
            // Git names such a path `<path>~<side>`: it counts as `<path>`.
            let staged = [c.ours.location(), c.theirs.location()].iter().any(|p| {
                let p = text(p);
                seen.iter().any(|s| {
                    *s == p
                        || s.strip_prefix(p.as_str())
                            .is_some_and(|r| r.starts_with('~'))
                })
            });
            if !staged {
                extra.entry(text(c.ours.location())).or_insert(kind_of(c));
            }
        }
    }

    let mut out: Vec<Listed> = seen
        .into_iter()
        .map(|path| {
            // A unique name `<path>~<side>` has the kind of the conflict at `<path>`.
            let plain = path.rsplit_once('~').map(|(p, _)| p);
            let kind = kinds
                .get(&path)
                .or_else(|| plain.and_then(|p| kinds.get(p)))
                .copied()
                .unwrap_or(ConflictKind::Content);
            Listed {
                kind,
                content: contents.get(&path).copied(),
                path,
            }
        })
        .collect();
    for (path, kind) in extra {
        if !out.iter().any(|l| l.path == path) {
            out.push(Listed {
                content: contents.get(&path).copied(),
                path,
                kind,
            });
        }
    }
    out.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    out
}

fn content_of(c: &Conflict) -> Option<Content> {
    let cm = c.content_merge()?;
    if !matches!(cm.resolution, gix::merge::blob::Resolution::Conflict) {
        return None;
    }
    let [_, Some(o), Some(t)] = c.entries() else {
        return None;
    };
    Some(Content {
        merged: cm.merged_blob_id,
        ours: o.id,
        theirs: t.id,
        link: o.mode.is_link() || t.mode.is_link(),
    })
}

/// The kind of an unresolved conflict. Binary content is decided later, from the blobs.
pub(crate) fn kind_of(c: &Conflict) -> ConflictKind {
    match &c.resolution {
        Ok(Resolution::OursModifiedTheirsModifiedThenBlobContentMerge { .. }) => {
            if c.entries()[0].is_none() {
                ConflictKind::AddAdd
            } else {
                ConflictKind::Content
            }
        }
        Ok(
            Resolution::OursModifiedTheirsRenamedAndChangedThenRename { .. }
            | Resolution::SourceLocationAffectedByRename { .. },
        ) => ConflictKind::Rename,
        Ok(Resolution::Forced(f)) | Err(f) => failure_kind(c, f),
    }
}

fn failure_kind(c: &Conflict, f: &ResolutionFailure) -> ConflictKind {
    match f {
        ResolutionFailure::SubmoduleMerge | ResolutionFailure::SubmoduleAddAdd => {
            ConflictKind::Submodule
        }
        ResolutionFailure::OursModifiedTheirsDeleted => ConflictKind::ModifyDelete,
        ResolutionFailure::OursDeletedTheirsRenamed
        | ResolutionFailure::OursRenamedTheirsRenamedDifferently { .. }
        | ResolutionFailure::OursRenamedTheirsRenamedToSameLocation
        | ResolutionFailure::OursModifiedTheirsRenamedTypeMismatch => ConflictKind::Rename,
        ResolutionFailure::OursDirectoryTheirsNonDirectoryTheirsRenamed { .. }
        | ResolutionFailure::OursModifiedTheirsDirectoryThenOursRenamed { .. } => {
            ConflictKind::DirectoryFile
        }
        ResolutionFailure::OursAddedTheirsAddedTypeMismatch { .. } => {
            if c.ours.entry_mode().is_tree() || c.theirs.entry_mode().is_tree() {
                ConflictKind::DirectoryFile
            } else {
                ConflictKind::AddAdd
            }
        }
        ResolutionFailure::Unknown => ConflictKind::Content,
    }
}
