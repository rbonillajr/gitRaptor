//! What the applier loads before touching anything: both snapshots re-verified (SEC-TMC-09),
//! their `meta`, the files and index of every worktree, the ref updates and the objects to copy.
//! Everything here is read-only.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use gitraptor_git::Oid;
use gitraptor_git::tm_write::files::Kind;
use gitraptor_git::tm_write::index::IndexEntry;
use gitraptor_git::tm_write::refs::{RefUpdate, branch_ref};
use gitraptor_git::tm_write::store::TreeEntryKind;
use gitraptor_git::tm_write::worktree::{HeadValue, WriteWorktree};

use super::{PlanWorktree, Refusal};
use crate::timemachine::store::{Meta, MetaWorktree, SnapshotStore};

/// One worktree, loaded.
pub(super) struct LoadedWorktree {
    pub key: String,
    pub root: PathBuf,
    /// `None` while the worktree is gone and will be recreated.
    pub worktree: Option<WriteWorktree>,
    pub recreate_id: Option<String>,
    /// What the prior snapshot says is in the working tree now.
    pub prior_files: BTreeMap<String, (Kind, Oid)>,
    /// What the target snapshot has.
    pub target_files: BTreeMap<String, (Kind, Oid)>,
    pub index: Vec<IndexEntry>,
    pub skip_worktree: Vec<Vec<u8>>,
    pub intent_to_add: Vec<String>,
    /// Paths left out of either snapshot: never written or removed.
    pub excluded: BTreeSet<String>,
    pub prior_head: Option<HeadValue>,
    pub target_head: Option<HeadValue>,
}

impl LoadedWorktree {
    pub fn is_excluded(&self, path: &str) -> bool {
        self.excluded.iter().any(|e| {
            path == e
                || path
                    .strip_prefix(e.as_str())
                    .is_some_and(|r| r.starts_with('/'))
        })
    }
}

pub(super) struct Loaded {
    pub worktrees: Vec<LoadedWorktree>,
    pub ref_updates: Vec<RefUpdate>,
    pub wants: Vec<Oid>,
    pub haves: Vec<Oid>,
    pub stash_moves: bool,
}

fn kind_of(kind: TreeEntryKind) -> Option<Kind> {
    match kind {
        TreeEntryKind::Blob => Some(Kind::File),
        TreeEntryKind::Executable => Some(Kind::Executable),
        TreeEntryKind::Symlink => Some(Kind::Symlink),
        TreeEntryKind::Gitlink | TreeEntryKind::Tree => None,
    }
}

fn mode_of(kind: TreeEntryKind) -> Option<u32> {
    match kind {
        TreeEntryKind::Blob => Some(0o100644),
        TreeEntryKind::Executable => Some(0o100755),
        TreeEntryKind::Symlink => Some(0o120000),
        TreeEntryKind::Gitlink => Some(0o160000),
        TreeEntryKind::Tree => None,
    }
}

fn mode_of_name(kind: &str) -> Option<u32> {
    match kind {
        "blob" => Some(0o100644),
        "executable" => Some(0o100755),
        "symlink" => Some(0o120000),
        "gitlink" => Some(0o160000),
        _ => None,
    }
}

fn oid(hex: &str, what: &str) -> Result<Oid, String> {
    Oid::from_hex(hex).ok_or_else(|| format!("{what}: not an object id"))
}

fn head_of(meta: Option<&MetaWorktree>) -> Result<Option<HeadValue>, String> {
    let Some(m) = meta else { return Ok(None) };
    if m.detached {
        let id = m
            .head_commit
            .as_deref()
            .ok_or("detached HEAD without commit")?;
        return Ok(Some(HeadValue::Detached(oid(id, "HEAD")?)));
    }
    match &m.head_branch {
        Some(name) => Ok(Some(HeadValue::Branch(
            branch_ref(&format!("refs/heads/{name}")).map_err(|e| e.to_string())?,
        ))),
        None => Ok(None),
    }
}

fn files_of(
    store: &SnapshotStore,
    snapshot: &str,
    key: &str,
) -> Result<BTreeMap<String, (Kind, Oid)>, String> {
    Ok(store
        .files(snapshot, key)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter_map(|(p, k, id)| kind_of(k).map(|k| (p, (k, id))))
        .collect())
}

/// Loads `plan`. Any problem is a refusal: nothing was touched.
pub(super) fn load(
    store: &SnapshotStore,
    target: &str,
    prior: &str,
    worktrees: &[PlanWorktree],
    move_refs: bool,
) -> Result<Loaded, Refusal> {
    let invalid = |snapshot: &str| {
        let snapshot = snapshot.to_owned();
        move |reason: String| Refusal::InvalidSnapshot {
            snapshot: snapshot.clone(),
            reason,
        }
    };
    for id in [target, prior] {
        store.verify(id).map_err(|e| invalid(id)(e.to_string()))?;
    }
    let target_meta: Meta = store
        .meta(target)
        .map_err(|e| invalid(target)(e.to_string()))?;
    let prior_meta: Meta = store
        .meta(prior)
        .map_err(|e| invalid(prior)(e.to_string()))?;

    let mut wants = BTreeSet::new();
    let mut haves = BTreeSet::new();
    let mut loaded = Vec::new();
    for plan in worktrees {
        let t_meta = target_meta.worktrees.iter().find(|w| w.key == plan.key);
        let Some(t_meta) = t_meta else {
            return Err(invalid(target)(format!("no worktree {:?}", plan.key)));
        };
        let p_meta = prior_meta.worktrees.iter().find(|w| w.key == plan.key);
        let worktree = match WriteWorktree::open(&plan.root) {
            Ok(w) => Some(w),
            Err(_) if plan.recreate_id.is_some() && !plan.root.exists() => None,
            Err(e) => {
                return Err(Refusal::WorktreeUnavailable {
                    worktree: plan.root.clone(),
                    reason: e.to_string(),
                });
            }
        };
        if worktree.is_some() && p_meta.is_none() {
            return Err(invalid(prior)(format!("no worktree {:?}", plan.key)));
        }
        let target_files = files_of(store, target, &plan.key).map_err(invalid(target))?;
        let prior_files = if p_meta.is_some() {
            files_of(store, prior, &plan.key).map_err(invalid(prior))?
        } else {
            BTreeMap::new()
        };

        let mut index = Vec::new();
        for (path, kind, id) in store
            .staged(target, &plan.key)
            .map_err(|e| invalid(target)(e.to_string()))?
        {
            let Some(mode) = mode_of(kind) else { continue };
            if kind != TreeEntryKind::Gitlink {
                wants.insert(id);
            }
            index.push(IndexEntry {
                path: path.into_bytes(),
                mode,
                id,
                stage: 0,
            });
        }
        for c in &t_meta.conflicts {
            let mode = mode_of_name(&c.kind)
                .ok_or_else(|| invalid(target)(format!("conflict kind {:?}", c.kind)))?;
            let id = oid(&c.id, "conflict").map_err(invalid(target))?;
            if mode != 0o160000 {
                wants.insert(id);
            }
            index.push(IndexEntry {
                path: c.path.clone().into_bytes(),
                mode,
                id,
                stage: c.stage,
            });
        }
        index.sort_by(|a, b| a.path.cmp(&b.path).then(a.stage.cmp(&b.stage)));

        // Exclusions are `<key>:<path>`; history gaps have no path.
        let prefix = format!("{}:", plan.key);
        let excluded = target_meta
            .exclusions
            .iter()
            .chain(prior_meta.exclusions.iter())
            .filter_map(|e| e.path.strip_prefix(&prefix))
            .filter(|p| !p.is_empty())
            .map(|p| p.trim_end_matches('/').to_owned())
            .collect();
        let target_head = head_of(Some(t_meta)).map_err(invalid(target))?;
        let prior_head = head_of(p_meta).map_err(invalid(prior))?;
        if let Some(HeadValue::Detached(id)) = &target_head {
            wants.insert(*id);
        }
        if let Some(m) = p_meta
            && let Some(id) = &m.head_commit
        {
            haves.insert(oid(id, "HEAD").map_err(invalid(prior))?);
        }
        loaded.push(LoadedWorktree {
            key: plan.key.clone(),
            root: plan.root.clone(),
            worktree,
            recreate_id: plan.recreate_id.clone(),
            prior_files,
            target_files,
            index,
            skip_worktree: t_meta
                .skip_worktree
                .iter()
                .map(|p| p.clone().into_bytes())
                .collect(),
            intent_to_add: t_meta.intent_to_add.clone(),
            excluded,
            prior_head,
            target_head,
        });
    }

    let mut ref_updates = Vec::new();
    let mut stash_moves = false;
    for id in prior_meta.branches.values().chain(prior_meta.stash.iter()) {
        haves.insert(oid(id, "branch").map_err(invalid(prior))?);
    }
    if move_refs {
        let names: BTreeSet<&String> = prior_meta
            .branches
            .keys()
            .chain(target_meta.branches.keys())
            .collect();
        for name in names {
            let old = prior_meta
                .branches
                .get(name)
                .map(|h| oid(h, "branch"))
                .transpose();
            let new = target_meta
                .branches
                .get(name)
                .map(|h| oid(h, "branch"))
                .transpose();
            let (old, new) = (old.map_err(invalid(prior))?, new.map_err(invalid(target))?);
            if old == new {
                continue;
            }
            wants.extend(new);
            let update = RefUpdate::new(&format!("refs/heads/{name}"), old, new)
                .map_err(|e| invalid(target)(e.to_string()))?;
            ref_updates.push(update);
        }
        let old = prior_meta
            .stash
            .as_deref()
            .map(|h| oid(h, "stash"))
            .transpose();
        let new = target_meta
            .stash
            .as_deref()
            .map(|h| oid(h, "stash"))
            .transpose();
        let (old, new) = (old.map_err(invalid(prior))?, new.map_err(invalid(target))?);
        if old != new {
            wants.extend(new);
            stash_moves = true;
            ref_updates.push(
                RefUpdate::new("refs/stash", old, new)
                    .map_err(|e| invalid(target)(e.to_string()))?,
            );
        }
    }
    let wants: Vec<Oid> = wants.difference(&haves).copied().collect();
    Ok(Loaded {
        worktrees: loaded,
        ref_updates,
        wants,
        haves: haves.into_iter().collect(),
        stash_moves,
    })
}
