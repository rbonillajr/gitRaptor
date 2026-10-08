//! Sweep of the applier's temporary entries after a crash (DS-TS-TMC-003, Enmienda T; L-02 of the
//! security review of XP-12; INF-TMC-001).
//!
//! A replacement on Windows is two exclusive renames, and a removal anywhere is a rename aside and
//! a comparison: a crash between them leaves the displaced entry under a `.gitraptor-tm-*` name
//! next to its path, the path absent and the operation `interrupted` at step 6. When the daemon
//! starts, after the recovery of the oplog and before any Time Machine operation is accepted, this
//! sweep looks at the folders of the prior and the target snapshots of the last operation of the
//! repo, if it is such an operation:
//!
//! - A temporary entry whose content is the prior content of exactly one path of its folder, and
//!   that path is free, goes back there with one exclusive rename. Nothing else is written.
//! - Any other temporary entry is left as it is and reported, with `raptor undo` as the way back:
//!   its path is taken, several paths could be it, its content is not one the prior snapshot has
//!   there (the new content of the interrupted write, or someone else's), or it changed.
//!
//! Nothing is ever deleted (NFR-01): a kept entry is an untracked file, so the guaranteed prior
//! snapshot of the next operation captures it. Every look and every rename goes through
//! [`RootDir`], relative to the root and never following a link or reparse point (SEC-TMC-04).
//! Running it twice changes nothing the second time.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use gitraptor_git::Oid;
use gitraptor_git::tm_write::files::{Kind, Restore, RootDir, TempEntry};

use super::oplog::{OperationState, Oplog, Target};
use super::store::SnapshotStore;

/// The applier step that writes the working tree: the only one that leaves temporary entries.
const FILES_STEP: u32 = 6;

/// The way back offered with every temporary entry kept.
pub const SUGGESTED_ACTION: &str = "raptor undo";

/// A temporary entry put back at its path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoredTemp {
    pub operation_id: String,
    pub worktree: PathBuf,
    /// The path, relative to the worktree, with `/`.
    pub path: String,
}

/// Why a temporary entry was left where it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeptReason {
    /// Its content is the prior content of its path, but something else is there now.
    PathOccupied,
    /// Its content is the prior content of several free paths of its folder.
    Ambiguous,
    /// Its content is not the prior content of any path of its folder: the new content of the
    /// interrupted write, or someone else's.
    Unknown,
    /// It changed, another entry took its name, or another program holds it, between the look
    /// and the rename.
    Changed,
    /// The file system has no exclusive rename, or a folder on the way changed.
    NotGuaranteed,
}

impl KeptReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::PathOccupied => "path-occupied",
            Self::Ambiguous => "ambiguous",
            Self::Unknown => "unknown",
            Self::Changed => "changed",
            Self::NotGuaranteed => "not-guaranteed",
        }
    }
}

/// A temporary entry left where it is, to report with [`SUGGESTED_ACTION`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeptTemp {
    pub operation_id: String,
    pub worktree: PathBuf,
    /// The temporary entry, relative to the worktree, with `/`.
    pub temp: String,
    /// The path it most likely belongs to, when exactly one matches its content.
    pub path: Option<String>,
    pub reason: KeptReason,
    /// Its content is a blob of the prior or the target snapshot: it is in the store. Otherwise
    /// it is someone else's, and only here.
    pub in_store: bool,
}

/// What the sweep did in one repo.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SweepReport {
    pub restored: Vec<RestoredTemp>,
    pub kept: Vec<KeptTemp>,
    /// Another entry took the temporary name between the last check and the rename, and is at
    /// the path now (Enmienda T2): nothing was overwritten or deleted, and it is not moved again.
    /// Not a kept entry: there is no temporary name left.
    pub moved_unverified: Vec<RestoredTemp>,
    /// Worktrees or snapshots that could not be read: nothing was touched there.
    pub unreadable: usize,
    /// Only the folders of the prior snapshot were swept: the target was not known by id or could
    /// not be read. A temporary entry in a folder only the target has is not reported.
    pub partial: bool,
}

impl SweepReport {
    pub fn is_clean(&self) -> bool {
        self.restored.is_empty()
            && self.kept.is_empty()
            && self.moved_unverified.is_empty()
            && self.unreadable == 0
    }
}

/// Sweeps the temporary entries left by the last operation of the repo, if it was interrupted
/// while writing files (see the module docs). An older interruption was already followed by
/// another operation, whose guaranteed prior snapshot captured its leftovers. Never fails: what
/// cannot be read is counted and left alone.
pub fn sweep_temps(oplog: &Oplog, store: &SnapshotStore) -> SweepReport {
    let mut report = SweepReport::default();
    let Ok(operations) = oplog.operations(&Default::default()) else {
        report.unreadable += 1;
        return report;
    };
    let Some(op) = operations.last() else {
        return report;
    };
    if op.state != OperationState::Interrupted || op.step != Some(FILES_STEP) {
        return report;
    }
    let Some(prior) = op.prior_snapshot.as_deref() else {
        return report;
    };
    let Ok(meta) = store.meta(prior) else {
        report.unreadable += 1;
        return report;
    };
    // The target is known by id only for a restore; for an undo or a redo the folders of the
    // prior snapshot are swept and the report says it is partial.
    let target = match &op.record.target {
        Target::Snapshot(id) => Some(id.as_str()),
        _ => {
            report.partial = true;
            None
        }
    };
    for wt in &meta.worktrees {
        let Some(prior_files) = files_of(store, prior, &wt.key) else {
            report.unreadable += 1;
            continue;
        };
        let target_files = match target.map(|t| files_of(store, t, &wt.key)) {
            Some(Some(files)) => files,
            Some(None) => {
                report.partial = true;
                BTreeMap::new()
            }
            None => BTreeMap::new(),
        };
        let sweep = Worktree {
            operation_id: &op.record.operation_id,
            root: Path::new(&wt.path),
            prior: &prior_files,
            target: &target_files,
        };
        if sweep.run(&mut report).is_err() {
            report.unreadable += 1;
        }
    }
    report
}

fn files_of(
    store: &SnapshotStore,
    snapshot: &str,
    key: &str,
) -> Option<BTreeMap<String, (Kind, Oid)>> {
    Some(
        store
            .files(snapshot, key)
            .ok()?
            .into_iter()
            .filter_map(|(p, k, id)| kind_of(k).map(|k| (p, (k, id))))
            .collect(),
    )
}

fn kind_of(kind: gitraptor_git::tm_write::store::TreeEntryKind) -> Option<Kind> {
    use gitraptor_git::tm_write::store::TreeEntryKind as K;
    match kind {
        K::Blob => Some(Kind::File),
        K::Executable => Some(Kind::Executable),
        K::Symlink => Some(Kind::Symlink),
        K::Gitlink | K::Tree => None,
    }
}

fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map(|(f, _)| f).unwrap_or("")
}

fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{folder}/{name}")
    }
}

/// One worktree of the swept operation.
struct Worktree<'a> {
    operation_id: &'a str,
    root: &'a Path,
    prior: &'a BTreeMap<String, (Kind, Oid)>,
    target: &'a BTreeMap<String, (Kind, Oid)>,
}

impl Worktree<'_> {
    fn run(&self, report: &mut SweepReport) -> gitraptor_git::tm_write::Result<()> {
        if !self.root.is_dir() {
            return Ok(());
        }
        let root = RootDir::open(self.root)?;
        let mut folders: BTreeSet<&str> = self
            .prior
            .keys()
            .chain(self.target.keys())
            .map(|p| folder_of(p))
            .collect();
        folders.insert("");
        let in_store: HashSet<Oid> = self
            .prior
            .values()
            .chain(self.target.values())
            .map(|(_, id)| *id)
            .collect();
        for folder in folders {
            for temp in root.temps(folder.as_bytes())? {
                if let Some(kept) = self.one(&root, folder, &temp, report)? {
                    report.kept.push(KeptTemp {
                        operation_id: self.operation_id.to_owned(),
                        worktree: self.root.to_owned(),
                        temp: join(folder, &temp.name),
                        path: kept.0,
                        reason: kept.1,
                        in_store: in_store.contains(&temp.id),
                    });
                }
            }
        }
        Ok(())
    }

    /// Restores one temporary entry or says why it stays.
    fn one(
        &self,
        root: &RootDir,
        folder: &str,
        temp: &TempEntry,
        report: &mut SweepReport,
    ) -> gitraptor_git::tm_write::Result<Option<(Option<String>, KeptReason)>> {
        let matching = paths_holding(self.prior, folder, temp);
        if matching.is_empty() {
            return Ok(Some((None, KeptReason::Unknown)));
        }
        let mut free = Vec::new();
        for path in &matching {
            if root.current(path.as_bytes())?.is_none() {
                free.push(path.clone());
            }
        }
        let path = match free.as_slice() {
            [] => {
                let path = (matching.len() == 1).then(|| matching[0].clone());
                return Ok(Some((path, KeptReason::PathOccupied)));
            }
            [one] => one.clone(),
            _ => return Ok(Some((None, KeptReason::Ambiguous))),
        };
        let reason = match root.restore_temp(path.as_bytes(), &temp.name, self.prior[&path])? {
            Restore::Restored => {
                report.restored.push(RestoredTemp {
                    operation_id: self.operation_id.to_owned(),
                    worktree: self.root.to_owned(),
                    path,
                });
                return Ok(None);
            }
            Restore::Swapped => {
                report.moved_unverified.push(RestoredTemp {
                    operation_id: self.operation_id.to_owned(),
                    worktree: self.root.to_owned(),
                    path,
                });
                return Ok(None);
            }
            Restore::Occupied => KeptReason::PathOccupied,
            Restore::Mismatch | Restore::Busy => KeptReason::Changed,
            Restore::Blocked | Restore::NotGuaranteed => KeptReason::NotGuaranteed,
        };
        Ok(Some((Some(path), reason)))
    }
}

/// Paths of `folder` whose prior content is the content of `temp`. Compared by blob id: the kind
/// is checked again, as the platform compares it, by the exclusive rename.
fn paths_holding(
    prior_files: &BTreeMap<String, (Kind, Oid)>,
    folder: &str,
    temp: &TempEntry,
) -> Vec<String> {
    prior_files
        .iter()
        .filter(|(p, (_, id))| folder_of(p) == folder && *id == temp.id)
        .map(|(p, _)| p.clone())
        .collect()
}
