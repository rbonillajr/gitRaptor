//! The child side of the worker: reads length-prefixed jobs from `input`, runs each with
//! [`InProcessMerger`](crate::predict::merger::InProcessMerger) and writes the result to
//! `output`. Sets its class `utility` first; EOF on `input` ends it. It opens no profile, no
//! channel, no daemon lock, and launches no process.

use std::io::{Read, Write};

use gitraptor_git::dry_merge::{DryMergeLimits, DryMergeOutcome};

/// Serves jobs until EOF.
///
/// # Errors
///
/// A broken pipe or a malformed frame.
pub fn serve(input: &mut impl Read, output: &mut impl Write) -> std::io::Result<()> {
    let _ = (input, output);
    unimplemented!("T006")
}

/// Reads one frame of at most `max` bytes; `None` on a clean EOF before it.
///
/// # Errors
///
/// A frame over `max`, a truncated frame, or a read error.
pub fn read_frame(input: &mut impl Read, max: usize) -> std::io::Result<Option<Vec<u8>>> {
    let _ = (input, max);
    unimplemented!("T006")
}

/// Writes one frame.
///
/// # Errors
///
/// A frame over `max`, or a write error.
pub fn write_frame(output: &mut impl Write, frame: &[u8], max: usize) -> std::io::Result<()> {
    let _ = (output, frame, max);
    unimplemented!("T006")
}

/// Checks an outcome the worker sent before the daemon uses it: no more files, ranges or path
/// bytes than `limits` allow. The worker is untrusted.
///
/// # Errors
///
/// What breaks the caps.
pub fn validate_outcome(outcome: &DryMergeOutcome, limits: &DryMergeLimits) -> Result<(), String> {
    let _ = (outcome, limits);
    unimplemented!("T006")
}
