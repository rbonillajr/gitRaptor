//! The commits a ref update brings and the paths they touch (DS-US-GRD-008 D5): what the
//! forbidden-path rule is evaluated against. Read without replacement objects or the
//! commit-graph, bounded in every dimension, and fail-closed: anything that cannot be read or
//! counted within the bounds is reported as `unverifiable`, never as "nothing touched".
//!
//! A commit is *new* when it is reachable from the new value and from nothing the update does
//! not replace: not from the old value of the ref and not from any other branch or
//! remote-tracking branch (tags and other refs do not count: an agent writes them without any
//! evaluation). A commit touches a path when the path differs from **every** parent, so a merge
//! only counts the paths it resolves itself, and a root commit touches everything it holds.

use std::collections::{BTreeSet, HashMap};

use crate::{ReadError, RepoReader};

/// The bounds of one read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathLimits {
    /// New commits one update may bring.
    pub commits: usize,
    /// Commits the walk may visit.
    pub visited: usize,
    /// Branch tips hidden from the walk. Past it the rest hide nothing, so more commits count as
    /// new, never fewer: it bounds the cost, not the safety.
    pub tips: usize,
    /// Changed paths one update may report.
    pub paths: usize,
    /// Tree entries that may be compared.
    pub entries: usize,
    /// Directories deep a tree may be read (Git's own `core.maxTreeDepth` is 2 048).
    pub depth: usize,
    /// Bytes of one reported path.
    pub path_bytes: usize,
}

impl Default for PathLimits {
    fn default() -> Self {
        Self {
            commits: 256,
            visited: 100_000,
            tips: 4_096,
            paths: 100_000,
            entries: 2_000_000,
            depth: 512,
            path_bytes: 4_096,
        }
    }
}

/// Which refs already hold the commits they reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hide {
    /// Nothing but `old`: a cheap first look whose commits are a superset of the new ones, so a
    /// clean answer is final and a hit is confirmed with one of the others.
    OldOnly,
    /// Every other local and remote-tracking branch (a ref update: what the repo already holds).
    OtherBranches,
    /// Only remote-tracking branches (a push: what leaves is judged whole, whatever the local
    /// branches hold).
    RemoteTracking,
    /// Only local branches (`refs/heads/*`) that are not part of the update: a commit parked
    /// under `refs/remotes/*` (which an agent writes without any evaluation) is still new.
    LocalBranches,
}

/// One entry of the root tree of a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootEntry {
    /// The entry name (lossy UTF-8, the same form the changed paths are reported in).
    pub name: String,
    /// The raw tree-entry mode: it tells a file, an executable, a link, a directory and a
    /// submodule apart.
    pub mode: u32,
    /// The object id in hex.
    pub oid: String,
}

/// What the new commits of an update touch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NewCommitPaths {
    /// Paths a new commit modifies, creates or deletes, sorted, without repeats.
    pub paths: Vec<String>,
    /// Some bound was reached or something could not be read: nothing can be said.
    pub unverifiable: bool,
    /// New commits found.
    pub commits: usize,
}

/// Why the read stopped.
struct Stop;

impl RepoReader {
    /// The paths the commits of the update `old → new` touch. `old` hides what the ref already
    /// held (`None` for a new ref); `updated` are the refs of the same transaction, which never
    /// hide anything. `Err` is a repo that cannot be opened for the read: the caller treats it
    /// as unverifiable too.
    pub fn fresh_commit_paths(
        &self,
        old: Option<&str>,
        new: &str,
        updated: &[&str],
        hide: Hide,
        limits: &PathLimits,
    ) -> Result<NewCommitPaths, ReadError> {
        let id = |hex: &str| {
            gix::ObjectId::from_hex(hex.as_bytes())
                .map_err(|_| ReadError::InvalidInput("invalid object id".into()))
        };
        let new = id(new)?;
        let mut hidden: Vec<gix::ObjectId> = Vec::new();
        if let Some(old) = old {
            hidden.push(id(old)?);
        }
        let unverifiable = NewCommitPaths {
            unverifiable: true,
            ..NewCommitPaths::default()
        };
        // An annotated tag (a push to a ref that is not a branch) is read as the commit it
        // peels to; any other object is not something to read: it cannot be verified.
        let new = match self.repo.try_find_object(new) {
            Ok(Some(object)) if object.kind == gix::object::Kind::Commit => new,
            Ok(Some(object)) if object.kind == gix::object::Kind::Tag => {
                match object.peel_to_kind(gix::object::Kind::Commit) {
                    Ok(commit) => commit.id,
                    Err(_) => return Ok(unverifiable),
                }
            }
            Ok(_) | Err(_) => return Ok(unverifiable),
        };
        if hide != Hide::OldOnly {
            hidden.extend(self.hidden_tips(updated, hide, limits.tips)?);
        }
        let walk = self
            .repo
            .rev_walk([new])
            .with_hidden(hidden)
            .use_commit_graph(false)
            .all()
            .map_err(|e| ReadError::Unavailable(format!("rev-walk: {e}")))?;
        let mut commits = Vec::new();
        for (n, info) in walk.enumerate() {
            let Ok(info) = info else {
                return Ok(unverifiable);
            };
            if n >= limits.visited || commits.len() >= limits.commits {
                return Ok(unverifiable);
            }
            commits.push((info.id, info.parent_ids.to_vec()));
        }
        let mut reader = Reader {
            repo: &self.repo,
            limits,
            entries: 0,
        };
        let mut paths = BTreeSet::new();
        for (commit, parents) in &commits {
            if reader.touched(*commit, parents, &mut paths).is_err() {
                return Ok(unverifiable);
            }
        }
        Ok(NewCommitPaths {
            paths: paths.into_iter().collect(),
            unverifiable: false,
            commits: commits.len(),
        })
    }

    /// The entries of the root tree of `commit` (an annotated tag is read as the commit it
    /// peels to), sorted by name. `Err` when it is not a commit, cannot be read or holds more
    /// entries than `limits.entries`: the caller treats it as unverifiable.
    pub fn root_entries(
        &self,
        commit: &str,
        limits: &PathLimits,
    ) -> Result<Vec<RootEntry>, ReadError> {
        let unavailable = |what: &str| ReadError::Unavailable(format!("root tree: {what}"));
        let id = gix::ObjectId::from_hex(commit.as_bytes())
            .map_err(|_| ReadError::InvalidInput("invalid object id".into()))?;
        let object = self
            .repo
            .try_find_object(id)
            .map_err(|_| unavailable("object"))?
            .ok_or_else(|| unavailable("missing"))?;
        let commit = object
            .peel_to_kind(gix::object::Kind::Commit)
            .map_err(|_| unavailable("not a commit"))?
            .into_commit();
        let tree = commit.tree().map_err(|_| unavailable("tree"))?;
        let decoded = tree.decode().map_err(|_| unavailable("decode"))?;
        if decoded.entries.len() > limits.entries {
            return Err(unavailable("too many entries"));
        }
        let mut entries: Vec<RootEntry> = decoded
            .entries
            .iter()
            .map(|e| RootEntry {
                name: String::from_utf8_lossy(e.filename).into_owned(),
                mode: u32::from(e.mode.value()),
                oid: e.oid.to_hex().to_string(),
            })
            .collect();
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    /// The tips of the branches that already hold commits, at most `max` of them.
    fn hidden_tips(
        &self,
        updated: &[&str],
        hide: Hide,
        max: usize,
    ) -> Result<Vec<gix::ObjectId>, ReadError> {
        let unavailable = |e: &dyn std::fmt::Display| ReadError::Unavailable(format!("refs: {e}"));
        let platform = self.repo.references().map_err(|e| unavailable(&e))?;
        let mut tips = Vec::new();
        for reference in platform.all().map_err(|e| unavailable(&e))? {
            let mut reference = reference.map_err(|e| unavailable(&e))?;
            // A symbolic ref holds nothing of its own: it follows another ref, which an agent
            // can point at a tag that hides nothing.
            if matches!(reference.target(), gix::refs::TargetRef::Symbolic(_)) {
                continue;
            }
            let name = reference.name().as_bstr().to_string();
            let trusted = match hide {
                Hide::OtherBranches => {
                    name.starts_with("refs/remotes/") || name.starts_with("refs/heads/")
                }
                Hide::RemoteTracking => name.starts_with("refs/remotes/"),
                // Never a remote-tracking branch: an agent forges those without a check.
                Hide::LocalBranches => name.starts_with("refs/heads/"),
                Hide::OldOnly => false,
            };
            debug_assert!(hide != Hide::OldOnly);
            if !trusted || updated.contains(&name.as_str()) {
                continue;
            }
            let Ok(id) = reference.peel_to_id() else {
                continue;
            };
            let id = id.detach();
            if self
                .repo
                .try_find_object(id)
                .ok()
                .flatten()
                .is_some_and(|o| o.kind == gix::object::Kind::Commit)
            {
                if tips.len() >= max {
                    break;
                }
                tips.push(id);
            }
        }
        Ok(tips)
    }
}

struct Reader<'a> {
    repo: &'a gix::Repository,
    limits: &'a PathLimits,
    entries: usize,
}

/// One entry of a tree: a directory or a leaf (a file, a link, a submodule).
struct Entry {
    tree: bool,
    /// A submodule: reported as a directory, since what it holds changes with it.
    gitlink: bool,
    mode: gix::objs::tree::EntryMode,
    id: gix::ObjectId,
}

impl Reader<'_> {
    /// Adds to `out` the paths `commit` differs from **all** of its `parents` in.
    fn touched(
        &mut self,
        commit: gix::ObjectId,
        parents: &[gix::ObjectId],
        out: &mut BTreeSet<String>,
    ) -> Result<(), Stop> {
        let tree = self.tree_of(commit)?;
        let mut mine: Option<BTreeSet<String>> = None;
        for parent in parents {
            let parent_tree = self.tree_of(*parent)?;
            let mut diff = BTreeSet::new();
            self.diff(Some(parent_tree), Some(tree), "", 0, &mut diff)?;
            mine = Some(match mine {
                None => diff,
                Some(previous) => previous.intersection(&diff).cloned().collect(),
            });
        }
        let paths = match mine {
            Some(paths) => paths,
            // A root commit touches everything it holds.
            None => {
                let mut all = BTreeSet::new();
                self.diff(None, Some(tree), "", 0, &mut all)?;
                all
            }
        };
        out.extend(paths);
        if out.len() > self.limits.paths {
            return Err(Stop);
        }
        Ok(())
    }

    fn tree_of(&self, commit: gix::ObjectId) -> Result<gix::ObjectId, Stop> {
        let commit = self.repo.find_commit(commit).map_err(|_| Stop)?;
        Ok(commit.tree_id().map_err(|_| Stop)?.detach())
    }

    fn entries(&mut self, tree: Option<gix::ObjectId>) -> Result<HashMap<Vec<u8>, Entry>, Stop> {
        let Some(tree) = tree else {
            return Ok(HashMap::new());
        };
        let tree = self.repo.find_tree(tree).map_err(|_| Stop)?;
        let decoded = tree.decode().map_err(|_| Stop)?;
        self.entries += decoded.entries.len();
        if self.entries > self.limits.entries {
            return Err(Stop);
        }
        let mut entries = HashMap::with_capacity(decoded.entries.len());
        for e in &decoded.entries {
            let entry = Entry {
                tree: e.mode.is_tree(),
                gitlink: e.mode.is_commit(),
                mode: e.mode,
                id: e.oid.to_owned(),
            };
            // Two entries with one name is a malformed tree (`fsck` flags it): one could hide
            // the other, so nothing can be said.
            if entries.insert(e.filename.to_vec(), entry).is_some() {
                return Err(Stop);
            }
        }
        Ok(entries)
    }

    /// The paths in which the tree `b` differs from `a` (either may be absent: everything is
    /// then created or deleted).
    fn diff(
        &mut self,
        a: Option<gix::ObjectId>,
        b: Option<gix::ObjectId>,
        prefix: &str,
        depth: usize,
        out: &mut BTreeSet<String>,
    ) -> Result<(), Stop> {
        if a == b {
            return Ok(());
        }
        if depth > self.limits.depth || prefix.len() > self.limits.path_bytes {
            return Err(Stop);
        }
        let left = self.entries(a)?;
        let right = self.entries(b)?;
        let mut names: Vec<&Vec<u8>> = left.keys().chain(right.keys()).collect();
        names.sort();
        names.dedup();
        for name in names {
            let path = format!("{prefix}{}", String::from_utf8_lossy(name));
            if path.len() > self.limits.path_bytes {
                return Err(Stop);
            }
            let (x, y) = (left.get(name), right.get(name));
            match (x, y) {
                (Some(x), Some(y)) if x.tree && y.tree => {
                    self.diff(Some(x.id), Some(y.id), &format!("{path}/"), depth + 1, out)?;
                }
                (Some(x), Some(y)) if !x.tree && !y.tree => {
                    if x.id != y.id || x.mode != y.mode {
                        out.insert(leaf(&path, x.gitlink || y.gitlink));
                    }
                }
                // A directory became a file or the other way round: both sides changed.
                (x, y) => {
                    for entry in [x, y].into_iter().flatten() {
                        if entry.tree {
                            self.leaves(entry.id, &format!("{path}/"), depth + 1, out)?;
                        } else {
                            out.insert(leaf(&path, entry.gitlink));
                        }
                    }
                }
            }
            if out.len() > self.limits.paths {
                return Err(Stop);
            }
        }
        Ok(())
    }

    /// Every leaf under a tree.
    fn leaves(
        &mut self,
        tree: gix::ObjectId,
        prefix: &str,
        depth: usize,
        out: &mut BTreeSet<String>,
    ) -> Result<(), Stop> {
        if depth > self.limits.depth || prefix.len() > self.limits.path_bytes {
            return Err(Stop);
        }
        let entries = self.entries(Some(tree))?;
        for (name, entry) in entries {
            let path = format!("{prefix}{}", String::from_utf8_lossy(&name));
            if path.len() > self.limits.path_bytes {
                return Err(Stop);
            }
            if entry.tree {
                self.leaves(entry.id, &format!("{path}/"), depth + 1, out)?;
            } else {
                out.insert(leaf(&path, entry.gitlink));
            }
            if out.len() > self.limits.paths {
                return Err(Stop);
            }
        }
        Ok(())
    }
}

/// The path of a leaf as reported: a submodule ends in `/`, so a directory pattern covers it.
fn leaf(path: &str, gitlink: bool) -> String {
    if gitlink {
        format!("{path}/")
    } else {
        path.to_owned()
    }
}
