//! The child side of the worker: reads length-prefixed jobs from `input`, runs each with
//! [`InProcessMerger`](crate::predict::merger::InProcessMerger) and writes the result to
//! `output`. Sets its class `utility` first; EOF on `input` ends it. It opens no profile, no
//! channel, no daemon lock, and launches no process.

use std::io::{Read, Write};
use std::path::Path;
use std::time::Instant;

use gitraptor_api::resources::WorkClass;
use gitraptor_git::dry_merge::{
    DryMergeError, DryMergeInput, DryMergeLimits, DryMergeOutcome, MergeBaseUsed,
};

use super::{Hello, MAX_WORKER_FRAME, Reply, WireJob, announced_version};
use crate::predict::merger::{DryMerger, InProcessMerger};
use crate::work_class;

/// Longest path, in bytes, a conflict file may carry back to the daemon.
const MAX_PATH_BYTES: usize = 4096;
/// Most bases of a virtual merge-base the daemon accepts.
const MAX_VIRTUAL_BASES: usize = 64;

/// Serves jobs until EOF.
///
/// # Errors
///
/// A broken pipe or a malformed frame.
pub fn serve(input: &mut impl Read, output: &mut impl Write) -> std::io::Result<()> {
    // Before any job: the worker never runs above `utility`.
    work_class::set_current(WorkClass::Utility);
    limit_address_space();
    let hello = serde_json::to_vec(&Hello {
        version: announced_version(),
    })?;
    write_frame(output, &hello, MAX_WORKER_FRAME)?;
    output.flush()?;
    while let Some(frame) = read_frame(input, MAX_WORKER_FRAME)? {
        let job: WireJob = serde_json::from_slice(&frame)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let reply = serde_json::to_vec(&Reply::from_result(run(job)))?;
        if reply.len() > MAX_WORKER_FRAME {
            // An outcome too large for the wire is cut like any other job over a cap.
            let cut = serde_json::to_vec(&Reply::from_result(Err(DryMergeError::LimitExceeded)))?;
            write_frame(output, &cut, MAX_WORKER_FRAME)?;
        } else {
            write_frame(output, &reply, MAX_WORKER_FRAME)?;
        }
        output.flush()?;
    }
    Ok(())
}

fn run(job: WireJob) -> Result<DryMergeOutcome, DryMergeError> {
    let input = DryMergeInput {
        ours: job.ours,
        theirs: job.theirs,
    };
    let limits = DryMergeLimits::from(job.limits);
    // The daemon owns the deadline: it kills this process when the job runs past it.
    InProcessMerger.merge(Path::new(&job.repo), &input, &limits, Instant::now())
}

/// Address space of the worker on Linux: well above [`super::JOB_MEMORY_CAP`] of resident
/// memory, because it also counts the packs `gix` maps and reserved, untouched pages.
#[cfg(target_os = "linux")]
const WORKER_ADDRESS_SPACE: u64 = 8 * 1024 * 1024 * 1024;

/// Caps the worker's address space on Linux, where the kernel enforces it, so a job that runs
/// away fails in the worker before the daemon's next memory reading. Only ever lowers the limit.
#[cfg(target_os = "linux")]
fn limit_address_space() {
    use nix::sys::resource::{Resource, getrlimit, setrlimit};
    let Ok((soft, hard)) = getrlimit(Resource::RLIMIT_AS) else {
        return;
    };
    let cap = WORKER_ADDRESS_SPACE.min(hard);
    if cap < soft {
        // Best effort: the daemon's reading of the resident memory is the cap that counts.
        let _ = setrlimit(Resource::RLIMIT_AS, cap, hard);
    }
}

/// macOS does not enforce `RLIMIT_AS` and Windows has no such limit: the daemon's reading of the
/// resident memory is the only cap there (Windows: pendiente de la etapa de validación
/// multiplataforma, a Job Object).
#[cfg(not(target_os = "linux"))]
fn limit_address_space() {}

/// Reads one frame of at most `max` bytes; `None` on a clean EOF before it.
///
/// # Errors
///
/// A frame over `max`, a truncated frame, or a read error.
pub fn read_frame(input: &mut impl Read, max: usize) -> std::io::Result<Option<Vec<u8>>> {
    let mut header = [0u8; 4];
    let mut got = 0;
    while got < header.len() {
        match input.read(&mut header[got..]) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "a frame header cut short",
                ));
            }
            Ok(n) => got += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    let len = usize::try_from(u32::from_be_bytes(header)).unwrap_or(usize::MAX);
    if len > max {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("a frame of {len} bytes, over the cap of {max}"),
        ));
    }
    let mut body = vec![0u8; len];
    input.read_exact(&mut body)?;
    Ok(Some(body))
}

/// Writes one frame.
///
/// # Errors
///
/// A frame over `max`, or a write error.
pub fn write_frame(output: &mut impl Write, frame: &[u8], max: usize) -> std::io::Result<()> {
    let len = u32::try_from(frame.len())
        .ok()
        .filter(|_| frame.len() <= max)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("a frame of {} bytes, over the cap of {max}", frame.len()),
            )
        })?;
    output.write_all(&len.to_be_bytes())?;
    output.write_all(frame)
}

/// Checks an outcome the worker sent before the daemon uses it: no more files, ranges or path
/// bytes than `limits` allow. The worker is untrusted.
///
/// # Errors
///
/// What breaks the caps.
pub fn validate_outcome(outcome: &DryMergeOutcome, limits: &DryMergeLimits) -> Result<(), String> {
    if outcome.files.len() > limits.max_files {
        return Err(format!(
            "{} files, over the cap of {}",
            outcome.files.len(),
            limits.max_files
        ));
    }
    let bases: &[String] = match &outcome.merge_base {
        MergeBaseUsed::Single(id) => std::slice::from_ref(id),
        MergeBaseUsed::Virtual { bases } => bases,
        MergeBaseUsed::None => &[],
    };
    if bases.len() > MAX_VIRTUAL_BASES || !bases.iter().all(|id| is_object_id(id)) {
        return Err("a merge-base that is not an object id".into());
    }
    for file in &outcome.files {
        if file.path.is_empty() || file.path.len() > MAX_PATH_BYTES || file.path.contains('\0') {
            return Err(format!("a path of {} bytes", file.path.len()));
        }
        if file.ours.len() > limits.max_hunks_per_file
            || file.theirs.len() > limits.max_hunks_per_file
        {
            return Err(format!(
                "more ranges than the cap of {} in a file",
                limits.max_hunks_per_file
            ));
        }
    }
    Ok(())
}

/// A SHA-1 or SHA-256 id in lowercase hex.
fn is_object_id(id: &str) -> bool {
    matches!(id.len(), 40 | 64)
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
