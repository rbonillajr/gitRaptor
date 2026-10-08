//! The temporary entries the sweep after a crash kept, for `raptor status` (DS-TS-TMC-003,
//! Enmienda T2).
//!
//! The sweep runs when the Time Machine of a repo is recovered (at startup and when a repo is
//! observed again) and records here what it left where it was. Nothing is persisted: the next
//! start sweeps again and finds them again. A snapshot served to a connection with
//! `timemachine.kept-temps` counts the ones **still there** at that moment, so once `raptor undo`
//! (or anyone) takes them away, the line goes without any other signal. The `repo.*` events
//! never carry it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use gitraptor_api::messages::RepoView;
use gitraptor_api::timemachine::KeptTempsView;
use gitraptor_git::tm_write::files::is_temp_name;

use super::oplog::{OperationFilter, OperationState, Oplog};
use super::protected::TmRepos;
use super::sweep::SweepReport;

/// One kept temporary entry.
#[derive(Debug, Clone)]
struct Entry {
    worktree: PathBuf,
    /// Relative to the worktree, with `/`.
    temp: String,
    in_store: bool,
}

#[derive(Debug, Clone)]
struct Kept {
    operation_id: String,
    entries: Vec<Entry>,
}

/// Kept temporary entries of every observed repo, by repo id.
#[derive(Debug, Default)]
pub struct KeptTemps {
    repos: Mutex<BTreeMap<String, Kept>>,
}

impl KeptTemps {
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Kept>> {
        self.repos.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// What the sweep of `repo_id` kept, replacing what an earlier sweep of it recorded: the
    /// sweep is the only source, and a repo is swept again only after it was retired (which
    /// forgets it) or when the daemon starts.
    pub fn record(&self, repo_id: &str, report: &SweepReport) {
        let Some(first) = report.kept.first() else {
            self.forget(repo_id);
            return;
        };
        let kept = Kept {
            operation_id: first.operation_id.clone(),
            entries: report
                .kept
                .iter()
                .map(|k| Entry {
                    worktree: k.worktree.clone(),
                    temp: k.temp.clone(),
                    in_store: k.in_store,
                })
                .collect(),
        };
        self.lock().insert(repo_id.to_owned(), kept);
    }

    /// A retired repo: nothing of it is shown any more.
    pub fn forget(&self, repo_id: &str) {
        self.lock().remove(repo_id);
    }

    fn get(&self, repo_id: &str) -> Option<Kept> {
        self.lock().get(repo_id).cloned()
    }
}

/// Fills `kept_temps` of every repo of a snapshot with the kept entries still there, inside a
/// worktree the repo still has, and under a temporary name of the applier. One `lstat` per kept
/// entry: never follows a link.
pub fn refresh_kept_temps(tm: &TmRepos, repos: &mut [RepoView]) {
    for repo in repos {
        let Some(kept) = tm.kept.get(&repo.repo_id) else {
            continue;
        };
        let worktrees: Vec<&Path> = repo
            .worktrees
            .iter()
            .map(|w| Path::new(w.path.raw()))
            .collect();
        let there: Vec<&Entry> = kept
            .entries
            .iter()
            .filter(|e| worktrees.contains(&e.worktree.as_path()) && still_there(e))
            .collect();
        if there.is_empty() {
            continue;
        }
        let undo_next = tm
            .oplog(&repo.repo_id)
            .is_some_and(|oplog| match oplog.try_lock() {
                Ok(oplog) => last_applied(&oplog, &kept.operation_id),
                // Busy with an operation: whether it is still the last one is not known.
                Err(_) => false,
            });
        let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        repo.kept_temps = Some(KeptTempsView {
            count: count(there.len()),
            foreign: count(there.iter().filter(|e| !e.in_store).count()),
            operation_id: kept.operation_id,
            undo_next,
        });
    }
}

fn still_there(entry: &Entry) -> bool {
    let name = entry.temp.rsplit('/').next().unwrap_or(&entry.temp);
    is_temp_name(name)
        && entry
            .worktree
            .join(&entry.temp)
            .symlink_metadata()
            .is_ok_and(|m| !m.is_dir())
}

/// Whether `operation_id` is still the last operation of the oplog that touched the repo: every
/// later one was rejected, aborted or never started applying.
fn last_applied(oplog: &Oplog, operation_id: &str) -> bool {
    let Ok(Some(op)) = oplog.operation(operation_id) else {
        return false;
    };
    let later = oplog.operations(&OperationFilter {
        from_ms: Some(op.record.recorded_ms),
        ..Default::default()
    });
    later.is_ok_and(|ops| {
        !ops.iter().any(|o| {
            o.record.seq > op.record.seq
                && matches!(
                    o.state,
                    OperationState::Applying
                        | OperationState::Finished
                        | OperationState::Interrupted
                )
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::ProfileDirs;
    use crate::timemachine::oplog::{
        Channel, CompleteInfo, NewOperation, NewSnapshot, OperationKind, OperationTransition,
        Requester, Scope, SnapshotLevel, Target,
    };

    fn new_op() -> NewOperation {
        NewOperation {
            kind: OperationKind::Restore,
            subtype: None,
            scope: Scope {
                worktrees: vec!["/w".into()],
                refs: vec![],
            },
            requester: Requester::Unattributed,
            channel: Channel::Cli,
            confirmed: true,
            target: Target::None,
            warnings: vec![],
            engine_mark: 1,
        }
    }

    #[test]
    fn the_undo_is_offered_while_no_later_operation_touched_the_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = ProfileDirs::under_root(tmp.path().join("profile"));
        let (mut log, _) = Oplog::open(&dirs, "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0", 1).unwrap();
        let interrupted = log.record_operation(&new_op(), 10).unwrap();
        assert!(last_applied(&log, &interrupted));
        // A rejected one never touched it.
        let rejected = log.record_operation(&new_op(), 11).unwrap();
        log.advance_operation(
            &rejected,
            OperationTransition::Rejected { reason: "busy" },
            11,
        )
        .unwrap();
        assert!(last_applied(&log, &interrupted));
        // One that started applying did.
        let later = log.record_operation(&new_op(), 12).unwrap();
        let snap = log
            .begin_snapshot(
                &NewSnapshot {
                    level: SnapshotLevel::GuaranteedPrior,
                    worktrees: vec!["/w".into()],
                    engine_mark: Some(1),
                    cause_operation: Some(later.clone()),
                    cause_event_seq: None,
                },
                12,
            )
            .unwrap();
        log.complete_snapshot(&snap, &CompleteInfo::default(), 12)
            .unwrap();
        for t in [
            OperationTransition::PriorSnapshot { snapshot_id: &snap },
            OperationTransition::Ready,
            OperationTransition::Applying { step: 1 },
        ] {
            log.advance_operation(&later, t, 12).unwrap();
        }
        assert!(!last_applied(&log, &interrupted));
        assert!(!last_applied(&log, "unknown"));
    }

    #[test]
    fn only_a_temporary_name_still_there_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let entry = |temp: &str| Entry {
            worktree: tmp.path().to_owned(),
            temp: temp.into(),
            in_store: true,
        };
        std::fs::create_dir(tmp.path().join("d")).unwrap();
        std::fs::write(tmp.path().join("d/.gitraptor-tm-1"), b"x").unwrap();
        std::fs::write(tmp.path().join(".gitraptor-tm-notes"), b"x").unwrap();
        std::fs::create_dir(tmp.path().join(".gitraptor-tm-2")).unwrap();
        assert!(still_there(&entry("d/.gitraptor-tm-1")));
        assert!(!still_there(&entry(".gitraptor-tm-3")), "gone");
        assert!(!still_there(&entry(".gitraptor-tm-notes")), "not ours");
        assert!(!still_there(&entry(".gitraptor-tm-2")), "a folder");
    }
}
