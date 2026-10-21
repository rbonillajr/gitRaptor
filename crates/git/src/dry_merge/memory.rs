//! The dry merge itself: `gix_merge::commit` called directly with platforms built here, over a
//! repository whose new objects stay in `with_object_memory()` for one job.
//!
//! No merge or diff driver, a filter pipeline without drivers, an empty attribute stack (nothing
//! from the working tree, the index or a tree) and caps taken from [`DryMergeLimits`], never from
//! the repository's configuration. Every object access goes through [`Bounded`].

use std::cell::Cell;
use std::path::Path;

use gix::merge::blob::builtin_driver::text;
use gix::merge::tree::apply_index_entries::RemovalMode;
use gix::objs::{Find, FindExt, FindHeader};

use super::conflicts::{self, Content, Listed};
use super::merge_attr::MergeRules;
use super::{
    ConflictFile, ConflictKind, DryMergeError, DryMergeInput, DryMergeLimits, DryMergeOutcome,
    MergeBaseUsed, hunk_ranges,
};
use crate::RepoReader;

/// Object cache of a job: the pairs of one worktree share most of their trees.
const OBJECT_CACHE_BYTES: usize = 64 * 1024 * 1024;

/// How much of a blob Git looks at to call it binary.
const BINARY_SNIFF_BYTES: usize = 8000;

/// Predicts the merge of the commits `input.theirs` into `input.ours` of the repository at
/// `repo`, writing nothing to it and running no program.
///
/// A blob above [`DryMergeLimits::max_blob_bytes`] is never loaded: its file is
/// [`ConflictKind::Binary`], without ranges.
///
/// # Errors
/// - [`DryMergeError::MissingObject`]: an object is not in the repository; nothing is fetched.
/// - [`DryMergeError::Shallow`]: a shallow repository whose history does not reach the
///   merge-base.
/// - [`DryMergeError::LimitExceeded`]: the merge wrote more than
///   [`DryMergeLimits::max_written_bytes`] to its object memory.
/// - [`DryMergeError::Unavailable`]: an id is not hex, or the repository cannot be read.
pub fn merge_in_memory(
    repo: &Path,
    input: &DryMergeInput,
    limits: &DryMergeLimits,
) -> Result<DryMergeOutcome, DryMergeError> {
    let ours = commit_id(&input.ours)?;
    let theirs = commit_id(&input.theirs)?;
    let reader = RepoReader::open_for_dry_merge(repo)
        .map_err(|e| DryMergeError::Unavailable(e.to_string()))?;
    let mut repo = reader.repo.with_object_memory();
    repo.object_cache_size_if_unset(OBJECT_CACHE_BYTES);
    if repo.is_shallow().map_err(unavailable)? && !shallow_reaches_base(&repo, ours, theirs)? {
        return Err(DryMergeError::Shallow);
    }
    let objects = Bounded::new(&repo, limits);
    let result = merge(&repo, &objects, ours, theirs, limits);
    // The caps and a missing object surface as opaque errors from inside the merge: the
    // wrapper's own record says which one it was.
    if objects.limit_hit.get() {
        Err(DryMergeError::LimitExceeded)
    } else if objects.missing.get() {
        Err(DryMergeError::MissingObject)
    } else {
        result
    }
}

fn commit_id(hex: &str) -> Result<gix::ObjectId, DryMergeError> {
    gix::ObjectId::from_hex(hex.as_bytes())
        .map_err(|_| DryMergeError::Unavailable(format!("not a commit id: {hex:?}")))
}

fn unavailable(e: impl std::fmt::Display) -> DryMergeError {
    DryMergeError::Unavailable(e.to_string())
}

/// In a shallow repository Git sees the boundary commits as parentless, while the graph walk
/// steps over a boundary whose parent happens to be present. The merge-base is only trusted when
/// no boundary commit lies between a tip and the bases.
fn shallow_reaches_base(
    repo: &gix::Repository,
    ours: gix::ObjectId,
    theirs: gix::ObjectId,
) -> Result<bool, DryMergeError> {
    let bases = repo
        .merge_bases_many(ours, &[theirs])
        .map_err(unavailable)?;
    if bases.is_empty() {
        return Ok(false);
    }
    let Some(boundary) = repo.shallow_commits().map_err(unavailable)? else {
        return Ok(true);
    };
    let bases: Vec<gix::ObjectId> = bases.into_iter().map(|id| id.detach()).collect();
    let walk = repo
        .rev_walk([ours, theirs])
        .with_hidden(bases)
        .all()
        .map_err(unavailable)?;
    for info in walk {
        let info = info.map_err(unavailable)?;
        if boundary.contains(&info.id) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn merge(
    repo: &gix::Repository,
    objects: &Bounded<'_>,
    ours: gix::ObjectId,
    theirs: gix::ObjectId,
    limits: &DryMergeLimits,
) -> Result<DryMergeOutcome, DryMergeError> {
    let cache = repo.commit_graph_if_enabled().map_err(unavailable)?;
    let mut graph = repo.revision_graph(cache.as_ref());
    let mut diff = diff_platform(repo, limits);
    let mut blobs = blob_platform(repo, limits);
    let options = gix::merge::plumbing::commit::Options {
        allow_missing_merge_base: true,
        tree_merge: tree_options(limits),
        use_first_merge_base: false,
    };
    let labels = text::Labels {
        ancestor: None,
        current: Some("ours".into()),
        other: Some("theirs".into()),
    };
    let mut out = gix::merge::plumbing::commit(
        ours,
        theirs,
        labels,
        &mut graph,
        &mut diff,
        &mut blobs,
        objects,
        &mut |id| id.to_hex_with_len(12).to_string(),
        options,
    )
    .map_err(unavailable)?;

    let merge_base = match &out.merge_bases {
        None => MergeBaseUsed::None,
        Some(bases) if bases.len() == 1 => MergeBaseUsed::Single(bases.first().to_string()),
        Some(bases) => {
            let mut bases: Vec<String> = bases.iter().map(ToString::to_string).collect();
            bases.sort();
            MergeBaseUsed::Virtual { bases }
        }
    };

    // The conflicted paths exactly as Git stages them: the merged tree (in the object memory)
    // as an index, with the conflict stages applied.
    let merged_tree = out
        .tree_merge
        .tree
        .write(|t| gix::objs::Write::write(objects, t))
        .map_err(unavailable)?;
    let mut index = gix::index::State::from_tree(&merged_tree, objects, Default::default())
        .map_err(unavailable)?;
    out.tree_merge.index_changed_after_applying_conflicts(
        &mut index,
        conflicts::how(),
        RemovalMode::Mark,
    );
    let mut listed = conflicts::extract(&index, &out.tree_merge.conflicts);
    let truncated = listed.len() > limits.max_files;
    listed.truncate(limits.max_files);

    let tip_trees = [ours, theirs]
        .iter()
        .map(|id| {
            Ok(objects
                .find_commit(id, &mut Vec::new())
                .map_err(unavailable)?
                .tree())
        })
        .collect::<Result<Vec<_>, DryMergeError>>()?;
    let mut rules = MergeRules::new(objects, tip_trees, limits.max_blob_bytes);
    let mut files = Vec::with_capacity(listed.len());
    for item in listed {
        let has_merge_rule = rules.has_merge_rule(&item.path).map_err(unavailable)?;
        files.push(file(objects, item, has_merge_rule, limits)?);
    }
    Ok(DryMergeOutcome {
        merge_base,
        files,
        truncated,
        rewrites_escalated: false,
    })
}

/// A listed conflict with its ranges, when its content merge left markers in text blobs.
fn file(
    objects: &Bounded<'_>,
    item: Listed,
    has_merge_rule: bool,
    limits: &DryMergeLimits,
) -> Result<ConflictFile, DryMergeError> {
    let mut file = ConflictFile {
        path: item.path,
        kind: item.kind,
        ours: Vec::new(),
        theirs: Vec::new(),
        hunks_truncated: false,
        has_merge_rule,
    };
    let Some(content) = item.content else {
        return Ok(file);
    };
    match side_texts(objects, &content, limits)? {
        Sides::Text {
            merged,
            ours,
            theirs,
        } => {
            let (o, t, cut) = hunk_ranges(&merged, &ours, &theirs, limits);
            file.ours = o;
            file.theirs = t;
            file.hunks_truncated = cut;
        }
        Sides::Binary => {
            if matches!(file.kind, ConflictKind::Content | ConflictKind::AddAdd) {
                file.kind = ConflictKind::Binary;
            }
        }
    }
    Ok(file)
}

enum Sides {
    Text {
        merged: Vec<u8>,
        ours: Vec<u8>,
        theirs: Vec<u8>,
    },
    Binary,
}

/// The three blobs of a content conflict, each size checked on its header before loading.
fn side_texts(
    objects: &Bounded<'_>,
    content: &Content,
    limits: &DryMergeLimits,
) -> Result<Sides, DryMergeError> {
    if content.link {
        return Ok(Sides::Binary);
    }
    for id in [content.ours, content.theirs, content.merged] {
        match objects.try_header(&id).map_err(unavailable)? {
            Some(h) if h.size <= limits.max_blob_bytes => {}
            Some(_) => return Ok(Sides::Binary),
            None => return Err(DryMergeError::MissingObject),
        }
    }
    let load = |id: &gix::ObjectId| -> Result<Vec<u8>, DryMergeError> {
        Ok(objects
            .find_blob(id, &mut Vec::new())
            .map_err(unavailable)?
            .data
            .to_vec())
    };
    let (ours, theirs) = (load(&content.ours)?, load(&content.theirs)?);
    let binary = |data: &[u8]| data[..data.len().min(BINARY_SNIFF_BYTES)].contains(&0);
    if binary(&ours) || binary(&theirs) {
        return Ok(Sides::Binary);
    }
    Ok(Sides::Text {
        merged: load(&content.merged)?,
        ours,
        theirs,
    })
}

/// Rename detection with GitRaptor's limit and Git's default similarity (50 %).
fn tree_options(limits: &DryMergeLimits) -> gix::merge::plumbing::tree::Options {
    gix::merge::plumbing::tree::Options {
        rewrites: Some(gix::diff::Rewrites {
            copies: None,
            percentage: Some(0.5),
            limit: limits.rewrites_limit,
            track_empty: false,
        }),
        blob_merge: gix::merge::blob::platform::merge::Options {
            is_virtual_ancestor: false,
            resolve_binary_with: None,
            text: text::Options {
                diff_algorithm: gix::diff::blob::Algorithm::Histogram,
                conflict: text::Conflict::Keep {
                    style: text::ConflictStyle::Merge,
                    marker_size: limits.marker_len(),
                },
            },
        },
        blob_merge_command_ctx: Default::default(),
        fail_on_conflict: None,
        // Left at gix's value: each level of a virtual base lengthens its markers, so they are
        // never `marker_size` long and the extractor reads them as content.
        ..Default::default()
    }
}

/// An attribute stack that reads nothing: no globals, no `info/attributes` and no id mappings,
/// so no `.gitattributes` from the working tree, the index or a tree.
fn empty_attributes(repo: &gix::Repository) -> gix::worktree::Stack {
    use gix::worktree::stack::state::attributes::Source;
    let attributes = gix::worktree::stack::state::Attributes::new(
        Default::default(),
        None,
        Source::IdMapping,
        Default::default(),
    );
    gix::worktree::Stack::new(
        repo.git_dir(),
        gix::worktree::stack::State::AttributesStack(attributes),
        gix::glob::pattern::Case::Sensitive,
        Vec::new(),
        Vec::new(),
    )
}

/// A filter pipeline without drivers: no `filter.<x>.*` program can run.
fn no_driver_filter(repo: &gix::Repository) -> gix::filter::plumbing::Pipeline {
    gix::filter::plumbing::Pipeline::new(
        gix::command::Context::default(),
        repo.object_hash(),
        gix::filter::plumbing::pipeline::Options::default(),
    )
}

fn diff_platform(repo: &gix::Repository, limits: &DryMergeLimits) -> gix::diff::blob::Platform {
    gix::diff::blob::Platform::new(
        gix::diff::blob::platform::Options {
            algorithm: Some(gix::diff::blob::Algorithm::Histogram),
            skip_internal_diff_if_external_is_configured: false,
        },
        gix::diff::blob::Pipeline::new(
            Default::default(),
            no_driver_filter(repo),
            Vec::new(),
            gix::diff::blob::pipeline::Options {
                large_file_threshold_bytes: limits.max_blob_bytes,
                fs: Default::default(),
            },
        ),
        gix::diff::blob::pipeline::Mode::ToGit,
        empty_attributes(repo),
    )
}

fn blob_platform(repo: &gix::Repository, limits: &DryMergeLimits) -> gix::merge::blob::Platform {
    gix::merge::blob::Platform::new(
        gix::merge::blob::Pipeline::new(
            Default::default(),
            no_driver_filter(repo),
            gix::merge::blob::pipeline::Options {
                // A blob above the cap is binary by its header, never loaded.
                large_file_threshold_bytes: limits.max_blob_bytes,
            },
        ),
        gix::merge::blob::pipeline::Mode::ToGit,
        empty_attributes(repo),
        Vec::new(),
        gix::merge::blob::platform::Options {
            default_driver: None,
        },
    )
}

/// The only object access of a job. It refuses to load a blob above the cap (its header is read
/// first), refuses writes past the written-bytes cap, and records a missing object, so the
/// opaque errors of the merge can be told apart.
struct Bounded<'r> {
    repo: &'r gix::Repository,
    max_blob_bytes: u64,
    max_written_bytes: u64,
    written: Cell<u64>,
    limit_hit: Cell<bool>,
    missing: Cell<bool>,
}

impl<'r> Bounded<'r> {
    fn new(repo: &'r gix::Repository, limits: &DryMergeLimits) -> Self {
        Self {
            repo,
            max_blob_bytes: limits.max_blob_bytes,
            max_written_bytes: limits.max_written_bytes,
            written: Cell::new(0),
            limit_hit: Cell::new(false),
            missing: Cell::new(false),
        }
    }

    fn limit(&self, what: String) -> gix::Error {
        self.limit_hit.set(true);
        gix::Error::from_error(std::io::Error::other(what))
    }

    fn account(&self, n: u64) -> gix::Result<()> {
        let total = self.written.get().saturating_add(n);
        self.written.set(total);
        if total > self.max_written_bytes {
            return Err(self.limit(format!(
                "wrote {total} bytes to the object memory (cap {})",
                self.max_written_bytes
            )));
        }
        Ok(())
    }
}

impl Find for Bounded<'_> {
    fn try_find<'a>(
        &self,
        id: &gix::oid,
        buffer: &'a mut Vec<u8>,
    ) -> gix::Result<Option<gix::objs::Data<'a>>> {
        match FindHeader::try_header(self.repo, id)? {
            None => {
                self.missing.set(true);
                return Ok(None);
            }
            Some(h) if h.kind == gix::objs::Kind::Blob && h.size > self.max_blob_bytes => {
                return Err(self.limit(format!(
                    "blob {id} has {} bytes (cap {})",
                    h.size, self.max_blob_bytes
                )));
            }
            Some(_) => {}
        }
        let found = Find::try_find(self.repo, id, buffer)?;
        if found.is_none() {
            self.missing.set(true);
        }
        Ok(found)
    }
}

impl FindHeader for Bounded<'_> {
    fn try_header(&self, id: &gix::oid) -> gix::Result<Option<gix::objs::Header>> {
        let header = FindHeader::try_header(self.repo, id)?;
        if header.is_none() {
            self.missing.set(true);
        }
        Ok(header)
    }
}

impl gix::objs::Write for Bounded<'_> {
    fn write_buf(&self, kind: gix::objs::Kind, from: &[u8]) -> gix::Result<gix::ObjectId> {
        self.account(from.len() as u64)?;
        gix::objs::Write::write_buf(self.repo, kind, from)
    }

    fn write_buf_with_known_id(
        &self,
        kind: gix::objs::Kind,
        from: &[u8],
        id: gix::ObjectId,
    ) -> gix::Result<gix::ObjectId> {
        self.account(from.len() as u64)?;
        gix::objs::Write::write_buf_with_known_id(self.repo, kind, from, id)
    }

    fn write_stream(
        &self,
        kind: gix::objs::Kind,
        size: u64,
        from: &mut dyn std::io::Read,
    ) -> gix::Result<gix::ObjectId> {
        self.account(size)?;
        gix::objs::Write::write_stream(self.repo, kind, size, from)
    }

    fn write_stream_with_known_id(
        &self,
        kind: gix::objs::Kind,
        size: u64,
        from: &mut dyn std::io::Read,
        id: gix::ObjectId,
    ) -> gix::Result<gix::ObjectId> {
        self.account(size)?;
        gix::objs::Write::write_stream_with_known_id(self.repo, kind, size, from, id)
    }
}
