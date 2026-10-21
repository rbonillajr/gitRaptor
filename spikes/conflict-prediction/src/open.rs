//! Opening the observed repository read-only, like `RepoReader::open` (crates/git/src/reader.rs):
//! no `git` binary, no system config or attributes (F9), isolated environment, replacement
//! objects ignored, and every object the merge writes kept in memory.

use std::path::Path;

use gix::open::Permissions;
use gix::sec::Permission;

pub fn permissions() -> Permissions {
    let mut p = Permissions::default();
    p.config.git_binary = false;
    p.attributes.git_binary = false;
    p.config.system = false;
    p.attributes.system = false;
    p.config.env = false;
    p.env = gix::open::permissions::Environment::isolated();
    p.env.home = Permission::Deny;
    p.config.user = false;
    p.config.git = false;
    p.attributes.git = false;
    p
}

/// Open `path` for one prediction job. `with_object_memory()` keeps the merged blobs and trees
/// in memory: nothing is ever written to the object database (F5).
pub fn open(path: &Path) -> Result<gix::Repository, String> {
    let opts = gix::open::Options::default()
        .permissions(permissions())
        .bail_if_untrusted(true);
    let mut repo =
        gix::open_opts(path, opts).map_err(|e| format!("open {}: {e}", path.display()))?;
    repo.objects.ignore_replacements = true;
    // A large object cache: the 10 pairs of a worktree share most of their trees.
    repo.object_cache_size_if_unset(64 * 1024 * 1024);
    Ok(repo.with_object_memory())
}
