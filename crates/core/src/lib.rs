//! GitRaptor engine: worktree watcher, oplog/snapshots and conflict prediction.

pub mod daemon;
pub mod profile;
pub mod timemachine;

pub use gitraptor_api::API_VERSION;

/// Engine version, taken from the crate manifest.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_manifest() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
