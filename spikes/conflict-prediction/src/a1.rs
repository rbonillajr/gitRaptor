//! A1 — control only (plan P1): the high-level `Repository::merge_commits`, which builds its
//! resource caches from the observed repository (merge drivers, filters, `.gitattributes`).
//! Used in E2/E4 to show that the canary fires with it and not with A2. Never a candidate.

use std::path::Path;

pub fn predict(path: &Path, ours: &str, theirs: &str) -> Result<Vec<String>, String> {
    // Default (trusting) open options, like a naive integration would use.
    let repo = gix::open(path)
        .map_err(|e| e.to_string())?
        .with_object_memory();
    let o = repo
        .rev_parse_single(ours)
        .map_err(|e| e.to_string())?
        .detach();
    let t = repo
        .rev_parse_single(theirs)
        .map_err(|e| e.to_string())?
        .detach();
    let opts = repo.tree_merge_options().map_err(|e| e.to_string())?;
    let out = repo
        .merge_commits(o, t, Default::default(), opts.into())
        .map_err(|e| e.to_string())?;
    Ok(out
        .tree_merge
        .conflicts
        .iter()
        .filter(|c| c.is_unresolved(gix::merge::tree::TreatAsUnresolved::git()))
        .map(|c| c.ours.location().to_string())
        .collect())
}
