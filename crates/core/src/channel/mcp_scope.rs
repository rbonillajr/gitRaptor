//! The scope of an MCP read (US-MCP-003, ADR-MCP-001 § 2): the observed
//! worktree that contains the caller's working folder. Membership is by
//! path components (`/w/repo` does not contain `/w/repo-x`) and the deepest
//! worktree wins, linked worktrees included.

use std::path::Path;

use gitraptor_api::messages::RepoView;

/// The repo and the worktree (indexes into `repos` and its `worktrees`)
/// that contain `cwd`, which must already be canonical.
pub(crate) fn locate(cwd: &Path, repos: &[RepoView]) -> Option<(usize, usize)> {
    let mut best: Option<(usize, usize, usize)> = None;
    for (r, repo) in repos.iter().enumerate() {
        for (w, worktree) in repo.worktrees.iter().enumerate() {
            let root = Path::new(worktree.path.raw());
            // `starts_with` compares whole components.
            if !root.is_absolute() || !cwd.starts_with(root) {
                continue;
            }
            let depth = root.components().count();
            if best.is_none_or(|(_, _, d)| depth > d) {
                best = Some((r, w, depth));
            }
        }
    }
    best.map(|(r, w, _)| (r, w))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(v: impl serde::Serialize) -> serde_json::Value {
        serde_json::to_value(v).unwrap()
    }

    fn repo(id: &str, worktrees: &[&str]) -> RepoView {
        let mut value = serde_json::json!({
            "repo_id": id,
            "state": "observed",
            "path": wire(gitraptor_api::Untrusted::new("/x/.git")),
            "base": {"name": wire(gitraptor_api::UntrustedName::new("main")), "status": "unconfirmed"},
            "worktrees": [],
        });
        let views: Vec<serde_json::Value> = worktrees
            .iter()
            .map(|p| {
                serde_json::json!({
                    "path": wire(gitraptor_api::Untrusted::new(*p)),
                    "main": false,
                    "status": {"state": "unavailable", "reason": "missing"},
                })
            })
            .collect();
        value["worktrees"] = serde_json::Value::Array(views);
        serde_json::from_value(value).expect("a repo view")
    }

    /// An absolute path on this OS: `/w/...` on Unix, `C:\\w\\...` on Windows.
    fn abs(path: &str) -> String {
        if cfg!(windows) {
            format!("C:{}", path.replace('/', "\\"))
        } else {
            path.to_owned()
        }
    }

    #[test]
    fn the_deepest_worktree_by_components_wins() {
        let (shop, feat, shop_x) = (abs("/w/shop"), abs("/w/shop/.wt/feat"), abs("/w/shop-x"));
        let repos = [
            repo("a", &[shop.as_str(), feat.as_str()]),
            repo("b", &[shop_x.as_str()]),
        ];
        let at = |p: &str| locate(Path::new(&abs(p)), &repos);
        assert_eq!(at("/w/shop/src"), Some((0, 0)));
        assert_eq!(at("/w/shop/.wt/feat/src"), Some((0, 1)));
        assert_eq!(at("/w/shop-x"), Some((1, 0)));
        assert_eq!(at("/w/sho"), None);
        assert_eq!(at("/elsewhere"), None);
    }
}
