//! Per-repo bookkeeping of the predicted pairs: which ones still lack their detail.

use gitraptor_api::prediction::{PairDetail, PredictionChanged, PredictionSummary};

/// The `rev`, level and state of each pair, and the detail fetched for it.
#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct PredictionTracker {}

#[allow(dead_code)]
impl PredictionTracker {
    pub fn reset(&mut self, _summary: Option<&PredictionSummary>) {}

    pub fn apply(&mut self, _changed: &PredictionChanged) {}

    pub fn adopt(&mut self, _details: &[PairDetail]) {}

    /// Ids of the conflict and overlap pairs whose detail is missing, conflicts first.
    pub fn wanted(&self) -> Vec<String> {
        Vec::new()
    }

    /// The snapshot was truncated and the cursor walk has not finished.
    pub fn needs_fill(&self) -> bool {
        false
    }
}
