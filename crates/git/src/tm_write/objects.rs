//! Objects from the store into the user's repository (ADR-TMC-002 § 3, step 4): a pack made by
//! `pack-objects --revs --stdout` on the store and read by `index-pack --stdin --strict` in the
//! repository, without any transport (SEC-TMC-02). The pack is kept (`.keep`) until the refs of
//! step 5 reach its objects, so a `gc` of an agent cannot drop them in between.

use std::path::{Path, PathBuf};

use super::worktree::WriteWorktree;
use super::{Result, WriteContext};
use crate::Oid;

/// A pack copied into the repository and still kept.
#[derive(Debug)]
#[must_use = "release the keep file once refs reach the objects"]
pub struct KeptPack {
    keep: PathBuf,
}

impl KeptPack {
    pub fn keep_path(&self) -> &Path {
        &self.keep
    }

    /// Removes the `.keep` file: the objects are now reachable from refs or the index.
    pub fn release(self) -> Result<()> {
        match std::fs::remove_file(&self.keep) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// Copies into `repo` what `wants` reach in the store at `store_dir`, minus what `haves` reach
/// (objects the repository already has). Returns `None` when there is nothing to copy.
pub fn copy_into_repo(
    ctx: &WriteContext,
    store_dir: &Path,
    repo: &WriteWorktree,
    wants: &[Oid],
    haves: &[Oid],
) -> Result<Option<KeptPack>> {
    if wants.is_empty() {
        return Ok(None);
    }
    let mut revs = String::new();
    for w in wants {
        revs.push_str(&format!("{w}\n"));
    }
    for h in haves {
        revs.push_str(&format!("^{h}\n"));
    }
    let tmp = ctx.scratch_path("pack");
    let result = (|| {
        let file = create_private(&tmp)?;
        super::cli::pack_objects(ctx, store_dir, revs.as_bytes(), file)?;
        let hash = super::cli::index_pack(ctx, repo.common_dir(), std::fs::File::open(&tmp)?)?;
        Ok(KeptPack {
            keep: repo
                .common_dir()
                .join("objects")
                .join("pack")
                .join(format!("pack-{hash}.keep")),
        })
    })();
    let _ = std::fs::remove_file(&tmp);
    result.map(Some)
}

/// A new file, exclusive and private (SEC-TMC-04).
fn create_private(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}
