//! What `raptor undo` prints for an `overlap` / `overlap-unverified` rejection and for the paths
//! an undo kept. Every text from the repo is printed sanitized.

use gitraptor_api::timemachine::{KeptPath, TmOverlap, TmRejectReason};

use crate::i18n::Tr;

#[cfg(test)]
#[path = "overlap_tests.rs"]
mod tests;

/// The lines of an overlap rejection: the reason, the undone change and one line per conflict
/// (the first 20 and "+K more" from the real total).
// Wired into `failure_text` together with the daemon side of the overlap.
#[allow(dead_code)]
pub(super) fn rejection_lines(
    tr: Tr,
    reason: TmRejectReason,
    overlap: Option<&TmOverlap>,
) -> Vec<String> {
    let _ = (tr, reason, overlap);
    Vec::new()
}

/// The lines announcing the paths an undo left as they were (the first 20 and "+K more"); none
/// when nothing was kept.
// Wired into `print_done` together with the daemon side of the overlap.
#[allow(dead_code)]
pub(super) fn kept_lines(tr: Tr, kept: &[KeptPath]) -> Vec<String> {
    let _ = (tr, kept);
    Vec::new()
}
