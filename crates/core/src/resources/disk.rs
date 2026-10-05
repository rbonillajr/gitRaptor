//! Disk the profile takes (RES-05) and the Time Machine of each repo
//! (RES-09), as allocated on disk. Read by the daemon and, with the engine
//! stopped, by the CLI (US-GRP-017) and `raptor doctor` (US-GRP-018).
//!
//! Symbolic links are never followed, and the read is bounded in time and
//! entries: past either bound the sizes are lower bounds
//! (`complete: false`).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gitraptor_api::resources::{DiskUsage, RepoDisk};

use crate::profile::ProfileDirs;
use crate::timemachine::oplog::TM_DIR;

/// Bounds of one read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskLimits {
    pub time: Duration,
    pub entries: u64,
}

impl Default for DiskLimits {
    fn default() -> Self {
        Self {
            time: Duration::from_secs(2),
            entries: 200_000,
        }
    }
}

struct Budget {
    deadline: Instant,
    left: u64,
    complete: bool,
}

impl Budget {
    fn take(&mut self) -> bool {
        if self.left == 0 || Instant::now() >= self.deadline {
            self.complete = false;
            return false;
        }
        self.left -= 1;
        true
    }
}

/// Bytes allocated on disk for one entry.
fn allocated(meta: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.blocks() * 512
    }
    #[cfg(not(unix))]
    {
        if meta.is_file() { meta.len() } else { 0 }
    }
}

/// Size of `root` and everything under it, skipping `skip`.
fn size_of(root: &Path, skip: Option<&Path>, budget: &mut Budget) -> u64 {
    let mut total = 0;
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        if skip.is_some_and(|s| path == s) || !budget.take() {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        total += allocated(&meta);
        if meta.is_dir()
            && let Ok(entries) = std::fs::read_dir(&path)
        {
            pending.extend(entries.flatten().map(|e| e.path()));
        }
    }
    total
}

/// The profile folders, without any that lies inside another.
fn roots(dirs: &ProfileDirs) -> Vec<PathBuf> {
    let mut all: Vec<PathBuf> = [Some(&dirs.data), Some(&dirs.config), Some(&dirs.state)]
        .into_iter()
        .chain([dirs.runtime.as_ref()])
        .flatten()
        .cloned()
        .collect();
    all.sort();
    all.dedup();
    let copy = all.clone();
    all.retain(|r| !copy.iter().any(|o| o != r && r.starts_with(o)));
    all
}

/// Reads the disk the profile takes.
pub fn measure(dirs: &ProfileDirs, limits: DiskLimits) -> DiskUsage {
    let mut budget = Budget {
        deadline: Instant::now() + limits.time,
        left: limits.entries,
        complete: true,
    };
    let tm = dirs.data.join(TM_DIR);
    let mut time_machine = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&tm) {
        let mut repos: Vec<_> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .collect();
        repos.sort_by_key(|e| e.file_name());
        for entry in repos {
            let Some(repo_id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let bytes = size_of(&entry.path(), None, &mut budget);
            time_machine.push(RepoDisk { repo_id, bytes });
        }
    }
    let profile_bytes = roots(dirs)
        .iter()
        .map(|root| size_of(root, Some(&tm), &mut budget))
        .sum();
    DiskUsage {
        profile_bytes,
        time_machine,
        complete: budget.complete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> (tempfile::TempDir, ProfileDirs) {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = ProfileDirs::under_root(tmp.path());
        for dir in [&dirs.data, &dirs.config, &dirs.state] {
            std::fs::create_dir_all(dir).unwrap();
        }
        (tmp, dirs)
    }

    fn write(path: &Path, bytes: usize) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![7u8; bytes]).unwrap();
    }

    #[test]
    fn the_time_machine_is_counted_apart_and_per_repo() {
        let (_tmp, dirs) = profile();
        write(&dirs.data.join("index.sqlite"), 64 * 1024);
        write(&dirs.state.join("raptor.log"), 16 * 1024);
        write(&dirs.data.join("tm/r1/store.git/objects/a"), 256 * 1024);
        write(&dirs.data.join("tm/r1/oplog.sqlite"), 32 * 1024);
        write(&dirs.data.join("tm/r2/oplog.sqlite"), 8 * 1024);
        let usage = measure(&dirs, DiskLimits::default());
        assert!(usage.complete);
        let ids: Vec<_> = usage
            .time_machine
            .iter()
            .map(|r| r.repo_id.as_str())
            .collect();
        assert_eq!(ids, ["r1", "r2"]);
        let r1 = usage.time_machine[0].bytes;
        assert!((288 * 1024..400 * 1024).contains(&r1), "{r1}");
        // The profile has the index and the log, never the Time Machine.
        assert!(usage.profile_bytes >= 80 * 1024, "{usage:?}");
        assert!(usage.profile_bytes < 200 * 1024, "{usage:?}");
        assert_eq!(usage.time_machine_bytes(), r1 + usage.time_machine[1].bytes);
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_links_are_not_followed() {
        let (tmp, dirs) = profile();
        let outside = tmp.path().join("outside");
        write(&outside.join("big"), 1024 * 1024);
        std::os::unix::fs::symlink(&outside, dirs.data.join("link")).unwrap();
        let usage = measure(&dirs, DiskLimits::default());
        assert!(usage.profile_bytes < 512 * 1024, "{usage:?}");
    }

    #[test]
    fn a_bound_leaves_the_read_incomplete() {
        let (_tmp, dirs) = profile();
        for i in 0..20 {
            write(&dirs.data.join(format!("f{i}")), 10);
        }
        let usage = measure(
            &dirs,
            DiskLimits {
                time: Duration::from_secs(60),
                entries: 5,
            },
        );
        assert!(!usage.complete);
        let usage = measure(
            &dirs,
            DiskLimits {
                time: Duration::ZERO,
                entries: 1_000,
            },
        );
        assert!(!usage.complete);
        assert_eq!(usage.profile_bytes, 0);
    }

    #[test]
    fn a_missing_profile_takes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = ProfileDirs::under_root(tmp.path().join("none"));
        let usage = measure(&dirs, DiskLimits::default());
        assert_eq!(usage.profile_bytes, 0);
        assert!(usage.time_machine.is_empty());
        assert!(usage.complete);
    }
}
