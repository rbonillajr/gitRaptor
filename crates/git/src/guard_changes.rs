//! The commits a ref update brings, with the facts the diff-size and commit-format rules read
//! (DS-US-GRD-009 D6, D7 and D15): the first line of each message, whether a body is separated
//! from it, and, when asked for, the lines each commit changes per file.
//!
//! The walk, the isolation and the fail-closed stance are those of [`crate::guard_paths`]. Lines
//! are *added plus deleted* against the first parent (the empty tree for a root commit) as
//! `git diff --numstat -M` counts them with Git's defaults: renames at 50 % similarity, no
//! copies, a binary file (a NUL in its first 8 000 bytes) 0, a submodule 1 per side, a mode
//! change alone 0. Neither `.gitattributes` nor `diff.*` are read, so an agent cannot change the
//! count with `git config`. A bound that cuts a count is reported (`partial`), never read as 0.

use std::collections::HashMap;

use crate::guard_paths::{Hide, PathLimits};
use crate::{ReadError, RepoReader};

/// The bounds of the line counting (the walk has its own, [`PathLimits`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineLimits {
    /// Bytes of one blob that may be read.
    pub blob_bytes: usize,
    /// Bytes of blobs one read may load.
    pub total_bytes: usize,
}

impl Default for LineLimits {
    fn default() -> Self {
        Self {
            blob_bytes: 16 * 1024 * 1024,
            total_bytes: 64 * 1024 * 1024,
        }
    }
}

/// Longest first line reported, in bytes; the grammar accepts exactly this many.
const MAX_HEAD_BYTES: usize = 4_096;
/// Bytes of a message searched for the line `git revert` writes.
const MAX_REVERT_SCAN: usize = 64 * 1024;
/// Bytes of a blob searched for a NUL, as Git does.
const BINARY_PROBE: usize = 8_000;
/// Git's default `diff.renameLimit` (not read from the repo): with more added and deleted files
/// than this on both sides, the all-pairs search for inexact renames is skipped (more lines
/// count, never fewer). The base-name pass has no limit, as in Git.
const RENAME_LIMIT: usize = 1_000;
/// Similarity of an inexact rename, in sixtieths of a thousand as Git scores (50 %).
const MAX_SCORE: u64 = 60_000;
const MIN_SCORE: u64 = MAX_SCORE / 2;
/// Hash-table entries the similarity phase may visit in one evaluation.
const SIMILARITY_WORK: u64 = 200_000_000;

/// One new commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitChange {
    /// The object id in hex.
    pub oid: String,
    /// Number of parents.
    pub parents: usize,
    /// The first line of the message, without its line end; empty when it is longer than
    /// 4 096 bytes, which no format accepts.
    pub head: Vec<u8>,
    /// The second line is blank or there is none.
    pub body_separated: bool,
    /// The body carries the `This reverts commit <oid>.` line of `git revert`.
    pub reverts: bool,
    /// Lines changed per file (new path) when they were asked for: files with no changed line
    /// are left out, and a merge has none. `None` when no line was asked for.
    pub files: Option<Vec<(String, u32)>>,
    /// A bound cut the count: the lines are at least what `files` adds up to.
    pub partial: bool,
}

/// What the new commits of an update bring, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewCommitChanges {
    pub commits: Vec<CommitChange>,
    /// Some bound was reached or something could not be read: nothing can be said.
    pub unverifiable: bool,
}

/// Why the read stopped.
struct Stop;

impl RepoReader {
    /// The new commits of the update `old → new` (see [`Self::fresh_commit_paths`] for `old`,
    /// `updated` and `hide`) with the facts of their messages and, with `lines`, the lines they
    /// change. Without `lines` no tree is read. `Err` is a repo that cannot be opened for the
    /// read; an object that cannot be read or a bound reached in the walk is `unverifiable`.
    pub fn fresh_commit_changes(
        &self,
        old: Option<&str>,
        new: &str,
        updated: &[&str],
        hide: Hide,
        limits: &PathLimits,
        lines: Option<&LineLimits>,
    ) -> Result<NewCommitChanges, ReadError> {
        let unverifiable = NewCommitChanges {
            unverifiable: true,
            ..NewCommitChanges::default()
        };
        let Some(walked) = self.walk_fresh(old, new, updated, hide, limits)? else {
            return Ok(unverifiable);
        };
        let mut counter = lines.map(|l| Counter {
            repo: &self.repo,
            path_limits: limits,
            limits: *l,
            entries: 0,
            read: 0,
            work: 0,
            prints: HashMap::new(),
            blobs: HashMap::new(),
            unreadable: Default::default(),
        });
        let mut commits = Vec::with_capacity(walked.len());
        // The walk is newest first; the rules name the oldest commit that breaks them.
        for (id, parents) in walked.iter().rev() {
            let Ok(commit) = self.repo.find_commit(*id) else {
                return Ok(unverifiable);
            };
            let Ok(message) = commit.message_raw() else {
                return Ok(unverifiable);
            };
            let Some(real_parents) = self.real_parents(parents) else {
                return Ok(unverifiable);
            };
            let (head, body_separated, reverts) = message_facts(message);
            let (files, partial) = match counter.as_mut() {
                None => (None, false),
                // A merge brings commits that were judged already (D6).
                Some(_) if real_parents >= 2 => (Some(Vec::new()), false),
                Some(counter) => match counter.lines_of(&commit, parents.first().copied()) {
                    Ok((files, partial)) => (Some(files), partial),
                    Err(Stop) => return Ok(unverifiable),
                },
            };
            commits.push(CommitChange {
                oid: id.to_string(),
                parents: real_parents,
                head,
                body_separated,
                reverts,
                files,
                partial,
            });
        }
        Ok(NewCommitChanges {
            commits,
            unverifiable: false,
        })
    }
}

impl RepoReader {
    /// The parents that count: a second parent that is the first one or already in its history
    /// adds nothing to the commit (`git commit-tree -p A -p A`), so such a commit is judged as
    /// a normal one. `None` when the ancestry cannot be read: nothing can be said.
    fn real_parents(&self, parents: &[gix::ObjectId]) -> Option<usize> {
        let Some(first) = parents.first() else {
            return Some(0);
        };
        for p in &parents[1..] {
            if p == first {
                continue;
            }
            let base = self.repo.merge_base(*first, *p).ok()?;
            if base.map(|b| b.detach()) != Some(*p) {
                return Some(parents.len());
            }
        }
        Some(1)
    }
}

/// The first line (empty when it is too long to conform), whether the second is blank and
/// whether the body has the revert line, of a message as Git stores it.
pub fn message_facts(message: &[u8]) -> (Vec<u8>, bool, bool) {
    let (first, rest) = match message.iter().position(|b| *b == b'\n') {
        Some(at) => (&message[..at], Some(&message[at + 1..])),
        None => (message, None),
    };
    let first = first.strip_suffix(b"\r").unwrap_or(first);
    let head = if first.len() > MAX_HEAD_BYTES {
        Vec::new()
    } else {
        first.to_vec()
    };
    let Some(rest) = rest else {
        return (head, true, false);
    };
    let body_separated = rest.is_empty() || rest.starts_with(b"\n") || rest.starts_with(b"\r\n");
    let scan = &rest[..rest.len().min(MAX_REVERT_SCAN)];
    let reverts = scan.split(|b| *b == b'\n').any(is_revert_line);
    (head, body_separated, reverts)
}

/// `This reverts commit <40 or 64 hex digits>.`, the line `git revert` writes.
fn is_revert_line(line: &[u8]) -> bool {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let Some(oid) = line
        .strip_prefix(b"This reverts commit ")
        .and_then(|rest| rest.strip_suffix(b"."))
    else {
        return false;
    };
    matches!(oid.len(), 40 | 64) && oid.iter().all(u8::is_ascii_hexdigit)
}

/// A leaf of a tree that changed.
#[derive(Clone, Copy)]
struct Leaf {
    id: gix::ObjectId,
    gitlink: bool,
}

struct Change {
    path: String,
    old: Option<Leaf>,
    new: Option<Leaf>,
}

struct Counter<'a> {
    repo: &'a gix::Repository,
    path_limits: &'a PathLimits,
    limits: LineLimits,
    entries: usize,
    /// Bytes of blobs loaded so far.
    read: usize,
    /// Similarity work done so far, shared by every commit of the read.
    work: u64,
    /// Fingerprints of the blobs of the commit being counted, and the blobs behind them.
    prints: HashMap<gix::ObjectId, Fingerprint>,
    blobs: HashMap<gix::ObjectId, Vec<u8>>,
    unreadable: std::collections::HashSet<gix::ObjectId>,
}

/// One entry of a tree.
struct Entry {
    tree: bool,
    gitlink: bool,
    id: gix::ObjectId,
}

impl Counter<'_> {
    /// Lines `commit` changes per file against `parent`, and whether a bound cut the count.
    fn lines_of(
        &mut self,
        commit: &gix::Commit<'_>,
        parent: Option<gix::ObjectId>,
    ) -> Result<(Vec<(String, u32)>, bool), Stop> {
        let tree = commit.tree_id().map_err(|_| Stop)?.detach();
        let before = match parent {
            Some(parent) => Some(
                self.repo
                    .find_commit(parent)
                    .map_err(|_| Stop)?
                    .tree_id()
                    .map_err(|_| Stop)?
                    .detach(),
            ),
            None => None,
        };
        let mut changes = Vec::new();
        let mut partial = false;
        self.prints.clear();
        self.blobs.clear();
        self.unreadable.clear();
        self.diff(before, Some(tree), "", 0, &mut changes, &mut partial)?;

        let (mut adds, mut dels, mut mods) = (Vec::new(), Vec::new(), Vec::new());
        let mut files: Vec<(String, u32)> = Vec::new();
        for change in changes {
            match (change.old, change.new) {
                (Some(old), Some(new)) if old.gitlink && new.gitlink => {
                    files.push((change.path, 2));
                }
                (Some(old), Some(new)) if old.gitlink || new.gitlink => {
                    // A file became a submodule or the other way round: both sides count.
                    files.push((change.path.clone(), 1));
                    let (leaf, side) = if old.gitlink {
                        (new, &mut adds)
                    } else {
                        (old, &mut dels)
                    };
                    side.push((change.path, leaf));
                }
                (Some(old), Some(new)) => mods.push((change.path, old, new)),
                (None, Some(new)) if new.gitlink => files.push((change.path, 1)),
                (None, Some(new)) => adds.push((change.path, new)),
                (Some(old), None) if old.gitlink => files.push((change.path, 1)),
                (Some(old), None) => dels.push((change.path, old)),
                (None, None) => {}
            }
        }

        // Renames first: a pair counts only its edited lines, an exact one none.
        let pairs = self.renames(&mut adds, &mut dels, &mut partial)?;
        for (path, old, new) in pairs.into_iter().chain(mods) {
            if old.id == new.id {
                continue;
            }
            let n = self.pair_lines(old.id, new.id, &mut partial)?;
            files.push((path, n));
        }
        for (path, leaf) in adds {
            let n = self.one_sided(leaf.id, &mut partial)?;
            files.push((path, n));
        }
        for (path, leaf) in dels {
            let n = self.one_sided(leaf.id, &mut partial)?;
            files.push((path, n));
        }
        self.blobs.clear();
        files.retain(|(_, n)| *n > 0);
        files.sort();
        Ok((files, partial))
    }

    /// The blob, when it fits the bounds; `None` (and `partial`) when it does not.
    fn blob(&mut self, id: gix::ObjectId, partial: &mut bool) -> Result<Option<Vec<u8>>, Stop> {
        let header = self.repo.find_header(id).map_err(|_| Stop)?;
        let size = usize::try_from(header.size()).unwrap_or(usize::MAX);
        if size > self.limits.blob_bytes || self.read.saturating_add(size) > self.limits.total_bytes
        {
            *partial = true;
            return Ok(None);
        }
        self.read += size;
        let blob = self.repo.find_blob(id).map_err(|_| Stop)?;
        Ok(Some(blob.data.clone()))
    }

    /// The blob kept from the pairing phase (already charged), or read now.
    fn take_blob(
        &mut self,
        id: gix::ObjectId,
        partial: &mut bool,
    ) -> Result<Option<Vec<u8>>, Stop> {
        match self.blobs.remove(&id) {
            Some(data) => Ok(Some(data)),
            None if self.unreadable.contains(&id) => {
                *partial = true;
                Ok(None)
            }
            None => self.blob(id, partial),
        }
    }

    /// Lines of a file added or deleted whole.
    fn one_sided(&mut self, id: gix::ObjectId, partial: &mut bool) -> Result<u32, Stop> {
        let Some(data) = self.take_blob(id, partial)? else {
            return Ok(0);
        };
        Ok(if is_binary(&data) {
            0
        } else {
            line_count(&data)
        })
    }

    /// Lines added plus deleted between two versions of one file.
    fn pair_lines(
        &mut self,
        old: gix::ObjectId,
        new: gix::ObjectId,
        partial: &mut bool,
    ) -> Result<u32, Stop> {
        let (Some(a), Some(b)) = (self.take_blob(old, partial)?, self.take_blob(new, partial)?)
        else {
            return Ok(0);
        };
        if is_binary(&a) || is_binary(&b) {
            return Ok(0);
        }
        let input = gix::diff::blob::InternedInput::new(a.as_slice(), b.as_slice());
        let diff = gix::diff::blob::Diff::compute(gix::diff::blob::Algorithm::Myers, &input);
        Ok(diff.count_removals().saturating_add(diff.count_additions()))
    }

    /// Pairs the added and deleted files the way Git's `-M` does (exact, then the unique
    /// base-name match, then 50 % similar); the files paired leave `adds` and `dels`.
    fn renames(
        &mut self,
        adds: &mut Vec<(String, Leaf)>,
        dels: &mut Vec<(String, Leaf)>,
        partial: &mut bool,
    ) -> Result<Vec<(String, Leaf, Leaf)>, Stop> {
        let mut pairs = Vec::new();
        if adds.is_empty() || dels.is_empty() {
            return Ok(pairs);
        }
        let empty = gix::ObjectId::empty_blob(self.repo.object_hash());
        // Exact: the same blob, the same base name first.
        let mut taken = vec![false; dels.len()];
        let mut paired_adds = vec![false; adds.len()];
        for (i, (path, add)) in adds.iter().enumerate() {
            if add.id == empty {
                continue;
            }
            let candidates = || {
                dels.iter()
                    .enumerate()
                    .filter(|(j, (_, del))| !taken[*j] && del.id == add.id)
            };
            let chosen = candidates()
                .find(|(_, (p, _))| base_name(p) == base_name(path))
                .or_else(|| candidates().next())
                .map(|(j, _)| j);
            if let Some(j) = chosen {
                taken[j] = true;
                paired_adds[i] = true;
                pairs.push((path.clone(), dels[j].1, *add));
            }
        }

        // Base-name pass (Git's `find_basename_matches`): a base name that is unique among the
        // sources and among the destinations pairs them at a higher score, with no limit.
        let rest =
            |paired: &[bool]| -> Vec<usize> { (0..paired.len()).filter(|k| !paired[*k]).collect() };
        let free_dels: Vec<bool> = taken.clone();
        let (rest_adds, rest_dels) = (rest(&paired_adds), rest(&free_dels));
        let by_name = |files: &[(String, Leaf)], idx: &[usize]| {
            let mut map: HashMap<String, Vec<usize>> = HashMap::new();
            for &k in idx {
                map.entry(base_name(&files[k].0).to_owned())
                    .or_default()
                    .push(k);
            }
            map
        };
        let (dst_by, src_by) = (by_name(adds, &rest_adds), by_name(dels, &rest_dels));
        let mut names: Vec<&String> = dst_by.keys().collect();
        names.sort();
        for name in names {
            let (Some(srcs), dsts) = (src_by.get(name), &dst_by[name]) else {
                continue;
            };
            if srcs.len() != 1 || dsts.len() != 1 {
                continue;
            }
            let (i, j) = (dsts[0], srcs[0]);
            let score = self.score(dels[j].1.id, adds[i].1.id, partial)?;
            if score >= MIN_SCORE + (MAX_SCORE - MIN_SCORE) / 2 {
                paired_adds[i] = true;
                taken[j] = true;
                pairs.push((adds[i].0.clone(), dels[j].1, adds[i].1));
            }
        }

        // Inexact: only when the candidates fit the limit.
        let rest_adds = rest(&paired_adds);
        let rest_dels = rest(&taken);
        if !rest_adds.is_empty()
            && !rest_dels.is_empty()
            && (rest_adds.len() <= RENAME_LIMIT || rest_dels.len() <= RENAME_LIMIT)
            && rest_adds.len().saturating_mul(rest_dels.len()) <= RENAME_LIMIT * RENAME_LIMIT
        {
            let mut scored: Vec<(u64, usize, usize)> = Vec::new();
            'dest: for &i in &rest_adds {
                let mut best: Option<(u64, usize)> = None;
                for &j in &rest_dels {
                    if self.work > SIMILARITY_WORK {
                        // Out of work: what is left counts whole, which never counts fewer.
                        break 'dest;
                    }
                    let score = self.score(dels[j].1.id, adds[i].1.id, partial)?;
                    if score >= MIN_SCORE && best.is_none_or(|(s, _)| score > s) {
                        best = Some((score, j));
                    }
                }
                if let Some((score, j)) = best {
                    scored.push((score, i, j));
                }
            }
            // Best pairs first; ties by path, so the result does not depend on the order read.
            scored.sort_by(|a, b| {
                b.0.cmp(&a.0)
                    .then_with(|| adds[a.1].0.cmp(&adds[b.1].0))
                    .then_with(|| dels[a.2].0.cmp(&dels[b.2].0))
            });
            for (_, i, j) in scored {
                if paired_adds[i] || taken[j] {
                    continue;
                }
                paired_adds[i] = true;
                taken[j] = true;
                pairs.push((adds[i].0.clone(), dels[j].1, adds[i].1));
            }
        }

        let mut keep = paired_adds.iter();
        adds.retain(|_| !keep.next().copied().unwrap_or(false));
        let mut keep = taken.iter();
        dels.retain(|_| !keep.next().copied().unwrap_or(false));
        Ok(pairs)
    }

    /// Git's score of the source `old` against the destination `new`; 0 when either cannot be
    /// read within the bounds (it then takes no part in the pairing). Each blob is read once and
    /// kept for the line count of the pair.
    fn score(
        &mut self,
        old: gix::ObjectId,
        new: gix::ObjectId,
        partial: &mut bool,
    ) -> Result<u64, Stop> {
        for id in [old, new] {
            if !self.prints.contains_key(&id)
                && !self.unreadable.contains(&id)
                && let Some(data) = self.blob(id, partial)?
            {
                self.prints.insert(id, Fingerprint::of(&data));
                self.blobs.insert(id, data);
            } else if !self.prints.contains_key(&id) {
                self.unreadable.insert(id);
            }
        }
        let (Some(a), Some(b)) = (self.prints.get(&old), self.prints.get(&new)) else {
            return Ok(0);
        };
        self.work = self
            .work
            .saturating_add(a.spans.len().min(b.spans.len()) as u64);
        Ok(a.score(b))
    }

    /// The leaves where the tree `b` differs from `a`, with their ids.
    fn diff(
        &mut self,
        a: Option<gix::ObjectId>,
        b: Option<gix::ObjectId>,
        prefix: &str,
        depth: usize,
        out: &mut Vec<Change>,
        partial: &mut bool,
    ) -> Result<(), Stop> {
        if a == b {
            return Ok(());
        }
        if depth > self.path_limits.depth || prefix.len() > self.path_limits.path_bytes {
            return Err(Stop);
        }
        let left = self.entries(a)?;
        let right = self.entries(b)?;
        let mut names: Vec<&Vec<u8>> = left.keys().chain(right.keys()).collect();
        names.sort();
        names.dedup();
        for name in names {
            if out.len() > self.path_limits.paths {
                // Too many files: the rest is not counted, and the count says so.
                *partial = true;
                return Ok(());
            }
            let path = format!("{prefix}{}", String::from_utf8_lossy(name));
            if path.len() > self.path_limits.path_bytes {
                return Err(Stop);
            }
            match (left.get(name), right.get(name)) {
                (Some(x), Some(y)) if x.tree && y.tree => {
                    self.diff(
                        Some(x.id),
                        Some(y.id),
                        &format!("{path}/"),
                        depth + 1,
                        out,
                        partial,
                    )?;
                }
                (Some(x), Some(y)) if !x.tree && !y.tree => {
                    // The same content with another mode changes no line.
                    if x.id != y.id || (x.gitlink != y.gitlink) {
                        out.push(Change {
                            path,
                            old: Some(leaf(x)),
                            new: Some(leaf(y)),
                        });
                    }
                }
                // A directory became a file or the other way round: both sides changed.
                (x, y) => {
                    for (entry, is_new) in [(x, false), (y, true)] {
                        let Some(entry) = entry else {
                            continue;
                        };
                        if entry.tree {
                            self.leaves(
                                entry.id,
                                &format!("{path}/"),
                                depth + 1,
                                is_new,
                                out,
                                partial,
                            )?;
                        } else {
                            let leaf = Some(leaf(entry));
                            out.push(Change {
                                path: path.clone(),
                                old: if is_new { None } else { leaf },
                                new: if is_new { leaf } else { None },
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Every leaf under a tree that appeared (`is_new`) or disappeared.
    fn leaves(
        &mut self,
        tree: gix::ObjectId,
        prefix: &str,
        depth: usize,
        is_new: bool,
        out: &mut Vec<Change>,
        partial: &mut bool,
    ) -> Result<(), Stop> {
        if depth > self.path_limits.depth || prefix.len() > self.path_limits.path_bytes {
            return Err(Stop);
        }
        // Sorted, so what a cut keeps does not depend on the hash order.
        let mut sorted: Vec<(Vec<u8>, Entry)> = self.entries(Some(tree))?.into_iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, entry) in sorted {
            let path = format!("{prefix}{}", String::from_utf8_lossy(&name));
            if path.len() > self.path_limits.path_bytes {
                return Err(Stop);
            }
            if entry.tree {
                self.leaves(
                    entry.id,
                    &format!("{path}/"),
                    depth + 1,
                    is_new,
                    out,
                    partial,
                )?;
            } else {
                let leaf = Some(leaf(&entry));
                out.push(Change {
                    path,
                    old: if is_new { None } else { leaf },
                    new: if is_new { leaf } else { None },
                });
            }
            if out.len() > self.path_limits.paths {
                *partial = true;
                return Ok(());
            }
        }
        Ok(())
    }

    fn entries(&mut self, tree: Option<gix::ObjectId>) -> Result<HashMap<Vec<u8>, Entry>, Stop> {
        let Some(tree) = tree else {
            return Ok(HashMap::new());
        };
        let tree = self.repo.find_tree(tree).map_err(|_| Stop)?;
        let decoded = tree.decode().map_err(|_| Stop)?;
        self.entries += decoded.entries.len();
        if self.entries > self.path_limits.entries {
            return Err(Stop);
        }
        let mut entries = HashMap::with_capacity(decoded.entries.len());
        for e in &decoded.entries {
            let entry = Entry {
                tree: e.mode.is_tree(),
                gitlink: e.mode.is_commit(),
                id: e.oid.to_owned(),
            };
            // Two entries with one name is a malformed tree: one could hide the other.
            if entries.insert(e.filename.to_vec(), entry).is_some() {
                return Err(Stop);
            }
        }
        Ok(entries)
    }
}

fn leaf(entry: &Entry) -> Leaf {
    Leaf {
        id: entry.id,
        gitlink: entry.gitlink,
    }
}

fn base_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// A NUL in the first 8 000 bytes, Git's own test for a binary file.
fn is_binary(data: &[u8]) -> bool {
    data[..data.len().min(BINARY_PROBE)].contains(&0)
}

/// Lines of a text: each `\n`, plus the last one when it has no line end.
fn line_count(data: &[u8]) -> u32 {
    let ends = data.iter().filter(|b| **b == b'\n').count();
    let open = usize::from(data.last().is_some_and(|b| *b != b'\n'));
    u32::try_from(ends + open).unwrap_or(u32::MAX)
}

/// Git's similarity fingerprint (`diffcore-delta.c`): the bytes of a blob cut into spans that
/// end at a line end or after 64 bytes, each counted under its hash.
struct Fingerprint {
    size: u64,
    spans: HashMap<u32, u64>,
}

impl Fingerprint {
    fn of(data: &[u8]) -> Self {
        const HASH_BASE: u32 = 107_927;
        let text = !is_binary(data);
        let mut spans: HashMap<u32, u64> = HashMap::new();
        let (mut accum1, mut accum2, mut n) = (0_u32, 0_u32, 0_u64);
        for (i, &c) in data.iter().enumerate() {
            // CRLF counts as LF in a text, as Git does.
            if text && c == b'\r' && data.get(i + 1) == Some(&b'\n') {
                continue;
            }
            let old = accum1;
            accum1 = (accum1 << 7) ^ (accum2 >> 25);
            accum2 = (accum2 << 7) ^ (old >> 25);
            accum1 = accum1.wrapping_add(u32::from(c));
            n += 1;
            if n < 64 && c != b'\n' {
                continue;
            }
            let hash = accum1.wrapping_add(accum2.wrapping_mul(0x61)) % HASH_BASE;
            *spans.entry(hash).or_default() += n;
            (accum1, accum2, n) = (0, 0, 0);
        }
        if n > 0 {
            let hash = accum1.wrapping_add(accum2.wrapping_mul(0x61)) % HASH_BASE;
            *spans.entry(hash).or_default() += n;
        }
        Self {
            size: data.len() as u64,
            spans,
        }
    }

    /// Git's score of `self` (the source) against `dst`: the bytes they share over the larger
    /// size, out of 60 000. Zero when the sizes are too far apart to reach the minimum.
    fn score(&self, dst: &Self) -> u64 {
        let max = self.size.max(dst.size);
        if max == 0 || self.size.abs_diff(dst.size) * MAX_SCORE > max * (MAX_SCORE - MIN_SCORE) {
            return 0;
        }
        let (small, large) = if self.spans.len() <= dst.spans.len() {
            (&self.spans, &dst.spans)
        } else {
            (&dst.spans, &self.spans)
        };
        let shared: u64 = small
            .iter()
            .filter_map(|(hash, a)| large.get(hash).map(|b| *a.min(b)))
            .sum();
        shared * MAX_SCORE / max
    }
}
