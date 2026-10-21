//! A2: `gix_merge::commit` / `gix_merge::tree` called directly, with platforms built here:
//! no merge or diff drivers, an empty attribute stack (nothing read from disk, index or tree),
//! a filter pipeline without drivers, a fixed rename limit and enlarged conflict markers.
//!
//! E5: this module must never call the high-level `Repository::merge_trees`,
//! `Repository::merge_commits` or `Repository::merge_resource_cache`, which read drivers,
//! filters and attributes from the observed repository (F4). `tests/api_barrier.rs` enforces it.

use std::cell::Cell;
use std::num::NonZeroU8;
use std::time::{Duration, Instant};

use gix::ObjectId;
use gix::bstr::ByteSlice;
use gix_merge::blob::builtin_driver::text;
use gix_merge::tree::{Resolution, ResolutionFailure, TreatAsUnresolved};

use crate::hunks;

#[derive(Debug, Clone)]
pub struct Limits {
    /// Blobs bigger than this (read from the object header, before loading) abort the pair.
    pub max_blob: u64,
    /// Fixed rename limit: GitRaptor's, never the repository's `merge.renameLimit`.
    pub rename_limit: usize,
    /// Abort when the merge has run longer than this, checked on every object access (P6).
    pub deadline: Option<Duration>,
    /// Stop at the first unresolved conflict ("is there any conflict?").
    pub fail_fast: bool,
    pub marker_size: u8,
    /// Bytes the merge may write to the object memory (merged blobs and trees). Over it the
    /// pair stops: with the blob cap and the object cache, this bounds the memory per job.
    pub max_written: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_blob: 8 * 1024 * 1024,
            rename_limit: 1000,
            deadline: None,
            fail_fast: false,
            marker_size: 32,
            max_written: 64 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Outcome {
    /// Conflicted paths (stage > 0, like `git merge-tree --name-only`) with their kind.
    pub conflicts: Vec<(String, &'static str)>,
    /// Per content-conflicted path: `[ours_start, ours_end, theirs_start, theirs_end]`,
    /// 1-based inclusive line ranges in each side's version (empty side: end = start - 1).
    pub hunks: Vec<(String, Vec<[u32; 4]>)>,
    pub merge_bases: usize,
    pub object_reads: u64,
    pub stopped_early: bool,
}

/// The object access every merge goes through: deadline and blob-size cap (M-06).
pub struct Guarded<'r> {
    repo: &'r gix::Repository,
    started: Instant,
    deadline: Option<Duration>,
    max_blob: u64,
    reads: Cell<u64>,
    max_written: u64,
    written: Cell<u64>,
}

// gix 0.88 (feature `gix088`, H9) returns `Exn` from the object traits; 0.89 returns `Error`.
#[cfg(not(feature = "gix088"))]
type ObjResult<T> = gix_error::Result<T>;
#[cfg(feature = "gix088")]
type ObjResult<T> = Result<T, gix_error::Exn>;

#[cfg(not(feature = "gix088"))]
fn limit_error(msg: String) -> gix_error::Error {
    gix_error::Error::from_error(std::io::Error::other(msg))
}
#[cfg(feature = "gix088")]
fn limit_error(msg: String) -> gix_error::Exn {
    gix_error::Exn::new(std::io::Error::other(msg)).erased()
}

impl Guarded<'_> {
    fn check(&self, id: &gix::oid) -> ObjResult<()> {
        self.reads.set(self.reads.get() + 1);
        if let Some(d) = self.deadline
            && self.started.elapsed() > d
        {
            return Err(limit_error(format!("ckp-limit: deadline of {d:?} passed")));
        }
        if let Some(h) = gix_object::FindHeader::try_header(self.repo, id)?
            && h.kind == gix_object::Kind::Blob
            && h.size > self.max_blob
        {
            return Err(limit_error(format!(
                "ckp-limit: blob {id} has {} bytes (cap {})",
                h.size, self.max_blob
            )));
        }
        Ok(())
    }
}

impl Guarded<'_> {
    fn account(&self, n: u64) -> ObjResult<()> {
        self.written.set(self.written.get() + n);
        if self.written.get() > self.max_written {
            return Err(limit_error(format!(
                "ckp-limit: written {} bytes to the object memory (cap {})",
                self.written.get(),
                self.max_written
            )));
        }
        Ok(())
    }
}

impl gix_object::Find for Guarded<'_> {
    fn try_find<'a>(
        &self,
        id: &gix::oid,
        buffer: &'a mut Vec<u8>,
    ) -> ObjResult<Option<gix_object::Data<'a>>> {
        self.check(id)?;
        gix_object::Find::try_find(self.repo, id, buffer)
    }
}

impl gix_object::FindHeader for Guarded<'_> {
    fn try_header(&self, id: &gix::oid) -> ObjResult<Option<gix_object::Header>> {
        gix_object::FindHeader::try_header(self.repo, id)
    }
}

impl gix_object::Write for Guarded<'_> {
    fn write_buf(&self, kind: gix_object::Kind, from: &[u8]) -> ObjResult<ObjectId> {
        self.account(from.len() as u64)?;
        gix_object::Write::write_buf(self.repo, kind, from)
    }
    fn write_buf_with_known_id(
        &self,
        kind: gix_object::Kind,
        from: &[u8],
        id: ObjectId,
    ) -> ObjResult<ObjectId> {
        self.account(from.len() as u64)?;
        gix_object::Write::write_buf_with_known_id(self.repo, kind, from, id)
    }
    fn write_stream(
        &self,
        kind: gix_object::Kind,
        size: u64,
        from: &mut dyn std::io::Read,
    ) -> ObjResult<ObjectId> {
        self.account(size)?;
        gix_object::Write::write_stream(self.repo, kind, size, from)
    }
    fn write_stream_with_known_id(
        &self,
        kind: gix_object::Kind,
        size: u64,
        from: &mut dyn std::io::Read,
        id: ObjectId,
    ) -> ObjResult<ObjectId> {
        self.account(size)?;
        gix_object::Write::write_stream_with_known_id(self.repo, kind, size, from, id)
    }
}

/// An attribute stack that reads nothing: no globals, no `info/attributes`, no id mappings
/// from the index, so no `.gitattributes` from disk, index or tree (L-04).
fn empty_attributes(repo: &gix::Repository) -> gix_worktree::Stack {
    use gix_worktree::stack::state::attributes::Source;
    let attrs = gix_worktree::stack::state::Attributes::new(
        Default::default(),
        None,
        Source::IdMapping,
        Default::default(),
    );
    gix_worktree::Stack::new(
        repo.git_dir(),
        gix_worktree::stack::State::AttributesStack(attrs),
        gix_glob::pattern::Case::Sensitive,
        Vec::new(),
        Vec::new(),
    )
}

fn no_driver_filter() -> gix_filter::Pipeline {
    #[cfg(not(feature = "gix088"))]
    return gix_filter::Pipeline::new(
        gix_command::Context::default(),
        gix_hash::Kind::Sha1,
        gix_filter::pipeline::Options::default(),
    );
    #[cfg(feature = "gix088")]
    return gix_filter::Pipeline::new(gix_command::Context::default(), Default::default());
}

pub fn diff_cache(repo: &gix::Repository) -> gix_diff::blob::Platform {
    gix_diff::blob::Platform::new(
        gix_diff::blob::platform::Options {
            algorithm: Some(gix_diff::blob::Algorithm::Histogram),
            skip_internal_diff_if_external_is_configured: false,
        },
        gix_diff::blob::Pipeline::new(
            Default::default(),
            no_driver_filter(),
            Vec::new(),
            Default::default(),
        ),
        gix_diff::blob::pipeline::Mode::ToGit,
        empty_attributes(repo),
    )
}

pub fn blob_platform(repo: &gix::Repository) -> gix_merge::blob::Platform {
    gix_merge::blob::Platform::new(
        gix_merge::blob::Pipeline::new(Default::default(), no_driver_filter(), Default::default()),
        gix_merge::blob::pipeline::Mode::ToGit,
        empty_attributes(repo),
        Vec::new(),
        gix_merge::blob::platform::Options {
            default_driver: None,
        },
    )
}

pub fn tree_options(limits: &Limits) -> gix_merge::tree::Options {
    gix_merge::tree::Options {
        rewrites: Some(gix_diff::Rewrites {
            copies: None,
            percentage: Some(0.5),
            limit: limits.rename_limit,
            track_empty: false,
        }),
        blob_merge: gix_merge::blob::platform::merge::Options {
            is_virtual_ancestor: false,
            resolve_binary_with: None,
            text: text::Options {
                // merge-ort uses histogram; CKP_DIFF_ALGO=myers is only for the E6 comparison.
                diff_algorithm: if std::env::var("CKP_DIFF_ALGO").is_ok_and(|a| a == "myers") {
                    gix_diff::blob::Algorithm::Myers
                } else {
                    gix_diff::blob::Algorithm::Histogram
                },
                conflict: text::Conflict::Keep {
                    style: text::ConflictStyle::Merge,
                    marker_size: NonZeroU8::new(limits.marker_size.max(7))
                        .unwrap_or(NonZeroU8::MIN),
                },
            },
        },
        blob_merge_command_ctx: Default::default(),
        fail_on_conflict: limits.fail_fast.then(TreatAsUnresolved::git),
        marker_size_multiplier: 0,
        symlink_conflicts: None,
        tree_conflicts: None,
    }
}

fn kind_of(c: &gix_merge::tree::Conflict) -> &'static str {
    match &c.resolution {
        Ok(Resolution::OursModifiedTheirsModifiedThenBlobContentMerge { .. }) => "content",
        Ok(Resolution::OursModifiedTheirsRenamedAndChangedThenRename { .. }) => "rename-content",
        Ok(Resolution::SourceLocationAffectedByRename { .. }) => "rename-dir",
        Ok(Resolution::Forced(_)) => "forced",
        Err(f) => match f {
            ResolutionFailure::SubmoduleMerge | ResolutionFailure::SubmoduleAddAdd => "submodule",
            ResolutionFailure::OursModifiedTheirsDeleted => "modify-delete",
            ResolutionFailure::OursDeletedTheirsRenamed => "rename-delete",
            ResolutionFailure::OursRenamedTheirsRenamedDifferently { .. }
            | ResolutionFailure::OursRenamedTheirsRenamedToSameLocation => "rename-rename",
            ResolutionFailure::OursAddedTheirsAddedTypeMismatch { .. }
            | ResolutionFailure::OursModifiedTheirsRenamedTypeMismatch => "type",
            ResolutionFailure::OursDirectoryTheirsNonDirectoryTheirsRenamed { .. }
            | ResolutionFailure::OursModifiedTheirsDirectoryThenOursRenamed { .. } => "dir-file",
            ResolutionFailure::Unknown => "unknown",
        },
    }
}

/// Predict the merge of `ours` into `theirs` (commits) without writing anything.
pub fn predict(
    repo: &gix::Repository,
    ours: ObjectId,
    theirs: ObjectId,
    limits: &Limits,
) -> Result<Outcome, String> {
    let objects = Guarded {
        repo,
        started: Instant::now(),
        deadline: limits.deadline,
        max_blob: limits.max_blob,
        reads: Cell::new(0),
        max_written: limits.max_written,
        written: Cell::new(0),
    };
    let cache = repo.commit_graph_if_enabled().map_err(|e| e.to_string())?;
    let mut graph = repo.revision_graph(cache.as_ref());
    let mut diff = diff_cache(repo);
    let mut blobs = blob_platform(repo);
    let options = gix_merge::commit::Options {
        allow_missing_merge_base: false,
        tree_merge: tree_options(limits),
        use_first_merge_base: false,
    };
    let labels = text::Labels {
        ancestor: Some("base".into()),
        current: Some("ours".into()),
        other: Some("theirs".into()),
    };
    let out = gix_merge::commit(
        ours,
        theirs,
        labels,
        &mut graph,
        &mut diff,
        &mut blobs,
        &objects,
        &mut |id| id.to_hex_with_len(12).to_string(),
        options,
    )
    .map_err(|e| format!("{e:?}"))?;

    if std::env::var_os("CKP_DEBUG").is_some() {
        for c in &out.tree_merge.conflicts {
            eprintln!(
                "conflict: {:?} ours={} theirs={}",
                c.resolution,
                c.ours.location(),
                c.theirs.location()
            );
        }
    }
    let how = TreatAsUnresolved::git();
    // Conflicted paths exactly as Git stages them: the merged tree (written to the object
    // memory, never to disk) as an index, plus the conflict entries (stages 1-3).
    let mut out = out;
    let merged_tree = out
        .tree_merge
        .tree
        .write(|t| gix_object::Write::write(&objects, t))
        .map_err(|e| format!("{e:?}"))?;
    let mut index = gix_index::State::from_tree(&merged_tree, &objects, Default::default())
        .map_err(|e| format!("{e:?}"))?;
    out.tree_merge.index_changed_after_applying_conflicts(
        &mut index,
        how,
        gix_merge::tree::apply_index_entries::RemovalMode::Mark,
    );
    let mut kinds: std::collections::BTreeMap<String, &'static str> = Default::default();
    for c in out
        .tree_merge
        .conflicts
        .iter()
        .filter(|c| c.is_unresolved(how))
    {
        let k = kind_of(c);
        kinds
            .entry(c.ours.location().to_str_lossy().into_owned())
            .or_insert(k);
        kinds
            .entry(c.theirs.location().to_str_lossy().into_owned())
            .or_insert(k);
    }
    let mut result = Outcome {
        merge_bases: out.merge_bases.as_ref().map_or(0, |b| b.len()),
        stopped_early: out.tree_merge.failed_on_first_unresolved_conflict,
        ..Default::default()
    };
    let mut seen = std::collections::BTreeSet::new();
    for e in index.entries() {
        if e.stage_raw() == 0 {
            continue;
        }
        let path = e.path(&index).to_str_lossy().into_owned();
        if seen.insert(path.clone()) {
            let kind = kinds.get(&path).copied().unwrap_or("other");
            result.conflicts.push((path, kind));
        }
    }
    // An unresolved conflict whose paths are no file of the merged tree (a directory took the
    // place, F2) leaves no stage in the index: report it at its own location, like Git's
    // `<path>~<side>`. A file moved by a directory rename is a conflict for merge-ort
    // (`merge.directoryRenames=conflict` by default), not for `TreatAsUnresolved::git()`.
    for c in &out.tree_merge.conflicts {
        let loc = |p: &gix::bstr::BStr| p.to_str_lossy().into_owned();
        if let Ok(Resolution::SourceLocationAffectedByRename { final_location }) = &c.resolution {
            if seen.insert(loc(final_location.as_ref())) {
                result
                    .conflicts
                    .push((loc(final_location.as_ref()), "rename-dir"));
            }
        } else if c.is_unresolved(how)
            && ![c.ours.location(), c.theirs.location()].iter().any(|p| {
                let p = loc(p);
                seen.iter()
                    .any(|s| *s == p || s.strip_prefix(&p).is_some_and(|r| r.starts_with('~')))
            })
        {
            seen.insert(loc(c.ours.location()));
            result.conflicts.push((loc(c.ours.location()), kind_of(c)));
        }
    }

    // Hunks: parse the merged blob (enlarged markers) and map each region to each side (E7).
    for c in out
        .tree_merge
        .conflicts
        .iter()
        .filter(|c| c.is_unresolved(how))
    {
        let Some(cm) = c.content_merge() else {
            continue;
        };
        if !matches!(cm.resolution, gix_merge::blob::Resolution::Conflict) {
            continue;
        }
        let [_, Some(o), Some(t)] = c.entries() else {
            continue;
        };
        let mut buf = Vec::new();
        let merged = match gix_object::Find::try_find(&objects, &cm.merged_blob_id, &mut buf) {
            Ok(Some(d)) => d.data.to_vec(),
            _ => continue,
        };
        let (mut ob, mut tb) = (Vec::new(), Vec::new());
        let ours_data = match gix_object::Find::try_find(&objects, &o.id, &mut ob) {
            Ok(Some(d)) => d.data.to_vec(),
            _ => continue,
        };
        let theirs_data = match gix_object::Find::try_find(&objects, &t.id, &mut tb) {
            Ok(Some(d)) => d.data.to_vec(),
            _ => continue,
        };
        if let Some(ranges) =
            hunks::side_ranges(&merged, &ours_data, &theirs_data, limits.marker_size)
        {
            result
                .hunks
                .push((c.ours.location().to_str_lossy().into_owned(), ranges));
        }
    }
    result.object_reads = objects.reads.get();
    Ok(result)
}
