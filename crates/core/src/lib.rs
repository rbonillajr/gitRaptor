//! GitRaptor engine: worktree watcher, oplog/snapshots and conflict prediction.

pub mod autostart;
pub mod channel;
pub mod client;
pub mod daemon;
pub mod detect;
pub mod discovery;
pub mod executor;
pub mod guardrails;
pub mod observe;
pub mod profile;
pub mod repo_lock;
pub mod resources;
pub mod timemachine;
pub mod watch;

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
