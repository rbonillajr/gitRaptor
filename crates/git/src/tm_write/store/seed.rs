//! Seeding the store from the user's packs (ADR-TMC-001 § 3, enmienda 2026-10-04).
//!
//! The source is untrusted input:
//! - only `objects/pack/*.pack` files are taken, opened without following symlinks and checked
//!   to be regular files; `.idx`, `.bitmap`, `.rev`, `.keep`, `.promisor` and the multi-pack
//!   index are never copied, `alternates` are never followed;
//! - the pack is cloned from the open descriptor (copy on write: `fclonefileat` on APFS,
//!   `FICLONE` on Btrfs and XFS) and falls back to a byte copy (other file systems, other
//!   volume); **never a hard link**, which would let a write to the store touch the user's pack
//!   (Git "freshens" a packed object by changing the pack's mtime);
//! - the clone becomes 0600 and loses its extended attributes;
//! - its trailing checksum must match its name, and its index is regenerated here with gix, which
//!   re-hashes every object while indexing.
//!
//! A pack that fails any check is skipped and reported; the store stays valid without it.

#[cfg(unix)]
use std::io;
#[cfg(unix)]
use std::path::{Path, PathBuf};

use super::{Result, StoreError, StoreRepo};
use crate::RepoReader;

/// A pack that was not seeded, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedPack {
    pub name: String,
    pub reason: String,
}

/// What a seeding did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SeedReport {
    pub cloned: usize,
    pub copied: usize,
    /// Already in the store from an earlier seeding.
    pub present: usize,
    pub objects: u64,
    pub bytes: u64,
    pub skipped: Vec<SkippedPack>,
}

/// Limits of the indexing, which runs in the background (ADR-TMC-001 § 3).
#[derive(Debug, Clone, Copy)]
pub struct SeedLimits {
    pub threads: usize,
    /// Largest single allocation while resolving deltas.
    pub alloc_limit_bytes: usize,
}

impl Default for SeedLimits {
    fn default() -> Self {
        Self {
            threads: std::thread::available_parallelism()
                .map_or(2, |n| n.get() / 2)
                .clamp(1, 4),
            alloc_limit_bytes: 1 << 30,
        }
    }
}

#[cfg(unix)]
enum Copied {
    Cloned,
    Copied,
}

impl StoreRepo {
    /// Seeds the store with the packs of `user` (its common Git directory).
    pub fn seed_from(&self, user: &RepoReader, limits: SeedLimits) -> Result<SeedReport> {
        #[cfg(unix)]
        {
            let src_dir = user.common_dir().join("objects").join("pack");
            let dst_dir = self.path.join("objects").join("pack");
            let mut report = SeedReport::default();
            let partial = user
                .history_gaps()
                .contains(&crate::HistoryGap::PartialClone);
            let entries = match std::fs::read_dir(&src_dir) {
                Ok(e) => e,
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(report),
                Err(e) => return Err(e.into()),
            };
            let mut names: Vec<String> = entries
                .filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|n| n.ends_with(".pack"))
                .collect();
            names.sort();
            for (n, name) in names.into_iter().enumerate() {
                let skip = |reason: &str| SkippedPack {
                    name: name.clone(),
                    reason: reason.into(),
                };
                let Some(hash) = pack_hash(&name) else {
                    report.skipped.push(skip("not a pack name"));
                    continue;
                };
                if partial || src_dir.join(format!("pack-{hash}.promisor")).exists() {
                    report.skipped.push(skip("partial-clone"));
                    continue;
                }
                let final_pack = dst_dir.join(&name);
                if final_pack.exists() && final_pack.with_extension("idx").exists() {
                    report.present += 1;
                    continue;
                }
                let tmp = dst_dir.join(format!("tmp-seed-{}-{n}.pack", super::durable::nanos()));
                let how = match clone_or_copy(&src_dir.join(&name), &tmp) {
                    Ok(how) => how,
                    Err(e) => {
                        let _ = std::fs::remove_file(&tmp);
                        report.skipped.push(skip(&format!("copy: {e}")));
                        continue;
                    }
                };
                match index_pack(&tmp, &hash, &dst_dir, limits) {
                    Ok((objects, bytes)) => {
                        report.objects += objects;
                        report.bytes += bytes;
                        match how {
                            Copied::Cloned => report.cloned += 1,
                            Copied::Copied => report.copied += 1,
                        }
                    }
                    Err(e) => {
                        let _ = std::fs::remove_file(&tmp);
                        let _ = std::fs::remove_file(tmp.with_extension("idx"));
                        report.skipped.push(skip(&e.to_string()));
                    }
                }
            }
            super::durable::fsync_dir(&dst_dir)?;
            Ok(report)
        }
        #[cfg(not(unix))]
        {
            let _ = (user, limits);
            Err(StoreError::Unsupported("seeding"))
        }
    }
}

/// `pack-<40 hex>.pack` → the hex.
#[cfg(unix)]
fn pack_hash(name: &str) -> Option<String> {
    let hex = name.strip_prefix("pack-")?.strip_suffix(".pack")?;
    (hex.len() == 40 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
        .then(|| hex.to_owned())
}

/// Clones `src` into `dst` from the open descriptor, or copies its bytes.
#[cfg(unix)]
fn clone_or_copy(src: &Path, dst: &Path) -> io::Result<Copied> {
    use rustix::fs::{CWD, FileType, Mode, OFlags};
    let src_fd = rustix::fs::openat(
        CWD,
        src,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let st = rustix::fs::fstat(&src_fd)?;
    if FileType::from_raw_mode(st.st_mode) != FileType::RegularFile {
        return Err(io::Error::other("not a regular file"));
    }
    #[cfg(target_vendor = "apple")]
    let cloned = {
        use rustix::fs::CloneFlags;
        rustix::fs::fclonefileat(
            &src_fd,
            CWD,
            dst,
            CloneFlags::NOFOLLOW | CloneFlags::NOOWNERCOPY,
        )
        .is_ok()
    };
    #[cfg(not(target_vendor = "apple"))]
    let cloned = false;
    // A clone keeps the source mode (Git leaves packs 0444): open it read-only, which is
    // enough for the owner to change its mode and attributes and to sync it.
    let dst_fd = if cloned {
        rustix::fs::openat(
            CWD,
            dst,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?
    } else {
        rustix::fs::openat(
            CWD,
            dst,
            OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )?
    };
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let cloned = cloned || rustix::fs::ioctl_ficlone(&dst_fd, &src_fd).is_ok();
    let mut dst_file = std::fs::File::from(dst_fd);
    if !cloned {
        let mut src_file = std::fs::File::from(src_fd);
        io::copy(&mut src_file, &mut dst_file)?;
    }
    rustix::fs::fchmod(&dst_file, Mode::from_raw_mode(0o600))?;
    strip_xattrs(&dst_file)?;
    rustix::fs::fsync(&dst_file)?;
    Ok(if cloned {
        Copied::Cloned
    } else {
        Copied::Copied
    })
}

/// Removes every extended attribute a clone carried over from the user's file.
#[cfg(unix)]
fn strip_xattrs(file: &std::fs::File) -> io::Result<()> {
    let mut buf = vec![0u8; 4096];
    let len = match rustix::fs::flistxattr(file, &mut buf[..]) {
        Ok(len) => len,
        Err(rustix::io::Errno::NOTSUP) => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    for name in buf[..len].split(|b| *b == 0).filter(|n| !n.is_empty()) {
        let name = std::ffi::CString::new(name.to_vec()).map_err(io::Error::other)?;
        rustix::fs::fremovexattr(file, name.as_c_str())?;
    }
    Ok(())
}

/// Bytes of one entry of the pack being indexed.
#[cfg(unix)]
fn resolve(range: gix_pack::data::EntryRange, pack: &gix_pack::data::File) -> Option<&[u8]> {
    pack.entry_slice(range)
}

/// Checks the pack trailer against its name, writes its index next to it (re-hashing every
/// object) and moves both into place. Returns objects and bytes.
#[cfg(unix)]
fn index_pack(tmp: &Path, hash: &str, dst_dir: &Path, limits: SeedLimits) -> Result<(u64, u64)> {
    use std::io::{Read, Seek, SeekFrom};
    use std::sync::atomic::AtomicBool;

    use gix_pack::data;

    let mut file = std::fs::File::open(tmp)?;
    let size = file.metadata()?.len();
    if size < 32 {
        return Err(StoreError::Corrupt("pack too small".into()));
    }
    let mut trailer = [0u8; 20];
    file.seek(SeekFrom::End(-20))?;
    file.read_exact(&mut trailer)?;
    let trailer_hex: String = trailer.iter().map(|b| format!("{b:02x}")).collect();
    if trailer_hex != hash {
        return Err(StoreError::Corrupt(
            "pack checksum does not match its name".into(),
        ));
    }
    file.seek(SeekFrom::Start(0))?;

    let kind = gix::hash::Kind::Sha1;
    let pack = data::File::at(tmp, kind).map_err(|e| StoreError::Corrupt(format!("pack: {e}")))?;
    let mut entries = data::input::BytesToEntriesIter::new_from_header(
        io::BufReader::with_capacity(1 << 16, file),
        data::input::Mode::Verify,
        data::input::EntryDataMode::Crc32,
        kind,
    )
    .map_err(|e| StoreError::Corrupt(format!("pack: {e}")))?;
    let version = entries.version();
    let tmp_idx: PathBuf = tmp.with_extension("idx");
    let mut idx = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp_idx)?
    };
    let interrupt = AtomicBool::new(false);
    let outcome = gix_pack::index::write_data_iter_to_stream(
        gix_pack::index::Version::default(),
        move || Ok((resolve, pack)),
        &mut entries,
        Some(limits.threads),
        &mut gix::progress::Discard,
        &mut idx,
        &interrupt,
        kind,
        Some(limits.alloc_limit_bytes),
        version,
    )
    .map_err(|e| StoreError::Corrupt(format!("index: {e}")))?;
    if !outcome.data_hash.to_hex().to_string().eq(hash) {
        return Err(StoreError::Corrupt(
            "pack checksum does not match its name".into(),
        ));
    }
    rustix::fs::fsync(&idx).map_err(io::Error::from)?;
    drop(idx);
    let final_pack = dst_dir.join(format!("pack-{hash}.pack"));
    std::fs::rename(tmp, &final_pack)?;
    std::fs::rename(&tmp_idx, final_pack.with_extension("idx"))?;
    Ok((u64::from(outcome.num_objects), size))
}
