//! Which predicted conflicts the cockpit already knows, so it only announces a new one.
//!
//! A pair enters the set with a `current` ⚡ and leaves it only with a `current` result without
//! one, a `removed` or an absence from a complete snapshot. Every other state keeps it, so a
//! recalculation, a moved base or a restarted daemon never repeats an alert. Nothing is kept
//! between runs. Pure: no clock, no I/O.

use std::collections::{BTreeMap, BTreeSet};

use gitraptor_api::prediction::{PairLevel, PairStateView};

use crate::tui::prediction::PairRow;

/// Ticks a new conflict waits for its detail before it is announced without a file.
const DETAIL_TICKS: u8 = 2;

/// How the set came to know a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Known {
    /// It was already there when the repo was first seen in this run: never announced.
    Seeded,
    /// It appeared in this run: announced and marked "new" while it lasts.
    Alerted,
}

/// What the model just ingested.
#[derive(Debug, Clone, Copy)]
pub enum Batch<'a> {
    /// The summary of a snapshot; `complete` is `!pairs_truncated`.
    Snapshot {
        repo_id: &'a str,
        pairs: &'a [PairRow],
        complete: bool,
    },
    /// The pairs a `prediction.list` page touched.
    Page {
        repo_id: &'a str,
        pairs: &'a [PairRow],
    },
    /// The pairs of one `prediction.changed` and the ids it removed.
    Changed {
        repo_id: &'a str,
        pairs: &'a [PairRow],
        removed: &'a [String],
    },
}

#[derive(Debug, Clone, Default)]
pub struct ConflictAlerts {
    known: BTreeMap<String, BTreeMap<String, Known>>,
    seeded: BTreeSet<String>,
    /// Repos whose snapshot was cut: the pages of the walk still seed.
    walking: BTreeSet<String>,
    /// New conflicts waiting for their detail, with the ticks waited.
    pending: BTreeMap<(String, String), u8>,
}

fn is_bolt(p: &PairRow) -> bool {
    p.level == Some(PairLevel::Conflict)
        && matches!(
            p.state,
            PairStateView::Current | PairStateView::Recalculating
        )
}

impl ConflictAlerts {
    /// Applies a batch and returns the `pair_id`s that just became a new ⚡.
    pub fn observe(&mut self, batch: Batch<'_>) -> Vec<String> {
        match batch {
            Batch::Snapshot {
                repo_id,
                pairs,
                complete,
            } => {
                let first = self.seeded.insert(repo_id.to_owned());
                // Only the snapshot that seeds the repo lets the pages seed too: a later one,
                // truncated or not, is compared (E1).
                if complete || !first {
                    self.walking.remove(repo_id);
                } else {
                    self.walking.insert(repo_id.to_owned());
                }
                if first {
                    self.seed(repo_id, pairs);
                    return Vec::new();
                }
                let new = self.apply(repo_id, pairs);
                if complete {
                    let present: BTreeSet<&str> =
                        pairs.iter().map(|p| p.pair_id.as_str()).collect();
                    let gone: Vec<String> = self
                        .known
                        .get(repo_id)
                        .into_iter()
                        .flat_map(|k| k.keys())
                        .filter(|id| !present.contains(id.as_str()))
                        .cloned()
                        .collect();
                    for id in gone {
                        self.leave(repo_id, &id);
                    }
                }
                new
            }
            Batch::Page { repo_id, pairs } => {
                if !self.seeded.contains(repo_id) || self.walking.contains(repo_id) {
                    self.seed(repo_id, pairs);
                    Vec::new()
                } else {
                    self.apply(repo_id, pairs)
                }
            }
            Batch::Changed {
                repo_id,
                pairs,
                removed,
            } => {
                let new = self.apply(repo_id, pairs);
                for id in removed {
                    self.leave(repo_id, id);
                }
                new
            }
        }
    }

    /// The repo is no longer observed: everything about it is forgotten.
    pub fn forget(&mut self, repo_id: &str) {
        self.known.remove(repo_id);
        self.seeded.remove(repo_id);
        self.walking.remove(repo_id);
        self.pending.retain(|(r, _), _| r != repo_id);
    }

    /// Whether the pair is a conflict that appeared in this run and is still one.
    pub fn is_new(&self, repo_id: &str, pair_id: &str) -> bool {
        self.known
            .get(repo_id)
            .and_then(|k| k.get(pair_id))
            .is_some_and(|k| *k == Known::Alerted)
    }

    /// Notes that these new conflicts wait for their detail.
    pub fn await_detail(&mut self, repo_id: &str, pair_ids: Vec<String>) {
        for id in pair_ids {
            self.pending.insert((repo_id.to_owned(), id), 0);
        }
    }

    /// Takes the waiting conflicts of `repo_id` whose detail `has_detail` says arrived.
    pub fn ready(&mut self, repo_id: &str, has_detail: impl Fn(&str) -> bool) -> Vec<String> {
        let ready: Vec<String> = self
            .pending
            .keys()
            .filter(|(r, id)| r == repo_id && has_detail(id))
            .map(|(_, id)| id.clone())
            .collect();
        for id in &ready {
            self.pending.remove(&(repo_id.to_owned(), id.clone()));
        }
        ready
    }

    /// One tick of waiting: returns the conflicts that waited long enough, now taken out.
    pub fn tick_pending(&mut self) -> Vec<(String, String)> {
        for ticks in self.pending.values_mut() {
            *ticks = ticks.saturating_add(1);
        }
        let due: Vec<(String, String)> = self
            .pending
            .iter()
            .filter(|(_, t)| **t >= DETAIL_TICKS)
            .map(|(k, _)| k.clone())
            .collect();
        for key in &due {
            self.pending.remove(key);
        }
        due
    }

    fn seed(&mut self, repo_id: &str, pairs: &[PairRow]) {
        let known = self.known.entry(repo_id.to_owned()).or_default();
        for p in pairs.iter().filter(|p| is_bolt(p)) {
            known.entry(p.pair_id.clone()).or_insert(Known::Seeded);
        }
    }

    fn apply(&mut self, repo_id: &str, pairs: &[PairRow]) -> Vec<String> {
        let mut new = Vec::new();
        for p in pairs {
            if p.state != PairStateView::Current {
                continue;
            }
            match p.level {
                Some(PairLevel::Conflict) => {
                    let known = self.known.entry(repo_id.to_owned()).or_default();
                    if !known.contains_key(&p.pair_id) {
                        known.insert(p.pair_id.clone(), Known::Alerted);
                        new.push(p.pair_id.clone());
                    }
                }
                Some(PairLevel::Clean | PairLevel::Overlap) => self.leave(repo_id, &p.pair_id),
                None => {}
            }
        }
        new
    }

    fn leave(&mut self, repo_id: &str, pair_id: &str) {
        if let Some(known) = self.known.get_mut(repo_id) {
            known.remove(pair_id);
        }
        self.pending
            .remove(&(repo_id.to_owned(), pair_id.to_owned()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::prediction::Side;

    fn row(id: &str, rev: u64, state: PairStateView, level: Option<PairLevel>) -> PairRow {
        PairRow {
            pair_id: id.into(),
            rev,
            a: Side::Base,
            b: Side::Base,
            state,
            reason: None,
            level,
            computed_at_utc_ms: None,
            overlap_files: 0,
            overlap_partial: false,
            conflict_files: 0,
            conflict_truncated: false,
        }
    }

    fn bolt(id: &str, rev: u64) -> PairRow {
        row(id, rev, PairStateView::Current, Some(PairLevel::Conflict))
    }

    fn snap(a: &mut ConflictAlerts, pairs: &[PairRow], complete: bool) -> Vec<String> {
        a.observe(Batch::Snapshot {
            repo_id: "r",
            pairs,
            complete,
        })
    }

    fn changed(a: &mut ConflictAlerts, pairs: &[PairRow], removed: &[&str]) -> Vec<String> {
        let removed: Vec<String> = removed.iter().map(|s| (*s).to_owned()).collect();
        a.observe(Batch::Changed {
            repo_id: "r",
            pairs,
            removed: &removed,
        })
    }

    fn opened() -> ConflictAlerts {
        let mut a = ConflictAlerts::default();
        assert!(snap(&mut a, &[], true).is_empty());
        a
    }

    #[test]
    fn opening_seeds_without_alerting() {
        let mut a = ConflictAlerts::default();
        assert!(snap(&mut a, &[bolt("a", 1), bolt("b", 1)], true).is_empty());
        assert!(!a.is_new("r", "a") && !a.is_new("r", "b"));
        assert_eq!(a.known["r"].len(), 2);
    }

    #[test]
    fn a_new_conflict_alerts_once_and_is_marked() {
        let mut a = opened();
        assert_eq!(changed(&mut a, &[bolt("a", 1)], &[]), ["a"]);
        assert!(a.is_new("r", "a"));
        assert!(changed(&mut a, &[bolt("a", 2)], &[]).is_empty());
    }

    #[test]
    fn base_moves_the_same_conflict_returns_and_no_toast() {
        let mut a = opened();
        changed(&mut a, &[bolt("a", 1)], &[]);
        let calc = row("a", 2, PairStateView::Calculating, None);
        assert!(changed(&mut a, &[calc], &[]).is_empty());
        assert!(changed(&mut a, &[bolt("a", 3)], &[]).is_empty());
    }

    #[test]
    fn recalculating_to_current_with_the_same_conflict_does_not_alert() {
        let mut a = ConflictAlerts::default();
        let rec = row(
            "a",
            1,
            PairStateView::Recalculating,
            Some(PairLevel::Conflict),
        );
        assert!(snap(&mut a, &[rec], true).is_empty());
        assert!(changed(&mut a, &[bolt("a", 2)], &[]).is_empty());
    }

    #[test]
    fn a_conflict_that_disappears_and_returns_alerts() {
        let mut a = opened();
        changed(&mut a, &[bolt("a", 1)], &[]);
        let overlap = row("a", 2, PairStateView::Current, Some(PairLevel::Overlap));
        assert!(changed(&mut a, &[overlap], &[]).is_empty());
        assert_eq!(changed(&mut a, &[bolt("a", 3)], &[]), ["a"]);
        assert!(changed(&mut a, &[], &["a"]).is_empty());
        assert_eq!(changed(&mut a, &[bolt("a", 4)], &[]), ["a"]);
    }

    #[test]
    fn pending_and_not_computable_keep_the_pair() {
        let mut a = opened();
        changed(&mut a, &[bolt("a", 1)], &[]);
        for (rev, state) in [
            (2, PairStateView::Pending),
            (3, PairStateView::NotComputable),
        ] {
            assert!(changed(&mut a, &[row("a", rev, state, None)], &[]).is_empty());
        }
        assert!(changed(&mut a, &[bolt("a", 4)], &[]).is_empty());
    }

    #[test]
    fn a_truncated_snapshot_drops_no_absent_pair() {
        let mut a = ConflictAlerts::default();
        snap(&mut a, &[bolt("a", 1)], true);
        snap(&mut a, &[], false);
        assert!(changed(&mut a, &[bolt("a", 2)], &[]).is_empty());
        snap(&mut a, &[], true);
        assert_eq!(changed(&mut a, &[bolt("a", 3)], &[]), ["a"]);
    }

    #[test]
    fn a_page_after_a_later_truncated_snapshot_compares_instead_of_seeding() {
        let mut a = ConflictAlerts::default();
        snap(&mut a, &[], false);
        snap(&mut a, &[], true);
        let new = a.observe(Batch::Page {
            repo_id: "r",
            pairs: &[bolt("x", 1)],
        });
        assert_eq!(new, ["x"]);
    }

    #[test]
    fn pages_of_a_cut_snapshot_seed_and_forget_clears_the_repo() {
        let mut a = ConflictAlerts::default();
        snap(&mut a, &[], false);
        assert!(
            a.observe(Batch::Page {
                repo_id: "r",
                pairs: &[bolt("a", 1)]
            })
            .is_empty()
        );
        assert!(changed(&mut a, &[bolt("a", 2)], &[]).is_empty());
        a.forget("r");
        assert!(a.known.is_empty() && !a.seeded.contains("r"));
    }

    #[test]
    fn a_pending_conflict_waits_two_ticks_and_leaves_with_its_pair() {
        let mut a = opened();
        a.await_detail("r", vec!["a".into(), "b".into()]);
        assert!(a.tick_pending().is_empty());
        assert_eq!(a.ready("r", |id| id == "b"), ["b"]);
        assert_eq!(a.tick_pending(), [("r".to_owned(), "a".to_owned())]);
        a.await_detail("r", vec!["c".into()]);
        changed(&mut a, &[], &["c"]);
        assert!(a.tick_pending().is_empty() && a.tick_pending().is_empty());
    }
}
